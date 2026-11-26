// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **A transformed layer is drawn for the scale it is shown at** (milestone 850).
//!
//! - **Shrinking**, while its scale moves, a layer's texture gets a mip chain: a fine pattern
//!   shows as the tone it has from afar, wherever the layer lies on the pixel grid, instead of
//!   whichever texels the samples landed on.
//! - **Shrunk and at rest**, its content is drawn at its exact scale: its edges and text are as
//!   sharp as content drawn at that size, which a chain would soften by about a pixel.
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
use frus_test::{render_scene, render_scenes, Snapshot};

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
/// a layer scaled by `scale` and moved to `(x, y)`.
fn checkerboard_layer(image: &ImageHandle, scale: f32, x: f32, y: f32) -> Scene {
    let mut content = Scene::new();
    let whole = Rect::new(0.0, 0.0, 1.0, 1.0);
    let rect = Rect::new(0.0, 0.0, SIDE as f32, SIDE as f32);
    content.draw_image_with(image, rect, whole, Color::WHITE, false, FilterQuality::Low);
    layer(
        content,
        Affine::scale(scale, scale).then(Affine::translation(x, y)),
    )
}

fn render(scene: &Scene, w: u32, h: u32) -> Option<Snapshot> {
    let frame = render_scene(scene, w, h, Color::BLACK);
    if frame.is_none() {
        eprintln!("no GPU adapter available: test skipped");
    }
    frame
}

/// The checkerboard's layer shrinking: a frame at its own size, then one at an eighth,
/// moved to `(x, y)`. Its scale is moving, so it is read through its chain.
fn shrinking_checkerboard(image: &ImageHandle, x: f32, y: f32) -> Option<Snapshot> {
    let frames = [
        checkerboard_layer(image, 1.0, x, y),
        checkerboard_layer(image, 0.125, x, y),
    ];
    let frame = render_scenes(&frames, CANVAS, CANVAS, Color::BLACK);
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

/// A one-pixel checkerboard in a layer shrinking to one eighth is an even grey. The layer
/// lies a sixteenth of a pixel off the grid, which puts every bilinear sample of the first
/// level on a texel centre: without a chain each pixel is exactly one texel, black or white.
#[test]
fn a_checkerboard_in_a_shrunk_layer_is_an_even_grey() {
    let image = checkerboard();
    let Some(frame) = shrinking_checkerboard(&image, 8.0 + 1.0 / 16.0, 8.0 + 1.0 / 16.0) else {
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
        let Some(frame) = shrinking_checkerboard(&image, at, 8.0 + 1.0 / 16.0) else {
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

/// A layer shrunk to 0.6 and at rest, as a `FittedBox` or a `Transform::scale` usually is, is
/// as sharp as its content drawn at that size: its letters keep their edges, and a box its
/// one-pixel border. Shown the same way while its scale moves, it is read through its chain,
/// about a pixel softer; that frame is printed for the milestone note, not asserted on.
#[test]
fn a_shrunk_layer_at_rest_is_as_sharp_as_content_drawn_at_its_size() {
    // Wide enough for the content laid flat: a layer holds what of it lies on the target.
    const W: u32 = 170;
    const H: u32 = 70;
    // Twenty pixels high at (5, 10), shown at 0.6: twelve high at (3, 6). A line of text
    // reaches across its clip, so the layer's region is the whole target: a texture drawn at
    // 0.6 must be placed by it, which the first version of this milestone did not do.
    let content = || {
        let mut scene = letters(20.0, Point::new(5.0, 10.0));
        scene.fill_rect(Rect::new(110.0, 10.0, 50.0, 50.0), Color::WHITE);
        scene
    };
    let shrunk = || layer(content(), Affine::scale(0.6, 0.6));
    let mut drawn = letters(12.0, Point::new(3.0, 6.0));
    drawn.fill_rect(Rect::new(66.0, 6.0, 30.0, 30.0), Color::WHITE);
    let (Some(at_rest), Some(drawn)) = (render(&shrunk(), W, H), render(&drawn, W, H)) else {
        return;
    };
    let moving = render_scenes(
        &[layer(content(), Affine::IDENTITY), shrunk()],
        W,
        H,
        Color::BLACK,
    )
    .expect("the adapter is there");
    let (rest, native, chain) = (
        edge_contrast(&at_rest),
        edge_contrast(&drawn),
        edge_contrast(&moving),
    );
    let differ = |f: &Snapshot| f.diff_count(&drawn, 24);
    eprintln!(
        "shrunk to 0.6: edge contrast {rest:.1} at rest, {chain:.1} while moving, {native:.1} \
         drawn at its size; pixels off by more than 24: {} at rest, {} while moving",
        differ(&at_rest),
        differ(&moving)
    );
    assert!(
        rest > native * 0.9,
        "the layer at rest is softer than its content drawn at its size: {rest} against {native}"
    );
    // The box's edge, which falls on whole pixels: one step from black to white, as drawn.
    let row = |f: &Snapshot| [64, 65, 66, 67].map(|x| f.pixel(x, 20)[1]);
    let (r, d) = (row(&at_rest), row(&drawn));
    for (a, b) in r.iter().zip(d) {
        assert!(
            a.abs_diff(b) <= 24,
            "the box's edge: {r:?} at rest, {d:?} drawn"
        );
    }
}

/// The same for a layer whose region starts off the target's corner, moved by a fraction of a
/// pixel: its texture's texels fall on the target's pixels, and its box comes out as the box
/// drawn where it lands. A texture whose corner fell between pixels was resampled half a pixel
/// off, and every edge spread over two (the goldens `boxes_that_fit`, `turned_and_rotated`).
#[test]
fn a_shrunk_layer_at_rest_lies_on_the_pixel_grid() {
    const S: u32 = 96;
    let mut content = Scene::new();
    content.fill_rect(Rect::new(37.0, 23.0, 50.0, 30.0), Color::WHITE);
    let affine = Affine::scale(0.6, 0.6).then(Affine::translation(5.0, 4.5));
    let shrunk = layer(content, affine);
    // Where it lands: (27.2, 18.3), 30 by 18.
    let mut drawn = Scene::new();
    drawn.fill_rect(Rect::new(27.2, 18.3, 30.0, 18.0), Color::WHITE);
    let (Some(at_rest), Some(drawn)) = (render(&shrunk, S, S), render(&drawn, S, S)) else {
        return;
    };
    let off = at_rest.diff_count(&drawn, 24);
    eprintln!("shrunk box off the corner: {off} pixels off by more than 24");
    assert!(
        off <= 2,
        "{off} pixels differ from the box drawn where it lands"
    );
}
