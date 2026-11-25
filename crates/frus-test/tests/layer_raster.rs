// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **A transformed layer is drawn for the scale it is shown at** (milestone 850).
//!
//! - **Shrunk**, a layer's texture gets a mip chain: a fine pattern shows as the tone it has
//!   from afar, wherever the layer lies on the pixel grid, instead of whichever texels the
//!   samples landed on.
//! - **Enlarged**, a layer's content is drawn at the scale it is shown at: its text is
//!   rasterised at its final size and keeps edges as sharp as text drawn at that size,
//!   instead of a small texture magnified.
//!
//! Each test prints the measure it asserts on, so that a run with `--nocapture` gives the
//! numbers of the milestone note.

use frus_core::{
    Affine, Color, FilterQuality, ImageData, ImageHandle, LayerFilter, LayerTransform, Point,
    Primitive, Rect, Scene, TextStyle,
};
use frus_test::{render_scene, Snapshot};

const SIDE: u32 = 256;
/// Larger than the picture: a transformed layer holds what of its content lies flat on the
/// target, and the picture lies there at its own size.
const CANVAS: u32 = 272;
/// sRGB of linear 0.5: what half black and half white looks like from afar.
const GREY: f32 = 187.5;

/// Black and white texels, one pixel each.
fn checkerboard() -> ImageHandle {
    let mut rgba = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let v = if (x + y) % 2 == 0 { 255 } else { 0 };
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
    }
    ImageData::from_rgba(SIDE, SIDE, rgba).into_handle()
}

/// `content` in a layer transformed by `affine`.
fn layer(content: Scene, affine: Affine) -> Scene {
    let mut content = content;
    let primitives = content.split_off(0);
    let mut scene = Scene::new();
    scene.push_primitive(Primitive::Layer {
        primitives,
        opacity: 1.0,
        clip: Rect::UNBOUNDED,
        clip_shape: frus_core::ClipShape::Rect,
        transform: Some(LayerTransform::new(affine)),
        filter: LayerFilter::NONE,
        owner: 0,
    });
    scene
}

/// The checkerboard at its own size, read texel for texel (`Low`, no chain of its own), in
/// a layer shrunk to one eighth and moved to `(x, y)`.
fn shrunk_checkerboard(image: &ImageHandle, x: f32, y: f32) -> Scene {
    let mut content = Scene::new();
    let whole = Rect::new(0.0, 0.0, 1.0, 1.0);
    let rect = Rect::new(0.0, 0.0, SIDE as f32, SIDE as f32);
    content.draw_image_with(image, rect, whole, Color::WHITE, false, FilterQuality::Low);
    layer(
        content,
        Affine::scale(0.125, 0.125).then(Affine::translation(x, y)),
    )
}

fn render(scene: &Scene, w: u32, h: u32) -> Option<Snapshot> {
    let frame = render_scene(scene, w, h, Color::BLACK);
    if frame.is_none() {
        eprintln!("no GPU adapter available: test skipped");
    }
    frame
}

/// The green channel of the pixels well inside the shrunk picture, clear of its edges.
fn inside(frame: &Snapshot) -> Vec<u8> {
    let mut values = Vec::new();
    for y in 12..36 {
        for x in 12..36 {
            values.push(frame.pixel(x, y)[1]);
        }
    }
    values
}

fn mean_and_spread(values: &[u8]) -> (f32, u8) {
    let mean = values.iter().map(|&v| f32::from(v)).sum::<f32>() / values.len() as f32;
    let spread = values.iter().max().unwrap() - values.iter().min().unwrap();
    (mean, spread)
}

/// A one-pixel checkerboard in a layer shrunk to one eighth is an even grey. The layer
/// lies a sixteenth of a pixel off the grid, which puts every bilinear sample of the first
/// level on a texel centre: without a chain each pixel is exactly one texel, black or white.
#[test]
fn a_checkerboard_in_a_shrunk_layer_is_an_even_grey() {
    let image = checkerboard();
    let Some(frame) = render(
        &shrunk_checkerboard(&image, 8.0 + 1.0 / 16.0, 8.0 + 1.0 / 16.0),
        CANVAS,
        CANVAS,
    ) else {
        return;
    };
    let values = inside(&frame);
    let (mean, spread) = mean_and_spread(&values);
    let aliased = values.iter().filter(|&&v| !(24..=231).contains(&v)).count();
    eprintln!(
        "shrunk checkerboard: mean {mean:.1} (grey {GREY}), spread {spread}, \
         {aliased} of {} pixels black or white",
        values.len()
    );
    assert!(
        (mean - GREY).abs() < 6.0,
        "averages {mean}, not the grey {GREY}"
    );
    assert!(spread <= 6, "varies by {spread}: the pattern shows through");
}

/// The shrunk layer does not crawl as it moves: at eight positions across a pixel, the
/// picture stays the same grey. Without a chain, what each pixel shows depends on where its
/// sample lands, and the tone swings from frame to frame of a moving layer.
#[test]
fn a_shrunk_layer_does_not_shimmer_as_it_moves() {
    let image = checkerboard();
    let mut means = Vec::new();
    let mut worst = 0u8;
    for step in 0..8 {
        // A sixteenth of a pixel off the grid, as above, then an eighth of a pixel (one
        // texel of the picture) further across each step: without a chain, black and white
        // in turn.
        let at = 8.0 + 1.0 / 16.0 + step as f32 / 8.0;
        let Some(frame) = render(
            &shrunk_checkerboard(&image, at, 8.0 + 1.0 / 16.0),
            CANVAS,
            CANVAS,
        ) else {
            return;
        };
        let (mean, spread) = mean_and_spread(&inside(&frame));
        means.push(mean);
        worst = worst.max(spread);
    }
    let low = means.iter().cloned().fold(f32::MAX, f32::min);
    let high = means.iter().cloned().fold(f32::MIN, f32::max);
    eprintln!(
        "moving shrunk checkerboard: means {low:.1}..{high:.1} over eight offsets, \
         worst spread {worst}"
    );
    assert!(
        high - low < 4.0,
        "the tone swings {low}..{high} as it moves"
    );
    assert!(worst <= 8, "a frame varies by {worst}");
}

/// The letters drawn white on black: `size` pixels high at `at`.
fn letters(size: f32, at: Point) -> Scene {
    let mut scene = Scene::new();
    scene.text(at, "HIIH", &TextStyle::new(size).resolved(), Color::WHITE);
    scene
}

/// How sharp the edges of white-on-black text are: along each row through it, the largest
/// step between two neighbouring pixels, averaged over the rows that hold any. Text drawn at
/// its size steps from black to white in a pixel or two; a small texture magnified spreads
/// each step over as many pixels as it was magnified by.
fn edge_contrast(frame: &Snapshot) -> f32 {
    let mut total = 0.0;
    let mut rows = 0;
    for y in 0..frame.height {
        let row: Vec<i32> = (0..frame.width)
            .map(|x| i32::from(frame.pixel(x, y)[1]))
            .collect();
        if row.iter().all(|&v| v < 128) {
            continue;
        }
        let step = row
            .windows(2)
            .map(|p| (p[1] - p[0]).abs())
            .max()
            .unwrap_or(0);
        total += step as f32;
        rows += 1;
    }
    if rows == 0 {
        0.0
    } else {
        total / rows as f32
    }
}

/// Text in a layer scaled up four times is as sharp as text drawn four times larger: its
/// content is drawn at the scale it is shown at, not magnified from a small texture.
#[test]
fn text_in_an_enlarged_layer_stays_sharp() {
    const W: u32 = 200;
    const H: u32 = 80;
    // Ten pixels high, at (2, 3), shown four times larger: forty high at (8, 12).
    let scaled = layer(letters(10.0, Point::new(2.0, 3.0)), Affine::scale(4.0, 4.0));
    let drawn = letters(40.0, Point::new(8.0, 12.0));
    let (Some(scaled), Some(drawn)) = (render(&scaled, W, H), render(&drawn, W, H)) else {
        return;
    };
    let (enlarged, native) = (edge_contrast(&scaled), edge_contrast(&drawn));
    let lit = |f: &Snapshot| f.lit_pixels(128);
    eprintln!(
        "text at 4x: edge contrast {enlarged:.1} in the layer, {native:.1} drawn at its size; \
         lit pixels {} and {}",
        lit(&scaled),
        lit(&drawn)
    );
    assert!(
        native > 150.0,
        "the text drawn at its size is sharp: {native}"
    );
    assert!(
        enlarged > native * 0.85,
        "the enlarged layer's text is blurred: {enlarged} against {native}"
    );
    // The same letters, in the same place: about as many pixels lit.
    let (a, b) = (lit(&scaled) as f32, lit(&drawn) as f32);
    assert!((a - b).abs() < b * 0.15, "lit {a} against {b}");
}

/// A layer scaled up and turned keeps its text sharp too: what it is drawn at is its
/// largest stretch, whatever the rotation.
#[test]
fn text_in_an_enlarged_turned_layer_stays_sharp() {
    const S: u32 = 200;
    let affine = Affine::scale(3.0, 3.0)
        .then(Affine::rotation(0.3))
        .then(Affine::translation(40.0, 20.0));
    let scaled = layer(letters(10.0, Point::new(2.0, 3.0)), affine);
    let Some(frame) = render(&scaled, S, S) else {
        return;
    };
    let contrast = edge_contrast(&frame);
    eprintln!("text at 3x, turned: edge contrast {contrast:.1}");
    assert!(contrast > 120.0, "blurred: {contrast}");
}
