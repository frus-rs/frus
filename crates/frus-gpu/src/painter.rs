// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The `Painter`: the primitive drawing logic, independent of any surface or
//! window. It can therefore paint onto a surface or onto an offscreen texture
//! alike, which is what makes rendering testable headlessly.

use bytemuck::{Pod, Zeroable};
use frus_core::{Color, Primitive, Scene};
use std::ops::Range;
use wgpu::util::DeviceExt;

use crate::batch::{Batch, Kind};
use crate::text::DecorationQuad;

/// A vertex of the unit quad, its corner in `[0,1]²`.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadVertex {
    pos: [f32; 2],
}

impl QuadVertex {
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x2];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<QuadVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRS,
        }
    }
}

/// The instance data handed to the GPU, one per rectangle, built from the
/// [`Scene`]'s primitives every frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Instance {
    rect: [f32; 4],
    color: [f32; 4],
    color2: [f32; 4],
    border_color: [f32; 4],
    /// The gradient's kind and tile mode (`kind + 4 × tile`, `0` for none), border_width,
    /// blur, and the gradient's row in the ramp texture.
    params: [f32; 4],
    /// The legacy gradient's direction (x, y); or a gradient's geometry, in pixels from the
    /// rectangle's centre (milestone 644): a line's two ends, a circle's centre, radius and
    /// focal radius, a sweep's centre and two angles.
    gradient: [f32; 4],
    /// The clip rectangle: x, y, width, height.
    clip: [f32; 4],
    /// Corner radii, per corner: tl, tr, br, bl.
    radii: [f32; 4],
}

impl Instance {
    /// The instance buffer's layout (locations 1..=8; 0 is the unit quad).
    fn layout() -> wgpu::VertexBufferLayout<'static> {
        const ATTRS: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
            1 => Float32x4,
            2 => Float32x4,
            3 => Float32x4,
            4 => Float32x4,
            5 => Float32x4,
            6 => Float32x4,
            7 => Float32x4,
            8 => Float32x4,
        ];
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Instance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &ATTRS,
        }
    }
}

/// Two triangles covering the unit quad.
const QUAD_VERTICES: &[QuadVertex] = &[
    QuadVertex { pos: [0.0, 0.0] },
    QuadVertex { pos: [1.0, 0.0] },
    QuadVertex { pos: [1.0, 1.0] },
    QuadVertex { pos: [0.0, 0.0] },
    QuadVertex { pos: [1.0, 1.0] },
    QuadVertex { pos: [0.0, 1.0] },
];
const QUAD_VERTEX_COUNT: u32 = 6;

/// The uniform handed to the shader: the surface size in pixels.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Viewport {
    size: [f32; 2],
    // Padding, to respect uniform buffers' 16-byte alignment.
    _pad: [f32; 2],
}

/// The deviation of a shadow's gaussian from its blur radius, as `quad.wgsl` computes it.
fn blur_sigma(blur: f32) -> f32 {
    blur * 0.57735 + 0.5
}

/// How far a shadow's quad reaches beyond its rect, as `quad.wgsl`'s vertex stage grows it.
fn shadow_margin(blur: f32) -> f32 {
    if blur > 0.0 {
        (3.0 * blur_sigma(blur) - blur).max(0.0) + 1.0
    } else {
        0.0
    }
}

/// Whether `instance` leaves nothing of what is under it wherever it covers a pixel fully:
/// one opaque colour (or an opaque two-colour fade), an opaque border or none, no blur.
fn is_opaque(instance: &Instance) -> bool {
    let [code, border_width, blur, _] = instance.params;
    code < 0.5
        && blur <= 0.0
        && instance.color[3] >= 1.0
        && ((instance.gradient[0] == 0.0 && instance.gradient[1] == 0.0)
            || instance.color2[3] >= 1.0)
        && (border_width <= 0.0 || instance.border_color[3] >= 1.0)
}

/// `[x0, y0, x1, y1]` of an `[x, y, width, height]` rectangle.
fn edges(r: [f32; 4]) -> [f32; 4] {
    [r[0], r[1], r[0] + r[2], r[1] + r[3]]
}

/// **Where an opaque rect covers every pixel fully**, held to its clip: the rect less its
/// largest corner and a pixel more all round, where the shader's coverage is exactly one.
/// Edges on whole pixels, inward, so that no pixel centre lies on one.
fn fully_covered(instance: &Instance) -> [f32; 4] {
    let inset = instance.radii.iter().fold(0.0f32, |a, &r| a.max(r)) + 1.0;
    let [x0, y0, x1, y1] = edges(instance.rect);
    let [cx0, cy0, cx1, cy1] = edges(instance.clip);
    [
        (x0 + inset).max(cx0).ceil(),
        (y0 + inset).max(cy0).ceil(),
        (x1 - inset).min(cx1).floor(),
        (y1 - inset).min(cy1).floor(),
    ]
}

/// The smallest hidden part worth cutting out of a shadow, in pixels on each side: below
/// it, four quads cost more than the pixels they save.
const SMALLEST_HIDDEN: f32 = 16.0;

/// How far after a shadow the rect that covers it is looked for: a surface's shadows come
/// right before it, one to three of them.
const COVER_LOOKAHEAD: usize = 6;

/// **Shadows are not shaded where the surface over them hides them** (milestone 837).
///
/// An elevated surface paints its shadows and then itself, over them, in the same batch: a
/// card in a list is three blurred rects the size of the card, then the card. Wherever the
/// card is opaque and covers a pixel fully, what the shadows put there is painted over, and
/// a shadow's pixel is the dearest pixel the rect shader computes. On the test phone a card
/// taller than the screen shaded about two screens of hidden shadow every frame.
///
/// So a shadow instance followed in its batch (within [`COVER_LOOKAHEAD`]) by an opaque
/// rect is replaced by up to four: the same shadow, each clipped to one band of the ring
/// left around the part the rect covers fully. The vertex stage holds each quad to its
/// clip, so the hidden part is never rasterised. The bands' shared edges lie on whole
/// pixels, so every pixel centre outside the hidden part is in exactly one band, and the
/// clip test gives it the same colour it had: the picture does not change.
///
/// Within one draw, instances are drawn in order, and nothing of another batch comes
/// between two instances of the same batch, so the opaque rect is the last word on those
/// pixels whatever came between.
fn cut_hidden_shadows(instances: &mut Vec<Instance>, start: usize, out: &mut Vec<Instance>) {
    let batch = &instances[start..];
    let worth = batch.iter().enumerate().any(|(i, s)| {
        s.params[2] > 0.0 && batch[i + 1..].iter().take(COVER_LOOKAHEAD).any(is_opaque)
    });
    if !worth {
        return;
    }
    out.clear();
    for (i, shadow) in batch.iter().enumerate() {
        let cover = (shadow.params[2] > 0.0)
            .then(|| {
                batch[i + 1..]
                    .iter()
                    .take(COVER_LOOKAHEAD)
                    .find(|c| is_opaque(c))
            })
            .flatten();
        let Some(cover) = cover else {
            out.push(*shadow);
            continue;
        };
        let [cx0, cy0, cx1, cy1] = edges(shadow.clip);
        let [hx0, hy0, hx1, hy1] = fully_covered(cover);
        // Held to the shadow's own clip and to its quad, which is all it shades.
        let m = shadow_margin(shadow.params[2]);
        let [qx0, qy0, qx1, qy1] = edges(shadow.rect);
        let (hx0, hy0) = (
            hx0.max(cx0.ceil()).max((qx0 - m).ceil()),
            hy0.max(cy0.ceil()).max((qy0 - m).ceil()),
        );
        let (hx1, hy1) = (
            hx1.min(cx1.floor()).min((qx1 + m).floor()),
            hy1.min(cy1.floor()).min((qy1 + m).floor()),
        );
        if hx1 - hx0 < SMALLEST_HIDDEN || hy1 - hy0 < SMALLEST_HIDDEN {
            out.push(*shadow);
            continue;
        }
        // The ring around the hidden part, in the shadow's clip: above, below, left, right.
        // The shader tests `x <= clip.x + clip.width`: a band whose far edge does not come
        // back exactly from its width would move a pixel centre lying on that edge, and the
        // shadow is then left whole.
        let bands = [
            [cx0, cy0, cx1, hy0],
            [cx0, hy1, cx1, cy1],
            [cx0, hy0, hx0, hy1],
            [hx1, hy0, cx1, hy1],
        ]
        .map(|[x0, y0, x1, y1]| (x1 > x0 && y1 > y0).then_some([x0, y0, x1 - x0, y1 - y0]));
        let exact = bands
            .iter()
            .zip([[cx1, hy0], [cx1, cy1], [hx0, hy1], [cx1, hy1]])
            .all(|(band, [x1, y1])| band.is_none_or(|b| b[0] + b[2] == x1 && b[1] + b[3] == y1));
        if !exact {
            out.push(*shadow);
            continue;
        }
        for clip in bands.into_iter().flatten() {
            out.push(Instance { clip, ..*shadow });
        }
        #[cfg(test)]
        tests::CUTS.with(|c| c.set(c.get() + 1));
    }
    instances.truncate(start);
    instances.extend_from_slice(out);
}

/// How many instances the buffer allocates to begin with.
const INITIAL_INSTANCE_CAPACITY: usize = 128;

/// Holds the pipeline and the buffers needed to draw the primitives.
pub(crate) struct Painter {
    pipeline: wgpu::RenderPipeline,
    quad_vertex_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    viewport_buffer: wgpu::Buffer,
    viewport_bind_group: wgpu::BindGroup,
    /// A reused CPU buffer for building the instances out of the scene.
    instances: Vec<Instance>,
    /// **The gradients' colour ramps** (milestone 644): one row of [`RAMP_WIDTH`] texels
    /// per gradient in the frame, baked on the CPU from its colours and stops.
    ramp_layout: wgpu::BindGroupLayout,
    ramp_sampler: wgpu::Sampler,
    ramp_texture: wgpu::Texture,
    ramp_bind_group: wgpu::BindGroup,
    ramp_rows: Vec<[u8; RAMP_WIDTH as usize * 4]>,
    /// A reused buffer for [`cut_hidden_shadows`].
    cut: Vec<Instance>,
}

/// How many colours a gradient is baked into.
pub(crate) const RAMP_WIDTH: u32 = 256;
/// The most gradients one frame bakes; past it, a gradient paints its first colour.
pub(crate) const RAMP_MAX_ROWS: u32 = 2048;

/// The layout of a ramp texture's bind group: the texture and its sampler, read by the
/// fragment stage.
pub(crate) fn ramp_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("frus.gradient_ramps.bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    })
}

/// The ramps' sampler: linear between texels, clamped at the ends (the tile mode is the
/// shader's).
pub(crate) fn ramp_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("frus.gradient_ramps.sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

/// A ramp texture `rows` tall, its view bound with `sampler`.
pub(crate) fn ramp_texture(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    rows: u32,
) -> (wgpu::Texture, wgpu::BindGroup) {
    // Unorm, not sRGB: the texels are sRGB values, filtered as such — the reference's
    // gradients mix their colours as written — and turned linear in the shader.
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frus.gradient_ramps"),
        size: wgpu::Extent3d {
            width: RAMP_WIDTH,
            height: rows,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("frus.gradient_ramps.bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    (texture, bind_group)
}

/// A gradient's colours at [`RAMP_WIDTH`] even points from its start to its end, as sRGB
/// bytes.
fn bake(gradient: &frus_core::Gradient) -> [u8; RAMP_WIDTH as usize * 4] {
    bake_with(|t| gradient.sample(t))
}

/// [`RAMP_WIDTH`] samples of `sample` from `0` to `1`, as sRGB bytes.
pub(crate) fn bake_with(sample: impl Fn(f32) -> frus_core::Color) -> [u8; RAMP_WIDTH as usize * 4] {
    let mut row = [0u8; RAMP_WIDTH as usize * 4];
    for i in 0..RAMP_WIDTH as usize {
        let c = sample(i as f32 / (RAMP_WIDTH - 1) as f32);
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        row[i * 4..i * 4 + 4].copy_from_slice(&[byte(c.r), byte(c.g), byte(c.b), byte(c.a)]);
    }
    row
}

/// A gradient's kind and geometry in `rect`'s pixels: `(code, gradient, extra)`, where
/// `code` is `kind + 4 × tile`, `gradient` the main geometry and `extra` the focal point,
/// the turn and whether there is a focal circle.
fn encode(gradient: &frus_core::Gradient, rect: &crate::Rect) -> (f32, [f32; 4], [f32; 4]) {
    use frus_core::{Gradient, TextDirection, TileMode};
    let half = (rect.width * 0.5, rect.height * 0.5);
    let shorter = rect.width.min(rect.height);
    let at = |a: frus_core::AlignmentGeometry| {
        let a = a.resolve(TextDirection::Ltr);
        (a.x * half.0, a.y * half.1)
    };
    let tile = match gradient.tile_mode() {
        TileMode::Clamp => 0.0,
        TileMode::Repeated => 1.0,
        TileMode::Mirror => 2.0,
        TileMode::Decal => 3.0,
    };
    let turn = gradient.rotation();
    match gradient {
        Gradient::Linear(g) => {
            let (b, e) = (at(g.begin), at(g.end));
            (
                1.0 + 4.0 * tile,
                [b.0, b.1, e.0, e.1],
                [0.0, 0.0, turn, 0.0],
            )
        }
        Gradient::Radial(g) => {
            let c = at(g.center);
            let f = g.focal.map(at);
            let (fx, fy) = f.unwrap_or(c);
            (
                2.0 + 4.0 * tile,
                [c.0, c.1, g.radius * shorter, g.focal_radius * shorter],
                [fx, fy, turn, if f.is_some() { 1.0 } else { 0.0 }],
            )
        }
        Gradient::Sweep(g) => {
            let c = at(g.center);
            (
                3.0 + 4.0 * tile,
                [c.0, c.1, g.start_angle, g.end_angle],
                [0.0, 0.0, turn, 0.0],
            )
        }
    }
}

impl Painter {
    /// Builds the painter for a given target format, surface or texture.
    /// `sample_count` is the MSAA sample count; 1 means no multisampling.
    pub(crate) fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        sample_count: u32,
        cache: Option<&wgpu::PipelineCache>,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("frus.quad.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/quad.wgsl").into()),
        });

        let viewport_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frus.viewport"),
            size: std::mem::size_of::<Viewport>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frus.viewport.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frus.viewport.bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        let ramp_layout = ramp_layout(device);
        let ramp_sampler = ramp_sampler(device);
        let (ramp_texture, ramp_bind_group) = ramp_texture(device, &ramp_layout, &ramp_sampler, 1);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("frus.pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout), Some(&ramp_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("frus.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(QuadVertex::layout()), Some(Instance::layout())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
                count: sample_count,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache,
        });

        let quad_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("frus.quad_vertex_buffer"),
            contents: bytemuck::cast_slice(QUAD_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frus.instance_buffer"),
            size: (INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<Instance>())
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            quad_vertex_buffer,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            viewport_buffer,
            viewport_bind_group,
            instances: Vec::new(),
            ramp_layout,
            ramp_sampler,
            ramp_texture,
            ramp_bind_group,
            ramp_rows: Vec::new(),
            cut: Vec::new(),
        }
    }

    /// Updates the surface size; call it at init and on resize.
    pub(crate) fn set_viewport(&self, queue: &wgpu::Queue, width: f32, height: f32) {
        let viewport = Viewport {
            size: [width.max(1.0), height.max(1.0)],
            _pad: [0.0, 0.0],
        };
        queue.write_buffer(&self.viewport_buffer, 0, bytemuck::bytes_of(&viewport));
    }

    /// Appends one rectangle primitive's instance. Anything else is another painter's
    /// business and is skipped.
    fn push(&mut self, primitive: &Primitive) {
        match primitive {
            Primitive::Rect {
                rect,
                color,
                color2,
                gradient_dir,
                radius,
                border_width,
                border_color,
                blur,
                clip,
                shader,
                blur_style,
                ..
            } => {
                // A gradient, baked into the next row of the ramps; past the last row, its
                // first colour, flat.
                let shaded = shader
                    .as_ref()
                    .filter(|_| (self.ramp_rows.len() as u32) < RAMP_MAX_ROWS)
                    .map(|gradient| {
                        let row = self.ramp_rows.len() as f32;
                        self.ramp_rows.push(bake(gradient));
                        let (code, geometry, extra) = encode(gradient, rect);
                        (code, row, geometry, extra)
                    });
                let shaded_row = shaded.is_some();
                let (code, row, geometry, extra) = shaded.unwrap_or((
                    0.0,
                    0.0,
                    [gradient_dir[0], gradient_dir[1], 0.0, 0.0],
                    color2.to_array(),
                ));
                self.instances.push(Instance {
                    rect: rect.to_array(),
                    color: color.to_array(),
                    color2: extra,
                    border_color: border_color.to_array(),
                    // A flat rectangle has no ramp row: the slot carries its blur style.
                    params: [
                        code,
                        *border_width,
                        *blur,
                        if shaded_row {
                            row
                        } else {
                            blur_style.code() as f32
                        },
                    ],
                    gradient: geometry,
                    clip: clip.to_array(),
                    // Negative radii are clamped to zero before rendering.
                    radii: radius.clamped().to_array(),
                })
            }
            // Text, vector paths and images are rendered by their own painters
            // (TextPainter, PathPainter, ImagePainter).
            Primitive::Text { .. }
            | Primitive::RichText { .. }
            | Primitive::Path { .. }
            | Primitive::Image { .. }
            | Primitive::Layer { .. } => {}
        }
    }

    /// Translates the scene's rectangles into GPU instances and uploads them, growing
    /// the buffer if needed.
    ///
    /// The buffer is filled **in batch order**, not scene order, so every batch is one
    /// contiguous run of instances and therefore one draw call. Returns the range each
    /// batch occupies — empty for the batches another painter owns — plus, last, the
    /// range of the text decorations.
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        decorations: &[DecorationQuad],
        batches: &[Batch],
    ) -> (Vec<Range<u32>>, Range<u32>) {
        self.instances.clear();
        self.ramp_rows.clear();
        let mut ranges = Vec::with_capacity(batches.len());
        for batch in batches {
            let start = self.instances.len() as u32;
            if batch.kind == Kind::Rect {
                for &index in &batch.members {
                    self.push(&scene.primitives()[index]);
                }
                #[cfg(test)]
                let cut = !tests::NO_CUT.with(|c| c.get());
                #[cfg(not(test))]
                let cut = true;
                if cut {
                    cut_hidden_shadows(&mut self.instances, start as usize, &mut self.cut);
                }
            }
            ranges.push(start..self.instances.len() as u32);
        }

        // Text decoration quads (underline, strikethrough): plain rectangles, drawn
        // in the quad pass, and therefore beneath the glyphs. They go last, with the
        // text they belong to, rather than into a batch of their own.
        let decoration_start = self.instances.len() as u32;
        for quad in decorations {
            self.instances.push(Instance {
                rect: quad.rect.to_array(),
                color: quad.color.to_array(),
                color2: quad.color.to_array(),
                border_color: Color::TRANSPARENT.to_array(),
                params: [0.0, 0.0, 0.0, 0.0],
                gradient: [0.0, 0.0, 0.0, 0.0],
                clip: quad.clip.to_array(),
                radii: [0.0, 0.0, 0.0, 0.0],
            });
        }
        let decoration_range = decoration_start..self.instances.len() as u32;

        // The frame's ramps, uploaded in one go; the texture grows to hold them.
        if !self.ramp_rows.is_empty() {
            let rows = self.ramp_rows.len() as u32;
            if rows > self.ramp_texture.height() {
                let (texture, bind_group) = ramp_texture(
                    device,
                    &self.ramp_layout,
                    &self.ramp_sampler,
                    rows.next_power_of_two(),
                );
                self.ramp_texture = texture;
                self.ramp_bind_group = bind_group;
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.ramp_texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&self.ramp_rows),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(RAMP_WIDTH * 4),
                    rows_per_image: Some(rows),
                },
                wgpu::Extent3d {
                    width: RAMP_WIDTH,
                    height: rows,
                    depth_or_array_layers: 1,
                },
            );
        }

        let count = self.instances.len();
        if count == 0 {
            return (ranges, decoration_range);
        }
        if count > self.instance_capacity {
            let new_capacity = count.next_power_of_two();
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frus.instance_buffer"),
                size: (new_capacity * std::mem::size_of::<Instance>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = new_capacity;
        }
        queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&self.instances),
        );
        (ranges, decoration_range)
    }

    /// Prepares the render by uploading the instances laid out for `batches`, and
    /// returns each batch's range plus the decorations'. `decorations` are the quads
    /// coming from text (see [`TextPainter::prepare_frame`]). Call it **before**
    /// opening the render pass.
    pub(crate) fn prepare_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene,
        decorations: &[DecorationQuad],
        batches: &[Batch],
    ) -> (Vec<Range<u32>>, Range<u32>) {
        self.prepare(device, queue, scene, decorations, batches)
    }

    /// Draws one batch's rectangles into an already-open render pass.
    pub(crate) fn draw<'pass>(&'pass self, pass: &mut wgpu::RenderPass<'pass>, range: Range<u32>) {
        if range.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        pass.set_bind_group(1, &self.ramp_bind_group, &[]);
        pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        pass.draw(0..QUAD_VERTEX_COUNT, range);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Rect};

    thread_local! {
        /// Turns [`cut_hidden_shadows`] off on this thread, to render the uncut picture to compare against.
        pub(super) static NO_CUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        /// How many shadows [`cut_hidden_shadows`] has cut on this thread.
        pub(super) static CUTS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    fn rect_instance(rect: [f32; 4], alpha: f32, blur: f32, clip: [f32; 4]) -> Instance {
        Instance {
            rect,
            color: [0.2, 0.3, 0.4, alpha],
            color2: [0.2, 0.3, 0.4, alpha],
            border_color: [0.0; 4],
            params: [0.0, 0.0, blur, 0.0],
            gradient: [0.0; 4],
            clip,
            radii: [10.0; 4],
        }
    }

    /// Whether a pixel centre is inside an instance's clip, as the shader tests it.
    fn in_clip(i: &Instance, x: f32, y: f32) -> bool {
        x >= i.clip[0] && x <= i.clip[0] + i.clip[2] && y >= i.clip[1] && y <= i.clip[1] + i.clip[3]
    }

    /// **A shadow under an opaque card becomes the ring around what the card hides**
    /// (milestone 837): every pixel centre the shadow could shade is in exactly one band,
    /// except those the card covers fully, which are in none.
    #[test]
    fn a_hidden_shadow_is_cut_to_a_ring() {
        let clip = [0.0, 50.0, 400.0, 300.25];
        let shadow = rect_instance([40.0, 20.0, 300.0, 500.0], 0.2, 8.0, clip);
        let card = rect_instance([48.0, 28.0, 284.0, 484.0], 1.0, 0.0, clip);
        let mut instances = vec![shadow, card];
        let mut scratch = Vec::new();
        cut_hidden_shadows(&mut instances, 0, &mut scratch);
        // The card runs past the clip's top: nothing is left above it, and three bands are.
        assert_eq!(
            instances.len(),
            4,
            "the shadow's three bands, then the card"
        );
        let hidden = fully_covered(&card);
        for y in 0..400 {
            for x in 0..400 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let bands = instances[..3].iter().filter(|b| in_clip(b, px, py)).count();
                let inside = px > hidden[0] && px < hidden[2] && py > hidden[1] && py < hidden[3];
                let want = usize::from(in_clip(&shadow, px, py) && !inside);
                assert_eq!(bands, want, "pixel {x},{y}");
            }
        }
        assert_eq!(instances[3].clip, card.clip, "the card is left as it was");
    }

    /// A translucent card, a blurred one, a card too far after the shadow and a
    /// shadow with nothing over it are all left whole.
    #[test]
    fn a_shadow_that_shows_is_left_whole() {
        let clip = [0.0, 0.0, 400.0, 400.0];
        let shadow = rect_instance([40.0, 20.0, 300.0, 300.0], 0.2, 8.0, clip);
        let card = rect_instance([48.0, 28.0, 284.0, 284.0], 1.0, 0.0, clip);
        let cases = [
            vec![
                shadow,
                rect_instance([48.0, 28.0, 284.0, 284.0], 0.9, 0.0, clip),
            ],
            vec![
                shadow,
                rect_instance([48.0, 28.0, 284.0, 284.0], 1.0, 4.0, clip),
            ],
            vec![shadow],
            vec![card, shadow],
            std::iter::once(shadow)
                .chain(std::iter::repeat_n(
                    rect_instance([0.0; 4], 0.5, 0.0, clip),
                    COVER_LOOKAHEAD,
                ))
                .chain(std::iter::once(card))
                .collect(),
        ];
        for case in cases {
            let mut instances = case.clone();
            cut_hidden_shadows(&mut instances, 0, &mut Vec::new());
            assert_eq!(instances.len(), case.len());
        }
    }

    /// **The cut does not change the picture** (milestone 837): cards with shadows, under
    /// clips that cut them on fractional pixels, rendered with and without the cut, are the
    /// same pixel for pixel.
    #[test]
    fn cutting_hidden_shadows_changes_no_pixel() {
        let mut scene = Scene::new();
        scene.fill_rect(
            Rect::new(0.0, 0.0, 200.0, 240.0),
            Color::rgb(0.95, 0.95, 0.97),
        );
        scene.set_clip(Rect::new(3.25, 20.5, 190.0, 200.75));
        for (i, (x, y)) in [(10.0, -30.0), (110.0, 60.5)].into_iter().enumerate() {
            let card = Rect::new(x, y, 80.0 + i as f32 * 0.4, 200.0);
            let grow = |r: Rect, by: f32| {
                Rect::new(r.x - by, r.y - by, r.width + 2.0 * by, r.height + 2.0 * by)
            };
            scene.shadow(grow(card, 4.0), Color::rgba(0.0, 0.0, 0.0, 0.2), 14.0, 4.0);
            scene.shadow(grow(card, 9.0), Color::rgba(0.0, 0.0, 0.0, 0.12), 19.0, 9.0);
            scene.draw_rect(card, Color::WHITE, 10.0, 1.0, Color::rgb(0.8, 0.8, 0.8));
        }
        CUTS.with(|c| c.set(0));
        let Some(cut) = crate::render_offscreen(&scene, 200, 240, Color::BLACK) else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };
        assert_eq!(CUTS.with(|c| c.get()), 4, "all four shadows are cut");
        NO_CUT.with(|c| c.set(true));
        let whole = crate::render_offscreen(&scene, 200, 240, Color::BLACK).unwrap();
        NO_CUT.with(|c| c.set(false));
        let differ = cut
            .rgba
            .chunks(4)
            .zip(whole.rgba.chunks(4))
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(differ, 0, "pixels changed by the cut");
        // And the shadows do show around the cards.
        let i = ((100 * 200 + 5) * 4) as usize;
        assert!(
            cut.rgba[i] < 240,
            "the shadow left of the first card is drawn"
        );
    }

    /// **The straight band weighs what the four samples weigh** (milestone 835): where every
    /// row a shadow's column samples is the same row, the shader takes that row once, times
    /// `STRAIGHT_BAND_WEIGHT`, instead of four times, each times its sample of the gaussian.
    /// The two agree only if the constant is the four weights' sum, and that sum must be the
    /// same at every deviation. A wrong constant by a fraction of a percent is under a level
    /// of the rendered picture, so the golden comparison cannot see it; this test can.
    #[test]
    fn the_straight_band_weighs_what_the_four_samples_weigh() {
        let source = include_str!("shaders/quad.wgsl");
        let constant: f64 = source
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("const STRAIGHT_BAND_WEIGHT: f32 = ")
            })
            .and_then(|rest| rest.trim_end_matches(';').trim().parse().ok())
            .expect("quad.wgsl declares STRAIGHT_BAND_WEIGHT");
        // The shader's column, in f64: from -3σ to 3σ in four steps, each sample at the
        // middle of its step, weighed by `gaussian(y, sigma) * step`.
        let four_samples = |sigma: f64| {
            let step = 6.0 * sigma / 4.0;
            (0..4)
                .map(|i| {
                    let y = -3.0 * sigma + step * (i as f64 + 0.5);
                    (-(y * y) / (2.0 * sigma * sigma)).exp()
                        / ((2.0 * std::f64::consts::PI).sqrt() * sigma)
                        * step
                })
                .sum::<f64>()
        };
        for sigma in [0.5, 1.0, 7.4, 12.0, 40.0, 300.0] {
            let sum = four_samples(sigma);
            assert!(
                (constant - sum).abs() < 1e-7,
                "at σ = {sigma}, the four samples weigh {sum}, the band {constant}"
            );
        }
        // Not simply one: a band that forgot to weigh its row would be told apart.
        assert!((1.0 - constant).abs() > 1e-3, "{constant}");
    }

    /// Instantiates a surfaceless (headless) wgpu device. `None` when there is no GPU.
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
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("frus.test.device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
            ..Default::default()
        }))
        .ok()?;
        Some((device, queue))
    }

    /// Renders a red rectangle covering a whole offscreen texture, then reads the
    /// centre pixel back: it must be opaque red. Automatic proof that the pipeline
    /// and the coordinate conversion work, with no window involved.
    #[test]
    fn renders_red_rect_to_center_pixel() {
        let Some((device, queue)) = headless_device() else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };

        const SIZE: u32 = 64; // 64 * 4 = 256 bytes/row => aligned for the copy.
        let format = wgpu::TextureFormat::Rgba8Unorm;

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frus.test.target"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut painter = Painter::new(&device, format, 1, None);
        painter.set_viewport(&queue, SIZE as f32, SIZE as f32);

        let mut scene = Scene::new();
        scene.fill_rect(
            Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32),
            Color::rgb(1.0, 0.0, 0.0),
        );

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let batches = crate::batch::plan(&scene);
        let (ranges, _) = painter.prepare_frame(&device, &queue, &scene, &[], &batches);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for range in &ranges {
                painter.draw(&mut pass, range.clone());
            }
        }

        // Copy the texture into a CPU-readable buffer.
        let bytes_per_row = SIZE * 4;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frus.test.readback"),
            size: (bytes_per_row * SIZE) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv()
            .expect("map_async")
            .expect("buffer mapping failed");

        let data = slice.get_mapped_range().expect("buffer mapping failed");
        let (cx, cy) = (SIZE / 2, SIZE / 2);
        let idx = (cy * bytes_per_row + cx * 4) as usize;
        let pixel = [data[idx], data[idx + 1], data[idx + 2], data[idx + 3]];

        assert_eq!(
            pixel,
            [255, 0, 0, 255],
            "the centre pixel must be opaque red"
        );
    }

    /// A heavily rounded rectangle covering the whole texture: the centre is
    /// filled, but the corner is cut away and stays the black background.
    #[test]
    fn rounded_rect_leaves_corner_transparent() {
        let Some((device, queue)) = headless_device() else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };

        const SIZE: u32 = 64;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frus.test.rounded"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut painter = Painter::new(&device, format, 1, None);
        painter.set_viewport(&queue, SIZE as f32, SIZE as f32);

        let mut scene = Scene::new();
        scene.draw_rect(
            Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32),
            Color::rgb(1.0, 0.0, 0.0),
            30.0, // a large radius
            0.0,
            Color::TRANSPARENT,
        );

        let batches = crate::batch::plan(&scene);
        let (ranges, _) = painter.prepare_frame(&device, &queue, &scene, &[], &batches);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for range in &ranges {
                painter.draw(&mut pass, range.clone());
            }
        }

        let bytes_per_row = SIZE * 4;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (bytes_per_row * SIZE) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv()
            .expect("map_async")
            .expect("buffer mapping failed");

        let data = slice.get_mapped_range().expect("buffer mapping failed");
        let px = |x: u32, y: u32| {
            let idx = (y * bytes_per_row + x * 4) as usize;
            [data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]
        };

        // A red centre, with the (0,0) corner cut away and black.
        assert_eq!(px(SIZE / 2, SIZE / 2), [255, 0, 0, 255], "centre rouge");
        assert_eq!(px(0, 0), [0, 0, 0, 255], "corner cut away");
    }

    /// **Per-corner** radii: only the rounded top-left corner is cut away, the
    /// other three stay square — the GPU path proved by readback.
    #[test]
    fn per_corner_radius_rounds_only_selected_corners() {
        let Some((device, queue)) = headless_device() else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };

        const SIZE: u32 = 64;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frus.test.per_corner"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut painter = Painter::new(&device, format, 1, None);
        painter.set_viewport(&queue, SIZE as f32, SIZE as f32);

        let mut scene = Scene::new();
        scene.draw_rect(
            Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32),
            Color::rgb(1.0, 0.0, 0.0),
            frus_core::BorderRadius {
                top_left: 30.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
            0.0,
            Color::TRANSPARENT,
        );

        let batches = crate::batch::plan(&scene);
        let (ranges, _) = painter.prepare_frame(&device, &queue, &scene, &[], &batches);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for range in &ranges {
                painter.draw(&mut pass, range.clone());
            }
        }

        let bytes_per_row = SIZE * 4;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (bytes_per_row * SIZE) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv()
            .expect("map_async")
            .expect("buffer mapping failed");

        let data = slice.get_mapped_range().expect("buffer mapping failed");
        let px = |x: u32, y: u32| {
            let idx = (y * bytes_per_row + x * 4) as usize;
            [data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]
        };

        // Only the top-left corner is cut away; the other three are square.
        assert_eq!(px(0, 0), [0, 0, 0, 255], "rounded top-left, cut away");
        assert_eq!(px(SIZE - 1, 0), [255, 0, 0, 255], "square top-right");
        assert_eq!(px(0, SIZE - 1), [255, 0, 0, 255], "square bottom-left");
        assert_eq!(
            px(SIZE - 1, SIZE - 1),
            [255, 0, 0, 255],
            "square bottom-right"
        );
        assert_eq!(px(SIZE / 2, SIZE / 2), [255, 0, 0, 255], "centre plein");
    }

    /// A solid red rectangle whose clip covers one corner only: the centre, outside
    /// the clip, stays background; the corner, inside it, is red.
    #[test]
    fn clip_excludes_pixels_outside() {
        let Some((device, queue)) = headless_device() else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };

        const SIZE: u32 = 64;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frus.test.clip"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut painter = Painter::new(&device, format, 1, None);
        painter.set_viewport(&queue, SIZE as f32, SIZE as f32);

        let mut scene = Scene::new();
        // The clip is limited to a 16x16 square at the top left.
        scene.set_clip(Rect::new(0.0, 0.0, 16.0, 16.0));
        scene.fill_rect(
            Rect::new(0.0, 0.0, SIZE as f32, SIZE as f32),
            Color::rgb(1.0, 0.0, 0.0),
        );

        let batches = crate::batch::plan(&scene);
        let (ranges, _) = painter.prepare_frame(&device, &queue, &scene, &[], &batches);
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for range in &ranges {
                painter.draw(&mut pass, range.clone());
            }
        }

        let bytes_per_row = SIZE * 4;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (bytes_per_row * SIZE) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(SIZE),
                },
            },
            wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv()
            .expect("map_async")
            .expect("buffer mapping failed");

        let data = slice.get_mapped_range().expect("buffer mapping failed");
        let px = |x: u32, y: u32| {
            let idx = (y * bytes_per_row + x * 4) as usize;
            [data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]
        };

        assert_eq!(px(4, 4), [255, 0, 0, 255], "inside the clip → red");
        assert_eq!(px(SIZE / 2, SIZE / 2), [0, 0, 0, 255], "hors clip → fond");
    }
}
