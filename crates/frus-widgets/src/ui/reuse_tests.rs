// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Milestone 856: a component that was not built again gives back what its subtree painted
//! and registered the frame before. Each test changes one thing a kept subtree is painted
//! from and checks the frame against the same frame painted from scratch, so a stale
//! reuse — a picture, a target, a focus stop or an accessibility node left where it was —
//! shows as a difference.

use std::cell::RefCell;
use std::fmt::Write as _;

use frus_core::{Rect, Size};

use super::{build_deferred, build_ui, Ui};
use crate::callback::Callback;
use crate::interaction::WidgetId;
use crate::runtime::Runtime;
use crate::theme::Theme;
use crate::widget::Widget;
use crate::{
    component, BuildContext, Button, Container, Flex, SingleChildScrollView, Text, UseState,
};

thread_local! {
    /// Each row's count, to change one as a handler would.
    static COUNTS: RefCell<Vec<UseState<u32>>> = const { RefCell::new(Vec::new()) };
}

/// A row with a state of its own: a tappable box holding its count, and a button.
fn row(index: usize, height: f32) -> Box<dyn Widget> {
    Box::new(component(move |cx: &BuildContext| {
        let count = cx.use_state(|| 0u32);
        COUNTS.with(|all| {
            let mut all = all.borrow_mut();
            if all.len() <= index {
                all.resize(index + 1, count.clone());
            }
            all[index] = count.clone();
        });
        Box::new(
            Flex::row()
                .child(
                    Container::new()
                        .width(200.0)
                        .height(height + count.get() as f32 * 10.0)
                        .on_click(Callback::new(|| {}))
                        .child(Text::new(format!("Row {index}: {}", count.get()))),
                )
                .child(Button::new("Open").on_press(Callback::new(|| {}))),
        ) as Box<dyn Widget>
    }))
}

fn rows(count: usize) -> Flex<Callback> {
    COUNTS.with(|all| all.borrow_mut().clear());
    let mut column = Flex::column();
    for index in 0..count {
        column = column.child_boxed(row(index, 30.0));
    }
    column
}

fn bump(index: usize) {
    COUNTS.with(|all| all.borrow()[index].update(|n| *n += 1));
}

const SIZE: Size = Size::new(400.0, 600.0);

/// One frame, as the shell runs it: the components marked are built, then the frame.
fn frame(rt: &Runtime, root: &dyn Widget, theme: &Theme) -> Ui<Callback> {
    rt.states.begin_build();
    build_deferred(root, theme, rt);
    let ui = build_ui(root, SIZE, rt, theme);
    rt.states.end_frame();
    ui
}

fn rect(r: Rect) -> String {
    format!("[{} {} {} {}]", r.x, r.y, r.width, r.height)
}

/// Everything a frame shows and answers, written out: what it paints, what can be pressed,
/// focused, selected, hovered and read aloud, and in what order.
fn digest(ui: &Ui<Callback>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "scene {:?}", ui.scene().primitives());
    for hit in ui.hits.iter().chain(&ui.long_presses) {
        let _ = writeln!(out, "hit {:?} {} {:?}", hit.id, rect(hit.rect), hit.xform);
    }
    let _ = writeln!(out, "focus {:?}", ui.focusables);
    let _ = writeln!(out, "order {:?}", ui.traversal_order());
    let _ = writeln!(out, "semantics {:?} {:?}", ui.semantics, ui.semantics_spans);
    for stop in &ui.text_stops {
        let _ = writeln!(
            out,
            "stop {:?} {} {:?}",
            stop.id,
            rect(stop.rect),
            stop.area
        );
    }
    let _ = writeln!(out, "hover {:?}", ui.hover_regions);
    let _ = writeln!(out, "inks {:?}", ui.inks);
    let _ = writeln!(out, "scrolls {}", ui.scrollables.len());
    let scopes: Vec<_> = ui.scopes.iter().map(|scope| scope.range.clone()).collect();
    let _ = writeln!(out, "scopes {scopes:?}");
    let _ = writeln!(out, "animating {}", ui.wants_animation());
    out
}

/// The frame `ui` is, painted from scratch: nothing kept.
fn fresh(rt: &Runtime, root: &dyn Widget, theme: &Theme) -> String {
    *rt.paint_cache.borrow_mut() = crate::PaintCache::default();
    digest(&build_ui(root, SIZE, rt, theme))
}

/// A frame against the same frame from scratch, and what the cache did in it.
fn checked(rt: &Runtime, root: &dyn Widget, theme: &Theme) -> (u32, u32) {
    let ui = frame(rt, root, theme);
    let stats = rt.paint_cache.borrow().last_frame_stats();
    let kept = digest(&ui);
    assert_eq!(kept, fresh(rt, root, theme), "a kept frame is the frame");
    // Fill the cache again for what comes next.
    frame(rt, root, theme);
    frame(rt, root, theme);
    stats
}

/// The heart of it: one row's own state changes among fifty; that row is painted, and the
/// other forty-nine are given back as they were, with everything they registered.
#[test]
fn one_row_changes_and_the_others_are_given_back() {
    let rt = Runtime::default();
    let theme = Theme::default();
    let root = rows(50);
    frame(&rt, &root, &theme);
    assert_eq!(rt.paint_cache.borrow().last_frame_stats(), (0, 50));
    frame(&rt, &root, &theme);
    assert_eq!(
        rt.paint_cache.borrow().last_frame_stats(),
        (50, 0),
        "nothing changed: everything given back"
    );
    // Row 20's own state: only it is painted. It grows by ten pixels, so the rows below it
    // move — and are moved, not painted, when they stay inside the clip.
    bump(20);
    let (kept, painted) = checked(&rt, &root, &theme);
    assert_eq!(painted, 1, "the row that changed, and only it");
    assert_eq!(kept, 49);
    // And again with nothing moving: the row is built again with the count it had, which
    // makes it a new subtree, painted; the others stay where they were and are kept.
    COUNTS.with(|all| all.borrow()[20].set(1));
    let (kept, painted) = checked(&rt, &root, &theme);
    assert_eq!((kept, painted), (49, 1));
}

/// A theme changed with nothing built (a theme in flight is one): nothing kept under the old
/// one is given back.
#[test]
fn a_new_theme_paints_everything_again() {
    let rt = Runtime::default();
    let root = rows(10);
    let light = Theme::light();
    frame(&rt, &root, &light);
    frame(&rt, &root, &light);
    let dark = Theme::dark();
    let (kept, painted) = checked(&rt, &root, &dark);
    assert_eq!((kept, painted), (0, 10));
    // The same theme as a new value is the same theme.
    let (kept, _) = checked(&rt, &root, &Theme::dark());
    assert_eq!(kept, 10);
}

/// The surface changed: nothing kept for the old one is given back.
#[test]
fn a_new_surface_paints_everything_again() {
    let rt = Runtime::default();
    let theme = Theme::default();
    let root = rows(10);
    frame(&rt, &root, &theme);
    frame(&rt, &root, &theme);
    let _surface = crate::MediaQuery::new(Size::new(500.0, 900.0))
        .with_device_pixel_ratio(2.0)
        .install();
    let (kept, painted) = checked(&rt, &root, &theme);
    assert_eq!((kept, painted), (0, 10));
}

/// A row hovered is painted again, with its hover; the others are kept.
#[test]
fn a_row_under_the_pointer_is_painted_again() {
    let mut rt = Runtime::default();
    let theme = Theme::default();
    let root = rows(10);
    let ui = frame(&rt, &root, &theme);
    frame(&rt, &root, &theme);
    // Row 2's box: each row registers its box, then its button.
    let target = ui.hits[4].id;
    rt.input.hovered = Some(target);
    let (kept, painted) = checked(&rt, &root, &theme);
    assert_eq!((kept, painted), (9, 1));
    // A value in flight on a kept row's box: that row is painted again.
    rt.input.hovered = None;
    frame(&rt, &root, &theme);
    rt.anims.entry(target).or_default().opacity = 0.5;
    let (kept, painted) = checked(&rt, &root, &theme);
    assert_eq!((kept, painted), (9, 1));
}

/// Rows in a scroll area: a scroll moves them by whole pixels, and what is well inside the
/// viewport is moved rather than painted; what crosses its edge is painted again. Either
/// way the frame is the frame, targets and focus stops included.
#[test]
fn a_scroll_moves_what_was_kept() {
    let mut rt = Runtime::default();
    let theme = Theme::default();
    COUNTS.with(|all| all.borrow_mut().clear());
    let root = SingleChildScrollView::new().child({
        let mut column = Flex::column();
        for index in 0..40 {
            column = column.child_boxed(row(index, 30.0));
        }
        column
    });
    let ui = frame(&rt, &root, &theme);
    frame(&rt, &root, &theme);
    let area = ui.scroll_regions()[0].id;
    rt.scroll.insert(area, (0.0, 64.0));
    let (kept, painted) = checked(&rt, &root, &theme);
    assert!(
        kept >= 10,
        "the rows well inside the viewport are moved: {kept}"
    );
    assert!(
        painted >= 1,
        "and those across its edges are painted: {painted}"
    );
    // A scroll by part of a pixel cannot be followed by moving: everything is painted.
    rt.scroll.insert(area, (0.0, 64.25));
    let (kept, _) = checked(&rt, &root, &theme);
    assert_eq!(kept, 0);
}

/// A transform above the kept rows: what they give back is transformed with them, the
/// targets included.
#[test]
fn an_ancestor_transform_is_applied_to_what_is_given_back() {
    let rt = Runtime::default();
    let theme = Theme::default();
    let root = crate::Transform::scale(0.5).child(rows(8));
    frame(&rt, &root, &theme);
    let (kept, _) = checked(&rt, &root, &theme);
    assert_eq!(kept, 8);
    bump(3);
    let (kept, painted) = checked(&rt, &root, &theme);
    assert_eq!((kept, painted), (7, 1));
}

/// Text in a selection area: the stops a selection runs through are given back with the
/// rows, in reading order.
#[test]
fn the_stops_of_a_selection_are_given_back() {
    let rt = Runtime::default();
    let theme = Theme::default();
    let root = crate::SelectionArea::around(rows(6));
    let ui = frame(&rt, &root, &theme);
    assert_eq!(
        ui.text_stops.len(),
        6,
        "a row's count; a button's label is not selectable"
    );
    bump(4);
    let (kept, painted) = checked(&rt, &root, &theme);
    assert_eq!((kept, painted), (5, 1));
}

/// A selection made across kept rows highlights them: every row it covers is painted.
#[test]
fn a_selection_over_kept_rows_paints_them() {
    let mut rt = Runtime::default();
    let theme = Theme::default();
    let root = crate::SelectionArea::around(rows(6));
    let ui = frame(&rt, &root, &theme);
    frame(&rt, &root, &theme);
    let (first, area) = (ui.text_stops[0].id, ui.text_stops[0].area);
    let at = |index| crate::RegionPoint { text: first, index };
    rt.region = Some(crate::RegionSelection {
        area,
        anchor: at(0),
        extent: at(3),
        ranges: [(first, (0, 3))].into_iter().collect(),
        all: false,
        bar: false,
        handles: false,
        start: Some(at(0)),
        end: Some(at(3)),
    });
    let (_, painted) = checked(&rt, &root, &theme);
    assert!(painted >= 1);
}

/// However deep the components, the cache holds a bounded multiple of the frame.
#[test]
fn nested_components_keep_a_bounded_amount() {
    fn nest(depth: usize, leaves: usize) -> Box<dyn Widget> {
        Box::new(component(move |_cx: &BuildContext| {
            let mut column = Flex::column();
            if depth == 0 {
                for index in 0..leaves {
                    column = column.child(Text::new(format!("Leaf {index}")));
                }
            } else {
                column = column.child_boxed(nest(depth - 1, leaves));
                column = column.child_boxed(nest(depth - 1, leaves));
            }
            Box::new(column) as Box<dyn Widget>
        }))
    }
    let rt = Runtime::default();
    let theme = Theme::default();
    let root = Container::new().child(nest(8, 40));
    let ui = frame(&rt, &root, &theme);
    let painted = ui.scene().primitives().len();
    frame(&rt, &root, &theme);
    let held = rt.paint_cache.borrow().held();
    assert!(
        held <= 4 * (painted + ui.semantics.len() + ui.text_stops.len()).max(1024),
        "{held} held for a frame of {painted}"
    );
    // And what is left is still given back.
    let (kept, _) = checked(&rt, &root, &theme);
    assert!(kept > 0);
}

thread_local! {
    /// State outside the tree that a paint reads, changed as a controller or a clock would.
    static SHADE: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

fn shade() -> u8 {
    SHADE.with(std::cell::Cell::get)
}

fn set_shade(value: u8) {
    SHADE.with(|cell| cell.set(value));
}

/// Rows, and a component whose subtree is painted from [`SHADE`] by `leaf`.
fn rows_and(count: usize, leaf: fn() -> Box<dyn Widget>) -> Flex<Callback> {
    rows(count).child(component(move |_cx: &BuildContext| {
        Box::new(Container::new().child(leaf())) as Box<dyn Widget>
    }))
}

/// A canvas painted by an application closure that reads [`SHADE`].
fn canvas() -> Box<dyn Widget> {
    Box::new(crate::CustomPaint::new(
        50.0,
        20.0,
        |scene, bounds, _theme| {
            scene.fill_rect(bounds, frus_core::Color::rgb8(shade(), 0, 0));
        },
    ))
}

/// A hand-written widget painted from [`SHADE`], which says so or does not.
struct Gauge {
    honest: bool,
}

impl Widget for Gauge {
    fn style(&self) -> frus_layout::Style {
        frus_layout::Style {
            width: frus_layout::Dimension::Length(50.0),
            height: frus_layout::Dimension::Length(20.0),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &[]
    }

    fn paint(
        &self,
        bounds: Rect,
        _status: crate::interaction::Status,
        _theme: &Theme,
        scene: &mut frus_core::Scene,
    ) {
        scene.fill_rect(bounds, frus_core::Color::rgb8(0, shade(), 0));
    }

    fn paint_source(&self) -> Option<u64> {
        self.honest.then(|| u64::from(shade()))
    }

    fn on_click(&self) -> Option<Callback> {
        None
    }
}

/// A `CustomPaint`'s closure may read anything, and says nothing of it: the component holding
/// one is painted every frame, and what it shows follows the state it reads although nothing
/// was built.
#[test]
fn a_custom_paint_is_painted_every_frame() {
    let rt = Runtime::default();
    let theme = Theme::default();
    set_shade(10);
    let root = rows_and(5, canvas);
    frame(&rt, &root, &theme);
    frame(&rt, &root, &theme);
    assert_eq!(
        rt.paint_cache.borrow().last_frame_stats(),
        (5, 1),
        "the rows are kept, the canvas is not, though nothing changed"
    );
    set_shade(200);
    let (kept, painted) = checked(&rt, &root, &theme);
    assert_eq!((kept, painted), (5, 1));
}

/// The contract on `paint` (milestone 856): a widget painted from shared state that says so
/// through `paint_source` is kept while its source stays and painted when it moves; one that
/// does not say so is given back stale, which is what the checks of this file catch.
#[test]
fn a_paint_source_says_when_to_paint_again() {
    let rt = Runtime::default();
    let theme = Theme::default();
    set_shade(10);
    let root = rows_and(5, || Box::new(Gauge { honest: true }));
    frame(&rt, &root, &theme);
    let (kept, _) = checked(&rt, &root, &theme);
    assert_eq!(kept, 6, "the source did not move: everything kept");
    set_shade(200);
    let (kept, painted) = checked(&rt, &root, &theme);
    assert_eq!(
        (kept, painted),
        (5, 1),
        "the source moved: the gauge is painted"
    );

    // Without `paint_source`, the same change is missed: the frame shows the old picture.
    let rt = Runtime::default();
    set_shade(10);
    let root = rows_and(5, || Box::new(Gauge { honest: false }));
    frame(&rt, &root, &theme);
    frame(&rt, &root, &theme);
    set_shade(200);
    let kept = digest(&frame(&rt, &root, &theme));
    assert_ne!(
        kept,
        fresh(&rt, &root, &theme),
        "the comparison catches a stale reuse"
    );
}

/// A row's own box after the walk: what a test taps.
#[allow(dead_code)]
fn box_of(ui: &Ui<Callback>, id: WidgetId) -> Option<Rect> {
    ui.hits.iter().find(|hit| hit.id == id).map(|hit| hit.rect)
}
