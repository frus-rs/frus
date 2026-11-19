// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The selection highlight, photographed.
//!
//! Text selection was built and tested through the scene — a rect of the theme's
//! `selection` colour here, two handle paths there — and checked by eye on a phone, but no
//! image pinned what it actually **paints** (milestone 821). Two goldens do that now:
//!
//! - a [`Text::selectable`] with part of its line selected by a finger, so with its two
//!   handles hanging below the ends of the selection;
//! - a [`SelectionArea`] whose touch selection starts in a plain text and ends inside a
//!   [`RichText`], so the highlight crosses from one text into the next and runs over the
//!   paragraph's differently styled spans.
//!
//! The state is put in the runtime the way the shell puts it there after a long press: an
//! [`Edit`] for the focused text and the handle mark, or a [`RegionSelection`] for the area.
//!
//! Beside each golden, the highlight's **colour** is checked by number: it must be the
//! theme's selection token, and a translucent token is blended by the GPU in linear light
//! (the render target is sRGB, see `blending.rs`), so the pixel under the highlight is the
//! linear blend of the token over the background — not the sRGB lerp the token's alpha
//! would suggest.
//!
//! With no GPU adapter the tests skip themselves, the harness returning `None`.

use frus_core::{Color, Point, Rect, TextSpan};
use frus_test::{Snapshot, Stage};
use frus_widgets::{
    Container, Edit, Flex, Primitive, RegionPoint, RegionSelection, RichText, SelectionArea, Text,
    Widget,
};

fn golden(name: &str) -> String {
    format!("{}/tests/goldens/{name}.png", env!("CARGO_MANIFEST_DIR"))
}

fn to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// `token` over the opaque `background`, blended the way the hardware does it: in linear
/// light.
fn linear_over(token: Color, background: Color) -> [u8; 3] {
    let [r, g, b, a] = token.to_array();
    let [br, bg, bb, _] = background.to_array();
    let blend = |s: f32, d: f32| {
        (to_srgb(a * to_linear(s) + (1.0 - a) * to_linear(d)) * 255.0).round() as u8
    };
    [blend(r, br), blend(g, bg), blend(b, bb)]
}

/// The same blend done in sRGB — what the selection would look like if the token were
/// read as a plain opacity over the background. Only used to prove the two are told apart.
fn srgb_over(token: Color, background: Color) -> [u8; 3] {
    let [r, g, b, a] = token.to_array();
    let [br, bg, bb, _] = background.to_array();
    let blend = |s: f32, d: f32| ((a * s + (1.0 - a) * d) * 255.0).round() as u8;
    [blend(r, br), blend(g, bg), blend(b, bb)]
}

fn close(pixel: [u8; 4], expected: [u8; 3], tolerance: u8) -> bool {
    (0..3).all(|i| pixel[i].abs_diff(expected[i]) <= tolerance)
}

/// The highlight rects the scene paints in `color`.
fn highlights(primitives: &[Primitive], color: Color) -> Vec<Rect> {
    primitives
        .iter()
        .filter_map(|p| match p {
            Primitive::Rect { rect, color: c, .. } if *c == color => Some(*rect),
            _ => None,
        })
        .collect()
}

/// The handle paths the scene fills in `color`.
fn handles(primitives: &[Primitive], color: Color) -> usize {
    primitives
        .iter()
        .filter(|p| matches!(p, Primitive::Path { fill: Some(c), .. } if *c == color))
        .count()
}

/// Asserts that the pixels inside `rect` that are not glyph ink are the linear blend of
/// `token` over `background` — and that this is not the sRGB lerp, so the check could fail.
fn assert_highlight_blend(shot: &Snapshot, rect: Rect, token: Color, background: Color) {
    let linear = linear_over(token, background);
    let srgb = srgb_over(token, background);
    assert!(
        (0..3).any(|i| linear[i].abs_diff(srgb[i]) > 6),
        "the two blends are too close to tell apart: {linear:?} {srgb:?}"
    );
    let (x0, y0) = (rect.x.ceil() as u32, rect.y.ceil() as u32);
    let (x1, y1) = (
        (rect.x + rect.width).floor() as u32,
        (rect.y + rect.height).floor() as u32,
    );
    let (mut total, mut matched, mut as_srgb) = (0usize, 0usize, 0usize);
    for y in y0..y1 {
        for x in x0..x1 {
            let pixel = shot.pixel(x, y);
            total += 1;
            matched += usize::from(close(pixel, linear, 2));
            as_srgb += usize::from(close(pixel, srgb, 2));
        }
    }
    // The words cover part of the box; the rest of it — between the letters, above and
    // below them — is the highlight alone.
    assert!(
        matched * 3 >= total,
        "{matched} of {total} pixels in {rect:?} are the linear blend {linear:?} \
         (sRGB lerp {srgb:?}: {as_srgb}); sample {:?}",
        shot.pixel(x0 + 1, y0 + 1)
    );
    assert_eq!(as_srgb, 0, "a pixel painted as the sRGB lerp {srgb:?}");
}

/// **A selectable text with part of its line selected by a finger**: the highlight over
/// "part of" and the two handles hanging below its ends, in the theme's colours.
#[test]
fn a_selectable_texts_touch_selection() {
    let root: Container<()> = Container::new().padding(16.0).child(
        Flex::column()
            .gap(12.0)
            .child(Text::new("Select part of this line").selectable().no_wrap())
            .child(Text::new("A second text, not selected").no_wrap()),
    );
    let mut stage = Stage::new(300, 110);
    let theme = stage.theme.clone();
    let (id, _) = stage
        .build(&root as &dyn Widget<()>)
        .focus_hit(Point::new(30.0, 24.0))
        .expect("the selectable text takes the focus");
    // What a long press on "part" and a drag to the end of "of" leave behind.
    stage.runtime.edits.insert(
        id,
        Edit {
            cursor: 14,
            anchor: Some(7),
            ..Edit::default()
        },
    );
    stage.runtime.input.focused = Some(id);
    stage.runtime.selection_handles = Some(id);
    stage.settle(&root);

    let token = theme.widgets.text_selection.selection_color_in(&theme);
    assert_eq!(
        token, theme.selection,
        "the untold highlight is the theme's token"
    );
    let ui = stage.build(&root as &dyn Widget<()>);
    let rects = highlights(ui.scene().primitives(), token);
    assert_eq!(rects.len(), 1, "one line, one highlight: {rects:?}");
    let primary = theme.widgets.text_selection.handle_color_in(&theme);
    assert_eq!(primary, theme.scheme.primary);
    assert_eq!(handles(ui.scene().primitives(), primary), 2, "two handles");

    let Some(shot) = stage.render(&root) else {
        eprintln!("no GPU adapter available: selection_text_touch skipped");
        return;
    };
    assert_highlight_blend(&shot, rects[0], token, theme.background);
    shot.assert_golden(golden("selection_text_touch"));
}

/// **A selection area's touch selection across two texts**, from the middle of a plain line
/// into the middle of a rich paragraph: one highlight per text, the second running over
/// spans of different weights and sizes, and a handle at each end.
#[test]
fn a_selection_area_across_a_text_and_a_rich_text() {
    let accent = Color::rgb(1.0, 0.702, 0.0);
    let root: SelectionArea<()> = SelectionArea::around(
        Container::new().padding(16.0).child(
            Flex::column()
                .gap(14.0)
                .child(Text::new("Plain words come first").no_wrap())
                .child(
                    RichText::new(
                        TextSpan::new("then a ")
                            .child(TextSpan::new("bold").bold())
                            .child(TextSpan::new(" and a big").size(24.0).color(accent))
                            .child(TextSpan::new(" ending")),
                    )
                    .no_wrap(),
                )
                .child(Text::new("and a last line left alone").no_wrap()),
        ),
    );
    let mut stage = Stage::new(340, 170);
    let theme = stage.theme.clone();
    stage.settle(&root);
    let stops = stage.build(&root as &dyn Widget<()>).text_stops().to_vec();
    assert_eq!(
        stops.len(),
        3,
        "the plain line, the paragraph, the last line"
    );
    // "Plain words come first" is 22 characters, the paragraph 28, the last line 26.
    let lengths = [(stops[0].id, 22), (stops[1].id, 28), (stops[2].id, 26)];
    let from = RegionPoint {
        text: stops[0].id,
        index: 6,
    };
    let to = RegionPoint {
        text: stops[1].id,
        index: 21,
    };
    let mut selection = RegionSelection::new(stops[0].area, from, to, &lengths);
    // Made with a finger: its handles, and no bar, so the picture is the highlight's.
    selection.handles = true;
    selection.bar = false;
    stage.runtime.region = Some(selection);
    stage.settle(&root);

    let token = theme.widgets.text_selection.selection_color_in(&theme);
    let ui = stage.build(&root as &dyn Widget<()>);
    let rects = highlights(ui.scene().primitives(), token);
    assert_eq!(rects.len(), 2, "one per text it crosses: {rects:?}");
    assert!(rects[0].y < rects[1].y, "in reading order: {rects:?}");
    assert!(
        rects[1].height > 24.0,
        "as tall as the paragraph's big run: {rects:?}"
    );
    assert_eq!(
        handles(ui.scene().primitives(), theme.scheme.primary),
        2,
        "a handle at each end"
    );

    let Some(shot) = stage.render(&root) else {
        eprintln!("no GPU adapter available: selection_area_rich skipped");
        return;
    };
    for rect in &rects {
        assert_highlight_blend(&shot, *rect, token, theme.background);
    }
    shot.assert_golden(golden("selection_area_rich"));
}
