// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! A pixel test for each widget milestone 786 added: the split button, the button group,
//! the floating action button menu, the toolbars, the side sheet, the full-screen dialog,
//! the rich tooltip and the contextual bar of selection mode.
//!
//! With no GPU adapter the tests skip themselves, the harness returning `None`.

use frus_test::render_widget;
use frus_widgets::{
    text, ButtonGroup, ButtonSize, Container, FabMenu, Flex, FullScreenDialog, IconButton, Icons,
    Positioned, RichTooltip, SelectionController, SideSheet, SplitButton, Stack, Text, Theme,
    Toolbar, Variant, Widget,
};

fn golden(name: &str) -> String {
    format!("{}/tests/goldens/{name}.png", env!("CARGO_MANIFEST_DIR"))
}

/// Renders `root` in a `width`×`height` dark-theme window, the way the shell would,
/// and compares it against `tests/goldens/<name>.png`.
fn check(name: &str, width: u32, height: u32, root: &dyn Widget<()>) {
    let theme = Theme::dark().with_platform(frus_core::TargetPlatform::Linux);
    let Some(snapshot) = render_widget(root, width, height, &theme) else {
        eprintln!("no GPU adapter available: {name} skipped");
        return;
    };
    assert!(snapshot.lit_pixels(48) > 40, "{name}: the frame is empty");
    snapshot.assert_golden(golden(name));
}

/// A page under the overlays, so the scrim has something to darken.
fn page() -> Container<()> {
    Container::new().padding(16.0).child(
        Flex::column()
            .gap(8.0)
            .child(text("The page underneath").size(15.0))
            .child(text("A second line, under the scrim.").size(15.0)),
    )
}

/// **Split buttons in four variants and three sizes**, one with its menu toggle open.
#[test]
fn the_split_buttons() {
    let root: Container<()> = Container::new().padding(16.0).child(
        Flex::column()
            .gap(12.0)
            .child(
                Flex::row()
                    .gap(12.0)
                    .child(
                        SplitButton::new("Send", ())
                            .icon(Icons::SEND)
                            .menu(false, ()),
                    )
                    .child(
                        SplitButton::new("Save", ())
                            .variant(Variant::Tonal)
                            .menu(true, ()),
                    ),
            )
            .child(
                Flex::row()
                    .gap(12.0)
                    .child(
                        SplitButton::new("Share", ())
                            .variant(Variant::Outlined)
                            .size(ButtonSize::ExtraSmall)
                            .menu(false, ()),
                    )
                    .child(
                        SplitButton::new("Edit", ())
                            .variant(Variant::Elevated)
                            .menu(false, ()),
                    ),
            )
            .child(
                SplitButton::new("Compose", ())
                    .icon(Icons::EDIT)
                    .size(ButtonSize::Medium)
                    .menu(false, ()),
            ),
    );
    check("m3_split_buttons", 380, 220, &root);
}

/// **Button groups**: connected single choice, standard multiple choice with glyphs, and
/// a medium connected group.
#[test]
fn the_button_groups() {
    let root: Container<()> = Container::new().padding(16.0).child(
        Flex::column()
            .gap(16.0)
            .child(
                ButtonGroup::single(Some(1), |_| ())
                    .connected(true)
                    .button("Day")
                    .button("Week")
                    .button("Month"),
            )
            .child(
                ButtonGroup::multiple([0, 2], |_| ())
                    .icon_button(Icons::FORMAT_BOLD, "Bold")
                    .icon_button(Icons::FORMAT_ITALIC, "Italic")
                    .icon_button(Icons::FORMAT_UNDERLINED, "Underline"),
            )
            .child(
                ButtonGroup::single(Some(0), |_| ())
                    .connected(true)
                    .size(ButtonSize::Medium)
                    .labelled_icon_button(Icons::EDIT, "Edit")
                    .labelled_icon_button(Icons::SEND, "Send"),
            ),
    );
    check("m3_button_groups", 380, 230, &root);
}

/// **A floating action button menu open** over its page: the actions stacked above the
/// close button, a scrim over the rest.
#[test]
fn a_fab_menu_open() {
    let root: Stack<()> = Stack::new().layer(page()).layer(
        Positioned::new(
            FabMenu::new(true, ())
                .item(Icons::EDIT, "Compose", ())
                .item(Icons::VIDEOCAM, "New meeting", ())
                .item(Icons::SEND, "Send file", ()),
        )
        .right(16.0)
        .bottom(16.0),
    );
    check("m3_fab_menu_open", 360, 360, &root);
}

/// **The toolbars**: docked across the bottom, floating horizontally, floating vertically
/// and vibrant.
#[test]
fn the_toolbars() {
    let icons = || {
        [
            Icons::FORMAT_BOLD,
            Icons::FORMAT_ITALIC,
            Icons::FORMAT_UNDERLINED,
            Icons::EDIT,
        ]
        .into_iter()
        .map(|icon| IconButton::<()>::new(icon).on_press(()))
    };
    let docked = icons().fold(Toolbar::docked(), |bar, button| bar.item(button));
    let floating = icons().fold(Toolbar::floating(), |bar, button| bar.item(button));
    let vertical = icons().take(3).fold(
        Toolbar::floating().vertical(true).vibrant(true),
        |bar, button| bar.item(button),
    );
    let root: Stack<()> = Stack::new()
        .width(380.0)
        .height(320.0)
        .layer(page())
        .layer(Positioned::new(vertical).right(16.0).top(16.0))
        .layer(Positioned::new(floating).left(16.0).bottom(88.0))
        .layer(Positioned::new(docked).left(0.0).right(0.0).bottom(0.0));
    check("m3_toolbars", 380, 320, &root);
}

/// **A modal side sheet** open from the end edge, with its heading, close button,
/// content and actions.
#[test]
fn a_modal_side_sheet() {
    let root: Container<()> = Container::new().child(
        SideSheet::modal(true)
            .title("Filters")
            .on_dismiss(())
            .child(
                Flex::column()
                    .gap(8.0)
                    .child(text("Price").size(14.0))
                    .child(text("Rating").size(14.0)),
            )
            .action(frus_widgets::button("Apply", ()))
            .action(frus_widgets::button("Cancel", ()).variant(Variant::Outlined))
            .body(page()),
    );
    check("m3_side_sheet_modal", 480, 320, &root);
}

/// **A standard side sheet** beside its page.
#[test]
fn a_standard_side_sheet() {
    let root: Container<()> = Container::new().width(520.0).height(240.0).child(
        SideSheet::standard(true)
            .title("Details")
            .on_dismiss(())
            .child(text("Owner, size, modified").size(14.0))
            .body(page()),
    );
    check("m3_side_sheet_standard", 520, 240, &root);
}

/// **A full-screen dialog**: the header with its cross, title and action over the form.
#[test]
fn a_full_screen_dialog() {
    let root: Container<()> = Container::new().child(
        FullScreenDialog::new(true)
            .title("New event")
            .on_close(())
            .action("Save", ())
            .child(
                Flex::column()
                    .gap(12.0)
                    .padding_each(16.0, 0.0, 0.0, 0.0)
                    .child(text("Title").size(15.0))
                    .child(text("Where and when").size(15.0)),
            )
            .body(page()),
    );
    // The shell describes the window; the dialog is as large as what it describes.
    let media = frus_widgets::MediaQuery {
        size: frus_core::Size::new(360.0, 300.0),
        ..frus_widgets::MediaQuery::UNSET
    };
    media.scope(|| check("m3_full_screen_dialog", 360, 300, &root));
}

/// **A persistent rich tooltip**, open over its anchor with a title, lines and actions.
#[test]
fn a_rich_tooltip_open() {
    let root: Container<()> = Container::new()
        .padding_each(190.0, 16.0, 16.0, 40.0)
        .child(
            RichTooltip::new(IconButton::<()>::new(Icons::INFO).on_press(()))
                .title("Grant value")
                .text("The amount awarded, before fees and taxes are taken out.")
                .action("Learn more", ())
                .action("Got it", ())
                .persistent(true, ()),
        );
    check("m3_rich_tooltip", 380, 260, &root);
}

/// **Selection mode**: the contextual bar and a list with two of four selected.
#[test]
fn a_list_in_selection_mode() {
    let mut selection = SelectionController::new();
    selection.long_press(1u32);
    selection.tap(&3);
    let mut column = Flex::column().child(
        selection.app_bar(()).action(
            IconButton::<()>::new(Icons::DELETE)
                .label("Delete")
                .on_press(()),
        ),
    );
    for key in 0..4u32 {
        column = column.child(
            selection.item(
                &key,
                Container::new()
                    .height(56.0)
                    .padding_each(0.0, 16.0, 0.0, 16.0)
                    .alignment(frus_core::Alignment::CENTER_LEFT)
                    .child(Text::new(format!("Message {key}"))),
                (),
                (),
            ),
        );
    }
    check("m3_selection_mode", 360, 300, &column);
}
