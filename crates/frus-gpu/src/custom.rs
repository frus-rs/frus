// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Custom shaders and GPU passes** (milestone 792): the layers whose colour is an
//! application's fragment shader, or a picture the application drew on the GPU itself.
//!
//! Both are composited as every layer is — the same quad, the same instance (clip, shape,
//! transform, opacity), the same texture bind group for the child — through a pipeline of
//! their own made from the application's WGSL and frus's frame around it
//! (`frus-core/src/shaders/frame.wgsl`). The frame's third bind group holds what the
//! application adds: its uniforms, up to four images, and for a GPU pass the picture.
//!
//! Nothing here exists until a shader is loaded: no pipeline, no buffer beyond a few
//! bytes, no texture, no thread. A pipeline is made **ahead** (milestone 800): when a
//! renderer sees a newly loaded shader, at the start of a frame, it compiles the pipeline
//! on a short-lived thread of its own, so the first frame that shows the shader does not
//! wait for the driver; if that frame comes before the thread is done, it waits for that
//! compilation rather than starting another. Pipelines are kept for as long as the
//! renderer lives, and made through the device's pipeline cache where there is one, so the
//! next launch finds them compiled.

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
#[cfg(feature = "warm-shaders")]
use std::sync::{Mutex, OnceLock};

use bytemuck::{Pod, Zeroable};
use frus_core::{ExternalPass, Rect, Scene, Shader, ShaderPaint, MAX_SHADER_IMAGES};

/// What the frame's `FrusShaderFrame` holds: the box, the child's region, and
/// `(time, pixel ratio, blend mode, flags)`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq)]
pub(crate) struct FrameParams {
    pub(crate) rect: [f32; 4],
    pub(crate) region: [f32; 4],
    pub(crate) info: [f32; 4],
    _pad: [f32; 4],
}

/// Flag: the layer has content, which the shader reads as `child`.
const HAS_CHILD: u32 = 1;
/// Flag: the colour is a GPU pass's picture, in `image0`.
const EXTERNAL: u32 = 2;

impl FrameParams {
    pub(crate) fn new(
        rect: Rect,
        region: [f32; 4],
        time: f32,
        ratio: f32,
        blend: u32,
        flags: u32,
    ) -> FrameParams {
        FrameParams {
            rect: rect.to_array(),
            region,
            info: [time, ratio, blend as f32, flags as f32],
            _pad: [0.0; 4],
        }
    }
}

/// One composite draw that goes through a custom pipeline: which shader, the numbers, and
/// the textures for its third bind group.
pub(crate) struct CustomDraw {
    shader: Shader,
    frame: FrameParams,
    uniforms: Vec<u8>,
    images: [Option<wgpu::TextureView>; MAX_SHADER_IMAGES],
}

impl CustomDraw {
    /// A shader layer: `paint` over the layer's content, `has_child` when it has some,
    /// whose texture lies at `region` of the target. `images` are the paint's images,
    /// already on the GPU.
    pub(crate) fn shader(
        paint: &ShaderPaint,
        has_child: bool,
        region: [f32; 4],
        images: [Option<wgpu::TextureView>; MAX_SHADER_IMAGES],
    ) -> CustomDraw {
        let blend = paint.blend_or(frus_core::BlendMode::Modulate).code();
        CustomDraw {
            shader: paint.shader().clone(),
            frame: FrameParams::new(
                paint.rect(),
                region,
                paint.seconds(),
                paint.pixel_ratio(),
                blend,
                if has_child { HAS_CHILD } else { 0 },
            ),
            uniforms: paint.uniform_bytes().to_vec(),
            images,
        }
    }

    /// A GPU pass: its picture `view`, filling the pass's box.
    pub(crate) fn external(pass: &ExternalPass, view: wgpu::TextureView) -> CustomDraw {
        let shader = external_shader();
        CustomDraw {
            frame: FrameParams::new(
                pass.rect(),
                [0.0; 4],
                pass.seconds(),
                pass.pixel_ratio(),
                frus_core::BlendMode::Src.code(),
                EXTERNAL,
            ),
            uniforms: vec![0; shader.uniform_size()],
            shader,
            images: [Some(view), None, None, None],
        }
    }
}

/// The shader a GPU pass is composited with: the frame reads the picture itself, so its
/// `shade` is never called.
fn external_shader() -> Shader {
    Shader::from_wgsl("fn shade(frag: Fragment) -> vec4<f32> {\n    return vec4<f32>(0.0);\n}\n")
        .expect("the GPU pass shader compiles")
}

/// What making a custom pipeline needs: everything but the shader. Cloned onto the
/// warm-up thread.
#[derive(Clone)]
struct Builder {
    pipeline_layout: wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    sample_count: u32,
    cache: Option<wgpu::PipelineCache>,
    /// WebGL 2 and OpenGL ES: shaders are translated to GLSL, which is checked first.
    gl: bool,
}

/// A pipeline being made ahead, or made: whoever asks first makes it, a second asker waits
/// for it.
#[cfg(feature = "warm-shaders")]
type Ahead = Arc<OnceLock<Option<wgpu::RenderPipeline>>>;

/// **Compiles shaders ahead of their first frame** (milestone 800): every pipeline of a
/// custom painter is made through it, on the render thread when a frame asks first, or on a
/// short-lived thread of its own as soon as the shader is loaded.
pub(crate) struct Warmer {
    builder: Builder,
    device: wgpu::Device,
    /// Every pipeline made or being made, by shader.
    #[cfg(feature = "warm-shaders")]
    ahead: Mutex<HashMap<u64, Ahead>>,
    /// How many loaded shaders were looked at.
    #[cfg(feature = "warm-shaders")]
    seen: Mutex<usize>,
    /// A pipeline was made since the renderer last wrote its pipeline cache.
    made: Arc<AtomicBool>,
}

impl Warmer {
    /// Starts compiling the shaders loaded since the last call, on a thread that ends when
    /// they are done; answers it, for a test to wait on. Nothing on the Web, whose GPU objects
    /// cannot leave the page's thread: a shader is compiled at its first frame there.
    #[cfg(not(feature = "warm-shaders"))]
    fn warm(&self) -> Option<std::thread::JoinHandle<()>> {
        None
    }

    /// See the other `warm`: this one, with the feature, does the work.
    #[cfg(feature = "warm-shaders")]
    fn warm(&self) -> Option<std::thread::JoinHandle<()>> {
        let work: Vec<(Shader, Ahead)> = {
            let mut seen = self.seen.lock().ok()?;
            let (shaders, now) = Shader::loaded_since(*seen);
            *seen = now;
            if shaders.is_empty() || cfg!(target_arch = "wasm32") {
                return None;
            }
            let mut ahead = self.ahead.lock().ok()?;
            shaders
                .into_iter()
                .filter_map(|shader| {
                    if ahead.contains_key(&shader.id()) {
                        return None;
                    }
                    let slot = Ahead::default();
                    ahead.insert(shader.id(), slot.clone());
                    Some((shader, slot))
                })
                .collect()
        };
        if work.is_empty() {
            return None;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let builder = self.builder.clone();
            let device = self.device.clone();
            let made = self.made.clone();
            std::thread::Builder::new()
                .name("frus-shader-warm-up".into())
                .spawn(move || {
                    for (shader, slot) in work {
                        let pipeline = slot.get_or_init(|| {
                            let started = web_time::Instant::now();
                            let pipeline = builder.build(&device, &shader);
                            log::info!(
                                "shader {:016x}: pipeline compiled ahead in {:.1} ms",
                                shader.id(),
                                started.elapsed().as_secs_f32() * 1000.0
                            );
                            pipeline
                        });
                        if pipeline.is_some() {
                            made.store(true, Ordering::Release);
                        }
                    }
                })
                .ok()
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = work;
            None
        }
    }

    /// Whether a pipeline was made since the last call, here or ahead.
    fn take_made(&self) -> bool {
        self.made.swap(false, Ordering::AcqRel)
    }

    /// `shader`'s pipeline: the one made ahead (waiting for it when it is still being made),
    /// or made now, on this thread.
    #[cfg(not(feature = "warm-shaders"))]
    fn pipeline(&self, shader: &Shader) -> Option<wgpu::RenderPipeline> {
        let made = self.builder.build(&self.device, shader);
        if made.is_some() {
            self.made.store(true, Ordering::Release);
        }
        made
    }

    /// See the other `pipeline`: this one takes what was made ahead.
    #[cfg(feature = "warm-shaders")]
    fn pipeline(&self, shader: &Shader) -> Option<wgpu::RenderPipeline> {
        let (slot, sent_ahead) = {
            let mut ahead = self.ahead.lock().ok()?;
            match ahead.get(&shader.id()) {
                Some(slot) => (slot.clone(), true),
                None => {
                    let slot = Ahead::default();
                    ahead.insert(shader.id(), slot.clone());
                    (slot, false)
                }
            }
        };
        let ready = slot.get().is_some();
        let started = web_time::Instant::now();
        let mut built_here = false;
        let made = slot
            .get_or_init(|| {
                built_here = true;
                self.builder.build(&self.device, shader)
            })
            .clone();
        // What the warm-up saved, or did not.
        if !ready {
            let ms = started.elapsed().as_secs_f32() * 1000.0;
            log::info!(
                "shader {:016x}: its first frame waited {ms:.1} ms for its pipeline ({})",
                shader.id(),
                if sent_ahead {
                    "being compiled ahead"
                } else {
                    "not loaded ahead"
                }
            );
        }
        if built_here && made.is_some() {
            self.made.store(true, Ordering::Release);
        }
        made
    }

    /// Whether `shader`'s pipeline is made, without waiting.
    #[cfg(all(test, feature = "warm-shaders"))]
    fn is_ready(&self, shader: &Shader) -> bool {
        self.ahead
            .lock()
            .ok()
            .and_then(|a| a.get(&shader.id()).map(|slot| slot.get().is_some()))
            .unwrap_or(false)
    }
}

/// The custom pipelines, their bind group layout, this frame's uniforms, and the GPU
/// passes' pictures.
pub(crate) struct CustomPainter {
    layout: wgpu::BindGroupLayout,
    /// What makes the pipelines, here or ahead.
    warmer: Arc<Warmer>,
    sampler: wgpu::Sampler,
    /// 1×1 transparent black: an image slot the paint left empty.
    blank: wgpu::TextureView,
    /// By shader: its pipeline, or `None` when it cannot be made on this device (said once).
    /// What the draws read, with no lock; the warmer's map is where they are made.
    pipelines: HashMap<u64, Option<wgpu::RenderPipeline>>,
    buffer: Option<wgpu::Buffer>,
    /// By composite draw: the pipeline's shader and the third bind group.
    groups: Vec<Option<(u64, wgpu::BindGroup)>>,
    /// The GPU passes' pictures and states, by key.
    passes: HashMap<u64, PassSlot>,
}

/// A GPU pass's picture, kept from frame to frame with the state its callback keeps.
struct PassSlot {
    texture: wgpu::Texture,
    state: Option<Box<dyn PassState>>,
    used: bool,
}

impl CustomPainter {
    // On the Web wgpu's objects are not `Send`: the warmer is shared there with nothing but
    // this thread, and with a warm-up thread only on native targets, where they are.
    #[allow(clippy::arc_with_non_send_sync)]
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        sample_count: u32,
        cache: Option<&wgpu::PipelineCache>,
        viewport_layout: &wgpu::BindGroupLayout,
        texture_layout: &wgpu::BindGroupLayout,
    ) -> CustomPainter {
        let uniform = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frus.custom.bgl"),
            entries: &[
                uniform(0),
                uniform(1),
                texture(2),
                texture(3),
                texture(4),
                texture(5),
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("frus.custom.pipeline_layout"),
            bind_group_layouts: &[Some(viewport_layout), Some(texture_layout), Some(&layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("frus.custom.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blank = wgpu::util::DeviceExt::create_texture_with_data(
            device,
            queue,
            &wgpu::TextureDescriptor {
                label: Some("frus.custom.blank"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &[0, 0, 0, 0],
        )
        .create_view(&wgpu::TextureViewDescriptor::default());
        CustomPainter {
            layout,
            warmer: Arc::new(Warmer {
                builder: Builder {
                    pipeline_layout,
                    format,
                    sample_count,
                    cache: cache.cloned(),
                    gl: device.adapter_info().backend == wgpu::Backend::Gl,
                },
                device: device.clone(),
                #[cfg(feature = "warm-shaders")]
                ahead: Mutex::new(HashMap::new()),
                #[cfg(feature = "warm-shaders")]
                seen: Mutex::new(0),
                made: Arc::new(AtomicBool::new(false)),
            }),
            sampler,
            blank,
            pipelines: HashMap::new(),
            buffer: None,
            groups: Vec::new(),
            passes: HashMap::new(),
        }
    }

    /// Whether a pipeline was made since the last call: the pipeline cache is then worth
    /// writing again.
    pub(crate) fn take_new_pipelines(&mut self) -> bool {
        self.warmer.take_made()
    }

    /// Starts compiling the shaders loaded so far, ahead of their first frame, and from now
    /// on every shader **as it is loaded** (milestone 800). Answers the thread compiling
    /// those loaded so far, for a test to wait on.
    pub(crate) fn warm_shaders(&mut self) -> Option<std::thread::JoinHandle<()>> {
        let worker = self.warmer.warm();
        #[cfg(all(feature = "warm-shaders", not(target_arch = "wasm32")))]
        {
            let warmer = Arc::downgrade(&self.warmer);
            Shader::on_load(move || match warmer.upgrade() {
                Some(warmer) => {
                    let _ = warmer.warm();
                    true
                }
                // The renderer is gone: the hook goes too.
                None => false,
            });
        }
        worker
    }

    /// The pipeline of `shader`, made ahead or now; `None` when this device cannot run it,
    /// which is said once.
    fn pipeline(
        &mut self,
        _device: &wgpu::Device,
        shader: &Shader,
    ) -> Option<&wgpu::RenderPipeline> {
        if !self.pipelines.contains_key(&shader.id()) {
            let made = self.warmer.pipeline(shader);
            self.pipelines.insert(shader.id(), made);
        }
        self.pipelines.get(&shader.id()).and_then(|p| p.as_ref())
    }

    /// Whether `shader`'s pipeline is ready without waiting.
    #[cfg(all(test, feature = "warm-shaders"))]
    fn is_ready(&self, shader: &Shader) -> bool {
        self.pipelines.contains_key(&shader.id()) || self.warmer.is_ready(shader)
    }
}

impl Builder {
    fn build(&self, device: &wgpu::Device, shader: &Shader) -> Option<wgpu::RenderPipeline> {
        if self.gl {
            if let Err(why) = gl_check(shader.module_source()) {
                log::error!(
                    "shader {:016x} cannot run on OpenGL ES / WebGL 2 here: {why}",
                    shader.id()
                );
                return None;
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("frus.custom.shader"),
            source: wgpu::ShaderSource::Wgsl(shader.module_source().into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("frus.custom.pipeline"),
            layout: Some(&self.pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("frus_vs"),
                buffers: &[
                    Some(crate::compositor::quad_vertex_layout()),
                    Some(crate::compositor::comp_instance_layout()),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("frus_fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: self.format,
                    // Premultiplied, as every composited layer.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: self.sample_count,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: self.cache.as_ref(),
        });
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(error) = pollster::block_on(scope.pop()) {
            log::error!("shader {:016x} did not compile: {error}", shader.id());
            return None;
        }
        Some(pipeline)
    }
}

impl CustomPainter {
    /// Prepares this frame's custom draws: `draws[i]` is composite draw `i`'s, if it is
    /// one. The uniforms go into one buffer, one aligned slot each.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        draws: &[Option<&CustomDraw>],
    ) {
        self.groups.clear();
        if draws.iter().all(Option::is_none) {
            return;
        }
        let align = device.limits().min_uniform_buffer_offset_alignment.max(16) as usize;
        let slot = |n: usize| n.div_ceil(align) * align;
        let mut bytes: Vec<u8> = Vec::new();
        let mut places = Vec::with_capacity(draws.len());
        for draw in draws {
            match draw {
                Some(d) => {
                    let frame_at = bytes.len();
                    bytes.extend_from_slice(bytemuck::bytes_of(&d.frame));
                    bytes.resize(slot(bytes.len()), 0);
                    let user_at = bytes.len();
                    bytes.extend_from_slice(&d.uniforms);
                    bytes.resize(slot(bytes.len()), 0);
                    places.push(Some((frame_at, user_at)));
                }
                None => places.push(None),
            }
        }
        let size = bytes.len() as u64;
        let stale = self.buffer.as_ref().is_none_or(|b| b.size() < size);
        if stale {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frus.custom.uniforms"),
                size: size.next_power_of_two(),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let buffer = self.buffer.clone().expect("made above");
        queue.write_buffer(&buffer, 0, &bytes);

        for (draw, place) in draws.iter().zip(places) {
            let (Some(d), Some((frame_at, user_at))) = (draw, place) else {
                self.groups.push(None);
                continue;
            };
            if self.pipeline(device, &d.shader).is_none() {
                self.groups.push(None);
                continue;
            }
            let image = |i: usize| d.images[i].as_ref().unwrap_or(&self.blank);
            let slice = |at: usize, len: usize| {
                wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: at as u64,
                    size: wgpu::BufferSize::new(len as u64),
                })
            };
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("frus.custom.bind_group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: slice(frame_at, std::mem::size_of::<FrameParams>()),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: slice(user_at, d.uniforms.len()),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(image(0)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(image(1)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(image(2)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(image(3)),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.groups.push(Some((d.shader.id(), group)));
        }
    }

    /// Whether composite draw `index` is a custom one this frame, ready or not.
    pub(crate) fn takes(&self, index: usize) -> bool {
        self.groups.get(index).is_some_and(Option::is_some)
    }

    /// Binds composite draw `index`'s pipeline and third bind group; `false` when it has
    /// none, and is then not drawn.
    pub(crate) fn bind<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        index: usize,
    ) -> bool {
        let Some(Some((shader, group))) = self.groups.get(index) else {
            return false;
        };
        let Some(Some(pipeline)) = self.pipelines.get(shader) else {
            return false;
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(2, group, &[]);
        true
    }

    /// Runs a GPU pass: its callback draws into the pass's picture, kept from the last
    /// frame when the size is the same, and cleared. `None` when there is nothing to draw:
    /// an empty box, or a payload that is not a [`GpuCallback`].
    pub(crate) fn run_pass(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &ExternalPass,
    ) -> Option<wgpu::TextureView> {
        let Some(callback) = pass.payload().downcast_ref::<GpuCallback>() else {
            log::warn!("a GPU pass with no frus_gpu::GpuCallback in it: not drawn");
            return None;
        };
        let rect = pass.rect();
        let max = device.limits().max_texture_dimension_2d;
        let width = (rect.width.round() as u32).min(max);
        let height = (rect.height.round() as u32).min(max);
        if width == 0 || height == 0 {
            return None;
        }
        let stale = self
            .passes
            .get(&pass.key())
            .is_none_or(|s| s.texture.width() != width || s.texture.height() != height);
        if stale {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("frus.gpu_pass"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: GPU_PASS_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            // The state lives on, whatever the size: what it holds is the application's.
            let state = self.passes.remove(&pass.key()).and_then(|s| s.state);
            self.passes.insert(
                pass.key(),
                PassSlot {
                    texture,
                    state,
                    used: false,
                },
            );
        }
        let slot = self.passes.get_mut(&pass.key()).expect("made above");
        slot.used = true;
        let view = slot
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frus.encoder.gpu_pass"),
        });
        // Cleared to transparent: what the callback draws is all there is.
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("frus.gpu_pass.clear"),
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
        let mut context = GpuPassContext {
            device,
            queue,
            encoder: &mut encoder,
            target: &view,
            texture: &slot.texture,
            format: GPU_PASS_FORMAT,
            width,
            height,
            pixel_ratio: pass.pixel_ratio(),
            time: pass.seconds(),
            state: &mut slot.state,
        };
        (callback.draw)(&mut context);
        queue.submit(std::iter::once(encoder.finish()));
        Some(view)
    }

    /// Calls `each` with every GPU pass's picture, for the memory count (milestone 796).
    pub(crate) fn each_texture(&self, mut each: impl FnMut(&wgpu::Texture)) {
        for slot in self.passes.values() {
            each(&slot.texture);
        }
    }

    /// Ends a frame: the pictures of the GPU passes no longer drawn go.
    pub(crate) fn end_frame(&mut self) {
        self.passes.retain(|_, s| s.used);
        for slot in self.passes.values_mut() {
            slot.used = false;
        }
    }
}

/// Whether `source` translates to GLSL ES 3.0 for WebGL 2, as wgpu does on OpenGL ES and in
/// a browser without WebGPU: the reason when it does not.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub(crate) fn gl_check(source: &str) -> Result<(), String> {
    use naga::back::glsl;
    let module = naga::front::wgsl::parse_str(source).map_err(|e| e.to_string())?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| e.to_string())?;
    let options = glsl::Options {
        version: glsl::Version::Embedded {
            version: 300,
            is_webgl: true,
        },
        ..Default::default()
    };
    for (stage, entry) in [
        (naga::ShaderStage::Vertex, "frus_vs"),
        (naga::ShaderStage::Fragment, "frus_fs"),
    ] {
        let pipeline = glsl::PipelineOptions {
            shader_stage: stage,
            entry_point: entry.into(),
            multiview: None,
        };
        let mut out = String::new();
        glsl::Writer::new(
            &mut out,
            &module,
            &info,
            &options,
            &pipeline,
            naga::proc::BoundsCheckPolicies::default(),
        )
        .and_then(|mut w| w.write().map(|_| ()))
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Apple's systems draw through Metal, never through OpenGL ES.
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub(crate) fn gl_check(_source: &str) -> Result<(), String> {
    Ok(())
}

/// The format of a GPU pass's picture: 8 bits a channel, encoded as sRGB by the hardware,
/// so a pass writes **linear** values, premultiplied by alpha, as frus's own drawing does.
pub const GPU_PASS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// What a GPU pass may keep from one frame to the next: anything it owns.
#[cfg(not(target_arch = "wasm32"))]
pub trait PassState: Any + Send + Sync {
    /// Itself, to be read back as what it is.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Any + Send + Sync> PassState for T {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// What a GPU pass may keep from one frame to the next: anything it owns. (On the Web,
/// where the GPU's objects belong to the page's one thread, it need not be `Send`.)
#[cfg(target_arch = "wasm32")]
pub trait PassState: Any {
    /// Itself, to be read back as what it is.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[cfg(target_arch = "wasm32")]
impl<T: Any> PassState for T {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// **What a GPU pass draws with**: frus's own device and queue, an encoder frus submits
/// after the callback, and the picture to draw into — the pass's box, in physical pixels,
/// cleared to transparent. It is composited like an image afterwards: clipped, faded and
/// transformed with the widgets around it.
///
/// Draw **premultiplied linear** colours into [`GpuPassContext::target`]
/// ([`GPU_PASS_FORMAT`], sRGB-encoded by the hardware). Pipelines, buffers and anything
/// else worth keeping go in [`GpuPassContext::state`], made once.
pub struct GpuPassContext<'a> {
    /// frus's device: make pipelines, buffers and textures with it.
    pub device: &'a wgpu::Device,
    /// frus's queue: write buffers with it, or submit work of your own.
    pub queue: &'a wgpu::Queue,
    /// An encoder frus submits when the callback returns; the picture was cleared in it.
    pub encoder: &'a mut wgpu::CommandEncoder,
    /// The picture, as a render target.
    pub target: &'a wgpu::TextureView,
    /// The picture's texture: a copy destination, too.
    pub texture: &'a wgpu::Texture,
    /// The picture's format, [`GPU_PASS_FORMAT`].
    pub format: wgpu::TextureFormat,
    /// Its width in physical pixels.
    pub width: u32,
    /// Its height in physical pixels.
    pub height: u32,
    /// Physical pixels per logical pixel.
    pub pixel_ratio: f32,
    /// Seconds: the widget's clock while it animates.
    pub time: f32,
    state: &'a mut Option<Box<dyn PassState>>,
}

impl GpuPassContext<'_> {
    /// **What this pass keeps between frames**: made by `init` the first time (or when a
    /// value of another type was kept), then the same value every frame after.
    ///
    /// ```ignore
    /// let pipeline = ctx.state(|device, format| make_triangle_pipeline(device, format));
    /// ```
    pub fn state<T: PassState>(
        &mut self,
        init: impl FnOnce(&wgpu::Device, wgpu::TextureFormat) -> T,
    ) -> &mut T {
        let keep = self
            .state
            .as_mut()
            .is_some_and(|s| (**s).as_any_mut().downcast_mut::<T>().is_some());
        if !keep {
            *self.state = Some(Box::new(init(self.device, self.format)));
        }
        self.state
            .as_mut()
            .and_then(|s| (**s).as_any_mut().downcast_mut::<T>())
            .expect("kept above")
    }
}

/// The function a GPU pass runs.
type PassFn = dyn Fn(&mut GpuPassContext<'_>) + Send + Sync;

/// **A GPU pass's drawing**: a function given frus's device and a picture to draw into
/// ([`GpuPassContext`]), run each frame the pass is drawn. Paint it into a scene with
/// [`GpuCallback::paint`]; the `GpuPass` widget does that from its box.
///
/// ```no_run
/// use frus_gpu::{GpuCallback, wgpu};
///
/// let clear_blue = GpuCallback::new(|ctx| {
///     let _pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
///         label: None,
///         color_attachments: &[Some(wgpu::RenderPassColorAttachment {
///             view: ctx.target,
///             depth_slice: None,
///             resolve_target: None,
///             ops: wgpu::Operations {
///                 load: wgpu::LoadOp::Clear(wgpu::Color::BLUE),
///                 store: wgpu::StoreOp::Store,
///             },
///         })],
///         depth_stencil_attachment: None,
///         timestamp_writes: None,
///         occlusion_query_set: None,
///         multiview_mask: None,
///     });
/// });
/// ```
#[derive(Clone)]
pub struct GpuCallback {
    key: u64,
    draw: Arc<PassFn>,
}

impl GpuCallback {
    /// A pass drawn by `draw`. Its identity — which picture and which state are its from
    /// one frame to the next — is the function's own type, so one written in one place is
    /// the same pass every frame; several from the same place tell themselves apart with
    /// [`GpuCallback::key`].
    pub fn new<F>(draw: F) -> GpuCallback
    where
        F: Fn(&mut GpuPassContext<'_>) + Send + Sync + 'static,
    {
        use std::hash::{Hash, Hasher};
        // The shader a pass is composited with, loaded with the first pass made: its pipeline
        // then compiles ahead, while the frame that shows the pass is built (milestone 800).
        #[cfg(feature = "warm-shaders")]
        {
            static COMPOSITING: std::sync::Once = std::sync::Once::new();
            COMPOSITING.call_once(|| {
                let _ = external_shader();
            });
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::any::TypeId::of::<F>().hash(&mut hasher);
        GpuCallback {
            key: hasher.finish(),
            draw: Arc::new(draw),
        }
    }

    /// Tells this pass apart from others drawn by the same function: a list of them.
    pub fn key(mut self, key: u64) -> GpuCallback {
        self.key = self.key.rotate_left(17) ^ key.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        self
    }

    /// The pass's identity.
    pub fn id(&self) -> u64 {
        self.key
    }

    /// The pass in `rect` (logical pixels) at `time`, as a scene carries it.
    pub fn pass(&self, rect: Rect, time: f32) -> ExternalPass {
        ExternalPass::new(self.key, rect, Arc::new(self.clone())).time(time)
    }

    /// Paints the pass into `scene`, filling `rect`.
    pub fn paint(&self, scene: &mut Scene, rect: Rect, time: f32) {
        scene.external_pass(self.pass(rect, time));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOISE: &str = "struct Uniforms { scale: f32, tint: vec4<f32> };

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn shade(frag: Fragment) -> vec4<f32> {
    let n = hash(floor(frag.position / uniforms.scale) + vec2<f32>(frag.time));
    let c = child(frag.uv);
    let i = image0(frag.uv);
    return vec4<f32>(uniforms.tint.rgb * n + c.rgb * 0.0 + i.rgb * 0.0, 1.0);
}
";

    /// What WebGL 2 and OpenGL ES run: the frame and an application's shader that uses
    /// all of it — uniforms, the child, an image, a loop of maths — translate to GLSL ES
    /// 3.0, as wgpu translates them in a browser without WebGPU.
    #[test]
    fn a_shader_translates_for_webgl() {
        let shader = Shader::from_wgsl(NOISE).unwrap();
        gl_check(shader.module_source()).unwrap();
        gl_check(external_shader().module_source()).unwrap();
    }

    /// What GLSL ES cannot say is a clear error, not a panic at draw time.
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    #[test]
    fn what_webgl_cannot_run_is_refused_with_a_reason() {
        // A storage buffer: valid WGSL, and nothing GLSL ES 3.0 has.
        let shader = Shader::from_wgsl(
            "@group(3) @binding(0) var<storage, read> data: array<f32>;\n\
             fn shade(frag: Fragment) -> vec4<f32> {\n    return vec4<f32>(data[0]);\n}\n",
        )
        .unwrap();
        let why = gl_check(shader.module_source()).unwrap_err();
        assert!(!why.is_empty());
    }

    #[test]
    fn a_frame_is_what_the_wgsl_reads() {
        // Four vec4s, 64 bytes, as `FrusShaderFrame`.
        assert_eq!(std::mem::size_of::<FrameParams>(), 64);
        let f = FrameParams::new(Rect::new(1.0, 2.0, 3.0, 4.0), [5.0; 4], 6.0, 2.0, 12, 1);
        assert_eq!(f.rect, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(f.info, [6.0, 2.0, 12.0, 1.0]);
    }

    fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: None,
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("frus.test.device"),
            required_limits: wgpu::Limits::downlevel_defaults(),
            ..Default::default()
        }))
        .ok()
    }

    /// Milestone 800: a shader loaded before its first frame has its pipeline compiled on a
    /// thread of the renderer's, and the frame that draws it finds it there.
    #[cfg(feature = "warm-shaders")]
    #[test]
    fn a_loaded_shader_is_compiled_ahead_of_its_first_frame() {
        let Some((device, queue)) = headless_device() else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };
        let mut painters = crate::compositor::Painters::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            1,
            None,
        );
        let custom = painters.custom_mut();
        // What other tests loaded first.
        if let Some(worker) = custom.warmer.warm() {
            worker.join().unwrap();
        }
        let _ = custom.take_new_pipelines();
        let shader = Shader::from_wgsl(
            "fn shade(frag: Fragment) -> vec4<f32> {
    return vec4<f32>(frag.uv, 0.8, 1.0);
}
",
        )
        .unwrap();
        assert!(
            !custom.is_ready(&shader),
            "nothing is compiled before a frame starts"
        );
        let worker = custom.warmer.warm().expect("a new shader is sent ahead");
        worker.join().unwrap();
        assert!(
            custom.is_ready(&shader),
            "compiled before any frame drew it"
        );
        assert!(custom.take_new_pipelines(), "the pipeline cache is told");
        // The first frame takes the pipeline made ahead.
        assert!(custom.pipeline(&device, &shader).is_some());
        // Loading it again sends nothing more.
        let again = Shader::from_wgsl(shader.source()).unwrap();
        assert_eq!(again, shader);
    }

    /// Milestone 800: once a renderer warms shaders, one loaded later starts compiling the
    /// moment it is loaded, with no frame asking.
    #[cfg(feature = "warm-shaders")]
    #[test]
    fn a_shader_starts_compiling_as_it_is_loaded() {
        let Some((device, queue)) = headless_device() else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };
        let mut painters = crate::compositor::Painters::new(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            1,
            None,
        );
        let custom = painters.custom_mut();
        if let Some(worker) = custom.warm_shaders() {
            worker.join().unwrap();
        }
        let shader = Shader::from_wgsl(
            "fn shade(frag: Fragment) -> vec4<f32> {
    return vec4<f32>(0.1, frag.uv, 1.0);
}
",
        )
        .unwrap();
        let started = std::time::Instant::now();
        while !custom.is_ready(&shader) {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(30),
                "never compiled ahead"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(custom.pipeline(&device, &shader).is_some());
    }

    #[test]
    fn a_callback_is_the_same_pass_every_frame() {
        fn make() -> GpuCallback {
            GpuCallback::new(|_ctx| {})
        }
        assert_eq!(make().id(), make().id());
        assert_ne!(make().key(1).id(), make().key(2).id());
        let other = GpuCallback::new(|_ctx| {});
        assert_ne!(make().id(), other.id());
    }
}
