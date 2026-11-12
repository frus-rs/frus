// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Render orchestration: gathers the content painters — rectangles, images,
//! paths, text — and handles **layers** ([`Primitive::Layer`]).
//!
//! A layer is rendered **separately** into a full-surface texture (a pre-pass,
//! submitted on its own so the instance buffers are not aliased), then composited
//! as a single block at its group opacity by the [`CompositePainter`]. Group alpha
//! is therefore correct, with no double-blending where children overlap.
//!
//! Every pipeline is created up front ([`Painters::new`]) then **warmed up**
//! ([`Painters::warm_up`]): the first real frame compiles no shader, so it never
//! stutters.

use crate::batch;
use bytemuck::{Pod, Zeroable};
use frus_core::{
    BoxFit, Color, ColorFilter, ImageData, ImageFilter, LayerFilter, MaskShader, Path, Point,
    Primitive, Rect, Scene,
};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

use crate::filter::FilterPainter;
use crate::image::ImagePainter;
use crate::painter::Painter;
use crate::path::PathPainter;
use crate::text::TextPainter;

/// A vertex of the unit quad, which covers the whole surface.
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

const QUAD_VERTICES: &[QuadVertex] = &[
    QuadVertex { pos: [0.0, 0.0] },
    QuadVertex { pos: [1.0, 0.0] },
    QuadVertex { pos: [1.0, 1.0] },
    QuadVertex { pos: [0.0, 0.0] },
    QuadVertex { pos: [1.0, 1.0] },
    QuadVertex { pos: [0.0, 1.0] },
];
const QUAD_VERTEX_COUNT: u32 = 6;

/// The unit quad's vertex layout, shared with the custom pipelines (milestone 792).
pub(crate) fn quad_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    QuadVertex::layout()
}

/// A composite instance: the clip, the **inverse** transform (screen → texture)
/// and the group opacity.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CompInstance {
    clip: [f32; 4],
    /// The linear part of the affine inverse: `ia, ib, ic, id`.
    inv_lin: [f32; 4],
    /// `ie, if, opacity, _` — the inverse's translation plus the group opacity.
    inv_tr_opacity: [f32; 4],
    /// The clip shape: `[kind, _, _, _]` (0 = rect, 1 = rrect, 2 = oval).
    shape: [f32; 4],
    /// The corner radii of a rrect: `[tl, tr, br, bl]`.
    radii: [f32; 4],
    /// Where the layer's texture lies on the target: `[x, y, width, height]`, in pixels
    /// (milestone 610). The whole target for a layer that may draw anywhere.
    region: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Viewport {
    size: [f32; 2],
    _pad: [f32; 2],
}

/// A layer's colour filter and mask, as the fragment shader reads them.
///
/// This rides in a **uniform** rather than in the instance, because a colour matrix
/// alone is twenty floats and a vertex attribute budget is a small, fixed thing. Each
/// layer already has its own bind group for its texture, so a slice of one shared
/// buffer costs nothing extra.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
struct FilterParams {
    /// `[colour kind, mask kind, colour blend, mask blend]`. Colour kind: 0 none,
    /// 1 matrix, 2 a colour blended in. Mask kind: 0 none, 1 linear, 2 radial.
    flags: [f32; 4],
    /// The colour matrix, one row per output channel, then the constant column.
    rows: [[f32; 4]; 4],
    constants: [f32; 4],
    /// The colour of a `Mode` colour filter.
    mode_color: [f32; 4],
    /// Linear: `(from.x, from.y, to.x, to.y)`. Radial: `(cx, cy, radius, _)`.
    mask_geom: [f32; 4],
    mask_c0: [f32; 4],
    mask_c1: [f32; 4],
    /// `[mode + 1, _, _, _]`: how the layer meets what is under it, `0` painting it over
    /// (milestone 646).
    blend: [f32; 4],
}

impl FilterParams {
    /// Uniform bindings must start at a device-aligned offset; 256 bytes is the
    /// strictest alignment in practice, so the layers take one slot each.
    const STRIDE: wgpu::BufferAddress = 256;

    /// Translates a scene-level filter into the numbers the shader wants. The image
    /// filter is not here: it has already run, as a pre-pass over the layer texture.
    fn new(filter: &LayerFilter) -> FilterParams {
        let mut p = FilterParams::default();
        match filter.color {
            None => {}
            Some(ColorFilter::Matrix(m)) => {
                p.flags[0] = 1.0;
                for row in 0..4 {
                    p.rows[row] = [m[row * 5], m[row * 5 + 1], m[row * 5 + 2], m[row * 5 + 3]];
                    p.constants[row] = m[row * 5 + 4];
                }
            }
            Some(ColorFilter::Mode(color, mode)) => {
                p.flags[0] = 2.0;
                p.flags[2] = mode.code() as f32;
                p.mode_color = color.to_array();
            }
        }
        if let Some(mask) = filter.mask {
            p.flags[1] = mask.shader.code() as f32 + 1.0;
            p.flags[3] = mask.blend.code() as f32;
            let (geom, c0, c1) = match mask.shader {
                MaskShader::Linear {
                    from,
                    to,
                    from_color,
                    to_color,
                } => ([from.x, from.y, to.x, to.y], from_color, to_color),
                MaskShader::Radial {
                    center,
                    radius,
                    from_color,
                    to_color,
                } => ([center.x, center.y, radius, 0.0], from_color, to_color),
            };
            p.mask_geom = geom;
            p.mask_c0 = c0.to_array();
            p.mask_c1 = c1.to_array();
        }
        if let Some(mode) = filter.blend {
            p.blend[0] = mode.code() as f32 + 1.0;
        }
        p
    }
}

/// The MSAA sample count we aim for: 4× is a good quality/cost trade-off and the
/// most widely supported, including the llvmpipe software rasteriser.
pub(crate) const MSAA_SAMPLES: u32 = 4;

/// Returns the MSAA sample count to use for `format` on this adapter:
/// [`MSAA_SAMPLES`] when supported, otherwise 1, which disables MSAA. Called once
/// at init by the windowed renderer and by the offscreen path alike.
pub(crate) fn preferred_sample_count(adapter: &wgpu::Adapter, format: wgpu::TextureFormat) -> u32 {
    let flags = adapter.get_texture_format_features(format).flags;
    if flags.sample_count_supported(MSAA_SAMPLES) {
        MSAA_SAMPLES
    } else {
        1
    }
}

/// A layer ready to be composited: its texture, opacity and clip, plus the
/// **inverse** transform (screen → texture) to apply when sampling.
struct LayerComposite {
    view: wgpu::TextureView,
    /// The coverage mask (`ClipShape::Path`): the path rendered in white. Other
    /// shapes get a solid 1×1 white texture, a neutral multiplication.
    mask: wgpu::TextureView,
    opacity: f32,
    clip: [f32; 4],
    /// The clip shape: `[kind, _, _, _]` — `kind` 0 = rect, 1 = rrect, 2 = oval,
    /// 3 = path, meaning a mask.
    shape: [f32; 4],
    /// The corner radii of a rrect: `[tl, tr, br, bl]`.
    radii: [f32; 4],
    /// The affine inverse `[ia, ib, ic, id, ie, if]`; identity means no transform.
    inverse: [f32; 6],
    /// The colour filter and mask, applied in the compositing fragment.
    filter: LayerFilter,
    /// Where its texture lies on the target, when it is not the whole of it.
    region: Option<[f32; 4]>,
    /// An application's shader or GPU pass colouring it, through a pipeline of its own
    /// (milestone 792).
    custom: Option<crate::custom::CustomDraw>,
}

/// **The part of the target a layer can draw on**, in whole pixels: its clip, held to the
/// target (milestone 610). `None` when it may draw anywhere, and has the whole target: a
/// layer that is transformed, filtered, painted over a backdrop, or clipped to a path, or
/// one with such a layer inside it. Those move or spread their content past its clip, or
/// hold geometry in the target's own coordinates.
///
/// A layer used to be rendered into a texture the size of the target and composited by a
/// quad covering it, whatever it held: on a phone, a group-opacity layer of 56 × 32 px cost
/// two passes over 1080 × 2340 multisampled pixels a frame.
fn layer_region(
    clip: Rect,
    clip_shape: &frus_core::ClipShape,
    transform: &Option<frus_core::LayerTransform>,
    filter: &LayerFilter,
    primitives: &[Primitive],
    w: u32,
    h: u32,
) -> Option<Rect> {
    if transform.is_some()
        || filter.image.is_some_and(|f| !f.is_identity())
        || filter.backdrop.is_some()
        || matches!(clip_shape, frus_core::ClipShape::Path(_))
        || !primitives.iter().all(boundable)
    {
        return None;
    }
    // No larger than what it holds (milestone 796): a layer's clip is often the screen's —
    // a group faded, a shader mask over a title — while what it draws is a line of text.
    // Its texture, its multisampled pass and its compositing draw were the screen's size;
    // the shimmering title of the shader gallery was a full-screen shader every frame.
    // What it holds is the batch planner's footprint, the one the draw order already trusts,
    // grown by a margin for an antialiased edge; a line of text, its whole width.
    let clip = match content_footprint(clip, filter, primitives) {
        Some(content) => clip.intersect(Rect::new(
            content.x - CONTENT_MARGIN,
            content.y - CONTENT_MARGIN,
            content.width + 2.0 * CONTENT_MARGIN,
            content.height + 2.0 * CONTENT_MARGIN,
        )),
        None => clip,
    };
    let x0 = clip.x.max(0.0).floor();
    let y0 = clip.y.max(0.0).floor();
    let x1 = (clip.x + clip.width).min(w as f32).ceil();
    let y1 = (clip.y + clip.height).min(h as f32).ceil();
    if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
        // Nothing of it is on the target: one pixel, which draws nothing.
        return Some(Rect::new(
            x0.min(w as f32 - 1.0).max(0.0),
            y0.min(h as f32 - 1.0).max(0.0),
            1.0,
            1.0,
        ));
    }
    // Nothing gained, and a texture of a different size, for a layer that covers it all.
    if x0 <= 0.0 && y0 <= 0.0 && x1 >= w as f32 && y1 >= h as f32 {
        return None;
    }
    Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// How far past its footprint a layer's content may paint, in pixels: an antialiased edge.
const CONTENT_MARGIN: f32 = 16.0;

/// Where a layer's content draws, inside `clip`: its primitives' footprints, and the box its
/// shader or GPU pass paints. `None` when it holds nothing with a footprint.
///
/// Text is trusted for its lines, not its width: a title laid out in a box narrower than
/// what it draws (a collapsing app bar's, mid-way) spills sideways, so a line of text counts
/// as the clip's whole width, and as much again as its own height above and below. A layer
/// inside counts as its own clip.
fn content_footprint(clip: Rect, filter: &LayerFilter, primitives: &[Primitive]) -> Option<Rect> {
    let own = filter
        .shader
        .as_ref()
        .map(|s| s.rect())
        .or(filter.external.as_ref().map(|e| e.rect()));
    primitives
        .iter()
        .filter_map(|p| match p {
            Primitive::Text { bounds, .. } | Primitive::RichText { bounds, .. } => {
                let reach = bounds.height.max(CONTENT_MARGIN);
                Some(Rect::new(
                    clip.x,
                    bounds.y - reach,
                    clip.width,
                    bounds.height + 2.0 * reach,
                ))
            }
            Primitive::Layer { clip, .. } => Some(*clip),
            _ => batch::footprint(p).map(|(_, bounds)| bounds),
        })
        .chain(own)
        .reduce(|a, b| a.union(b))
}

/// Whether a layer is only an application's shader filling its box, or a GPU pass: it has
/// no content of its own to render before it is composited (milestone 792).
fn custom_only(filter: &LayerFilter, primitives: &[Primitive]) -> bool {
    filter.external.is_some() || (filter.shader.is_some() && primitives.is_empty())
}

/// **A clip around a shader or a GPU pass, folded into it** (milestone 796): the layer to
/// composite instead of `primitive`, when `primitive` is a layer that only clips — or fades —
/// a single shader filling a box, or a single GPU pass.
///
/// Such a layer has no content but the shader, and the shader changes every frame it
/// plays: the layer was rendered again every frame into a texture of its own — a
/// multisampled pass the size of the box — only to be composited with a rounded corner.
/// The compositing draw clips with a rounded rectangle, an oval or a rectangle as well as
/// the layer's draw would: the shader is composited once, straight onto the frame, clipped
/// to the outer layer's shape. A rounded card around an animated shader (the gallery's
/// plasma) costs one draw instead of two passes.
///
/// Only where the result is the same: the outer layer has no transform and no filter, its
/// shape is not a path; the inner layer is unclipped by a shape of its own and untransformed;
/// and its clip holds the outer one — or the outer one is a plain rectangle, and the two
/// rectangles meet in a rectangle.
fn folded_clip(primitive: &Primitive) -> Option<Primitive> {
    let Primitive::Layer {
        primitives,
        opacity,
        clip,
        clip_shape,
        transform: None,
        filter,
        ..
    } = primitive
    else {
        return None;
    };
    let [Primitive::Layer {
        primitives: inner,
        opacity: inner_opacity,
        clip: inner_clip,
        clip_shape: frus_core::ClipShape::Rect,
        transform: None,
        filter: inner_filter,
        owner,
    }] = primitives.as_slice()
    else {
        return None;
    };
    if *filter != LayerFilter::NONE
        || matches!(clip_shape, frus_core::ClipShape::Path(_))
        || !custom_only(inner_filter, inner)
        || inner_filter.backdrop.is_some()
        || inner_filter.blend.is_some()
    {
        return None;
    }
    let (clip, clip_shape) = if matches!(clip_shape, frus_core::ClipShape::Rect) {
        (clip.intersect(*inner_clip), frus_core::ClipShape::Rect)
    } else if holds(*inner_clip, *clip) {
        (*clip, clip_shape.clone())
    } else {
        return None;
    };
    Some(Primitive::Layer {
        primitives: inner.clone(),
        opacity: opacity * inner_opacity,
        clip,
        clip_shape,
        transform: None,
        filter: inner_filter.clone(),
        owner: *owner,
    })
}

/// Whether `outer` holds all of `inner`, to a hundredth of a pixel.
fn holds(outer: Rect, inner: Rect) -> bool {
    const SLACK: f32 = 0.01;
    outer.x <= inner.x + SLACK
        && outer.y <= inner.y + SLACK
        && outer.x + outer.width >= inner.x + inner.width - SLACK
        && outer.y + outer.height >= inner.y + inner.height - SLACK
}

/// Whether a primitive can be drawn moved: everything but a layer that needs the whole
/// target, by [`layer_region`]'s rule.
fn boundable(primitive: &Primitive) -> bool {
    match primitive {
        Primitive::Layer {
            primitives,
            clip_shape,
            transform,
            filter,
            ..
        } => {
            transform.is_none()
                && filter.image.is_none_or(|f| f.is_identity())
                && filter.backdrop.is_none()
                && !matches!(clip_shape, frus_core::ClipShape::Path(_))
                && primitives.iter().all(boundable)
        }
        _ => true,
    }
}

/// `primitives` moved into a region's own frame, its corner at the origin.
fn into_region(
    primitives: &[Primitive],
    region: Option<Rect>,
) -> std::borrow::Cow<'_, [Primitive]> {
    match region {
        Some(r) => std::borrow::Cow::Owned(
            primitives
                .iter()
                .map(|p| p.translated(-r.x, -r.y))
                .collect(),
        ),
        None => std::borrow::Cow::Borrowed(primitives),
    }
}

/// A draw waiting for the frame underneath it to exist: a backdrop, or a layer that
/// blends with what is under it. The frame is drawn up to it, read, and the draw pointed
/// at what was read.
struct Cut {
    /// Its index in the composite draw list. Everything before it is what it reads.
    at: usize,
    /// The scene index of the layer it belongs to, which is how the frame is cut: the
    /// copy is taken just before the **batch** that draws that layer.
    scene: usize,
    kind: CutKind,
}

/// What a [`Cut`] does with the frame it reads.
enum CutKind {
    /// Filters it, and draws the result.
    Backdrop {
        filter: ImageFilter,
        /// `true` for [`frus_core::BlendMode::Src`]: the filtered copy **replaces** what
        /// it was copied from rather than being painted over it.
        replace: bool,
        /// The sharing key, when several backdrops are filtered once between them.
        key: Option<u64>,
    },
    /// Copies it as it is, for the layer to blend with (milestone 646).
    Blend,
}

/// A copy of `source`, `w × h`, to read while `source` is drawn on.
fn copy_of(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    source: &wgpu::Texture,
    w: u32,
    h: u32,
) -> wgpu::Texture {
    let size = wgpu::Extent3d {
        width: w.max(1),
        height: h.max(1),
        depth_or_array_layers: 1,
    };
    let copy = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frus.under"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("frus.encoder.under"),
    });
    encoder.copy_texture_to_texture(source.as_image_copy(), copy.as_image_copy(), size);
    queue.submit(std::iter::once(encoder.finish()));
    copy
}

/// This frame's draws, prepared once and replayed by whichever pass draws them.
///
/// The batches are the **whole** order of the frame, layers included: a layer is one
/// batch of its own, holding the scene index of the group, and `layer_draws` says which
/// composite draws that batch issues — two when the layer takes a backdrop (the filtered
/// copy, then the layer over it), one otherwise.
struct ContentPlan<'a> {
    batches: &'a [batch::Batch],
    rect_ranges: &'a [std::ops::Range<u32>],
    image_ranges: &'a [std::ops::Range<u32>],
    path_ranges: &'a [std::ops::Range<u32>],
    decoration_ranges: &'a [std::ops::Range<u32>],
    decoration_base: u32,
    /// Scene index of a layer → the composite draws that put it on the screen.
    layer_draws: &'a HashMap<usize, std::ops::Range<usize>>,
    /// How many text batches precede each batch, so a pass over a *range* of batches
    /// still hands the text painter the right renderer.
    text_slots: &'a [usize],
}

/// A layer texture **kept across frames** — a repaint boundary on the GPU side —
/// together with a snapshot of its content and its dimensions: as long as those do
/// not change the texture is reused as is, and the pre-pass (submit, tessellation
/// and draw) is skipped entirely.
struct CachedLayer {
    /// What the layer draws, **in its own frame**: moved so that its region starts at
    /// the origin. A layer carried across the screen whole — a row scrolled — draws the
    /// same there, and is not rendered again (milestone 610).
    primitives: Vec<Primitive>,
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    /// The image filter already baked into `texture`. Part of the key: the same
    /// primitives blurred by a different amount are a different picture, and an
    /// animated blur would otherwise hold the first frame it drew for ever.
    image: Option<ImageFilter>,
}

/// An intermediate MSAA texture reused as a render target: painting happens there,
/// multisampled, then **resolves** to the single-sample target. One is enough — the
/// layer pre-passes and the main pass are all full-surface and take turns using it,
/// since their submits are sequential.
struct MsaaScratch {
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    texture: wgpu::Texture,
    /// Made with [`wgpu::TextureUsages::TRANSIENT_ATTACHMENT`]: on a tiled GPU it lives in
    /// the tile memory and takes no memory of its own (milestone 796). It can only be
    /// cleared at the start of a pass and is never stored, so only a draw done in **one**
    /// pass may use it.
    transient: bool,
    /// Whether this frame drew with it: what no draw of a frame used is let go at its end.
    used: bool,
}

/// What a texture takes in GPU memory, its samples and levels included.
fn texture_bytes(texture: &wgpu::Texture) -> u64 {
    let texel = texture
        .format()
        .block_copy_size(Some(wgpu::TextureAspect::All))
        .unwrap_or(4) as u64;
    let mut bytes = 0;
    for level in 0..texture.mip_level_count() {
        let w = (texture.width() >> level).max(1) as u64;
        let h = (texture.height() >> level).max(1) as u64;
        bytes += w * h * texel;
    }
    bytes * texture.sample_count() as u64
}

/// The layer compositing pipeline.
struct CompositePainter {
    pipeline: wgpu::RenderPipeline,
    /// The same pipeline with blending off, so a draw can **replace** what is under
    /// it instead of painting over it. Only a backdrop asks for that
    /// ([`frus_core::BlendMode::Src`]).
    pipeline_src: wgpu::RenderPipeline,
    viewport_buffer: wgpu::Buffer,
    viewport_bind_group: wgpu::BindGroup,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    quad_vertex_buffer: wgpu::Buffer,
    instance_buffer: wgpu::Buffer,
    instance_capacity: usize,
    /// The bind groups for this frame's layer textures.
    bind_groups: Vec<wgpu::BindGroup>,
    /// The **neutral** mask, 1×1 opaque white: bound when a layer has no path clip,
    /// so alpha is multiplied by 1 and nothing changes.
    white_mask: wgpu::Texture,
    /// This frame's filter parameters, one aligned slot per layer.
    filter_buffer: wgpu::Buffer,
    filter_capacity: usize,
    /// The layers an application's shader or GPU pass colours (milestone 792).
    pub(crate) custom: crate::custom::CustomPainter,
}

impl CompositePainter {
    /// A view of the neutral mask, opaque white, bound to layers with no path clip.
    fn white_mask_view(&self) -> wgpu::TextureView {
        self.white_mask
            .create_view(&wgpu::TextureViewDescriptor::default())
    }
}

impl CompositePainter {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        sample_count: u32,
        cache: Option<&wgpu::PipelineCache>,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("frus.composite.shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/composite.wgsl").into()),
        });

        let viewport_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frus.composite.viewport"),
            size: std::mem::size_of::<Viewport>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let viewport_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frus.composite.viewport.bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // The fragment also reads `viewport.size`, to counter-rotate a layer.
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let viewport_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frus.composite.viewport.bind_group"),
            layout: &viewport_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: viewport_buffer.as_entire_binding(),
            }],
        });

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frus.composite.texture.bgl"),
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
                // The path clip mask, sampled in the fragment shader.
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // The colour filter and mask.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // What is under a layer that blends with it (milestone 646).
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        // Linear: a layer that is scaled or turned falls between texels, and the nearest one
        // drops strokes from text and steps edges (milestone 732). A layer that is neither
        // samples texel centres, where linear reads exactly the texel.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("frus.composite.sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("frus.composite.pipeline_layout"),
            bind_group_layouts: &[Some(&viewport_layout), Some(&texture_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("frus.composite.pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(QuadVertex::layout()), Some(comp_instance_layout())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // **Premultiplied**, unlike every other pipeline here, because
                    // what this one samples is a layer texture and a layer texture is
                    // premultiplied: it was painted over a transparent target. Passing
                    // it through the straight-alpha blend would multiply the colour by
                    // the coverage a second time, which is invisible on opaque content
                    // and wrong everywhere else — a mask fade would run to black
                    // instead of to the background.
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
                count: sample_count,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache,
        });

        let pipeline_src = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("frus.composite.pipeline.src"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(QuadVertex::layout()), Some(comp_instance_layout())],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // No blending at all: the source is written as it stands.
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
            multisample: wgpu::MultisampleState {
                count: sample_count,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache,
        });

        let quad_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("frus.composite.quad_vertex_buffer"),
            contents: bytemuck::cast_slice(QUAD_VERTICES),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frus.composite.instance_buffer"),
            size: (8 * std::mem::size_of::<CompInstance>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let filter_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frus.composite.filter_buffer"),
            size: FilterParams::STRIDE * 8,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // The neutral mask: 1×1 opaque white, so alpha is 1 everywhere.
        let white = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("frus.composite.white_mask"),
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
            &[255, 255, 255, 255],
        );

        let custom = crate::custom::CustomPainter::new(
            device,
            queue,
            format,
            sample_count,
            cache,
            &viewport_layout,
            &texture_layout,
        );
        Self {
            custom,
            pipeline,
            pipeline_src,
            viewport_buffer,
            viewport_bind_group,
            texture_layout,
            sampler,
            quad_vertex_buffer,
            instance_buffer,
            instance_capacity: 8,
            bind_groups: Vec::new(),
            white_mask: white,
            filter_buffer,
            filter_capacity: 8,
        }
    }

    /// Prepares the compositing of `layers`: instances and bind groups.
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layers: &[LayerComposite],
        w: f32,
        h: f32,
    ) {
        let viewport = Viewport {
            size: [w.max(1.0), h.max(1.0)],
            _pad: [0.0, 0.0],
        };
        queue.write_buffer(&self.viewport_buffer, 0, bytemuck::bytes_of(&viewport));

        self.bind_groups.clear();
        if layers.is_empty() {
            return;
        }
        let instances: Vec<CompInstance> = layers
            .iter()
            .map(|l| {
                let i = l.inverse;
                CompInstance {
                    clip: l.clip,
                    inv_lin: [i[0], i[1], i[2], i[3]],
                    inv_tr_opacity: [i[4], i[5], l.opacity, 0.0],
                    shape: l.shape,
                    radii: l.radii,
                    region: l.region.unwrap_or([0.0, 0.0, w.max(1.0), h.max(1.0)]),
                }
            })
            .collect();
        if instances.len() > self.instance_capacity {
            let cap = instances.len().next_power_of_two();
            self.instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frus.composite.instance_buffer"),
                size: (cap * std::mem::size_of::<CompInstance>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_capacity = cap;
        }
        queue.write_buffer(&self.instance_buffer, 0, bytemuck::cast_slice(&instances));

        if layers.len() > self.filter_capacity {
            let cap = layers.len().next_power_of_two();
            self.filter_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frus.composite.filter_buffer"),
                size: FilterParams::STRIDE * cap as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.filter_capacity = cap;
        }
        for (i, layer) in layers.iter().enumerate() {
            let params = FilterParams::new(&layer.filter);
            queue.write_buffer(
                &self.filter_buffer,
                i as wgpu::BufferAddress * FilterParams::STRIDE,
                bytemuck::bytes_of(&params),
            );
        }

        let white = self.white_mask_view();
        for (i, layer) in layers.iter().enumerate() {
            let group = self.bind_group(device, i, &layer.view, &layer.mask, &white);
            self.bind_groups.push(group);
        }
        let custom: Vec<_> = layers.iter().map(|l| l.custom.as_ref()).collect();
        self.custom.prepare(device, queue, &custom);
    }

    /// The bind group of draw `index`: its texture, its clip mask, its slot of the filter
    /// buffer, and what is under it — read only by a layer that blends with it.
    fn bind_group(
        &self,
        device: &wgpu::Device,
        index: usize,
        view: &wgpu::TextureView,
        mask: &wgpu::TextureView,
        under: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frus.composite.texture.bind_group"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(mask),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.filter_buffer,
                        offset: index as wgpu::BufferAddress * FilterParams::STRIDE,
                        size: wgpu::BufferSize::new(std::mem::size_of::<FilterParams>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(under),
                },
            ],
        })
    }

    /// Points draw `index` at a texture that did not exist when the frame was
    /// prepared — which is every backdrop, since what a backdrop filters is the frame
    /// itself and the frame is drawn after this.
    fn rebind(&mut self, device: &wgpu::Device, index: usize, view: &wgpu::TextureView) {
        let white = self.white_mask_view();
        self.bind_groups[index] = self.bind_group(device, index, view, &white, &white);
    }

    /// Gives draw `index` — a layer that blends with what is under it — the copy of
    /// what is under it, which exists only once the frame has been drawn that far.
    fn rebind_under(
        &mut self,
        device: &wgpu::Device,
        index: usize,
        layer: &LayerComposite,
        under: &wgpu::TextureView,
    ) {
        self.bind_groups[index] = self.bind_group(device, index, &layer.view, &layer.mask, under);
    }

    /// Draws a **range** of the composite list. `replace_at` names the one draw that
    /// writes instead of blending, which only a backdrop asks for.
    fn draw_range<'pass>(
        &'pass self,
        pass: &mut wgpu::RenderPass<'pass>,
        range: std::ops::Range<usize>,
        replace_at: Option<usize>,
    ) {
        if range.is_empty() || self.bind_groups.is_empty() {
            return;
        }
        pass.set_bind_group(0, &self.viewport_bind_group, &[]);
        pass.set_vertex_buffer(0, self.quad_vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
        // `None` rather than `Some(false)`, so the first draw always sets a pipeline.
        let mut bound: Option<bool> = None;
        for i in range {
            if self.custom.takes(i) {
                // An application's shader: its own pipeline, the same quad and instance.
                if self.custom.bind(pass, i) {
                    pass.set_bind_group(1, &self.bind_groups[i], &[]);
                    let inst = i as u32;
                    pass.draw(0..QUAD_VERTEX_COUNT, inst..inst + 1);
                }
                bound = None;
                continue;
            }
            let replace = replace_at == Some(i);
            if bound != Some(replace) {
                pass.set_pipeline(if replace {
                    &self.pipeline_src
                } else {
                    &self.pipeline
                });
                bound = Some(replace);
            }
            pass.set_bind_group(1, &self.bind_groups[i], &[]);
            let inst = i as u32;
            pass.draw(0..QUAD_VERTEX_COUNT, inst..inst + 1);
        }
    }
}

pub(crate) fn comp_instance_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        1 => Float32x4,
        2 => Float32x4,
        3 => Float32x4,
        4 => Float32x4,
        5 => Float32x4,
        6 => Float32x4,
    ];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<CompInstance>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &ATTRS,
    }
}

/// The full set of content painters, plus layer compositing.
pub(crate) struct Painters {
    rect: Painter,
    image: ImagePainter,
    path: PathPainter,
    text: TextPainter,
    composite: CompositePainter,
    /// The separable image-filter pre-pass: blur, dilate, erode.
    filter: FilterPainter,
    /// The MSAA sample count; 1 means no multisampling.
    sample_count: u32,
    /// The intermediate MSAA texture, created on demand and recreated on resize.
    /// The multisampled scratch targets, one per size drawn at: the surface's, and each
    /// layer region's (milestone 610).
    msaa: Vec<MsaaScratch>,
    /// The staging texture a frame **with backdrops** is built in, so that a backdrop
    /// can read the frame so far. `None` until the first such frame.
    stage: Option<MsaaScratch>,
    /// Layer textures kept across frames, indexed by the layer's rank.
    layer_cache: Vec<CachedLayer>,
    /// Whether the multisampled scratch targets may be transient: the adapter says it saves
    /// memory there, a tiled GPU (milestone 796).
    transient: bool,
    /// How many layer pre-passes were actually rendered — the cache's proof.
    #[allow(dead_code)]
    layer_renders: u64,
}

impl Painters {
    /// `cache`, where the device keeps compiled pipelines between launches (Vulkan): the
    /// pipelines are built through it (milestone 685, #217).
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        sample_count: u32,
        cache: Option<&wgpu::PipelineCache>,
    ) -> Self {
        Self {
            rect: Painter::new(device, format, sample_count, cache),
            image: ImagePainter::new(device, format, sample_count, cache),
            path: PathPainter::new(device, format, sample_count, cache),
            text: TextPainter::new(device, queue, format, sample_count),
            composite: CompositePainter::new(device, queue, format, sample_count, cache),
            filter: FilterPainter::new(device, format, cache),
            sample_count,
            msaa: Vec::new(),
            stage: None,
            layer_cache: Vec::new(),
            transient: false,
            layer_renders: 0,
        }
    }

    /// Lets the multisampled scratch targets be transient, where the adapter says that saves
    /// memory (`AdapterInfo::transient_saves_memory`): a phone's GPU keeps them in its tile
    /// memory, and a full-screen one stops costing 40 MB (milestone 796).
    pub(crate) fn set_transient_scratch(&mut self, transient: bool) {
        self.transient = transient;
    }

    /// The GPU memory the painters hold in textures of their own, at the end of the last
    /// frame: the layers kept, the multisampled scratch targets (a transient one counts for
    /// nothing), the staging texture, the images, the glyph atlases and the GPU passes'
    /// pictures (milestone 796).
    pub(crate) fn memory(&self) -> crate::GpuMemory {
        let mut memory = crate::GpuMemory::default();
        let mut add = |texture: &wgpu::Texture, counted: bool| {
            memory.textures += 1;
            if counted {
                memory.bytes += texture_bytes(texture);
            }
        };
        for layer in &self.layer_cache {
            add(&layer.texture, true);
        }
        for scratch in &self.msaa {
            add(&scratch.texture, !scratch.transient);
        }
        if let Some(stage) = &self.stage {
            add(&stage.texture, true);
        }
        self.image.each_texture(|t| add(t, true));
        self.composite.custom.each_texture(|t| add(t, true));
        // The two glyph atlases, mask and colour.
        memory.textures += 2;
        memory.bytes += self.text.atlas_bytes();
        memory
    }

    /// Layer pre-passes rendered since creation, for the cache test.
    #[cfg(test)]
    pub(crate) fn layer_render_count(&self) -> u64 {
        self.layer_renders
    }

    /// Returns a fresh view of the MSAA texture, recreating it when the size or the
    /// format changes, or `None` when MSAA is off (`sample_count == 1`). The view is
    /// created on the fly — it is cheap — and returned by value, which releases the
    /// borrow of `self` before the render pass opens.
    ///
    /// `one_pass` when the draw is done in a single pass that clears first: its scratch may
    /// then be transient. A draw cut into segments reloads it, and needs one that is kept.
    fn ensure_msaa(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        w: u32,
        h: u32,
        one_pass: bool,
    ) -> Option<wgpu::TextureView> {
        if self.sample_count == 1 {
            return None;
        }
        let transient = self.transient && one_pass;
        let at = self.msaa.iter().position(|s| {
            s.width == w && s.height == h && s.format == format && s.transient == transient
        });
        let at = match at {
            Some(at) => at,
            None => {
                // A handful of sizes are drawn at from one frame to the next; a list
                // that grows past that is regions that have changed, and is started
                // again rather than kept.
                if self.msaa.len() >= 16 {
                    self.msaa.clear();
                }
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("frus.msaa.scratch"),
                    size: wgpu::Extent3d {
                        width: w.max(1),
                        height: h.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: self.sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: if transient {
                        wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TRANSIENT_ATTACHMENT
                    } else {
                        wgpu::TextureUsages::RENDER_ATTACHMENT
                    },
                    view_formats: &[],
                });
                self.msaa.push(MsaaScratch {
                    width: w,
                    height: h,
                    format,
                    texture,
                    transient,
                    used: false,
                });
                self.msaa.len() - 1
            }
        };
        self.msaa[at].used = true;
        Some(
            self.msaa[at]
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
        )
    }

    /// Ends a frame: lets go of the scratch targets and the staging texture no draw of it
    /// used (milestone 796). A layer drawn once and then kept as it is no longer needs the
    /// multisampled target it was drawn in, four times its own size; a page that blurred
    /// what was under it and is gone no longer needs a copy of the screen. Kept, they were
    /// memory a page left behind it for as long as the application ran.
    fn end_frame(&mut self) {
        self.msaa.retain(|s| s.used);
        for scratch in &mut self.msaa {
            scratch.used = false;
        }
        if self.stage.as_ref().is_some_and(|s| !s.used) {
            self.stage = None;
        }
        if let Some(stage) = &mut self.stage {
            stage.used = false;
        }
    }

    fn set_viewport(&self, queue: &wgpu::Queue, w: f32, h: f32) {
        self.rect.set_viewport(queue, w, h);
        self.image.set_viewport(queue, w, h);
        self.path.set_viewport(queue, w, h);
    }

    /// Renders `scene`, layers included, into `target` of size `w×h`. `clear` is
    /// `Some(colour)` to clear the target, `None` to paint over it. **Submitted
    /// internally**: one pass per layer, plus the main pass — and, when a layer asks
    /// for a backdrop, one more pass per backdrop.
    ///
    /// Note: under MSAA, `clear == None` — painting over — is not supported, since
    /// the multisampled target does not hold `target`'s existing content. Every
    /// current caller passes `Some(_)`.
    // The device, the queue, the target and its size: a render call's irreducible
    // arguments.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        target: &wgpu::TextureView,
        w: u32,
        h: u32,
        scene: &Scene,
        clear: Option<wgpu::Color>,
    ) {
        self.text.begin_frame();
        self.render_frame(device, queue, format, target, w, h, scene, clear);
        self.end_frame();
    }

    /// [`Painters::render`]'s frame, between its start and its end.
    #[allow(clippy::too_many_arguments)]
    fn render_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        target: &wgpu::TextureView,
        w: u32,
        h: u32,
        scene: &Scene,
        clear: Option<wgpu::Color>,
    ) {
        self.set_viewport(queue, w as f32, h as f32);

        // Pre-passes: each layer into its own full-surface texture — but **reused**
        // as is when its content has not changed, which is the GPU-side cache.
        //
        // The result is a flat list of composite draws in the order they happen. A
        // layer that asks for a backdrop contributes **two** entries: the filtered
        // copy of what is underneath, and then the layer itself over it.
        let mut draws: Vec<LayerComposite> = Vec::new();
        let mut cuts: Vec<Cut> = Vec::new();
        // Which composite draws each layer issues, by its index in the scene. The batch
        // planner orders layers among the content, and this is how a layer batch finds
        // the draws that belong to it.
        let mut layer_draws: HashMap<usize, std::ops::Range<usize>> = HashMap::new();
        let mut layer_index = 0usize;
        for (scene_index, primitive) in scene.primitives().iter().enumerate() {
            let folded = folded_clip(primitive);
            let primitive = folded.as_ref().unwrap_or(primitive);
            if let Primitive::Layer {
                primitives,
                opacity,
                clip,
                clip_shape,
                transform,
                filter,
                ..
            } = primitive
            {
                let region = layer_region(*clip, clip_shape, transform, filter, primitives, w, h);
                let (rw, rh) = region.map_or((w, h), |r| (r.width as u32, r.height as u32));
                // A shader filling a box and a GPU pass have no content to render first
                // (milestone 792); they take no slot of the layer cache either.
                let bare = custom_only(filter, primitives);
                let view = if bare {
                    self.composite.white_mask_view()
                } else {
                    let local = into_region(primitives, region);
                    let view = self.layer_texture(
                        device,
                        queue,
                        format,
                        layer_index,
                        &local,
                        filter.image.filter(|f| !f.is_identity()),
                        rw,
                        rh,
                    );
                    layer_index += 1;
                    view
                };
                let mut entry = self.layer_entry(
                    device, queue, format, view, *opacity, *clip, clip_shape, transform, filter, w,
                    h,
                );
                entry.region = region.map(|r| r.to_array());
                if !self.attach_custom(device, queue, &mut entry, filter, bare, w, h) {
                    continue;
                }
                let first_draw = draws.len();
                // The backdrop goes **first**: it is a picture of what was already
                // there, and the layer is painted over it. It borrows the layer's clip
                // and shape, which is the region it applies to.
                if let Some(backdrop) = filter.backdrop {
                    cuts.push(Cut {
                        at: draws.len(),
                        scene: scene_index,
                        kind: CutKind::Backdrop {
                            filter: backdrop.filter,
                            replace: backdrop.blend == frus_core::BlendMode::Src,
                            key: backdrop.key,
                        },
                    });
                    draws.push(LayerComposite {
                        // Filled in once the frame so far has actually been drawn;
                        // until then any texture will do, since nothing samples it.
                        view: self.composite.white_mask_view(),
                        mask: self.composite.white_mask_view(),
                        opacity: 1.0,
                        clip: entry.clip,
                        shape: entry.shape,
                        radii: entry.radii,
                        // The copy is of the screen, at the screen's own coordinates.
                        inverse: frus_core::Affine::IDENTITY.m,
                        filter: LayerFilter::NONE,
                        region: None,
                        custom: None,
                    });
                } else if filter.blend.is_some() {
                    // A layer that blends reads what is under it: the frame is cut at it
                    // too, and the layer itself is the draw that reads.
                    cuts.push(Cut {
                        at: draws.len(),
                        scene: scene_index,
                        kind: CutKind::Blend,
                    });
                }
                draws.push(entry);
                layer_draws.insert(scene_index, first_draw..draws.len());
            }
        }
        // Forget vanished layers: the scene has fewer than it had last frame.
        self.layer_cache.truncate(layer_index);
        // Every GPU pass of the frame has run: the pictures of those gone go.
        self.composite.custom.end_frame();
        // Each layer set the painters to its own size: the frame is the surface's again.
        self.set_viewport(queue, w as f32, h as f32);

        self.composite
            .prepare(device, queue, &draws, w as f32, h as f32);
        // What may share a draw call, and in what order — see `batch`. The plan comes
        // first now: text is interleaved with the rest, so the text painter needs to
        // know the batches before it can prepare a renderer for each.
        let batches = batch::plan(scene);
        let (decorations, decoration_ranges) = self
            .text
            .prepare_frame(device, queue, scene, w, h, &batches);
        let (rect_ranges, decoration_base) =
            self.rect
                .prepare_frame(device, queue, scene, &decorations, &batches);
        let image_ranges = self.image.prepare_frame(device, queue, scene, &batches);
        let path_ranges = self.path.prepare_frame(device, queue, scene, &batches);
        // The frame's layers were prepared before it: the kept triangles may age now,
        // and the textures no draw used go.
        self.path.end_frame();
        self.image.end_frame();
        // The text painter keeps one renderer per text batch, in order; a pass drawing
        // batches `a..b` needs to know how many of them came before `a`.
        let mut text_slots = Vec::with_capacity(batches.len());
        let mut slots = 0usize;
        for batch in &batches {
            text_slots.push(slots);
            if batch.kind == batch::Kind::Text {
                slots += 1;
            }
        }
        let content = ContentPlan {
            batches: &batches,
            rect_ranges: &rect_ranges,
            image_ranges: &image_ranges,
            path_ranges: &path_ranges,
            decoration_ranges: &decoration_ranges,
            decoration_base: decoration_base.start,
            layer_draws: &layer_draws,
            text_slots: &text_slots,
        };

        // With MSAA we paint into the multisampled texture then resolve; without it
        // we paint straight into whatever the pass is aimed at.
        // One pass that clears: the scratch may be transient.
        let one_pass = cuts.is_empty() && clear.is_some();
        let msaa_view = self.ensure_msaa(device, format, w, h, one_pass);
        let load = match clear {
            Some(c) => wgpu::LoadOp::Clear(c),
            None => wgpu::LoadOp::Load,
        };

        if cuts.is_empty() {
            // The ordinary frame: one pass, straight into the target, exactly as it
            // was before backdrops existed.
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frus.encoder"),
            });
            self.pass(
                &mut encoder,
                msaa_view.as_ref(),
                false,
                target,
                load,
                &content,
                0..batches.len(),
                None,
            );
            queue.submit(std::iter::once(encoder.finish()));
            return;
        }

        // A backdrop is a filter of **what is already painted**, and a blended layer
        // meets it, so the frame has to be readable half-way through. It cannot be read
        // out of `target`: a surface texture is a render attachment and nothing else. So
        // the frame is built in a staging texture we own, cut into segments at each
        // backdrop and blend, and blitted to the target at the end.
        let stage = self.stage(device, format, w, h).clone();
        self.draw_segmented(
            device,
            queue,
            format,
            msaa_view.as_ref(),
            &stage,
            load,
            &content,
            &cuts,
            &draws,
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frus.encoder.blit"),
        });
        // And the staged frame onto the target it was always meant for.
        let stage_view = stage.create_view(&wgpu::TextureViewDescriptor::default());
        self.filter
            .blit(device, queue, &mut encoder, &stage_view, target);
        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Draws `content` into `target`, stopping at each **cut** to read what has been
    /// drawn so far: filtered for a backdrop, copied as it is for a layer that blends
    /// with it. The cut's draw is then pointed at what was read, and replaces what is
    /// under it rather than painting over it. With no cuts, one pass.
    ///
    /// The top of the frame does this in its staging texture; a group does it in its
    /// own, so a layer blending inside a group meets the group's own drawing.
    #[allow(clippy::too_many_arguments)]
    fn draw_segmented(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        msaa_view: Option<&wgpu::TextureView>,
        target: &wgpu::Texture,
        load: wgpu::LoadOp<wgpu::Color>,
        content: &ContentPlan,
        cuts: &[Cut],
        draws: &[LayerComposite],
    ) {
        let (w, h) = (target.width(), target.height());
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        // Backdrops sharing a key are filtered **once**: the copy is taken before the
        // first of them and every other one reads the same texture. That is what
        // turns a list of frosted rows from one full-surface blur per row into one.
        let mut shared: HashMap<u64, wgpu::Texture> = HashMap::new();
        // The frame is cut at the **batch** that draws the cut's layer, because that is
        // where "what is already painted" ends now that layers are ordered among the
        // content rather than after it.
        let batches = content.batches;
        let cut_at = |scene: usize| {
            batches
                .iter()
                .position(|b| b.kind == batch::Kind::Layer && b.members.first() == Some(&scene))
                .unwrap_or(batches.len())
        };
        let mut cursor = 0usize;
        let mut first = true;
        // Every segment after the first **starts** with a cut — that is what ends the
        // segment before it — so a segment has at most one draw that replaces, and this
        // is it.
        let mut replace_at: Option<usize> = None;
        for cut in cuts {
            {
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("frus.encoder.segment"),
                });
                // Kept: the next segment goes on drawing over it.
                self.pass(
                    &mut encoder,
                    msaa_view,
                    true,
                    &view,
                    if first { load } else { wgpu::LoadOp::Load },
                    content,
                    cursor..cut_at(cut.scene),
                    replace_at,
                );
                // Submitted here and not at the end: what follows reads the target, and
                // a command buffer still being recorded has not written it yet.
                queue.submit(std::iter::once(encoder.finish()));
            }
            first = false;
            match cut.kind {
                CutKind::Backdrop {
                    filter,
                    replace,
                    key,
                } => {
                    let reuse = key
                        .and_then(|k| shared.get(&k))
                        .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()));
                    let filtered = match reuse {
                        Some(view) => view,
                        None => {
                            let filtered = self
                                .filter
                                .apply(device, queue, format, target, filter, w, h);
                            let view =
                                filtered.create_view(&wgpu::TextureViewDescriptor::default());
                            if let Some(k) = key {
                                shared.insert(k, filtered);
                            }
                            view
                        }
                    };
                    self.composite.rebind(device, cut.at, &filtered);
                    replace_at = replace.then_some(cut.at);
                }
                CutKind::Blend => {
                    let under = copy_of(device, queue, format, target, w, h)
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    self.composite
                        .rebind_under(device, cut.at, &draws[cut.at], &under);
                    replace_at = Some(cut.at);
                }
            }
            cursor = cut_at(cut.scene);
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frus.encoder.tail"),
        });
        self.pass(
            &mut encoder,
            msaa_view,
            false,
            &view,
            if first { load } else { wgpu::LoadOp::Load },
            content,
            cursor..batches.len(),
            replace_at,
        );
        queue.submit(std::iter::once(encoder.finish()));
    }

    /// Turns one [`Primitive::Layer`] into the composite draw that puts it on the
    /// screen: its clip shape as an SDF kind plus radii, or a rendered coverage mask
    /// for a free path, and the **inverse** of its transform, since the fragment
    /// samples at the counter-transformed position.
    ///
    /// Shared by the top-level frame and by a group compositing the layers nested
    /// inside it: the two ask exactly the same question.
    #[allow(clippy::too_many_arguments)]
    fn layer_entry(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        view: wgpu::TextureView,
        opacity: f32,
        clip: Rect,
        clip_shape: &frus_core::ClipShape,
        transform: &Option<frus_core::LayerTransform>,
        filter: &LayerFilter,
        w: u32,
        h: u32,
    ) -> LayerComposite {
        let inverse = match transform {
            Some(t) => t.affine.inverse().m,
            None => frus_core::Affine::IDENTITY.m,
        };
        let (shape, radii) = match clip_shape {
            frus_core::ClipShape::Rect => ([0.0, 0.0, 0.0, 0.0], [0.0; 4]),
            frus_core::ClipShape::RRect(br) => ([1.0, 0.0, 0.0, 0.0], br.to_array()),
            frus_core::ClipShape::Oval => ([2.0, 0.0, 0.0, 0.0], [0.0; 4]),
            frus_core::ClipShape::Path(_) => ([3.0, 0.0, 0.0, 0.0], [0.0; 4]),
        };
        let mask = match clip_shape {
            frus_core::ClipShape::Path(path) => self.render_mask(device, queue, format, path, w, h),
            _ => self.composite.white_mask_view(),
        };
        LayerComposite {
            view,
            mask,
            opacity,
            clip: clip.to_array(),
            shape,
            radii,
            inverse,
            // The image filter has already been spent, in the pre-pass; the backdrop
            // is a draw of its own. A layer over a backdrop is painted over it: the
            // frame is cut once at a layer, for its backdrop.
            filter: LayerFilter {
                image: None,
                backdrop: None,
                blend: filter.blend.filter(|_| filter.backdrop.is_none()),
                ..filter.clone()
            },
            region: None,
            custom: None,
        }
    }

    /// One render pass: the content batches (when `content` is given — only the first
    /// pass of a frame draws them) followed by a range of composite draws.
    ///
    /// `replace_at` names a draw that **replaces** rather than paints over, which only
    /// a backdrop asks for.
    // A pass is its attachment, its load, and what to put in it; the arguments are the
    // pass.
    #[allow(clippy::too_many_arguments)]
    /// One render pass over a **range of batches**, in order.
    ///
    /// The order is the whole point. A layer is composited from a texture of its own,
    /// and for a long time every layer was composited after all of the content — so a
    /// group that something had covered came back on top of it. A device found it as the
    /// home screen's translucent square painted over the Kanban board (milestone 349).
    /// The batches now hold the layers too, in the order they must be drawn, and this
    /// walks them: content pipelines and composite draws interleaved.
    ///
    /// `keep_msaa` when a pass after this one goes on drawing in the same multisampled
    /// target: its samples are then stored. Otherwise only the resolved picture is written
    /// and the samples are discarded — on a phone's tiled GPU they never leave the tile
    /// memory, which saves writing four times the target's size every pass (milestone 796).
    fn pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        msaa_view: Option<&wgpu::TextureView>,
        keep_msaa: bool,
        target: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
        content: &ContentPlan,
        batches: std::ops::Range<usize>,
        replace_at: Option<usize>,
    ) {
        // An empty range still has to run when the pass is the one that clears: a frame
        // with nothing in it is a frame of the clear colour, not a frame of whatever was
        // in the buffer before.
        if batches.is_empty() && !matches!(load, wgpu::LoadOp::Clear(_)) {
            return;
        }
        let (view, resolve_target, store) = match msaa_view {
            Some(msaa) if !keep_msaa => (msaa, Some(target), wgpu::StoreOp::Discard),
            Some(msaa) => (msaa, Some(target), wgpu::StoreOp::Store),
            None => (target, None, wgpu::StoreOp::Store),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("frus.render_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations { load, store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        for i in batches {
            let batch = &content.batches[i];
            match batch.kind {
                batch::Kind::Rect => self.rect.draw(&mut pass, content.rect_ranges[i].clone()),
                batch::Kind::Image => self.image.draw(&mut pass, content.image_ranges[i].clone()),
                batch::Kind::Path => self.path.draw(&mut pass, content.path_ranges[i].clone()),
                batch::Kind::Text => {
                    // The underlines first — they are rectangles, and they belong
                    // beneath the glyphs they decorate rather than in a batch of
                    // their own.
                    let base = content.decoration_base;
                    let d = &content.decoration_ranges[i];
                    self.rect.draw(&mut pass, base + d.start..base + d.end);
                    self.text.draw(&mut pass, content.text_slots[i]);
                }
                batch::Kind::Layer => {
                    // One member, always: the scene index of the group. Its draws are
                    // the layer itself, preceded by its backdrop when it takes one.
                    if let Some(range) = batch
                        .members
                        .first()
                        .and_then(|scene| content.layer_draws.get(scene))
                    {
                        self.composite
                            .draw_range(&mut pass, range.clone(), replace_at);
                    }
                }
            }
        }
    }

    /// Gives `entry` its custom draw when its layer is an application's shader or GPU pass
    /// (milestone 792): runs the pass, puts the paint's images on the GPU. `false` when
    /// the layer is to draw nothing — a pass with no picture — and is to be left out.
    #[allow(clippy::too_many_arguments)]
    fn attach_custom(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        entry: &mut LayerComposite,
        filter: &LayerFilter,
        bare: bool,
        w: u32,
        h: u32,
    ) -> bool {
        if let Some(pass) = &filter.external {
            return match self.composite.custom.run_pass(device, queue, pass) {
                Some(view) => {
                    entry.custom = Some(crate::custom::CustomDraw::external(pass, view));
                    true
                }
                None => false,
            };
        }
        let Some(paint) = &filter.shader else {
            return true;
        };
        let images = paint.images().each_ref().map(|i| {
            i.as_ref()
                .map(|img| self.image.texture_view(device, queue, img))
        });
        let region = entry
            .region
            .unwrap_or([0.0, 0.0, w.max(1) as f32, h.max(1) as f32]);
        entry.custom = Some(crate::custom::CustomDraw::shader(
            paint, !bare, region, images,
        ));
        true
    }

    /// Whether a custom pipeline was made since the last call, so that the pipeline cache
    /// is worth writing again (milestone 792).
    pub(crate) fn take_new_pipelines(&mut self) -> bool {
        self.composite.custom.take_new_pipelines()
    }

    /// The staging texture a frame with backdrops is built in, created on demand and
    /// recreated on resize. It exists because a backdrop has to *read* the frame so
    /// far, and a surface texture cannot be read.
    fn stage(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        w: u32,
        h: u32,
    ) -> &wgpu::Texture {
        let stale = match &self.stage {
            Some(s) => s.width != w || s.height != h || s.format != format,
            None => true,
        };
        if stale {
            self.stage = Some(MsaaScratch {
                width: w,
                height: h,
                format,
                transient: false,
                used: false,
                texture: device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("frus.stage"),
                    size: wgpu::Extent3d {
                        width: w.max(1),
                        height: h.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    // Copied from, for a layer that blends with it.
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                }),
            });
        }
        let stage = self.stage.as_mut().expect("just created");
        stage.used = true;
        &stage.texture
    }

    /// Returns the view of a layer's texture: **reused** as is when its content and
    /// dimensions are unchanged since the previous frame, otherwise (re)rendered by
    /// [`Painters::render_group_into`] and, if the layer asks for one, put through the
    /// image-filter pre-pass. `index` is the layer's rank in the scene, a stable
    /// cache key; a key that slips — because layers were reordered — only misses the
    /// cache, which re-renders correctly and never wrongly.
    // Same as `render`, one layer at a time.
    #[allow(clippy::too_many_arguments)]
    fn layer_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        index: usize,
        primitives: &[Primitive],
        image: Option<ImageFilter>,
        w: u32,
        h: u32,
    ) -> wgpu::TextureView {
        let hit = matches!(
            self.layer_cache.get(index),
            Some(c) if c.width == w && c.height == h && c.image == image
                && c.primitives.as_slice() == primitives
        );
        if !hit {
            // The texture it had, when it is the same size and nothing was filtered into it:
            // drawn into again rather than made anew. A layer whose content moves every frame
            // (a page coming in while it animates) otherwise made a texture the size of the
            // screen every frame, which a phone's driver allocates and clears each time
            // (milestone 796).
            let reuse = match self.layer_cache.get(index) {
                Some(c) if c.width == w && c.height == h && c.image.is_none() => {
                    Some(c.texture.clone())
                }
                _ => None,
            };
            let mut texture =
                self.render_group_into(device, queue, format, primitives, w, h, reuse);
            if let Some(f) = image {
                texture = self.filter.apply(device, queue, format, &texture, f, w, h);
            }
            let entry = CachedLayer {
                primitives: primitives.to_vec(),
                width: w,
                height: h,
                texture,
                image,
            };
            if index < self.layer_cache.len() {
                self.layer_cache[index] = entry;
            } else {
                self.layer_cache.push(entry);
            }
        }
        self.layer_cache[index]
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// Renders a **clip mask**: `path`, in absolute screen coordinates, filled with
    /// opaque white on a transparent background, at the surface's size. The returned
    /// view keeps the texture alive, since wgpu counts references.
    fn render_mask(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        path: &frus_core::Path,
        w: u32,
        h: u32,
    ) -> wgpu::TextureView {
        let prim = Primitive::Path {
            path: path.clone(),
            fill: Some(frus_core::Color::WHITE),
            // A coverage mask is flat by definition.
            gradient: None,
            stroke: None,
            clip: frus_core::Rect::UNBOUNDED,
            owner: 0,
        };
        let tex = self.render_group_into(device, queue, format, &[prim], w, h, None);
        tex.create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// Renders a group of primitives into a full-surface texture with a transparent
    /// background, for later compositing.
    ///
    /// Into `reuse` when it is given — a texture of this size and format, the layer's own from
    /// an earlier frame — and into a new one otherwise.
    #[allow(clippy::too_many_arguments)]
    fn render_group_into(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        primitives: &[Primitive],
        w: u32,
        h: u32,
        reuse: Option<wgpu::Texture>,
    ) -> wgpu::Texture {
        self.layer_renders += 1;

        // **Nested** layers first, depth-first: each rendered into a texture of its
        // own so that this group's pass can composite it. Without this a layer inside
        // a layer simply vanished — a rounded card around a fading group, a clip
        // around a transform — because a group's pass draws primitives and a layer is
        // not one.
        //
        // The recursion is safe to interleave with the shared instance buffers because
        // every level prepares, records and **submits** before returning: the deepest
        // group is on the queue before its parent writes a single instance.
        let mut nested: Vec<LayerComposite> = Vec::new();
        // Which nested composite belongs to which primitive, so the pass below can put
        // it back in scene order among this group's own drawing rather than after all
        // of it — the same ordering the top-level frame does.
        let mut nested_of: HashMap<usize, usize> = HashMap::new();
        // The nested layers that blend with this group's drawing under them.
        let mut cuts: Vec<Cut> = Vec::new();
        for (scene_index, primitive) in primitives.iter().enumerate() {
            let folded = folded_clip(primitive);
            let primitive = folded.as_ref().unwrap_or(primitive);
            if let Primitive::Layer {
                primitives: inner,
                opacity,
                clip,
                clip_shape,
                transform,
                filter,
                ..
            } = primitive
            {
                let region = layer_region(*clip, clip_shape, transform, filter, inner, w, h);
                let (rw, rh) = region.map_or((w, h), |r| (r.width as u32, r.height as u32));
                let bare = custom_only(filter, inner);
                let view = if bare {
                    self.composite.white_mask_view()
                } else {
                    let local = into_region(inner, region);
                    let mut texture =
                        self.render_group_into(device, queue, format, &local, rw, rh, None);
                    if let Some(f) = filter.image.filter(|f| !f.is_identity()) {
                        texture = self
                            .filter
                            .apply(device, queue, format, &texture, f, rw, rh);
                    }
                    // The view holds a reference to its texture, so the texture outlives
                    // this loop without being named again.
                    texture.create_view(&wgpu::TextureViewDescriptor::default())
                };
                let mut entry = self.layer_entry(
                    device, queue, format, view, *opacity, *clip, clip_shape, transform, filter, w,
                    h,
                );
                entry.region = region.map(|r| r.to_array());
                if !self.attach_custom(device, queue, &mut entry, filter, bare, w, h) {
                    continue;
                }
                nested_of.insert(scene_index, nested.len());
                if filter.blend.is_some() && filter.backdrop.is_none() {
                    cuts.push(Cut {
                        at: nested.len(),
                        scene: scene_index,
                        kind: CutKind::Blend,
                    });
                }
                nested.push(entry);
            }
        }

        let mut sub = Scene::new();
        for primitive in primitives {
            sub.push_primitive(primitive.clone());
        }

        let texture = reuse.unwrap_or_else(|| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("frus.layer.texture"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                // Copied from, for a nested layer that blends with it.
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        });

        // Its own size for the painters: a nested layer set them to its own.
        self.set_viewport(queue, w as f32, h as f32);
        let batches = batch::plan(&sub);
        let (decorations, decoration_ranges) =
            self.text.prepare_frame(device, queue, &sub, w, h, &batches);
        let (rect_ranges, decoration_base) =
            self.rect
                .prepare_frame(device, queue, &sub, &decorations, &batches);
        let image_ranges = self.image.prepare_frame(device, queue, &sub, &batches);
        let path_ranges = self.path.prepare_frame(device, queue, &sub, &batches);
        if !nested.is_empty() {
            self.composite
                .prepare(device, queue, &nested, w as f32, h as f32);
        }

        // With MSAA the layer is painted multisampled then resolved to its
        // single-sample texture, the one the compositor samples afterwards.
        let msaa_view = self.ensure_msaa(device, format, w, h, cuts.is_empty());

        // The same plan the top of the frame draws from: a nested layer in its place
        // among this group's own drawing.
        let mut text_slots = Vec::with_capacity(batches.len());
        let mut slots = 0usize;
        for batch in &batches {
            text_slots.push(slots);
            if batch.kind == batch::Kind::Text {
                slots += 1;
            }
        }
        let layer_draws: HashMap<usize, std::ops::Range<usize>> =
            nested_of.iter().map(|(&s, &n)| (s, n..n + 1)).collect();
        let content = ContentPlan {
            batches: &batches,
            rect_ranges: &rect_ranges,
            image_ranges: &image_ranges,
            path_ranges: &path_ranges,
            decoration_ranges: &decoration_ranges,
            decoration_base: decoration_base.start,
            layer_draws: &layer_draws,
            text_slots: &text_slots,
        };
        // Transparent background: a layer covers only its primitives.
        self.draw_segmented(
            device,
            queue,
            format,
            msaa_view.as_ref(),
            &texture,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &content,
            &cuts,
            &nested,
        );
        texture
    }

    /// **Warms up** every pipeline by rendering a small scene that exercises each
    /// render path — rectangle, image, path, text, layer → composite — so the first
    /// real frame compiles nothing.
    pub(crate) fn warm_up(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) {
        let mut scene = Scene::new();
        scene.fill_rect(Rect::new(0.0, 0.0, 2.0, 2.0), Color::WHITE);
        scene.fill_path(&Path::rect(Rect::new(0.0, 0.0, 2.0, 2.0)), Color::WHITE);
        let img = ImageData::from_rgba(1, 1, vec![255, 255, 255, 255]).into_handle();
        scene.image(&img, Rect::new(0.0, 0.0, 2.0, 2.0), BoxFit::Fill);
        scene.text(
            Point::new(0.0, 0.0),
            "x",
            &frus_core::ResolvedTextStyle::exact(8.0),
            Color::WHITE,
        );
        scene.layer(0.5, |inner| {
            inner.fill_rect(Rect::new(0.0, 0.0, 2.0, 2.0), Color::WHITE)
        });

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frus.warmup.texture"),
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.render(
            device,
            queue,
            format,
            &view,
            4,
            4,
            &scene,
            Some(wgpu::Color::BLACK),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headless() -> Option<(wgpu::Device, wgpu::Queue)> {
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
            label: Some("frus.compositor.test.device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
            ..Default::default()
        }))
        .ok()
    }

    fn target(device: &wgpu::Device, format: wgpu::TextureFormat, n: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frus.compositor.test.target"),
            size: wgpu::Extent3d {
                width: n,
                height: n,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }

    /// A **static** layer is rendered once: the second frame reuses its texture, with
    /// no new pre-pass. Changing its content forces a re-render; removing it purges
    /// the cache.
    #[test]
    fn static_layer_texture_is_reused_across_frames() {
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        // sample_count 1: the cache does not depend on MSAA.
        let mut painters = Painters::new(&device, &queue, format, 1, None);
        let tex = target(&device, format, 16);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let clear = Some(wgpu::Color::BLACK);

        let red =
            |s: &mut Scene| s.fill_rect(Rect::new(0.0, 0.0, 8.0, 8.0), Color::rgb(1.0, 0.0, 0.0));
        let mut scene = Scene::new();
        scene.layer(0.5, red);

        painters.render(&device, &queue, format, &view, 16, 16, &scene, clear);
        assert_eq!(
            painters.layer_render_count(),
            1,
            "first frame: layer rendered"
        );
        painters.render(&device, &queue, format, &view, 16, 16, &scene, clear);
        assert_eq!(
            painters.layer_render_count(),
            1,
            "layer unchanged: texture reused"
        );

        // Content changed → re-render.
        let mut scene2 = Scene::new();
        scene2.layer(0.5, |s| {
            s.fill_rect(Rect::new(0.0, 0.0, 8.0, 8.0), Color::rgb(0.0, 1.0, 0.0))
        });
        painters.render(&device, &queue, format, &view, 16, 16, &scene2, clear);
        assert_eq!(
            painters.layer_render_count(),
            2,
            "content changed: re-render"
        );

        // Layer gone → the cache is purged, with no pre-pass.
        painters.render(&device, &queue, format, &view, 16, 16, &Scene::new(), clear);
        assert_eq!(
            painters.layer_render_count(),
            2,
            "no layer left: nothing to render"
        );
        assert!(painters.layer_cache.is_empty(), "cache purged");
    }

    fn layer(
        clip: Rect,
        primitives: Vec<Primitive>,
    ) -> (
        Rect,
        frus_core::ClipShape,
        Option<frus_core::LayerTransform>,
        LayerFilter,
        Vec<Primitive>,
    ) {
        (
            clip,
            frus_core::ClipShape::Rect,
            None,
            LayerFilter::NONE,
            primitives,
        )
    }

    fn square() -> Primitive {
        let mut scene = Scene::new();
        scene.fill_rect(Rect::new(10.0, 10.0, 20.0, 20.0), frus_core::Color::WHITE);
        scene.primitives()[0].clone()
    }

    /// **A layer's region is its clip in whole pixels, held to the target** (milestone
    /// 610): a texture of that size, not the target's.
    #[test]
    fn a_layer_is_drawn_in_its_clip_and_no_more() {
        // Content over the whole target: the clip decides.
        let mut everywhere = Scene::new();
        everywhere.fill_rect(Rect::new(-100.0, -100.0, 3000.0, 3000.0), Color::WHITE);
        let (clip, shape, transform, filter, prims) = layer(
            Rect::new(10.5, 20.2, 56.0, 32.0),
            everywhere.primitives().to_vec(),
        );
        let region = layer_region(clip, &shape, &transform, &filter, &prims, 1080, 2340);
        assert_eq!(region, Some(Rect::new(10.0, 20.0, 57.0, 33.0)));
        // Partly off the target: what is on it.
        let region = layer_region(
            Rect::new(-30.0, 2300.0, 100.0, 100.0),
            &shape,
            &transform,
            &filter,
            &prims,
            1080,
            2340,
        );
        assert_eq!(region, Some(Rect::new(0.0, 2300.0, 70.0, 40.0)));
        // Wholly off it: a pixel, which draws nothing.
        let region = layer_region(
            Rect::new(2000.0, 10.0, 50.0, 50.0),
            &shape,
            &transform,
            &filter,
            &prims,
            1080,
            2340,
        );
        assert_eq!(region.map(|r| (r.width, r.height)), Some((1.0, 1.0)));
        // Covering it all, or unbounded: the target, as before.
        assert_eq!(
            layer_region(
                Rect::UNBOUNDED,
                &shape,
                &transform,
                &filter,
                &prims,
                1080,
                2340
            ),
            None
        );
    }

    /// **A layer's region is no larger than what it holds** (milestone 796): a group whose
    /// clip is the whole screen and which holds a 20 px square gets a texture around the
    /// square, with a margin, not one the size of the screen; and a shader's box counts as
    /// held.
    #[test]
    fn a_layer_is_drawn_around_what_it_holds() {
        let none = LayerFilter::NONE;
        let region = layer_region(
            Rect::UNBOUNDED,
            &frus_core::ClipShape::Rect,
            &None,
            &none,
            &[square()],
            1080,
            2340,
        );
        // The square is (10, 10, 20, 20); 16 px around it, held to the target.
        assert_eq!(region, Some(Rect::new(0.0, 0.0, 46.0, 46.0)));
        let shader = frus_core::Shader::from_wgsl(
            "fn shade(frag: Fragment) -> vec4<f32> {
    return vec4<f32>(1.0);
}
",
        )
        .expect("the test shader compiles");
        let masked = LayerFilter {
            shader: Some(Box::new(
                shader.paint().at(Rect::new(100.0, 200.0, 300.0, 50.0)),
            )),
            ..LayerFilter::NONE
        };
        let region = layer_region(
            Rect::UNBOUNDED,
            &frus_core::ClipShape::Rect,
            &None,
            &masked,
            &[square()],
            1080,
            2340,
        );
        assert_eq!(region, Some(Rect::new(0.0, 0.0, 416.0, 266.0)));
        // A line of text laid out in a box narrower than it draws: the whole width, and its
        // own height again above and below, plus the margin.
        let mut title = Scene::new();
        title.set_bounds(Rect::new(40.0, 400.0, 10.0, 30.0));
        title.text(
            frus_core::Point::new(40.0, 400.0),
            "Library",
            &frus_core::ResolvedTextStyle::exact(24.0),
            Color::WHITE,
        );
        let region = layer_region(
            Rect::new(0.0, 0.0, 1080.0, 2340.0),
            &frus_core::ClipShape::Rect,
            &None,
            &none,
            title.primitives(),
            1080,
            2340,
        );
        assert_eq!(region, Some(Rect::new(0.0, 354.0, 1080.0, 122.0)));
    }

    /// **What may draw past its clip, or holds the target's own coordinates, keeps the
    /// target**: a transform, an image filter, a backdrop, a path clip, or such a layer
    /// anywhere inside.
    #[test]
    fn what_may_draw_anywhere_keeps_the_whole_target() {
        let clip = Rect::new(10.0, 10.0, 100.0, 100.0);
        let none = LayerFilter::NONE;
        let moved = Some(frus_core::LayerTransform::new(
            frus_core::Affine::translation(5.0, 0.0),
        ));
        assert_eq!(
            layer_region(
                clip,
                &frus_core::ClipShape::Rect,
                &moved,
                &none,
                &[square()],
                1080,
                2340
            ),
            None
        );
        let blurred = LayerFilter {
            image: Some(ImageFilter::Blur {
                sigma_x: 4.0,
                sigma_y: 4.0,
            }),
            ..LayerFilter::NONE
        };
        assert_eq!(
            layer_region(
                clip,
                &frus_core::ClipShape::Rect,
                &None,
                &blurred,
                &[square()],
                1080,
                2340
            ),
            None
        );
        let path = frus_core::ClipShape::Path(frus_core::Path::oval(clip));
        assert_eq!(
            layer_region(clip, &path, &None, &none, &[square()], 1080, 2340),
            None
        );
        // Inside: a path-clipped layer in a plain one.
        let mut scene = Scene::new();
        scene.push_primitive(Primitive::Layer {
            primitives: vec![square()],
            opacity: 0.5,
            clip,
            clip_shape: frus_core::ClipShape::Path(frus_core::Path::oval(clip)),
            transform: None,
            filter: LayerFilter::NONE,
            owner: 0,
        });
        let inner = scene.primitives().to_vec();
        assert_eq!(
            layer_region(
                clip,
                &frus_core::ClipShape::Rect,
                &None,
                &none,
                &inner,
                1080,
                2340
            ),
            None
        );
        // Two levels down: a plain layer holding the path-clipped one.
        let wrapped = vec![Primitive::Layer {
            primitives: inner.clone(),
            opacity: 1.0,
            clip,
            clip_shape: frus_core::ClipShape::Rect,
            transform: None,
            filter: LayerFilter::NONE,
            owner: 0,
        }];
        assert_eq!(
            layer_region(
                clip,
                &frus_core::ClipShape::Rect,
                &None,
                &none,
                &wrapped,
                1080,
                2340
            ),
            None
        );
        // A plain layer inside a plain one is fine.
        assert!(boundable(&Primitive::Layer {
            primitives: vec![square()],
            opacity: 0.5,
            clip,
            clip_shape: frus_core::ClipShape::RRect(frus_core::BorderRadius::uniform(4.0)),
            transform: None,
            filter: LayerFilter::NONE,
            owner: 0,
        }));
    }

    /// A shader that plays, filling a box, the way `AnimatedShader` paints it at `seconds`.
    fn playing_shader(rect: Rect, seconds: f32) -> Scene {
        let shader = frus_core::Shader::from_wgsl(
            "fn shade(frag: Fragment) -> vec4<f32> {\n    return vec4<f32>(frag.uv, 0.5, 1.0);\n}\n",
        )
        .expect("the test shader compiles");
        let mut scene = Scene::new();
        scene.shader_rect(rect, &shader.paint().time(seconds));
        scene
    }

    /// `content` inside a layer that clips it to a rounded rectangle, as `ClipRRect` does.
    fn rounded(clip: Rect, content: Scene) -> Primitive {
        Primitive::Layer {
            primitives: content.primitives().to_vec(),
            opacity: 1.0,
            clip,
            clip_shape: frus_core::ClipShape::RRect(frus_core::BorderRadius::from(8.0)),
            transform: None,
            filter: LayerFilter::NONE,
            owner: 0,
        }
    }

    /// **A rounded clip around a playing shader is not a layer of its own** (milestone 796).
    ///
    /// The gallery's plasma, in a rounded card: the shader changes every frame, and the clip
    /// around it was rendered again every frame into a multisampled texture of its own, only
    /// to be composited with its corners. Folded, the shader is composited straight onto the
    /// frame through the rounded clip, and no frame renders a layer — and the corner is still
    /// cut, the middle still painted.
    #[test]
    fn a_rounded_clip_around_a_playing_shader_renders_no_layer() {
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut painters = Painters::new(&device, &queue, format, 1, None);
        let tex = target(&device, format, 64);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let card = Rect::new(8.0, 8.0, 48.0, 48.0);
        for frame in 0..3 {
            let mut scene = Scene::new();
            scene.push_primitive(rounded(card, playing_shader(card, frame as f32 * 0.016)));
            painters.render(
                &device,
                &queue,
                format,
                &view,
                64,
                64,
                &scene,
                Some(wgpu::Color::BLACK),
            );
        }
        assert_eq!(
            painters.layer_render_count(),
            0,
            "the shader is composited through the clip, frame after frame"
        );
        let folded = folded_clip(&rounded(card, playing_shader(card, 0.0)));
        let Some(Primitive::Layer {
            clip,
            clip_shape,
            filter,
            ..
        }) = folded
        else {
            panic!("folded into the shader's layer");
        };
        assert_eq!(clip, card);
        assert!(matches!(clip_shape, frus_core::ClipShape::RRect(_)));
        assert!(filter.shader.is_some());
    }

    /// The fold is taken only where it draws the same: a rounded clip that cuts into the
    /// shader's box elsewhere than at its own edge, a clip that also fades through a filter,
    /// or anything but a lone shader inside, stays a layer.
    #[test]
    fn a_clip_is_folded_only_where_it_draws_the_same() {
        let card = Rect::new(8.0, 8.0, 48.0, 48.0);
        // The shader's box smaller than the rounded clip: the corners would land elsewhere.
        let inner = Rect::new(16.0, 16.0, 20.0, 20.0);
        assert!(folded_clip(&rounded(card, playing_shader(inner, 0.0))).is_none());
        // A plain rectangle: the two rectangles meet in one.
        let Some(Primitive::Layer { clip, .. }) = folded_clip(&Primitive::Layer {
            primitives: playing_shader(inner, 0.0).primitives().to_vec(),
            opacity: 0.5,
            clip: card,
            clip_shape: frus_core::ClipShape::Rect,
            transform: None,
            filter: LayerFilter::NONE,
            owner: 0,
        }) else {
            panic!("a rectangle folds")
        };
        assert_eq!(clip, inner);
        // A shape inside a clip: not a lone shader.
        let mut shape = Scene::new();
        shape.fill_rect(card, Color::WHITE);
        assert!(folded_clip(&rounded(card, shape)).is_none());
        // A filter on the clip.
        let Primitive::Layer {
            primitives, clip, ..
        } = rounded(card, playing_shader(card, 0.0))
        else {
            unreachable!()
        };
        let filtered = Primitive::Layer {
            primitives,
            opacity: 1.0,
            clip,
            clip_shape: frus_core::ClipShape::Rect,
            transform: None,
            filter: LayerFilter {
                color: Some(frus_core::ColorFilter::grayscale()),
                ..LayerFilter::NONE
            },
            owner: 0,
        };
        assert!(folded_clip(&filtered).is_none());
    }

    /// **Text under a playing shader mask is rendered once** (milestone 796): the mask's
    /// time is the compositing draw's, not the layer's content, so the text's texture is kept
    /// from frame to frame.
    #[test]
    fn a_playing_shader_mask_keeps_its_text() {
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut painters = Painters::new(&device, &queue, format, 1, None);
        let tex = target(&device, format, 64);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let shader = frus_core::Shader::from_wgsl(
            "fn shade(frag: Fragment) -> vec4<f32> {\n    return vec4<f32>(1.0, 1.0, 1.0, fract(frag.time));\n}\n",
        )
        .expect("the test shader compiles");
        for frame in 0..3 {
            let mut scene = Scene::new();
            let rect = Rect::new(0.0, 0.0, 64.0, 32.0);
            scene.shaded(rect, &shader.paint().time(frame as f32 * 0.016), |inner| {
                inner.text(
                    frus_core::Point::new(2.0, 2.0),
                    "Shimmer",
                    &frus_core::ResolvedTextStyle::exact(14.0),
                    Color::WHITE,
                )
            });
            painters.render(
                &device,
                &queue,
                format,
                &view,
                64,
                64,
                &scene,
                Some(wgpu::Color::BLACK),
            );
        }
        assert_eq!(painters.layer_render_count(), 1, "rendered once, then kept");
    }

    /// **A page left leaves no GPU memory behind it** (milestone 796).
    ///
    /// A transition draws its pages into layers of their own, multisampled; once it has
    /// settled, the page is drawn straight onto the frame again. The layers' textures went,
    /// but the multisampled targets they were drawn in stayed — up to sixteen sizes of them,
    /// four times their layer each — and so did the staging copy of a page that blurred what
    /// was under it. The memory the painters hold after the page has gone must be what it
    /// was before it came.
    #[test]
    fn a_page_left_leaves_no_memory_behind() {
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut painters = Painters::new(&device, &queue, format, 4, None);
        let tex = target(&device, format, 128);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let render = |painters: &mut Painters, scene: &Scene| {
            painters.render(
                &device,
                &queue,
                format,
                &view,
                128,
                128,
                scene,
                Some(wgpu::Color::BLACK),
            )
        };
        let mut home = Scene::new();
        home.fill_rect(Rect::new(4.0, 4.0, 40.0, 40.0), Color::WHITE);
        render(&mut painters, &home);
        let before = painters.memory();

        // The page comes in: fading, growing, over a frosted backdrop, frame after frame.
        for frame in 0..12 {
            let mut page = home.clone();
            let size = 40.0 + frame as f32 * 6.0;
            page.layer(0.1 * frame as f32, |s| {
                s.fill_rect(Rect::new(10.0, 10.0, size, size), Color::rgb(1.0, 0.0, 0.0))
            });
            page.push_primitive(Primitive::Layer {
                primitives: Vec::new(),
                opacity: 1.0,
                clip: Rect::new(0.0, 0.0, size, size),
                clip_shape: frus_core::ClipShape::Rect,
                transform: None,
                filter: LayerFilter {
                    backdrop: Some(frus_core::Backdrop::blur(4.0)),
                    ..LayerFilter::NONE
                },
                owner: 0,
            });
            render(&mut painters, &page);
        }
        assert!(
            painters.memory().bytes > before.bytes,
            "the page held memory"
        );

        // And goes.
        render(&mut painters, &home);
        let after = painters.memory();
        assert_eq!(
            (after.textures, after.bytes),
            (before.textures, before.bytes),
            "home again holds what it held"
        );
    }

    /// **The glyph atlas reuses its room** (milestone 796): text of a new size every frame,
    /// for long enough that every glyph ever drawn could not fit in the first atlas, keeps the
    /// atlas at a size that holds one frame's glyphs. Never trimmed, every glyph stayed in
    /// use, the atlas could only double, and it never came back.
    #[test]
    fn the_glyph_atlas_reuses_its_room() {
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut painters = Painters::new(&device, &queue, format, 1, None);
        let tex = target(&device, format, 256);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let first = painters.text.atlas_bytes();
        for frame in 0..40 {
            let mut scene = Scene::new();
            scene.text(
                frus_core::Point::new(0.0, 0.0),
                "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
                &frus_core::ResolvedTextStyle::exact(30.0 + frame as f32),
                Color::WHITE,
            );
            painters.render(
                &device,
                &queue,
                format,
                &view,
                256,
                256,
                &scene,
                Some(wgpu::Color::BLACK),
            );
        }
        // 26 capitals at 30 to 70 px, every size: well over a 1024-texel atlas's room. One
        // frame's fits in 512.
        assert!(
            painters.text.atlas_bytes() <= first.max(512 * 512 + 256 * 256 * 4),
            "the atlas grew to {} bytes",
            painters.text.atlas_bytes()
        );
    }

    /// **A layer drawn again is drawn into the texture it had** (milestone 796): a page that
    /// animates while it comes in changes every frame, and a texture the size of the screen
    /// made every frame is one a phone's driver allocates and clears every frame.
    #[test]
    fn a_layer_drawn_again_reuses_its_texture() {
        let Some((device, queue)) = headless() else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut painters = Painters::new(&device, &queue, format, 1, None);
        let tex = target(&device, format, 32);
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let mut kept = Vec::new();
        for frame in 0..3 {
            let mut scene = Scene::new();
            scene.layer(0.5, |s| {
                s.fill_rect(
                    Rect::new(4.0, 4.0, 8.0, 8.0),
                    Color::rgb(1.0, 0.1 * frame as f32, 0.0),
                )
            });
            painters.render(
                &device,
                &queue,
                format,
                &view,
                32,
                32,
                &scene,
                Some(wgpu::Color::BLACK),
            );
            kept.push(painters.layer_cache[0].texture.clone());
        }
        assert_eq!(painters.layer_render_count(), 3, "drawn again every frame");
        assert!(
            kept[0] == kept[1] && kept[1] == kept[2],
            "into the same texture"
        );
    }
}
