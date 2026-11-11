// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Custom fragment shaders and GPU passes** (milestone 792), rendered: the colour a shader
//! answers is the colour on the screen, its uniforms and images arrive where it reads them,
//! a mask meets its child by its blend mode, and a GPU pass's picture is composited in its
//! box, clipped and faded like any layer. The goldens pin the look; the pixel checks say
//! what each one must be, so a golden regenerated wrong still fails here.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use frus_core::{BlendMode, Color, ImageData, Path, Rect, Scene, Shader};
use frus_gpu::{wgpu, GpuCallback};
use frus_test::render_scene;
use frus_widgets::{Container, ShaderMask, Theme};

fn golden(name: &str) -> String {
    format!("{}/tests/goldens/{name}.png", env!("CARGO_MANIFEST_DIR"))
}

/// A gradient from the `start` uniform to the `end` one across the box, with value noise
/// over it: what an application writes first.
const GRADIENT_NOISE: &str = "struct Uniforms {
    start: vec4<f32>,
    end: vec4<f32>,
    grain: f32,
};

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn shade(frag: Fragment) -> vec4<f32> {
    let base = mix(uniforms.start, uniforms.end, frag.uv.x);
    let n = noise(frag.position / 8.0) - 0.5;
    return vec4<f32>(base.rgb + vec3<f32>(n * uniforms.grain), base.a);
}
";

fn gradient() -> Shader {
    Shader::from_wgsl(GRADIENT_NOISE).expect("the gradient compiles")
}

fn rgb(snap: &frus_test::Snapshot, x: u32, y: u32) -> [i32; 3] {
    let p = snap.pixel(x, y);
    [p[0] as i32, p[1] as i32, p[2] as i32]
}

fn near(got: [i32; 3], want: [i32; 3], tolerance: i32) -> bool {
    got.iter()
        .zip(want)
        .all(|(g, w)| (g - w).abs() <= tolerance)
}

/// The colour a shader answers is sRGB, straight: a blend from black to white across the
/// box is 50% grey **as written** half-way, not the lighter grey a mix in linear light
/// would show.
#[test]
fn a_gradient_shader_paints_its_colours_as_written() {
    let paint = gradient()
        .paint()
        .color("start", Color::BLACK)
        .color("end", Color::WHITE)
        .float("grain", 0.0);
    let mut scene = Scene::new();
    scene.shader_rect(Rect::new(0.0, 0.0, 100.0, 20.0), &paint);
    let Some(snap) = render_scene(&scene, 100, 20, Color::rgb(1.0, 0.0, 0.0)) else {
        eprintln!("no GPU adapter available: test skipped");
        return;
    };
    let left = rgb(&snap, 0, 10);
    let middle = rgb(&snap, 50, 10);
    let right = rgb(&snap, 99, 10);
    assert!(left.iter().all(|&c| c <= 3), "black on the left: {left:?}");
    assert!(
        right.iter().all(|&c| c >= 251),
        "white on the right: {right:?}"
    );
    assert!(
        near(middle, [128, 128, 128], 4),
        "the middle is half-way in sRGB: {middle:?}"
    );
}

/// A shader paints its box and nothing else: the clear colour stays around it.
#[test]
fn a_shader_stays_in_its_box_and_its_clip() {
    let paint = gradient()
        .paint()
        .color("start", Color::rgb(0.0, 0.0, 1.0))
        .color("end", Color::rgb(0.0, 0.0, 1.0));
    let mut scene = Scene::new();
    scene.set_clip(Rect::new(0.0, 0.0, 30.0, 40.0));
    scene.shader_rect(Rect::new(10.0, 10.0, 40.0, 20.0), &paint);
    let Some(snap) = render_scene(&scene, 64, 40, Color::BLACK) else {
        return;
    };
    assert_eq!(rgb(&snap, 20, 20), [0, 0, 255], "inside box and clip");
    assert_eq!(rgb(&snap, 5, 20), [0, 0, 0], "left of the box");
    assert_eq!(
        rgb(&snap, 40, 20),
        [0, 0, 0],
        "inside the box, outside the clip"
    );
    assert_eq!(rgb(&snap, 20, 35), [0, 0, 0], "below the box");
}

/// The golden of the gradient with its grain: the look an application gets.
#[test]
fn gradient_noise_matches_golden() {
    let paint = gradient()
        .paint()
        .color("start", Color::rgb8(32, 64, 160))
        .color("end", Color::rgb8(240, 120, 40))
        .float("grain", 0.25);
    let mut scene = Scene::new();
    scene.shader_rect(Rect::new(8.0, 8.0, 144.0, 64.0), &paint);
    let Some(snap) = render_scene(&scene, 160, 80, Color::BLACK) else {
        return;
    };
    assert_eq!(rgb(&snap, 2, 2), [0, 0, 0], "the clear colour outside");
    // Blue on the left, orange on the right, the grain on top.
    let (l, r) = (rgb(&snap, 12, 40), rgb(&snap, 148, 40));
    assert!(l[2] > l[0] && r[0] > r[2], "left {l:?}, right {r:?}");
    snap.assert_golden_with(golden("shader_gradient_noise"), 3, 8);
}

/// A shader colours a path where the path covers, and leaves the rest.
#[test]
fn a_shader_fills_a_path_where_it_covers() {
    let paint = gradient()
        .paint()
        .color("start", Color::rgb(0.0, 1.0, 0.0))
        .color("end", Color::rgb(0.0, 1.0, 0.0));
    let path = Path::new()
        .move_to(frus_core::Point::new(0.0, 0.0))
        .line_to(frus_core::Point::new(40.0, 0.0))
        .line_to(frus_core::Point::new(0.0, 40.0))
        .close();
    let mut scene = Scene::new();
    scene.shader_path(&path, &paint);
    let Some(snap) = render_scene(&scene, 40, 40, Color::BLACK) else {
        return;
    };
    assert_eq!(rgb(&snap, 5, 5), [0, 255, 0], "inside the triangle");
    assert_eq!(rgb(&snap, 35, 35), [0, 0, 0], "outside the triangle");
}

/// Uniforms set from Rust arrive where the shader reads them, colour by colour.
#[test]
fn uniforms_arrive_where_the_shader_reads_them() {
    let shader = Shader::from_wgsl(
        "struct Uniforms { pad: f32, offset: vec2<f32>, tint: vec4<f32> };
         fn shade(frag: Fragment) -> vec4<f32> {
             return vec4<f32>(uniforms.tint.r, uniforms.offset.x, uniforms.offset.y, 1.0);
         }",
    )
    .unwrap();
    let paint = shader
        .paint()
        .float("pad", 9.0)
        .vec2("offset", [0.4, 0.6])
        .color("tint", Color::rgb(0.2, 0.0, 0.0));
    let mut scene = Scene::new();
    scene.shader_rect(Rect::new(0.0, 0.0, 8.0, 8.0), &paint);
    let Some(snap) = render_scene(&scene, 8, 8, Color::BLACK) else {
        return;
    };
    assert!(
        near(rgb(&snap, 4, 4), [51, 102, 153], 1),
        "{:?}",
        rgb(&snap, 4, 4)
    );
}

/// An image given to the paint is what `image0` samples: left half red, right half blue.
#[test]
fn an_image_is_sampled_as_given() {
    let shader =
        Shader::from_wgsl("fn shade(frag: Fragment) -> vec4<f32> { return image0(frag.uv); }")
            .unwrap();
    let image = ImageData::from_rgba(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).into_handle();
    let mut scene = Scene::new();
    scene.shader_rect(
        Rect::new(0.0, 0.0, 40.0, 10.0),
        &shader.paint().image(0, &image),
    );
    let Some(snap) = render_scene(&scene, 40, 10, Color::BLACK) else {
        return;
    };
    assert_eq!(rgb(&snap, 2, 5), [255, 0, 0]);
    assert_eq!(rgb(&snap, 37, 5), [0, 0, 255]);
}

/// The shader-mask widget fades its child by the shader's alpha, multiplying, as a mask
/// does: opaque on the left, gone on the right, the child's own colour kept.
fn masked_box() -> ShaderMask<()> {
    let fade = Shader::from_wgsl(
        "fn shade(frag: Fragment) -> vec4<f32> { return vec4<f32>(1.0, 1.0, 1.0, 1.0 - frag.uv.x); }",
    )
    .unwrap();
    ShaderMask::shader(fade.paint()).child(
        Container::new()
            .width(120.0)
            .height(40.0)
            .color(Color::rgb(1.0, 0.5, 0.0)),
    )
}

#[test]
fn a_shader_mask_fades_its_child() {
    let theme = Theme::dark().with_platform(frus_core::TargetPlatform::Linux);
    let root = masked_box();
    let Some(snap) = frus_test::render_widget(&root, 120, 40, &theme) else {
        return;
    };
    let left = snap.pixel(1, 20);
    let right = snap.pixel(118, 20);
    let bg = snap.pixel(119, 39);
    assert!(left[0] > 240 && (120..=135).contains(&left[1]), "{left:?}");
    // Faded to about 1% at the right edge: the background, give or take what 1% of
    // orange is once blended in linear light.
    assert!(
        (right[0] as i32 - bg[0] as i32).abs() <= 16 && right[0] < left[0] / 4,
        "the right edge is the background: {right:?} vs {bg:?}"
    );
    snap.assert_golden_with(golden("shader_mask_fade"), 3, 8);
}

/// A shader reading `child` as a texture: the child's colours, inverted, where it is.
#[test]
fn a_mask_reads_its_child() {
    let invert = Shader::from_wgsl(
        "fn shade(frag: Fragment) -> vec4<f32> {
             let c = child(frag.uv);
             return vec4<f32>(1.0 - c.rgb, c.a);
         }",
    )
    .unwrap();
    let mut scene = Scene::new();
    scene.shader_masked(
        Rect::new(0.0, 0.0, 20.0, 20.0),
        &invert.paint().blend(BlendMode::Src),
        |s| s.fill_rect(Rect::new(0.0, 0.0, 10.0, 20.0), Color::rgb(1.0, 0.2, 0.0)),
    );
    let Some(snap) = render_scene(&scene, 20, 20, Color::BLACK) else {
        return;
    };
    assert!(
        near(rgb(&snap, 4, 10), [0, 204, 255], 2),
        "{:?}",
        rgb(&snap, 4, 10)
    );
    assert_eq!(rgb(&snap, 15, 10), [0, 0, 0], "no child there: transparent");
}

/// The WGSL of a triangle drawn by a GPU pass: positions from the vertex index, one colour.
const TRIANGLE: &str = "
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 3>(vec2<f32>(-0.8, -0.8), vec2<f32>(0.8, -0.8), vec2<f32>(0.0, 0.8));
    return vec4<f32>(p[i], 0.0, 1.0);
}

@fragment
fn fs() -> @location(0) vec4<f32> {
    // Linear light, premultiplied: opaque green.
    return vec4<f32>(0.0, 1.0, 0.0, 1.0);
}
";

/// A GPU pass's state: its pipeline, made once.
struct Triangle {
    pipeline: wgpu::RenderPipeline,
}

fn triangle_pass(made: Arc<AtomicUsize>, drawn: Arc<AtomicUsize>) -> GpuCallback {
    GpuCallback::new(move |ctx| {
        let made = made.clone();
        let state = ctx.state(move |device, format| {
            made.fetch_add(1, Ordering::SeqCst);
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("triangle"),
                source: wgpu::ShaderSource::Wgsl(TRIANGLE.into()),
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("triangle"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    targets: &[Some(format.into())],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
            Triangle { pipeline }
        });
        let pipeline = state.pipeline.clone();
        let mut pass = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("triangle"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: ctx.target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
        drawn.fetch_add(1, Ordering::SeqCst);
    })
}

/// A GPU pass draws a triangle with its own pipeline into its box: green inside the
/// triangle, the clear colour around it and outside the box.
#[test]
fn a_gpu_pass_draws_a_triangle_in_its_box() {
    let (made, drawn) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let pass = triangle_pass(made.clone(), drawn.clone());
    let mut scene = Scene::new();
    scene.fill_rect(Rect::new(0.0, 0.0, 120.0, 80.0), Color::rgb8(20, 20, 40));
    pass.paint(&mut scene, Rect::new(20.0, 10.0, 80.0, 60.0), 0.0);
    let Some(snap) = render_scene(&scene, 120, 80, Color::BLACK) else {
        return;
    };
    assert_eq!(drawn.load(Ordering::SeqCst), 1, "the callback ran once");
    assert_eq!(made.load(Ordering::SeqCst), 1, "the pipeline was made once");
    // The triangle's centre: 60, 40. Its top corner is at the bottom of NDC +0.8 → near
    // the top of the box (y 16), its base near the bottom (y 64).
    assert_eq!(rgb(&snap, 60, 45), [0, 255, 0], "inside the triangle");
    assert_eq!(
        rgb(&snap, 25, 15),
        [20, 20, 40],
        "the box's corner shows what is under it"
    );
    assert_eq!(rgb(&snap, 5, 5), [20, 20, 40], "outside the box");
    snap.assert_golden_with(golden("gpu_pass_triangle"), 3, 8);
}

/// A GPU pass is composited like any layer: inside a faded group it is faded with it.
#[test]
fn a_gpu_pass_fades_with_its_group() {
    let (made, drawn) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let pass = triangle_pass(made, drawn);
    let mut scene = Scene::new();
    scene.layer(0.5, |s| pass.paint(s, Rect::new(0.0, 0.0, 40.0, 40.0), 0.0));
    let Some(snap) = render_scene(&scene, 40, 40, Color::BLACK) else {
        return;
    };
    let g = rgb(&snap, 20, 24);
    // Half of green over black, in sRGB: about 188.
    assert!(
        g[0] == 0 && (175..=200).contains(&g[1]) && g[2] == 0,
        "{g:?}"
    );
}
