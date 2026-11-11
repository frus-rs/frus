// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **External textures** (milestone 793): frames pushed into the registry are drawn with
//! the colours they were pushed with, whatever their layout — RGBA, BGRA, NV12 or I420 — and
//! a new frame of the same size replaces the old one on the next render.

use frus_core::{
    textures, Color, ExternalTexture, FilterQuality, ImageData, Rect, Scene, YuvColorSpace,
    YuvPlane,
};
use frus_test::{render_scene, Snapshot};

const CANVAS: u32 = 64;

fn render(scene: &Scene) -> Option<Snapshot> {
    let frame = render_scene(scene, CANVAS, CANVAS, Color::BLACK);
    if frame.is_none() {
        eprintln!("no GPU adapter available: test skipped");
    }
    frame
}

/// A scene drawing `texture`'s latest frame into `rect`, as the `Texture` widget does.
fn draw(scene: &mut Scene, texture: &ExternalTexture, rect: Rect) {
    let info = textures()
        .frame_info(texture.id())
        .expect("a frame was pushed");
    let image = ImageData::external(texture.id(), info.generation, info.width, info.height);
    scene.draw_image_with(
        &image.into_handle(),
        rect,
        Rect::new(0.0, 0.0, 1.0, 1.0),
        Color::WHITE,
        false,
        FilterQuality::None,
    );
}

fn close(got: [u8; 4], want: [u8; 3], tolerance: i32) -> bool {
    got.iter()
        .zip(want)
        .all(|(&g, w)| (i32::from(g) - i32::from(w)).abs() <= tolerance)
}

/// A 4×4 frame, left half one colour and right half another, in each layout.
fn halves(texture: &ExternalTexture, layout: &str) {
    let (w, h) = (4u32, 4u32);
    match layout {
        "rgba" => {
            let mut px = Vec::new();
            for _ in 0..h {
                for x in 0..w {
                    px.extend_from_slice(if x < 2 {
                        &[255, 0, 0, 255]
                    } else {
                        &[0, 0, 255, 255]
                    });
                }
            }
            texture.push_rgba(w, h, &px).unwrap();
        }
        "bgra" => {
            let mut px = Vec::new();
            for _ in 0..h {
                for x in 0..w {
                    // Blue, green, red, alpha: red on the left, blue on the right.
                    px.extend_from_slice(if x < 2 {
                        &[0, 0, 255, 255]
                    } else {
                        &[255, 0, 0, 255]
                    });
                }
            }
            texture.push_bgra(w, h, &px).unwrap();
        }
        // BT.601 limited: red is (81, 90, 240), blue (41, 240, 110).
        "nv12" | "i420" => {
            let y: Vec<u8> = (0..h)
                .flat_map(|_| (0..w).map(|x| if x < 2 { 81 } else { 41 }))
                .collect();
            let (u, v) = ([90u8, 240, 90, 240], [240u8, 110, 240, 110]);
            if layout == "nv12" {
                let uv: Vec<u8> = u.iter().zip(&v).flat_map(|(&a, &b)| [a, b]).collect();
                texture
                    .push_nv12(
                        w,
                        h,
                        YuvPlane::packed(&y, 4),
                        YuvPlane::packed(&uv, 4),
                        YuvColorSpace::Bt601Limited,
                    )
                    .unwrap();
            } else {
                texture
                    .push_i420(
                        w,
                        h,
                        YuvPlane::packed(&y, 4),
                        YuvPlane::packed(&u, 2),
                        YuvPlane::packed(&v, 2),
                        YuvColorSpace::Bt601Limited,
                    )
                    .unwrap();
            }
        }
        _ => unreachable!(),
    }
}

#[test]
fn every_layout_shows_the_colours_it_was_pushed_with() {
    for layout in ["rgba", "bgra", "nv12", "i420"] {
        let texture = textures().create();
        halves(&texture, layout);
        let mut scene = Scene::new();
        draw(&mut scene, &texture, Rect::new(0.0, 0.0, 64.0, 64.0));
        let Some(frame) = render(&scene) else {
            return;
        };
        // Clear of the middle, where the chroma of a 4:2:0 frame blends the two halves.
        let (left, right) = (frame.pixel(4, 32), frame.pixel(60, 32));
        let tolerance = if matches!(layout, "nv12" | "i420") {
            6
        } else {
            1
        };
        assert!(
            close(left, [255, 0, 0], tolerance),
            "{layout}: left {left:?}"
        );
        assert!(
            close(right, [0, 0, 255], tolerance),
            "{layout}: right {right:?}"
        );
    }
}

#[test]
fn a_new_frame_replaces_the_old_one() {
    let texture = textures().create();
    let red = [255u8, 0, 0, 255].repeat(16);
    let green = [0u8, 255, 0, 255].repeat(16);
    texture.push_rgba(4, 4, &red).unwrap();
    let mut scene = Scene::new();
    draw(&mut scene, &texture, Rect::new(0.0, 0.0, 64.0, 64.0));
    let Some(first) = render(&scene) else {
        return;
    };
    assert!(close(first.pixel(32, 32), [255, 0, 0], 1));
    texture.push_rgba(4, 4, &green).unwrap();
    let mut scene = Scene::new();
    draw(&mut scene, &texture, Rect::new(0.0, 0.0, 64.0, 64.0));
    let second = render(&scene).expect("rendered once");
    assert!(
        close(second.pixel(32, 32), [0, 255, 0], 1),
        "{:?}",
        second.pixel(32, 32)
    );
    assert_eq!(texture.stats().shown, 2);
}

#[test]
fn a_texture_that_is_gone_draws_nothing() {
    let texture = textures().create();
    texture.push_rgba(1, 1, &[255, 255, 255, 255]).unwrap();
    let mut scene = Scene::new();
    draw(&mut scene, &texture, Rect::new(0.0, 0.0, 64.0, 64.0));
    drop(texture);
    let Some(frame) = render(&scene) else {
        return;
    };
    assert_eq!(frame.pixel(32, 32), [0, 0, 0, 255]);
}

/// The four layouts side by side, a gradient in each: what the conversion looks like.
#[test]
fn golden_external_texture_layouts() {
    let (w, h) = (16u32, 16u32);
    let textures_: Vec<ExternalTexture> = (0..4).map(|_| textures().create()).collect();
    // RGBA and BGRA: a red-green ramp.
    let mut rgba = Vec::new();
    let mut bgra = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let (r, g, b) = ((x * 16) as u8, (y * 16) as u8, 128u8);
            rgba.extend_from_slice(&[r, g, b, 255]);
            bgra.extend_from_slice(&[b, g, r, 255]);
        }
    }
    textures_[0].push_rgba(w, h, &rgba).unwrap();
    textures_[1].push_bgra(w, h, &bgra).unwrap();
    // YUV: a luma ramp with chroma sweeping across.
    let luma: Vec<u8> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (16 + (x + y) * 6) as u8))
        .collect();
    let u: Vec<u8> = (0..h / 2)
        .flat_map(|_| (0..w / 2).map(|x| (64 + x * 16) as u8))
        .collect();
    let v: Vec<u8> = (0..h / 2)
        .flat_map(|y| (0..w / 2).map(move |_| (192 - y * 16) as u8))
        .collect();
    let uv: Vec<u8> = u.iter().zip(&v).flat_map(|(&a, &b)| [a, b]).collect();
    textures_[2]
        .push_nv12(
            w,
            h,
            YuvPlane::packed(&luma, 16),
            YuvPlane::packed(&uv, 16),
            YuvColorSpace::Bt709Limited,
        )
        .unwrap();
    textures_[3]
        .push_i420(
            w,
            h,
            YuvPlane::packed(&luma, 16),
            YuvPlane::packed(&u, 8),
            YuvPlane::packed(&v, 8),
            YuvColorSpace::Bt709Limited,
        )
        .unwrap();
    let mut scene = Scene::new();
    for (i, texture) in textures_.iter().enumerate() {
        let x = (i % 2) as f32 * 32.0;
        let y = (i / 2) as f32 * 32.0;
        draw(&mut scene, texture, Rect::new(x, y, 32.0, 32.0));
    }
    let Some(frame) = render(&scene) else {
        return;
    };
    frame.assert_golden(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/goldens/external_texture_layouts.png"
    ));
}
