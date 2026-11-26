// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **What transformed layers cost** (milestone 850): frame times and GPU memory of a page
//! scaled the ways an application scales one, on whatever GPU the machine has.
//!
//! Ignored by default, since it measures rather than checks:
//!
//! ```text
//! cargo test -p frus-gpu --release layer_bench -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Each frame is timed from the start of its render to the end of its GPU work.

use crate::compositor::Painters;
use frus_core::{Affine, Color, LayerTransform, Point, Rect, Scene, TextStyle};
use std::time::Instant;

const W: u32 = 1080;
const H: u32 = 2340;

fn device() -> Option<(wgpu::Device, wgpu::Queue, String)> {
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
    let info = adapter.get_info();
    let name = format!("{} ({:?})", info.name, info.backend);
    let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("frus.layer_bench.device"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        memory_hints: wgpu::MemoryHints::default(),
        ..Default::default()
    }))
    .ok()?;
    Some((device, queue, name))
}

/// A page of text and boxes, the size of a phone's screen; `mark` changes one line.
fn page(mark: u32) -> Scene {
    let mut scene = Scene::new();
    scene.fill_rect(Rect::new(0.0, 0.0, W as f32, H as f32), Color::WHITE);
    let style = TextStyle::new(34.0).resolved();
    for line in 0..40 {
        let y = 40.0 + line as f32 * 56.0;
        scene.fill_rect(Rect::new(24.0, y, 12.0, 40.0), Color::rgb(0.2, 0.3, 0.8));
        let text = if line == 0 {
            format!("Frame {mark}: the quick brown fox jumps over the lazy dog")
        } else {
            format!("Line {line}: the quick brown fox jumps over the lazy dog")
        };
        scene.text(Point::new(48.0, y), text, &style, Color::BLACK);
    }
    scene
}

/// The page in a layer scaled by `s` about the screen's centre.
fn scaled(s: f32, mark: u32) -> Scene {
    let mut content = page(mark);
    let primitives = content.split_off(0);
    let centre = Point::new(W as f32 / 2.0, H as f32 / 2.0);
    let mut scene = Scene::new();
    scene.transformed(
        LayerTransform::new(Affine::scale(s, s).about(centre)),
        |inner| {
            for p in primitives {
                inner.push_primitive(p);
            }
        },
    );
    scene
}

struct Run {
    times: Vec<f64>,
}

impl Run {
    fn median(&self) -> f64 {
        let mut t = self.times.clone();
        t.sort_by(|a, b| a.partial_cmp(b).unwrap());
        t[t.len() / 2]
    }
    fn max(&self) -> f64 {
        self.times.iter().cloned().fold(0.0, f64::max)
    }
    fn total(&self) -> f64 {
        self.times.iter().sum()
    }
}

#[test]
#[ignore = "a measurement: run it by name with --ignored --nocapture"]
fn layer_bench() {
    let Some((device, queue, name)) = device() else {
        eprintln!("no GPU adapter available: skipped");
        return;
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let samples = 4;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("frus.layer_bench.target"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let clear = Some(wgpu::Color::BLACK);
    eprintln!("layer_bench on {name}, {W}x{H}, {samples}x MSAA");

    let run = |label: &str, scenes: &[Scene]| {
        let mut painters = Painters::new(&device, &queue, format, samples, None);
        // A first frame of the plain page, so that what follows is the animation alone.
        painters.render(&device, &queue, format, &view, W, H, &page(0), clear);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let renders = painters.layer_render_count();
        let mut times = Vec::new();
        for scene in scenes {
            let start = Instant::now();
            painters.render(&device, &queue, format, &view, W, H, scene, clear);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            times.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        let run = Run { times };
        eprintln!(
            "{label}: {} frames, median {:.2} ms, max {:.2} ms, total {:.1} ms, \
             {} layer draws, GPU memory {:.1} MB",
            run.times.len(),
            run.median(),
            run.max(),
            run.total(),
            painters.layer_render_count() - renders,
            painters.memory().megabytes(),
        );
    };

    // The page alone, not transformed, idle: the floor.
    let idle: Vec<Scene> = (0..20).map(|_| page(0)).collect();
    run("plain page, idle", &idle);
    // A page zooming out as it leaves, its content still: 1 to 0.85.
    let zoom_out: Vec<Scene> = (0..30)
        .map(|i| scaled(1.0 - 0.15 * i as f32 / 29.0, 0))
        .collect();
    run("zoom out 1 to 0.85, content still", &zoom_out);
    // Shrunk to a half, its content changing every frame: drawn again, and its chain made
    // again, every frame.
    let busy: Vec<Scene> = (0..30).map(|i| scaled(0.5, i)).collect();
    run("at 0.5, content changing every frame", &busy);
    // Shrunk to a half, still: idle frames.
    let still: Vec<Scene> = (0..20).map(|_| scaled(0.5, 0)).collect();
    run("at 0.5, idle", &still);
    // Zooming in, 1 to 2.5, content still, then at rest.
    let mut zoom_in: Vec<Scene> = (0..30)
        .map(|i| scaled(1.0 + 1.5 * i as f32 / 29.0, 0))
        .collect();
    zoom_in.extend((0..5).map(|_| scaled(2.5, 0)));
    run("zoom in 1 to 2.5 then 5 idle frames", &zoom_in);
    let rest: Vec<Scene> = (0..20).map(|_| scaled(2.5, 0)).collect();
    run("at 2.5, idle", &rest);
}
