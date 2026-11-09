// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Goldens for **how text is shaped** (milestone 784): OpenType features, the language, the
//! variable font axes, and Thai broken into lines between its words.
//!
//! A test binary of its own, because it registers a font for the whole process.

use frus_core::{Color, FontFamily, Point, Scene, TextStyle};
use frus_test::render_scene;

fn golden(name: &str) -> String {
    format!("{}/tests/goldens/{name}.png", env!("CARGO_MANIFEST_DIR"))
}

/// The columns holding a lit pixel in rows `top..bottom`: how wide what was drawn there is.
fn ink_columns(snapshot: &frus_test::Snapshot, top: u32, bottom: u32) -> u32 {
    (0..snapshot.width)
        .filter(|&x| (top..bottom).any(|y| snapshot.pixel(x, y)[0] > 96))
        .count() as u32
}

/// **Features and the language**: "fi" as a ligature and with `liga` off, and the same
/// Cyrillic letters in Russian and in Serbian, whose forms DejaVu Sans localizes.
#[test]
fn features_and_language_match_golden() {
    let mut scene = Scene::new();
    let base = TextStyle::new(28.0);
    let lines = [
        ("fi fl office", base),
        ("fi fl office", base.font_feature("liga", 0)),
        (
            "\u{0431}\u{0433}\u{0434}\u{043f}\u{0442}",
            base.locale("ru"),
        ),
        (
            "\u{0431}\u{0433}\u{0434}\u{043f}\u{0442}",
            base.locale("sr"),
        ),
    ];
    for (i, (text, style)) in lines.iter().enumerate() {
        scene.text(
            Point::new(8.0, 6.0 + i as f32 * 38.0),
            *text,
            &style.resolved(),
            Color::WHITE,
        );
    }
    let Some(snapshot) = render_scene(&scene, 240, 160, Color::BLACK) else {
        eprintln!("no GPU adapter available: test skipped");
        return;
    };
    let rows = |i: u32| (6 + i * 38, 6 + i * 38 + 34);
    let line = |i: u32| {
        let (top, bottom) = rows(i);
        (top..bottom)
            .flat_map(|y| (0..snapshot.width).map(move |x| (x, y)))
            .map(|(x, y)| snapshot.pixel(x, y))
            .collect::<Vec<_>>()
    };
    assert_ne!(line(0), line(1), "liga off draws other glyphs");
    assert_ne!(line(2), line(3), "Serbian draws other forms than Russian");
    snapshot.assert_golden(golden("text_features_language"));
}

/// **Variable font axes**: the same word at `wdth` 50, 75 and 100, then at `wght` 400, 550
/// and 700, in a small variable test font whose axes only stretch the letters, so the widths
/// can be predicted.
#[test]
fn variable_axes_match_golden() {
    frus_text::add_font(
        include_bytes!("../../frus-text/tests/fonts/FrusTestVariable.ttf").to_vec(),
    );
    let face = TextStyle::new(24.0).family(FontFamily::Named("Frus Test Variable"));
    let lines = [
        face.font_variation("wdth", 50.0),
        face.font_variation("wdth", 75.0),
        face,
        face.font_variation("wght", 550.0),
        face.font_variation("wght", 700.0),
    ];
    let mut scene = Scene::new();
    for (i, style) in lines.iter().enumerate() {
        scene.text(
            Point::new(8.0, 4.0 + i as f32 * 32.0),
            "HIHIH",
            &style.resolved(),
            Color::WHITE,
        );
    }
    let Some(snapshot) = render_scene(&scene, 160, 168, Color::BLACK) else {
        eprintln!("no GPU adapter available: test skipped");
        return;
    };
    let width = |i: u32| ink_columns(&snapshot, 4 + i * 32, 4 + i * 32 + 30);
    let (w50, w75, w100) = (width(0), width(1), width(2));
    assert!(w50 < w75 && w75 < w100, "wdth widens: {w50} {w75} {w100}");
    let ratio = w50 as f32 / w100 as f32;
    assert!((ratio - 0.5).abs() < 0.08, "wdth 50 is about half: {ratio}");
    // `wght` reaches the axis through the weight, resolved against this registered family,
    // whose variable face spans 400 to 700: 550 is drawn as 550, between the two.
    let (w550, w700) = (width(3), width(4));
    assert!(
        w100 < w550 && w550 < w700,
        "wght widens: {w100} {w550} {w700}"
    );
    snapshot.assert_golden(golden("text_variable_axes"));
}

/// **Thai breaks between its words**: "I eat rice at home, my house is near the market" has
/// no space in it, and wraps at a width of a few words with every line starting a word.
#[test]
fn thai_wrapping_matches_golden() {
    let text = "\u{0e09}\u{0e31}\u{0e19}\u{0e01}\u{0e34}\u{0e19}\u{0e02}\u{0e49}\u{0e32}\
                \u{0e27}\u{0e17}\u{0e35}\u{0e48}\u{0e1a}\u{0e49}\u{0e32}\u{0e19}\u{0e1a}\
                \u{0e49}\u{0e32}\u{0e19}\u{0e02}\u{0e2d}\u{0e07}\u{0e09}\u{0e31}\u{0e19}\
                \u{0e2d}\u{0e22}\u{0e39}\u{0e48}\u{0e43}\u{0e01}\u{0e25}\u{0e49}\u{0e15}\
                \u{0e25}\u{0e32}\u{0e14}";
    let style = TextStyle::new(22.0).resolved();
    let width = 120.0;
    let spans = frus_text::line_spans_resolved(text, &style, Some(width), true);
    assert!(spans.len() >= 3, "it wraps: {spans:?}");
    let mut scene = Scene::new();
    scene.text_wrapped(Point::new(8.0, 8.0), text, &style, Color::WHITE, width);
    let Some(snapshot) = render_scene(&scene, 140, 200, Color::BLACK) else {
        eprintln!("no GPU adapter available: test skipped");
        return;
    };
    assert!(snapshot.lit_pixels(96) > 200, "the Thai is drawn");
    snapshot.assert_golden(golden("text_thai_wrapping"));
}
