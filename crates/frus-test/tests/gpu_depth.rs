// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **A GPU pass with a depth buffer and multisampling** (milestone 844), rendered: with a
//! depth buffer the nearer surface wins whatever order it is drawn in, and without one the
//! last drawn does, which shows the test can tell; with 4 samples a slanted edge has
//! partly covered pixels, and with 1 it has none; and a 3D pass under a clip is cut like
//! any widget in a scrolled list.

use frus_core::{Color, Rect, Scene};
use frus_gpu::{wgpu, GpuCallback, GPU_PASS_DEPTH_FORMAT};
use frus_test::render_scene;

/// Two quads across the whole height: a near red one (depth 0.25) over the left two thirds
/// of the box and a far blue one (depth 0.75) over the right two thirds. They overlap in
/// the middle third. Instance 0 is the red one.
const QUADS: &str = "
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> Out {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[v];
    var out: Out;
    let y = mix(-1.0, 1.0, c.y);
    if i == 0u {
        out.position = vec4<f32>(mix(-1.0, 1.0 / 3.0, c.x), y, 0.25, 1.0);
        out.color = vec4<f32>(1.0, 0.0, 0.0, 1.0);
    } else {
        out.position = vec4<f32>(mix(-1.0 / 3.0, 1.0, c.x), y, 0.75, 1.0);
        out.color = vec4<f32>(0.0, 0.0, 1.0, 1.0);
    }
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4<f32> {
    return in.color;
}
";

/// A slanted triangle in opaque green: its long edge crosses pixels at every height.
const SLANT: &str = "
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    var p = array<vec2<f32>, 3>(vec2<f32>(-0.9, -0.9), vec2<f32>(0.9, -0.9), vec2<f32>(-0.9, 0.7));
    return vec4<f32>(p[i], 0.5, 1.0);
}

@fragment
fn fs() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 1.0, 0.0, 1.0);
}
";

/// A pass drawing `source`'s `vs`/`fs` with `instances`, one draw each, in that order. Its
/// pipeline is made for the targets the pass was given, depth test `Less`.
fn pass(source: &'static str, instances: &'static [u32], vertices: u32) -> GpuCallback {
    GpuCallback::new(move |ctx| {
        let targets = ctx.targets;
        let pipeline = ctx
            .state(move |device, _format| {
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("depth test"),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                });
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("depth test"),
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
                        targets: &[Some(targets.color_target())],
                        compilation_options: Default::default(),
                    }),
                    primitive: Default::default(),
                    depth_stencil: targets.depth_stencil(wgpu::CompareFunction::Less),
                    multisample: targets.multisample(),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .clone();
        let mut pass = ctx.begin_render_pass(Some("depth test"));
        pass.set_pipeline(&pipeline);
        for &i in instances {
            pass.draw(0..vertices, i..i + 1);
        }
    })
}

fn rgb(snap: &frus_test::Snapshot, x: u32, y: u32) -> [u8; 3] {
    let p = snap.pixel(x, y);
    [p[0], p[1], p[2]]
}

/// The two quads drawn in `order` into a 90 × 30 box filling the frame.
fn quads(order: &'static [u32], depth: bool) -> Option<frus_test::Snapshot> {
    let mut callback = pass(QUADS, order, 6);
    if depth {
        callback = callback.depth(GPU_PASS_DEPTH_FORMAT);
    }
    let mut scene = Scene::new();
    callback.paint(&mut scene, Rect::new(0.0, 0.0, 90.0, 30.0), 0.0);
    render_scene(&scene, 90, 30, Color::BLACK)
}

const RED: [u8; 3] = [255, 0, 0];
const BLUE: [u8; 3] = [0, 0, 255];

/// With a depth buffer the near red quad covers the overlap whether it is drawn first or
/// last; the parts only one quad covers keep their colour.
#[test]
fn the_nearer_surface_wins_whatever_the_order() {
    for order in [&[0u32, 1][..], &[1, 0]] {
        let Some(snap) = quads(order, true) else {
            return;
        };
        assert_eq!(rgb(&snap, 10, 15), RED, "only red, order {order:?}");
        assert_eq!(
            rgb(&snap, 45, 15),
            RED,
            "the overlap: red is nearer, order {order:?}"
        );
        assert_eq!(rgb(&snap, 80, 15), BLUE, "only blue, order {order:?}");
    }
}

/// Without a depth buffer the last quad drawn covers the overlap: the test above tells the
/// two apart.
#[test]
fn without_a_depth_buffer_the_last_drawn_wins() {
    let Some(near_last) = quads(&[1, 0], false) else {
        return;
    };
    assert_eq!(rgb(&near_last, 45, 15), RED);
    let Some(far_last) = quads(&[0, 1], false) else {
        return;
    };
    assert_eq!(
        rgb(&far_last, 45, 15),
        BLUE,
        "drawn last, the far quad covers"
    );
}

/// How many pixels of the frame are partly green: an edge smoothed by multisampling.
fn partly_covered(samples: u32) -> Option<usize> {
    let callback = pass(SLANT, &[0], 3)
        .depth(GPU_PASS_DEPTH_FORMAT)
        .samples(samples);
    let mut scene = Scene::new();
    callback.paint(&mut scene, Rect::new(0.0, 0.0, 64.0, 64.0), 0.0);
    let snap = render_scene(&scene, 64, 64, Color::BLACK)?;
    let mut partial = 0;
    for y in 0..64 {
        for x in 0..64 {
            let [r, g, b] = rgb(&snap, x, y);
            if r == 0 && b == 0 && (20..235).contains(&g) {
                partial += 1;
            }
        }
    }
    Some(partial)
}

/// With 4 samples the slanted edge is smoothed (partly covered pixels along it); with one
/// every pixel is in or out.
#[test]
fn four_samples_smooth_the_edges() {
    let Some(sharp) = partly_covered(1) else {
        return;
    };
    let Some(smooth) = partly_covered(4) else {
        return;
    };
    assert_eq!(sharp, 0, "one sample: every pixel in or out");
    assert!(
        smooth >= 30,
        "4 samples: the edge is smoothed ({smooth} pixels)"
    );
}

/// A 3D pass half out of a clip, as a row scrolled half out of a list: what is outside
/// the clip shows what is under it, what is inside is still depth-tested.
#[test]
fn a_depth_tested_pass_is_clipped_like_a_widget() {
    let callback = pass(QUADS, &[0, 1], 6)
        .depth(GPU_PASS_DEPTH_FORMAT)
        .samples(4);
    let mut scene = Scene::new();
    scene.fill_rect(Rect::new(0.0, 0.0, 90.0, 40.0), Color::rgb8(20, 20, 40));
    scene.set_clip(Rect::new(0.0, 0.0, 90.0, 20.0));
    callback.paint(&mut scene, Rect::new(0.0, 5.0, 90.0, 30.0), 0.0);
    let Some(snap) = render_scene(&scene, 90, 40, Color::BLACK) else {
        return;
    };
    assert_eq!(rgb(&snap, 45, 12), RED, "inside the clip, depth-tested");
    assert_eq!(
        rgb(&snap, 45, 30),
        [20, 20, 40],
        "below the clip, what is under"
    );
    assert_eq!(rgb(&snap, 45, 2), [20, 20, 40], "above the box");
}
