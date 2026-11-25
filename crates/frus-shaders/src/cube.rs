// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The 3D page: a lit cube turning in a [`GpuPass`] with a depth buffer and 4× multisampling,
//! one row of a scrolling list among ordinary widgets (milestone 844).

use frus::wgpu;
use frus::{GpuCallback, GpuPass, GpuPassContext, PassTargets, GPU_PASS_DEPTH_FORMAT};

/// The cube's shader: its vertices, perspective and light.
pub const CUBE: &str = include_str!("cube.wgsl");

/// What the cube pass keeps: its pipeline, made for the pass's targets, and the buffer its
/// time and aspect are written to.
struct Cube {
    pipeline: wgpu::RenderPipeline,
    params: wgpu::Buffer,
    group: wgpu::BindGroup,
}

impl Cube {
    fn new(device: &wgpu::Device, targets: PassTargets) -> Cube {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cube"),
            source: wgpu::ShaderSource::Wgsl(CUBE.into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cube"),
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
            // No culling: the depth test alone keeps the far faces behind.
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: targets.depth_stencil(wgpu::CompareFunction::Less),
            multisample: targets.multisample(),
            multiview_mask: None,
            cache: None,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cube.params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cube.group"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            }],
        });
        Cube {
            pipeline,
            params,
            group,
        }
    }
}

/// Draws the cube into the pass's picture, its depth buffer and its samples.
fn draw_cube(ctx: &mut GpuPassContext<'_>) {
    let aspect = ctx.width as f32 / ctx.height.max(1) as f32;
    let time = ctx.time;
    let queue = ctx.queue;
    let targets = ctx.targets;
    let state = ctx.state(|device, _format| Cube::new(device, targets));
    queue.write_buffer(
        &state.params,
        0,
        &[time, aspect, 0.0, 0.0]
            .iter()
            .flat_map(|f: &f32| f.to_le_bytes())
            .collect::<Vec<u8>>(),
    );
    let (pipeline, group) = (state.pipeline.clone(), state.group.clone());
    let mut pass = ctx.begin_render_pass(Some("cube"));
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &group, &[]);
    pass.draw(0..36, 0..1);
}

/// The turning cube: a depth buffer so the near faces hide the far ones, 4 samples so its
/// edges are smooth.
pub fn cube(playing: bool) -> GpuPass {
    GpuPass::from_callback(cube_callback()).playing(playing)
}

/// The cube's drawing, with its depth buffer and samples.
fn cube_callback() -> GpuCallback {
    GpuCallback::new(draw_cube)
        .depth(GPU_PASS_DEPTH_FORMAT)
        .samples(4)
}

#[cfg(test)]
mod tests {
    use super::*;
    use frus::{Color, Rect, Scene};

    /// The page's cube, rendered offscreen at time 0, unturned: the face towards the camera
    /// (purple, lit) covers the centre, although the back face (orange) is drawn after it.
    /// Without the depth buffer the back face would show there.
    #[test]
    fn the_near_face_hides_the_back_face() {
        let mut scene = Scene::new();
        cube_callback().paint(&mut scene, Rect::new(0.0, 0.0, 96.0, 96.0), 0.0);
        let Some(snap) = frus_test::render_scene(&scene, 96, 96, Color::BLACK) else {
            eprintln!("no GPU adapter available: test skipped");
            return;
        };
        let [r, g, b, _] = snap.pixel(48, 48);
        // The front face: (0.75, 0.35, 0.90) lit at 0.69, about (189, 134, 206) in sRGB.
        assert!(
            b > r && r > g && b > 150,
            "the lit front face: {:?}",
            [r, g, b]
        );
        // Outside the cube, the picture is transparent and the background shows.
        assert_eq!(snap.pixel(2, 2)[..3], [0, 0, 0]);
    }
}
