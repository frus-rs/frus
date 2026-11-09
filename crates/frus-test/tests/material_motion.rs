// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The Material motion patterns (milestone 785), **rendered** in the middle of a change:
//! a shared axis and a fade through between two children, two pages mid-push, a container
//! growing into the window, and a list item folding away. The ends of a transition are
//! what a jump also draws; the middle is what each pattern is.
//!
//! With no GPU adapter the tests skip themselves, the harness returning `None`.

use frus_core::{Color, Size};
use frus_test::{Snapshot, Stage};
use frus_widgets::{
    text, AnimatedSwitcher, Container, Flex, MediaQuery, MotionPattern, Navigator, OpenContainer,
    PageTransitionsBuilder, PresenceList, SharedAxis,
};

fn golden(name: &str) -> String {
    format!("{}/tests/goldens/{name}.png", env!("CARGO_MANIFEST_DIR"))
}

fn accept(name: &str, snapshot: Option<Snapshot>) {
    let Some(snapshot) = snapshot else {
        eprintln!("no GPU adapter available: {name} skipped");
        return;
    };
    assert!(snapshot.lit_pixels(48) > 40, "{name}: the frame is empty");
    snapshot.assert_golden(golden(name));
}

const COLOURS: [Color; 3] = [
    Color::rgb(0.20, 0.35, 0.65),
    Color::rgb(0.55, 0.25, 0.50),
    Color::rgb(0.15, 0.50, 0.40),
];

fn panel(n: u32) -> Container<()> {
    Container::new()
        .width(140.0)
        .height(90.0)
        .radius(12.0)
        .color(COLOURS[n as usize % 3])
        .padding(12.0)
        .child(
            text(format!("Step {}", n + 1))
                .size(18.0)
                .color(Color::WHITE),
        )
}

/// **A shared X axis and a fade through, a third of the way**: on the left the new step
/// coming in from the right as the old one has slid left and gone; on the right the old
/// album gone and the new one not yet in — the fade through's empty middle, on its fill.
#[test]
fn two_switches_a_third_of_the_way() {
    let view = |n: u32| {
        Flex::<()>::row()
            .gap(20.0)
            .padding(20.0)
            .child(AnimatedSwitcher::patterned(
                MotionPattern::SharedAxis(SharedAxis::X),
                n,
                |n: &u32| panel(*n),
            ))
            .child(AnimatedSwitcher::patterned(
                MotionPattern::FadeThrough,
                n,
                |n: &u32| panel(*n + 1),
            ))
    };
    let mut stage = Stage::new(360, 130);
    stage.settle(&view(0));
    stage.advance(&view(1), 0.0);
    stage.advance(&view(1), 0.1);
    accept("motion_switches_third", stage.render(&view(1)));
    // And at 60 %: the step nearly in, the album fading in from 92 %.
    stage.advance(&view(1), 0.08);
    accept("motion_switches_sixty", stage.render(&view(1)));
}

/// **Two pages halfway through a push**, under a navigator told its transition: along the
/// Z axis (the page arriving grows from 80 % over the surface) and as a fade scale (the
/// page under it does not move).
#[test]
fn a_navigator_mid_push_along_z_and_as_a_fade_scale() {
    let screen = |title: &str, colour: Color| -> Container<()> {
        Container::new()
            .width(260.0)
            .height(140.0)
            .color(colour)
            .padding(16.0)
            .child(text(title.to_string()).size(18.0))
    };
    for (name, builder) in [
        (
            "navigator_mid_push_shared_z",
            PageTransitionsBuilder::SharedAxis(SharedAxis::Z),
        ),
        (
            "navigator_mid_push_fade_scale",
            PageTransitionsBuilder::FadeScale,
        ),
    ] {
        let root: Navigator<()> = Navigator::new("second", screen("Second", COLOURS[0]))
            .size(260.0, 140.0)
            .from("first", screen("First", COLOURS[1]), 0.5, true)
            .transition(builder);
        let mut stage = Stage::new(260, 140);
        stage.settle(&root);
        accept(name, stage.render(&root));
    }
}

/// **A card a third of the way into a page**: the container between the card's box and the
/// window's, its corners part-way to square, the page's content scaled to its width, a
/// scrim over what is behind.
#[test]
fn a_container_a_third_of_the_way_open() {
    let _surface = MediaQuery {
        size: Size::new(320.0, 240.0),
        ..MediaQuery::of()
    }
    .install();
    let view = |open: bool| {
        Container::<()>::new()
            .width(320.0)
            .height(240.0)
            .color(Color::rgb(0.08, 0.09, 0.12))
            .padding(24.0)
            .child(
                Flex::column().child(
                    OpenContainer::new(
                        open,
                        Container::new()
                            .width(120.0)
                            .height(80.0)
                            .padding(12.0)
                            .child(text("Lisbon").color(Color::WHITE)),
                        Container::new()
                            .width(320.0)
                            .height(240.0)
                            .padding(24.0)
                            .child(text("Lisbon, open").size(24.0).color(Color::WHITE)),
                    )
                    .closed_color(COLOURS[0])
                    .open_color(COLOURS[2]),
                ),
            )
    };
    let mut stage = Stage::new(320, 240);
    stage.settle(&view(false));
    stage.advance(&view(true), 0.0);
    stage.advance(&view(true), 0.1);
    accept("open_container_third", stage.render(&view(true)));
}

/// **A list item halfway out**: the middle row folding away where it was, faded, the row
/// under it moving up into its room.
#[test]
fn a_list_item_halfway_out() {
    let row = |n: &u32| {
        Container::<()>::new()
            .padding_each(4.0, 12.0, 4.0, 12.0)
            .child(
                Container::new()
                    .width(200.0)
                    .height(36.0)
                    .radius(8.0)
                    .color(COLOURS[*n as usize % 3])
                    .padding(8.0)
                    .child(text(format!("Item {n}")).color(Color::WHITE)),
            )
    };
    let view = |items: Vec<u32>| PresenceList::new(items, |n| *n, row);
    let mut stage = Stage::new(240, 150);
    stage.settle(&view(vec![0, 1, 2]));
    stage.advance(&view(vec![0, 2]), 0.0);
    stage.advance(&view(vec![0, 2]), 0.1);
    accept("presence_list_halfway_out", stage.render(&view(vec![0, 2])));
}
