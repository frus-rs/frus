// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Hardware buffer frames, without a copy** (milestone 800): an Android `AHardwareBuffer`
//! a decoder, a camera or a canvas wrote is imported into Vulkan where it lies and drawn
//! into the external texture's RGBA texture by one small pass.
//!
//! # Why a pass of its own
//!
//! A decoder's buffer is YUV in a layout only the driver knows (an "external format"), and
//! the only way to read it is through a sampler carrying a `VkSamplerYcbcrConversion`,
//! bound immutably in the descriptor set layout. WGSL cannot name such a sampler and wgpu
//! cannot make one, so this module records the read itself, in raw Vulkan, into a wgpu
//! command encoder (`as_hal_mut`), and submits it through wgpu's queue: the submission
//! stays in wgpu's order, and wgpu's "work done" callback tells when the GPU has finished
//! reading the buffer. The pass writes the RGBA texture that wgpu owns and the image
//! painter samples like any other: everything after the pass (clips, layers, batching,
//! filtering) is unchanged. The alternative, sampling the buffer from the image painter's
//! own WGSL pipeline, would need a second, hand-made copy of that pipeline per buffer
//! format; the extra pass costs one textured triangle the size of the video, read once
//! per new frame, and nothing on frames that bring none.
//!
//! # Synchronisation
//!
//! - The frame (and so the producer's image) is held until wgpu says the submission that
//!   read it is done: the producer cannot write into a buffer still being read.
//! - The RGBA texture is first sampled once through wgpu (`prime`), so wgpu has made it,
//!   cleared it and believes it ready for sampling. Every pass then takes it from
//!   "undefined" to a colour attachment and leaves it ready for sampling, as wgpu believes it
//!   is; barriers order it after earlier sampling and before later sampling, by submission
//!   order on the one queue.
//! - The buffer's image is taken from the foreign queue family each time it is read.

use std::collections::HashMap;
use std::ffi::{c_char, CStr};
use std::sync::{Arc, Mutex};

use ash::vk;
use frus_core::{TextureFrame, YuvColorSpace};
use wgpu::hal::api::Vulkan;

/// The device extensions an import needs, besides those wgpu asks for. Most are core in
/// Vulkan 1.1; naming them when the driver lists them costs nothing.
const IMPORT_EXTENSIONS: [&CStr; 7] = [
    ash::android::external_memory_android_hardware_buffer::NAME,
    ash::khr::sampler_ycbcr_conversion::NAME,
    ash::khr::external_memory::NAME,
    ash::khr::dedicated_allocation::NAME,
    ash::khr::get_memory_requirements2::NAME,
    ash::khr::bind_memory2::NAME,
    ash::ext::queue_family_foreign::NAME,
];

/// Opens the device with what importing hardware buffers needs, when the GPU is a Vulkan 1.1
/// one offering `VK_ANDROID_external_memory_android_hardware_buffer` and the sampler Y'CbCr
/// conversion. `None` otherwise (or on any failure): the caller opens a plain device, and
/// frames are copied.
pub(crate) fn open_device(
    adapter: &wgpu::Adapter,
    desc: &wgpu::DeviceDescriptor<'_>,
) -> Option<(wgpu::Device, wgpu::Queue)> {
    let open = {
        let hal_adapter = unsafe { adapter.as_hal::<Vulkan>() }?;
        let instance = hal_adapter.shared_instance().raw_instance();
        let phd = hal_adapter.raw_physical_device();
        let properties = unsafe { instance.get_physical_device_properties(phd) };
        if properties.api_version < vk::API_VERSION_1_1 {
            log::info!("hardware buffers: copied (Vulkan 1.0 here)");
            return None;
        }
        let available = unsafe { instance.enumerate_device_extension_properties(phd) }.ok()?;
        let has = |name: &CStr| {
            available
                .iter()
                .any(|e| e.extension_name_as_c_str() == Ok(name))
        };
        if !has(IMPORT_EXTENSIONS[0]) {
            log::info!(
                "hardware buffers: copied (no VK_ANDROID_external_memory_android_hardware_buffer)"
            );
            return None;
        }
        let mut ycbcr = vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default();
        let mut features2 = vk::PhysicalDeviceFeatures2::default().push_next(&mut ycbcr);
        unsafe { instance.get_physical_device_features2(phd, &mut features2) };
        if ycbcr.sampler_ycbcr_conversion != vk::TRUE {
            log::info!("hardware buffers: copied (no sampler Y'CbCr conversion)");
            return None;
        }

        let features = desc.required_features;
        let mut extensions = hal_adapter.required_device_extensions(features);
        for name in IMPORT_EXTENSIONS {
            if has(name) && !extensions.contains(&name) {
                extensions.push(name);
            }
        }
        let mut phd_features = hal_adapter.physical_device_features(&extensions, features);
        let names: Vec<*const c_char> = extensions.iter().map(|n| n.as_ptr()).collect();
        let priorities = [1.0];
        // Queue family 0, queue 0: what wgpu opens itself.
        let families = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(0)
            .queue_priorities(&priorities)];
        let mut own_ycbcr = vk::PhysicalDeviceSamplerYcbcrConversionFeatures::default()
            .sampler_ycbcr_conversion(true);
        let info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&families)
            .enabled_extension_names(&names);
        let mut info = phd_features.add_to_device_create(info);
        // wgpu chains the Y'CbCr feature structure on a 1.1 device, switched off; switch it
        // on there rather than chaining a second one, which Vulkan forbids.
        if !unsafe { enable_ycbcr_in_chain(info.p_next) } {
            info = info.push_next(&mut own_ycbcr);
        }
        let raw = match unsafe { instance.create_device(phd, &info, None) } {
            Ok(raw) => raw,
            Err(err) => {
                log::warn!("hardware buffers: copied (the device with imports failed: {err})");
                return None;
            }
        };
        match unsafe {
            hal_adapter.device_from_raw(
                raw.clone(),
                None,
                &extensions,
                features,
                &desc.required_limits,
                &desc.memory_hints,
                0,
                0,
            )
        } {
            Ok(open) => open,
            Err(err) => {
                unsafe { raw.destroy_device(None) };
                log::warn!("hardware buffers: copied (wgpu refused the device: {err})");
                return None;
            }
        }
    };
    match unsafe { adapter.create_device_from_hal(open, desc) } {
        Ok(opened) => {
            log::info!("hardware buffers: imported by Vulkan, no copy");
            Some(opened)
        }
        Err(err) => {
            log::warn!("hardware buffers: copied (wgpu refused the device: {err})");
            None
        }
    }
}

/// Sets `samplerYcbcrConversion` in the feature structure of `chain`; whether it found one.
///
/// # Safety
///
/// `chain` is a valid `pNext` chain whose structures were linked from mutable references.
unsafe fn enable_ycbcr_in_chain(chain: *const std::ffi::c_void) -> bool {
    let mut next = chain as *mut vk::BaseOutStructure<'_>;
    while !next.is_null() {
        if (*next).s_type == vk::StructureType::PHYSICAL_DEVICE_SAMPLER_YCBCR_CONVERSION_FEATURES {
            let features = next.cast::<vk::PhysicalDeviceSamplerYcbcrConversionFeatures<'_>>();
            (*features).sampler_ycbcr_conversion = vk::TRUE;
            return true;
        }
        next = (*next).p_next;
    }
    false
}

/// What decides a sampler and a pipeline: the buffer's format and how its YUV is read.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Key {
    format: vk::Format,
    external_format: u64,
    convert: bool,
    model: vk::SamplerYcbcrModelConversion,
    range: vk::SamplerYcbcrRange,
    components: [vk::ComponentSwizzle; 4],
    x_offset: vk::ChromaLocation,
    y_offset: vk::ChromaLocation,
    filter: vk::Filter,
}

/// A conversion, its immutable sampler, and the pipeline reading through them; destroyed
/// when the last import read through it is.
struct Pipe {
    raw: ash::Device,
    conversion: Option<vk::SamplerYcbcrConversion>,
    sampler: vk::Sampler,
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
}

/// The device-wide part: the render pass, the shaders, the pipelines by format.
pub(crate) struct Importer {
    raw: ash::Device,
    ahb: ash::android::external_memory_android_hardware_buffer::Device,
    family: u32,
    foreign: bool,
    render_pass: vk::RenderPass,
    vert: vk::ShaderModule,
    frag: vk::ShaderModule,
    pipes: Mutex<HashMap<Key, Arc<Pipe>>>,
    queue: wgpu::Queue,
    /// Keeps the device alive for as long as anything here does; dropped last.
    _device: wgpu::Device,
}

// The raw handles are only used under the rules Vulkan sets for them: the pipelines' map
// is behind a lock, and recording happens on the thread that owns the encoder.
unsafe impl Send for Importer {}
unsafe impl Sync for Importer {}

impl Importer {
    /// The importer of `device`, when it was opened with the import extensions.
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Arc<Importer>> {
        let hal = unsafe { device.as_hal::<Vulkan>() }?;
        if !hal
            .enabled_device_extensions()
            .contains(&ash::android::external_memory_android_hardware_buffer::NAME)
        {
            return None;
        }
        let raw = hal.raw_device().clone();
        let instance = hal.shared_instance().raw_instance();
        let ahb =
            ash::android::external_memory_android_hardware_buffer::Device::new(instance, &raw);
        let foreign = hal
            .enabled_device_extensions()
            .contains(&ash::ext::queue_family_foreign::NAME);
        let family = hal.queue_family_index();
        let made = unsafe { Self::make_fixed(&raw) };
        let (render_pass, vert, frag) = match made {
            Ok(made) => made,
            Err(err) => {
                log::warn!("hardware buffers: the import pass could not be made ({err})");
                return None;
            }
        };
        Some(Arc::new(Importer {
            raw,
            ahb,
            family,
            foreign,
            render_pass,
            vert,
            frag,
            pipes: Mutex::new(HashMap::new()),
            queue: queue.clone(),
            _device: device.clone(),
        }))
    }

    unsafe fn make_fixed(
        raw: &ash::Device,
    ) -> Result<(vk::RenderPass, vk::ShaderModule, vk::ShaderModule), vk::Result> {
        let attachment = [vk::AttachmentDescription::default()
            .format(vk::Format::R8G8B8A8_UNORM)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::DONT_CARE)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
        let color = [vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
        let subpass = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&color)];
        let render_pass = raw.create_render_pass(
            &vk::RenderPassCreateInfo::default()
                .attachments(&attachment)
                .subpasses(&subpass),
            None,
        )?;
        let module = |bytes: &[u8]| {
            let words = ash::util::read_spv(&mut std::io::Cursor::new(bytes))
                .map_err(|_| vk::Result::ERROR_INVALID_SHADER_NV)?;
            raw.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
        };
        let vert = module(include_bytes!("shaders/hardware.vert.spv"))?;
        let frag = module(include_bytes!("shaders/hardware.frag.spv"))?;
        Ok((render_pass, vert, frag))
    }

    /// The pipeline for `key`, made the first time.
    fn pipe(&self, key: Key) -> Result<Arc<Pipe>, vk::Result> {
        let mut pipes = self.pipes.lock().map_err(|_| vk::Result::ERROR_UNKNOWN)?;
        if let Some(pipe) = pipes.get(&key) {
            return Ok(pipe.clone());
        }
        let pipe = Arc::new(unsafe { self.make_pipe(key) }?);
        pipes.insert(key, pipe.clone());
        Ok(pipe)
    }

    unsafe fn make_pipe(&self, key: Key) -> Result<Pipe, vk::Result> {
        let raw = &self.raw;
        let conversion = if key.convert {
            let mut external =
                vk::ExternalFormatANDROID::default().external_format(key.external_format);
            let [r, g, b, a] = key.components;
            let info = vk::SamplerYcbcrConversionCreateInfo::default()
                .format(key.format)
                .ycbcr_model(key.model)
                .ycbcr_range(key.range)
                .components(vk::ComponentMapping { r, g, b, a })
                .x_chroma_offset(key.x_offset)
                .y_chroma_offset(key.y_offset)
                .chroma_filter(key.filter)
                .force_explicit_reconstruction(false)
                .push_next(&mut external);
            Some(raw.create_sampler_ycbcr_conversion(&info, None)?)
        } else {
            None
        };
        let mut conversion_info = vk::SamplerYcbcrConversionInfo::default();
        let mut sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(key.filter)
            .min_filter(key.filter)
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .max_lod(0.0);
        if let Some(conversion) = conversion {
            conversion_info = conversion_info.conversion(conversion);
            sampler_info = sampler_info.push_next(&mut conversion_info);
        }
        let sampler = raw.create_sampler(&sampler_info, None)?;
        let samplers = [sampler];
        let binding = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)
            .immutable_samplers(&samplers)];
        let set_layout = raw.create_descriptor_set_layout(
            &vk::DescriptorSetLayoutCreateInfo::default().bindings(&binding),
            None,
        )?;
        let set_layouts = [set_layout];
        let push = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX)
            .offset(0)
            .size(16)];
        let layout = raw.create_pipeline_layout(
            &vk::PipelineLayoutCreateInfo::default()
                .set_layouts(&set_layouts)
                .push_constant_ranges(&push),
            None,
        )?;
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(self.vert)
                .name(c"main"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(self.frag)
                .name(c"main"),
        ];
        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(false)
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachment);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&assembly)
            .viewport_state(&viewport)
            .rasterization_state(&raster)
            .multisample_state(&multisample)
            .color_blend_state(&blend)
            .dynamic_state(&dynamic)
            .layout(layout)
            .render_pass(self.render_pass)
            .subpass(0)];
        let pipeline = raw
            .create_graphics_pipelines(vk::PipelineCache::null(), &info, None)
            .map_err(|(_, err)| err)?[0];
        Ok(Pipe {
            raw: raw.clone(),
            conversion,
            sampler,
            set_layout,
            layout,
            pipeline,
        })
    }

    /// The buffer's format, and how it is to be read with `space`.
    unsafe fn key_of(
        &self,
        buffer: *mut std::ffi::c_void,
        space: YuvColorSpace,
    ) -> Result<(Key, vk::AndroidHardwareBufferPropertiesANDROID<'static>), vk::Result> {
        let mut format = vk::AndroidHardwareBufferFormatPropertiesANDROID::default();
        let mut props =
            vk::AndroidHardwareBufferPropertiesANDROID::default().push_next(&mut format);
        self.ahb
            .get_android_hardware_buffer_properties(buffer.cast(), &mut props)?;
        // Only the size and the memory types are kept from it: its chain points at `format`.
        let kept = vk::AndroidHardwareBufferPropertiesANDROID::default()
            .allocation_size(props.allocation_size)
            .memory_type_bits(props.memory_type_bits);
        let convert = format.format == vk::Format::UNDEFINED || is_ycbcr(format.format);
        let linear = format
            .format_features
            .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_YCBCR_CONVERSION_LINEAR_FILTER);
        let (model, range) = ycbcr_model(space);
        let c = format.sampler_ycbcr_conversion_components;
        let key = Key {
            format: format.format,
            external_format: if format.format == vk::Format::UNDEFINED {
                format.external_format
            } else {
                0
            },
            convert,
            model,
            range,
            components: [c.r, c.g, c.b, c.a],
            x_offset: format.suggested_x_chroma_offset,
            y_offset: format.suggested_y_chroma_offset,
            filter: if !convert || linear {
                vk::Filter::LINEAR
            } else {
                vk::Filter::NEAREST
            },
        };
        Ok((key, kept))
    }

    /// Imports `buffer` (`width × height`) as an image read through `pipe`.
    unsafe fn import(
        &self,
        buffer: *mut std::ffi::c_void,
        width: u32,
        height: u32,
        key: Key,
        props: &vk::AndroidHardwareBufferPropertiesANDROID<'_>,
        pipe: &Arc<Pipe>,
    ) -> Result<Imported, vk::Result> {
        let raw = &self.raw;
        let mut external_memory = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::ANDROID_HARDWARE_BUFFER_ANDROID);
        let mut external_format =
            vk::ExternalFormatANDROID::default().external_format(key.external_format);
        let image_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(key.format)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut external_memory)
            .push_next(&mut external_format);
        let image = raw.create_image(&image_info, None)?;
        let mut imported = Imported {
            buffer,
            image,
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            pool: vk::DescriptorPool::null(),
            set: vk::DescriptorSet::null(),
            key,
            pipe: pipe.clone(),
            last_used: 0,
        };
        let type_index = props.memory_type_bits.trailing_zeros();
        if type_index >= 32 {
            imported.destroy(raw);
            return Err(vk::Result::ERROR_OUT_OF_DEVICE_MEMORY);
        }
        let mut import =
            vk::ImportAndroidHardwareBufferInfoANDROID::default().buffer(buffer.cast());
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let allocate = vk::MemoryAllocateInfo::default()
            .allocation_size(props.allocation_size)
            .memory_type_index(type_index)
            .push_next(&mut import)
            .push_next(&mut dedicated);
        let result = (|| {
            imported.memory = raw.allocate_memory(&allocate, None)?;
            raw.bind_image_memory(image, imported.memory, 0)?;
            let mut conversion = vk::SamplerYcbcrConversionInfo::default();
            let mut view_info = vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(key.format)
                .subresource_range(color_range());
            if let Some(c) = pipe.conversion {
                conversion = conversion.conversion(c);
                view_info = view_info.push_next(&mut conversion);
            }
            imported.view = raw.create_image_view(&view_info, None)?;
            // A Y'CbCr image may take a descriptor per plane: room for three.
            let sizes = [vk::DescriptorPoolSize {
                ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                descriptor_count: 3,
            }];
            imported.pool = raw.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(1)
                    .pool_sizes(&sizes),
                None,
            )?;
            let layouts = [pipe.set_layout];
            imported.set = raw.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(imported.pool)
                    .set_layouts(&layouts),
            )?[0];
            let image_info = [vk::DescriptorImageInfo::default()
                .image_view(imported.view)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let write = [vk::WriteDescriptorSet::default()
                .dst_set(imported.set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&image_info)];
            raw.update_descriptor_sets(&write, &[]);
            Ok(())
        })();
        match result {
            Ok(()) => Ok(imported),
            Err(err) => {
                imported.destroy(raw);
                Err(err)
            }
        }
    }
}

impl Drop for Importer {
    fn drop(&mut self) {
        // The pipelines go with the map (each when its last import does).
        unsafe {
            self.raw.destroy_render_pass(self.render_pass, None);
            self.raw.destroy_shader_module(self.vert, None);
            self.raw.destroy_shader_module(self.frag, None);
        }
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        unsafe {
            self.raw.destroy_pipeline(self.pipeline, None);
            self.raw.destroy_pipeline_layout(self.layout, None);
            self.raw
                .destroy_descriptor_set_layout(self.set_layout, None);
            self.raw.destroy_sampler(self.sampler, None);
            if let Some(conversion) = self.conversion {
                self.raw.destroy_sampler_ycbcr_conversion(conversion, None);
            }
        }
    }
}

fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

/// Whether `format` is one of Vulkan's Y'CbCr formats (which only a conversion reads).
fn is_ycbcr(format: vk::Format) -> bool {
    (1_000_156_000..=1_000_156_033).contains(&format.as_raw())
}

/// The conversion's matrix and range for a colour space.
fn ycbcr_model(space: YuvColorSpace) -> (vk::SamplerYcbcrModelConversion, vk::SamplerYcbcrRange) {
    match space {
        YuvColorSpace::Bt601Limited => (
            vk::SamplerYcbcrModelConversion::YCBCR_601,
            vk::SamplerYcbcrRange::ITU_NARROW,
        ),
        YuvColorSpace::Bt601Full => (
            vk::SamplerYcbcrModelConversion::YCBCR_601,
            vk::SamplerYcbcrRange::ITU_FULL,
        ),
        YuvColorSpace::Bt709Limited => (
            vk::SamplerYcbcrModelConversion::YCBCR_709,
            vk::SamplerYcbcrRange::ITU_NARROW,
        ),
        YuvColorSpace::Bt709Full => (
            vk::SamplerYcbcrModelConversion::YCBCR_709,
            vk::SamplerYcbcrRange::ITU_FULL,
        ),
    }
}

/// The crop in the buffer's texture coordinates: offset, then size.
fn crop_rect(size: (u32, u32), crop: [u32; 4]) -> [f32; 4] {
    let (w, h) = (size.0.max(1) as f32, size.1.max(1) as f32);
    [
        crop[0] as f32 / w,
        crop[1] as f32 / h,
        (crop[2] - crop[0]) as f32 / w,
        (crop[3] - crop[1]) as f32 / h,
    ]
}

/// A buffer imported as a Vulkan image, kept while the producer keeps handing it out (an
/// image reader cycles through a few).
struct Imported {
    buffer: *mut std::ffi::c_void,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
    key: Key,
    /// Holds the sampler and layout the descriptor set was made for.
    pipe: Arc<Pipe>,
    last_used: u64,
}

impl Imported {
    /// Destroys what was made; the memory's import holds the buffer, freed with it.
    unsafe fn destroy(&mut self, raw: &ash::Device) {
        if self.pool != vk::DescriptorPool::null() {
            raw.destroy_descriptor_pool(self.pool, None);
        }
        if self.view != vk::ImageView::null() {
            raw.destroy_image_view(self.view, None);
        }
        raw.destroy_image(self.image, None);
        if self.memory != vk::DeviceMemory::null() {
            raw.free_memory(self.memory, None);
        }
    }
}

/// How many frames an imported buffer may go unused before it is let go: an image reader
/// cycles through a handful, so one unseen this long was reallocated or the reader is gone.
const STALE_AFTER: u64 = 30;

/// The import side of one external texture: the view and framebuffer over its RGBA texture,
/// and the buffers imported so far.
pub(crate) struct Target {
    importer: Arc<Importer>,
    texture: wgpu::Texture,
    view: vk::ImageView,
    framebuffer: vk::Framebuffer,
    size: (u32, u32),
    imports: Vec<Imported>,
    tick: u64,
}

// Raw handles, used on the render thread only; destroyed after the GPU is done with them.
unsafe impl Send for Target {}
unsafe impl Sync for Target {}

impl Target {
    /// The import side of `texture` (`Rgba8UnormSrgb`, viewable as `Rgba8Unorm`, sampled and
    /// rendered to), `width × height`.
    pub(crate) fn new(
        importer: &Arc<Importer>,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Option<Target> {
        let image = unsafe { texture.as_hal::<Vulkan>() }.map(|t| unsafe { t.raw_handle() })?;
        let raw = &importer.raw;
        let view = unsafe {
            raw.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .subresource_range(color_range()),
                None,
            )
        }
        .ok()?;
        let attachments = [view];
        let framebuffer = unsafe {
            raw.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(importer.render_pass)
                    .attachments(&attachments)
                    .width(width)
                    .height(height)
                    .layers(1),
                None,
            )
        };
        let framebuffer = match framebuffer {
            Ok(framebuffer) => framebuffer,
            Err(_) => {
                unsafe { raw.destroy_image_view(view, None) };
                return None;
            }
        };
        Some(Target {
            importer: importer.clone(),
            texture: texture.clone(),
            view,
            framebuffer,
            size: (width, height),
            imports: Vec::new(),
            tick: 0,
        })
    }

    /// Draws `frame`'s buffer into the texture, and holds the frame until the GPU has read
    /// it. Answers whether it was drawn.
    pub(crate) fn draw(&mut self, device: &wgpu::Device, frame: &Arc<TextureFrame>) -> bool {
        let Some(buffer) = frame.hardware_buffer() else {
            return false;
        };
        self.tick += 1;
        let index = match self.import(buffer.as_ptr(), buffer.size(), frame.color_space()) {
            Ok(index) => index,
            Err(err) => {
                warn_once(&format!("a hardware buffer could not be imported ({err})"));
                return false;
            }
        };
        let crop = crop_rect(buffer.size(), buffer.crop());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frus.hardware.encoder"),
        });
        let recorded = unsafe {
            encoder.as_hal_mut::<Vulkan, _, _>(|encoder| {
                let Some(encoder) = encoder else {
                    return false;
                };
                self.record(encoder.raw_handle(), index, crop);
                true
            })
        };
        if !recorded {
            return false;
        }
        self.importer.queue.submit(Some(encoder.finish()));
        // The producer gets its buffer back once the GPU has read it, not before.
        let held = frame.clone();
        self.importer
            .queue
            .on_submitted_work_done(move || drop(held));
        self.forget_stale();
        true
    }

    /// The index of `buffer`'s import, importing it the first time.
    fn import(
        &mut self,
        buffer: *mut std::ffi::c_void,
        (width, height): (u32, u32),
        space: YuvColorSpace,
    ) -> Result<usize, vk::Result> {
        let importer = &self.importer;
        let (key, props) = unsafe { importer.key_of(buffer, space) }?;
        if let Some(i) = self
            .imports
            .iter()
            .position(|i| i.buffer == buffer && i.key == key)
        {
            self.imports[i].last_used = self.tick;
            return Ok(i);
        }
        let pipe = importer.pipe(key)?;
        let mut imported = unsafe { importer.import(buffer, width, height, key, &props, &pipe) }?;
        imported.last_used = self.tick;
        // The same buffer read another way (the colour space changed): the old import goes.
        if let Some(i) = self.imports.iter().position(|i| i.buffer == buffer) {
            let old = self.imports.swap_remove(i);
            self.retire(vec![old]);
        }
        self.imports.push(imported);
        Ok(self.imports.len() - 1)
    }

    unsafe fn record_inner(&self, cmd: vk::CommandBuffer, index: usize, crop: [f32; 4]) {
        let raw = &self.importer.raw;
        let imported = &self.imports[index];
        let (width, height) = self.size;
        let source_family = if self.importer.foreign {
            vk::QUEUE_FAMILY_FOREIGN_EXT
        } else {
            vk::QUEUE_FAMILY_EXTERNAL
        };
        let before = [
            // The producer wrote the buffer: take it, for reading.
            vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(source_family)
                .dst_queue_family_index(self.importer.family)
                .image(imported.image)
                .subresource_range(color_range()),
            // The texture: after earlier frames sampled it, all of it drawn again.
            vk::ImageMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(self.texture_image())
                .subresource_range(color_range()),
        ];
        raw.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::TOP_OF_PIPE | vk::PipelineStageFlags::ALL_GRAPHICS,
            vk::PipelineStageFlags::FRAGMENT_SHADER
                | vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &before,
        );
        let area = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D { width, height },
        };
        raw.cmd_begin_render_pass(
            cmd,
            &vk::RenderPassBeginInfo::default()
                .render_pass(self.importer.render_pass)
                .framebuffer(self.framebuffer)
                .render_area(area),
            vk::SubpassContents::INLINE,
        );
        raw.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, imported.pipe.pipeline);
        raw.cmd_set_viewport(
            cmd,
            0,
            &[vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            }],
        );
        raw.cmd_set_scissor(cmd, 0, &[area]);
        raw.cmd_bind_descriptor_sets(
            cmd,
            vk::PipelineBindPoint::GRAPHICS,
            imported.pipe.layout,
            0,
            &[imported.set],
            &[],
        );
        raw.cmd_push_constants(
            cmd,
            imported.pipe.layout,
            vk::ShaderStageFlags::VERTEX,
            0,
            bytemuck::cast_slice(&crop),
        );
        raw.cmd_draw(cmd, 3, 1, 0, 0);
        raw.cmd_end_render_pass(cmd);
        // Ready for sampling, as wgpu believes it is.
        let after = [vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.texture_image())
            .subresource_range(color_range())];
        raw.cmd_pipeline_barrier(
            cmd,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &after,
        );
    }

    fn record(&self, cmd: vk::CommandBuffer, index: usize, crop: [f32; 4]) {
        unsafe { self.record_inner(cmd, index, crop) }
    }

    fn texture_image(&self) -> vk::Image {
        unsafe { self.texture.as_hal::<Vulkan>() }
            .map(|t| unsafe { t.raw_handle() })
            .unwrap_or_default()
    }

    /// Lets go of buffers the producer stopped handing out.
    fn forget_stale(&mut self) {
        let tick = self.tick;
        if !self
            .imports
            .iter()
            .any(|i| tick - i.last_used > STALE_AFTER)
        {
            return;
        }
        let (stale, kept) = std::mem::take(&mut self.imports)
            .into_iter()
            .partition(|i| tick - i.last_used > STALE_AFTER);
        self.imports = kept;
        self.retire(stale);
    }

    /// Destroys `imports` once the GPU has finished what was submitted so far.
    fn retire(&self, imports: Vec<Imported>) {
        if imports.is_empty() {
            return;
        }
        let doomed = Doomed {
            raw: self.importer.raw.clone(),
            imports,
            view: vk::ImageView::null(),
            framebuffer: vk::Framebuffer::null(),
            _texture: None,
            _device: self.importer._device.clone(),
        };
        self.importer
            .queue
            .on_submitted_work_done(move || drop(doomed));
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        let doomed = Doomed {
            raw: self.importer.raw.clone(),
            imports: std::mem::take(&mut self.imports),
            view: self.view,
            framebuffer: self.framebuffer,
            // The image outlives the view made over it.
            _texture: Some(self.texture.clone()),
            _device: self.importer._device.clone(),
        };
        self.importer
            .queue
            .on_submitted_work_done(move || drop(doomed));
    }
}

/// What a target leaves behind, destroyed when dropped: after the GPU is done with it.
/// It holds the device, not the importer: the queue holding it must not hold the importer
/// that holds the queue.
struct Doomed {
    raw: ash::Device,
    imports: Vec<Imported>,
    view: vk::ImageView,
    framebuffer: vk::Framebuffer,
    _texture: Option<wgpu::Texture>,
    /// Dropped last: the device outlives what is destroyed on it.
    _device: wgpu::Device,
}

unsafe impl Send for Doomed {}

impl Drop for Doomed {
    fn drop(&mut self) {
        let raw = &self.raw;
        unsafe {
            for imported in &mut self.imports {
                imported.destroy(raw);
            }
            if self.framebuffer != vk::Framebuffer::null() {
                raw.destroy_framebuffer(self.framebuffer, None);
            }
            if self.view != vk::ImageView::null() {
                raw.destroy_image_view(self.view, None);
            }
        }
    }
}

fn warn_once(message: &str) {
    static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());
    if let Ok(mut said) = SAID.lock() {
        if !said.iter().any(|s| s == message) {
            log::warn!("{message}");
            said.push(message.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_crop_is_in_texture_coordinates() {
        assert_eq!(
            crop_rect((320, 256), [0, 0, 320, 240]),
            [0.0, 0.0, 1.0, 0.9375]
        );
        assert_eq!(
            crop_rect((100, 100), [10, 20, 60, 70]),
            [0.1, 0.2, 0.5, 0.5]
        );
    }

    #[test]
    fn ycbcr_formats_are_told_apart() {
        assert!(is_ycbcr(vk::Format::G8_B8R8_2PLANE_420_UNORM));
        assert!(is_ycbcr(vk::Format::G8_B8_R8_3PLANE_420_UNORM));
        assert!(!is_ycbcr(vk::Format::R8G8B8A8_UNORM));
        assert!(!is_ycbcr(vk::Format::UNDEFINED));
    }

    #[test]
    fn colour_spaces_pick_the_matching_conversion() {
        assert_eq!(
            ycbcr_model(YuvColorSpace::Bt709Full),
            (
                vk::SamplerYcbcrModelConversion::YCBCR_709,
                vk::SamplerYcbcrRange::ITU_FULL
            )
        );
        assert_eq!(
            ycbcr_model(YuvColorSpace::Bt601Limited).1,
            vk::SamplerYcbcrRange::ITU_NARROW
        );
    }
}
