// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus-shaders` — custom fragment shaders and a GPU pass, as an application uses them
//! (milestone 792):
//!
//! - an [`AnimatedShader`] painting a plasma, its clock running while it plays;
//! - a [`ShaderMask`] running a shimmer across a title;
//! - a [`GpuPass`] drawing a turning triangle with its own wgpu pipeline.
//!
//! "Full screen" shows the plasma alone, over the whole window: a tap stops or starts it.
//! That is the page the frame times and the idle CPU of the milestone were measured on.
//!
//! Run it with `frus run` in `crates/frus-shaders`.

#![deny(missing_docs)]

use frus::wgpu;
use frus::{
    button, column, text, Align, AnimatedShader, BlendMode, ClipRRect, Color, Container, FrusApp,
    GestureDetector, GpuPass, GpuPassContext, MediaQuery, Shader, ShaderMask, Size, State,
    StateContext, StatefulWidget, Variant, Widget,
};

/// The plasma: three waves, mixed between two colours the application sets.
pub const PLASMA: &str = include_str!("plasma.wgsl");

/// A bright band running across what it masks.
pub const SHIMMER: &str = include_str!("shimmer.wgsl");

/// The triangle the GPU pass draws, turned by a uniform.
const TRIANGLE: &str = include_str!("triangle.wgsl");

/// The page.
#[derive(Clone)]
struct Gallery;

/// What the page keeps between rebuilds.
#[derive(Default)]
struct GalleryState {
    /// The plasma alone over the whole window.
    full_screen: bool,
    /// Whether the clocks are stopped.
    paused: bool,
}

impl StatefulWidget for Gallery {
    type State = GalleryState;

    fn create_state(&self) -> GalleryState {
        GalleryState::default()
    }
}

/// The plasma's paint: compiled once, its colours set by name.
fn plasma() -> AnimatedShader {
    let shader = Shader::from_wgsl(PLASMA).unwrap_or_else(Shader::placeholder);
    AnimatedShader::new(
        shader
            .paint()
            .color("deep", Color::rgb8(18, 24, 80))
            .color("bright", Color::rgb8(255, 140, 60))
            .float("scale", 6.0),
    )
}

/// What the triangle pass keeps: its pipeline, and the buffer its angle is written to.
struct Triangle {
    pipeline: wgpu::RenderPipeline,
    angle: wgpu::Buffer,
    group: wgpu::BindGroup,
}

impl Triangle {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Triangle {
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
        let angle = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("triangle.angle"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("triangle.group"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: angle.as_entire_binding(),
            }],
        });
        Triangle {
            pipeline,
            angle,
            group,
        }
    }
}

/// Draws the turning triangle into the pass's picture.
fn draw_triangle(ctx: &mut GpuPassContext<'_>) {
    let aspect = ctx.width as f32 / ctx.height.max(1) as f32;
    let time = ctx.time;
    let queue = ctx.queue;
    let state = ctx.state(Triangle::new);
    queue.write_buffer(
        &state.angle,
        0,
        &[time * 1.5, aspect, 0.0, 0.0]
            .iter()
            .flat_map(|f: &f32| f.to_le_bytes())
            .collect::<Vec<u8>>(),
    );
    let (pipeline, group) = (state.pipeline.clone(), state.group.clone());
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
    pass.set_bind_group(0, &group, &[]);
    pass.draw(0..3, 0..1);
}

impl State for GalleryState {
    type Widget = Gallery;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let Size { width, height } = MediaQuery::of().size;
        let playing = !self.paused;
        if self.full_screen {
            // The measured page: the plasma over the whole window, a tap to stop it.
            return Box::new(
                GestureDetector::new(plasma().playing(playing).size(width, height))
                    .on_tap(cx.callback(|s: &mut GalleryState| s.paused = !s.paused)),
            );
        }
        let shimmer = Shader::from_wgsl(SHIMMER).unwrap_or_else(Shader::placeholder);
        let title = ShaderMask::shader(shimmer.paint().color("base", Color::rgb8(150, 160, 190)))
            .blend(BlendMode::SrcIn)
            .playing(playing)
            .child(text("Custom shaders").size(36.0));
        let page = column![
            title,
            GpuPass::new(draw_triangle)
                .playing(playing)
                .size(220.0, 160.0),
            // The room left under the rest, rounded.
            ClipRRect::new(16.0).child(
                plasma()
                    .playing(playing)
                    .size((width - 48.0).max(1.0), (height - 470.0).max(120.0))
            ),
            column![
                button(
                    if playing { "Stop" } else { "Play" },
                    cx.callback(|s: &mut GalleryState| s.paused = !s.paused),
                )
                .variant(Variant::Outlined),
                button(
                    "Full screen",
                    cx.callback(|s: &mut GalleryState| s.full_screen = true),
                )
                .variant(Variant::Filled),
            ]
            .gap(8.0)
            .align(Align::Center),
        ]
        .gap(16.0)
        .padding(24.0)
        .align(Align::Center)
        .width(width)
        .height(height);
        Box::new(
            Container::new()
                .width(width)
                .height(height)
                .color(cx.theme().background)
                .child(page),
        )
    }
}

frus::main!(FrusApp::stateful(Gallery).title("frus — shaders"));

#[cfg(test)]
mod tests {
    use super::*;

    /// The shaders the page ships compile, and say their uniforms.
    #[test]
    fn the_shaders_compile() {
        let plasma = Shader::from_wgsl(PLASMA).unwrap();
        let names: Vec<&str> = plasma.uniforms().iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["deep", "bright", "scale"]);
        Shader::from_wgsl(SHIMMER).unwrap();
    }
}
