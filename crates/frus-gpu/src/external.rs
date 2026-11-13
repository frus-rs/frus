// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **External texture frames** on the GPU (milestone 793).
//!
//! A frame pushed into the [texture registry](frus_core::textures) is drawn like any image:
//! the image painter samples one RGBA texture per external texture, cached under the
//! texture's id. What this module adds is keeping that texture **current**:
//!
//! - a new frame of the same size and layout is written into the textures already there —
//!   no allocation, no new bind group;
//! - an RGBA frame is written straight into the sampled texture;
//! - a BGRA, NV12 or I420 frame is written into plane textures of its own layout, then
//!   turned into RGBA by one pass of `external.wgsl` (the YUV to RGB conversion runs on
//!   the GPU, with the frame's colour space);
//! - a frame with an [orientation](frus_core::FrameOrientation) (milestone 798) is turned in
//!   that same pass — an RGBA one goes through it too — into a texture of the size it is drawn
//!   at, so a camera's sideways frames cost nothing more than upright ones.
//! - a frame in a hardware buffer (Android) is not copied at all: the buffer is imported
//!   into Vulkan and drawn into the RGBA texture by the pass of `hardware.rs`
//!   (milestone 800).

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use frus_core::{FrameOrientation, PixelFormat, TextureFrame, YuvColorSpace};
use wgpu::util::DeviceExt;

use crate::mipmap;

/// The conversion pass's parameters: see `external.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
struct Params {
    mode: u32,
    /// Clockwise quarter turns, `0..4`.
    turns: u32,
    /// `1` when mirrored left to right before the turn.
    mirrored: u32,
    _pad: u32,
    row_r: [f32; 4],
    row_g: [f32; 4],
    row_b: [f32; 4],
    offset: [f32; 4],
}

/// The matrix rows and the offsets that take `(Y, U, V)`, each in `0..1`, to non-linear
/// RGB: `rgb = rows · (yuv - offset)`.
fn yuv_matrix(space: YuvColorSpace) -> ([[f32; 4]; 3], [f32; 4]) {
    let (kr_v, kg_u, kg_v, kb_u) = match space {
        YuvColorSpace::Bt601Limited | YuvColorSpace::Bt601Full => {
            (1.402, -0.344_136, -0.714_136, 1.772)
        }
        YuvColorSpace::Bt709Limited | YuvColorSpace::Bt709Full => {
            (1.5748, -0.187_324, -0.468_124, 1.8556)
        }
    };
    let limited = matches!(
        space,
        YuvColorSpace::Bt601Limited | YuvColorSpace::Bt709Limited
    );
    // Limited range: luma spans 16–235 and chroma 16–240 of 255.
    let (ys, cs, y0) = if limited {
        (255.0 / 219.0, 255.0 / 224.0, 16.0 / 255.0)
    } else {
        (1.0, 1.0, 0.0)
    };
    let c0 = 128.0 / 255.0;
    (
        [
            [ys, 0.0, kr_v * cs, 0.0],
            [ys, kg_u * cs, kg_v * cs, 0.0],
            [ys, kb_u * cs, 0.0, 0.0],
        ],
        [y0, c0, c0, 0.0],
    )
}

/// The CPU twin of the shader's conversion, for the tests: 8-bit YUV to 8-bit sRGB.
#[cfg(test)]
fn yuv_to_rgb(y: u8, u: u8, v: u8, space: YuvColorSpace) -> [u8; 3] {
    let (rows, offset) = yuv_matrix(space);
    let yuv = [
        f32::from(y) / 255.0 - offset[0],
        f32::from(u) / 255.0 - offset[1],
        f32::from(v) / 255.0 - offset[2],
    ];
    rows.map(|row| {
        let value = row[0] * yuv[0] + row[1] * yuv[1] + row[2] * yuv[2];
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    })
}

fn params(frame: &TextureFrame) -> Params {
    let mode = match frame.format() {
        PixelFormat::Bgra8 => 0,
        PixelFormat::Nv12 => 1,
        PixelFormat::I420 => 2,
        // Only when turned: an upright RGBA frame is written straight into its texture.
        PixelFormat::Rgba8 => 3,
    };
    let (rows, offset) = yuv_matrix(frame.color_space());
    let orientation = frame.orientation();
    Params {
        mode,
        turns: orientation.quarter_turns(),
        mirrored: orientation.mirrored() as u32,
        _pad: 0,
        row_r: rows[0],
        row_g: rows[1],
        row_b: rows[2],
        offset,
    }
}

/// The plane texture format for a frame's plane `index`.
fn plane_format(format: PixelFormat, index: usize) -> wgpu::TextureFormat {
    match (format, index) {
        (PixelFormat::Rgba8, _) => mipmap::FORMAT,
        // Decoded to linear by the load, as RGBA would be: only the order changes.
        (PixelFormat::Bgra8, _) => wgpu::TextureFormat::Rgba8UnormSrgb,
        (PixelFormat::Nv12, 1) => wgpu::TextureFormat::Rg8Unorm,
        _ => wgpu::TextureFormat::R8Unorm,
    }
}

/// What decides whether a slot's textures fit a frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Shape {
    width: u32,
    height: u32,
    format: PixelFormat,
    orientation: FrameOrientation,
    /// The frame lies in a hardware buffer, imported rather than copied.
    hardware: bool,
}

impl Shape {
    fn of(frame: &TextureFrame) -> Self {
        Self {
            width: frame.width(),
            height: frame.height(),
            format: frame.format(),
            orientation: frame.orientation(),
            hardware: frame.hardware_buffer().is_some(),
        }
    }

    /// Whether the frame goes through the conversion pass: every layout but an upright RGBA
    /// one, which is written straight into the sampled texture.
    fn converted(&self) -> bool {
        self.format != PixelFormat::Rgba8 || !self.orientation.is_upright()
    }
}

/// The GPU side of one external texture: the RGBA texture the image painter samples, and
/// for a converted layout its planes and the pass that converts them.
pub(crate) struct ExternalSlot {
    shape: Shape,
    texture: wgpu::Texture,
    planes: Vec<wgpu::Texture>,
    convert: Option<(wgpu::Buffer, wgpu::BindGroup, Params)>,
    /// For a frame in a hardware buffer: what imports it and draws it into `texture`.
    #[cfg(all(target_os = "android", feature = "hardware-buffers"))]
    hardware: Option<crate::hardware::Target>,
    /// The generation of the frame the texture holds.
    pub(crate) generation: u64,
}

impl ExternalSlot {
    /// The view the image painter samples.
    pub(crate) fn view(&self) -> wgpu::TextureView {
        self.texture
            .create_view(&wgpu::TextureViewDescriptor::default())
    }
}

/// The conversion pipeline, made the first time a frame needs it.
pub(crate) struct ExternalUploader {
    convert: Option<Converter>,
    /// What imports hardware buffers on this device, looked for at the first such frame:
    /// `Some(None)` when the device cannot.
    #[cfg(all(target_os = "android", feature = "hardware-buffers"))]
    importer: Option<Option<Arc<crate::hardware::Importer>>>,
}

struct Converter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    /// Bound where a layout has no third (or second) plane.
    blank: wgpu::Texture,
}

impl ExternalUploader {
    pub(crate) fn new() -> Self {
        Self {
            convert: None,
            #[cfg(all(target_os = "android", feature = "hardware-buffers"))]
            importer: None,
        }
    }

    /// Brings `slot` up to `frame`: writes it into the textures there when they fit,
    /// makes new ones when they do not. Answers whether the sampled texture is a new one —
    /// its bind groups must then be made again — or `None` when the frame cannot be shown
    /// here (a hardware buffer this device cannot import), the slot then left as it was.
    pub(crate) fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut Option<ExternalSlot>,
        frame: &Arc<TextureFrame>,
    ) -> Option<bool> {
        if frame.hardware_buffer().is_some() {
            #[cfg(all(target_os = "android", feature = "hardware-buffers"))]
            return self.update_hardware(device, queue, slot, frame);
            #[cfg(not(all(target_os = "android", feature = "hardware-buffers")))]
            return None;
        }
        let shape = Shape::of(frame);
        let fresh = slot.as_ref().is_none_or(|s| s.shape != shape);
        if fresh {
            *slot = Some(self.make(device, shape));
        }
        let slot = slot.as_mut().expect("made above");
        if !shape.converted() {
            write_plane(queue, &slot.texture, frame, 0);
        } else {
            for (index, plane) in slot.planes.iter().enumerate() {
                write_plane(queue, plane, frame, index);
            }
            let converter = self.convert.as_ref().expect("made with the slot");
            let wanted = params(frame);
            if let Some((buffer, _, current)) = slot.convert.as_mut() {
                if *current != wanted {
                    queue.write_buffer(buffer, 0, bytemuck::bytes_of(&wanted));
                    *current = wanted;
                }
            }
            let (_, bind_group, _) = slot.convert.as_ref().expect("made with the slot");
            converter.run(device, queue, bind_group, &slot.texture);
        }
        slot.generation = frame.generation();
        Some(fresh)
    }

    /// [`ExternalUploader::update`] for a frame in a hardware buffer: imported and drawn into
    /// the slot's texture, no pixel copied by the CPU.
    #[cfg(all(target_os = "android", feature = "hardware-buffers"))]
    fn update_hardware(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: &mut Option<ExternalSlot>,
        frame: &Arc<TextureFrame>,
    ) -> Option<bool> {
        let importer = self
            .importer
            .get_or_insert_with(|| crate::hardware::Importer::new(device, queue))
            .clone();
        let Some(importer) = importer else {
            log::warn!("a hardware buffer frame reached a renderer that cannot import it");
            return None;
        };
        let shape = Shape::of(frame);
        let fresh = slot.as_ref().is_none_or(|s| s.shape != shape);
        if fresh {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("frus.external.hardware"),
                size: wgpu::Extent3d {
                    width: shape.width,
                    height: shape.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: mipmap::FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                // The import pass writes the sRGB bytes as they are, through a plain view.
                view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
            });
            // Made, cleared and made ready for sampling by wgpu itself, once: from then on
            // the import pass leaves it the way wgpu believes it is.
            self.prime(device, queue, &texture);
            let target =
                crate::hardware::Target::new(&importer, &texture, shape.width, shape.height)?;
            *slot = Some(ExternalSlot {
                shape,
                texture,
                planes: Vec::new(),
                convert: None,
                hardware: Some(target),
                generation: 0,
            });
        }
        let slot = slot.as_mut()?;
        slot.hardware.as_mut()?.draw(device, frame);
        // A frame that could not be drawn is not tried again: it said why, once.
        slot.generation = frame.generation();
        Some(fresh)
    }

    /// Samples `texture` once through wgpu, into a 1×1 picture nobody looks at: wgpu then
    /// holds it initialised and ready for sampling.
    #[cfg(all(target_os = "android", feature = "hardware-buffers"))]
    fn prime(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) {
        let converter = self.convert.get_or_insert_with(|| Converter::new(device));
        let params = Params {
            mode: u32::MAX,
            ..Params::zeroed()
        };
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("frus.external.prime"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let blank = converter
            .blank
            .create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frus.external.prime"),
            layout: &converter.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&blank),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&blank),
                },
            ],
        });
        let target = new_texture(
            device,
            "frus.external.prime_target",
            1,
            1,
            mipmap::FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        converter.run(device, queue, &bind_group, &target);
    }

    fn make(&mut self, device: &wgpu::Device, shape: Shape) -> ExternalSlot {
        let converted = shape.converted();
        let mut usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        if converted {
            usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
        }
        // The size the frame is drawn at: its sides swapped by an odd number of quarter turns.
        let (width, height) = shape.orientation.size(shape.width, shape.height);
        let texture = new_texture(
            device,
            "frus.external.texture",
            width,
            height,
            mipmap::FORMAT,
            usage,
        );
        let mut planes = Vec::new();
        let mut convert = None;
        if converted {
            let count = match shape.format {
                PixelFormat::Nv12 => 2,
                PixelFormat::I420 => 3,
                _ => 1,
            };
            let (cw, ch) = (shape.width.div_ceil(2), shape.height.div_ceil(2));
            for index in 0..count {
                let (w, h) = if index == 0 {
                    (shape.width, shape.height)
                } else {
                    (cw, ch)
                };
                planes.push(new_texture(
                    device,
                    "frus.external.plane",
                    w,
                    h,
                    plane_format(shape.format, index),
                    wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                ));
            }
            let converter = self.convert.get_or_insert_with(|| Converter::new(device));
            let params = Params {
                mode: u32::MAX,
                ..Params::zeroed()
            };
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("frus.external.params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            let views: Vec<wgpu::TextureView> = (0..3)
                .map(|i| {
                    planes
                        .get(i)
                        .unwrap_or(&converter.blank)
                        .create_view(&wgpu::TextureViewDescriptor::default())
                })
                .collect();
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("frus.external.bind_group"),
                layout: &converter.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&views[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&views[1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&views[2]),
                    },
                ],
            });
            convert = Some((buffer, bind_group, params));
        }
        ExternalSlot {
            shape,
            texture,
            planes,
            convert,
            #[cfg(all(target_os = "android", feature = "hardware-buffers"))]
            hardware: None,
            generation: 0,
        }
    }
}

fn new_texture(
    device: &wgpu::Device,
    label: &'static str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// Writes plane `index` of `frame` into `texture`, which has its size.
fn write_plane(queue: &wgpu::Queue, texture: &wgpu::Texture, frame: &TextureFrame, index: usize) {
    let Some(plane) = frame.plane(index) else {
        return;
    };
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        plane.data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(plane.width * plane.bytes_per_texel),
            rows_per_image: Some(plane.height),
        },
        wgpu::Extent3d {
            width: plane.width,
            height: plane.height,
            depth_or_array_layers: 1,
        },
    );
}

impl Converter {
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("frus.external.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/external.wgsl").into()),
        });
        let plane = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frus.external.bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                plane(1),
                plane(2),
                plane(3),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("frus.external.pipeline_layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("frus.external.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: mipmap::FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let blank = new_texture(
            device,
            "frus.external.blank",
            1,
            1,
            wgpu::TextureFormat::R8Unorm,
            wgpu::TextureUsages::TEXTURE_BINDING,
        );
        Self {
            pipeline,
            layout,
            blank,
        }
    }

    /// Converts the planes bound in `bind_group` into `target`, and submits the pass: the
    /// plane writes queued before it run first.
    fn run(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bind_group: &wgpu::BindGroup,
        target: &wgpu::Texture,
    ) {
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frus.external.encoder"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frus.external.pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit(Some(encoder.finish()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_range_black_and_white_reach_the_ends() {
        for space in [YuvColorSpace::Bt601Limited, YuvColorSpace::Bt709Limited] {
            assert_eq!(yuv_to_rgb(16, 128, 128, space), [0, 0, 0], "{space:?}");
            assert_eq!(
                yuv_to_rgb(235, 128, 128, space),
                [255, 255, 255],
                "{space:?}"
            );
        }
        assert_eq!(yuv_to_rgb(0, 128, 128, YuvColorSpace::Bt601Full), [0, 0, 0]);
        assert_eq!(
            yuv_to_rgb(255, 128, 128, YuvColorSpace::Bt601Full),
            [255, 255, 255]
        );
    }

    #[test]
    fn the_primaries_come_out_where_the_standards_put_them() {
        // BT.601 limited red, green and blue, as an encoder writes them.
        let close = |got: [u8; 3], want: [u8; 3]| {
            got.iter()
                .zip(want)
                .all(|(&g, w)| (i32::from(g) - i32::from(w)).abs() <= 2)
        };
        let red = yuv_to_rgb(81, 90, 240, YuvColorSpace::Bt601Limited);
        assert!(close(red, [255, 0, 0]), "{red:?}");
        let green = yuv_to_rgb(145, 54, 34, YuvColorSpace::Bt601Limited);
        assert!(close(green, [0, 255, 0]), "{green:?}");
        let blue = yuv_to_rgb(41, 240, 110, YuvColorSpace::Bt601Limited);
        assert!(close(blue, [0, 0, 255]), "{blue:?}");
        // BT.709 limited red.
        let red = yuv_to_rgb(63, 102, 240, YuvColorSpace::Bt709Limited);
        assert!(close(red, [255, 0, 0]), "{red:?}");
    }

    #[test]
    fn turned_frames_are_converted_into_a_texture_of_their_drawn_size() {
        let shape = |format, turns| Shape {
            width: 640,
            height: 480,
            format,
            orientation: FrameOrientation::new(turns, false),
        };
        assert!(!shape(PixelFormat::Rgba8, 0).converted());
        assert!(shape(PixelFormat::Rgba8, 1).converted());
        assert!(shape(PixelFormat::Nv12, 0).converted());
        let turned = shape(PixelFormat::I420, 1);
        assert_eq!(
            turned.orientation.size(turned.width, turned.height),
            (480, 640)
        );
    }

    #[test]
    fn the_params_are_sized_for_webgl() {
        // WebGL 2 binds uniforms in whole 16-byte rows.
        assert_eq!(std::mem::size_of::<Params>() % 16, 0);
    }
}
