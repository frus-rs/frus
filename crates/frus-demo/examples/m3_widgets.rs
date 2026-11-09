// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The expressive Material 3 widgets of milestone 786 on one page: the split button, the
//! button groups, the floating action button menu, the toolbars, the side sheet, the
//! full-screen dialog, the rich tooltip and selection mode.
//!
//! `cargo run -p frus-demo --example m3_widgets` on the desktop. `FRUS_M3=` with `fab`,
//! `sheet`, `dialog`, `tip`, `split` or `select` opens that one at start, for a
//! screenshot.

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
fn main() -> frus_shell::anyhow::Result<()> {
    use frus_shell::FrusApp;
    use frus_widgets::{
        button, ButtonGroup, ButtonSize, Container, FabMenu, Flex, FullScreenDialog, HideOnScroll,
        IconButton, Icons, Positioned, RichTooltip, SelectionController, SideSheet,
        SingleChildScrollView, SplitButton, Stack, Text, Toolbar, Variant, Widget,
    };

    let start = std::env::var("FRUS_M3").unwrap_or_default();
    let app = FrusApp::from_fn(move |cx| {
        let theme = cx.theme().clone();
        let selection = cx.use_state(|| {
            let mut s = SelectionController::<u32>::new();
            if start == "select" {
                s.long_press(1);
                s.tap(&3);
            }
            s
        });
        let split_open = cx.use_state(|| start == "split");
        let view = cx.use_state(|| Some(1usize));
        let styles = cx.use_state(|| vec![0usize]);
        let fab_open = cx.use_state(|| start == "fab");
        let sheet_open = cx.use_state(|| start == "sheet");
        let dialog_open = cx.use_state(|| start == "dialog");
        let tip_open = cx.use_state(|| start == "tip");
        let hide = cx.use_state(HideOnScroll::new);
        let said = cx.use_state(String::new);

        let say = |said: &frus_widgets::UseState<String>, what: &str| {
            let said = said.clone();
            let what = what.to_string();
            move || said.set(what.clone())
        };
        let heading = |words: &str| {
            Text::styled(words.to_string(), theme.text.title_medium).color(theme.scheme.on_surface)
        };

        // Selection mode: the contextual bar replaces the title while it is on.
        let sel = selection.get();
        let top: Box<dyn Widget> = if sel.is_active() {
            let s = selection.clone();
            let s2 = selection.clone();
            Box::new(
                sel.app_bar(move || s.update(|s| s.clear())).action(
                    IconButton::new(Icons::DELETE)
                        .label("Delete")
                        .on_press(move || s2.update(|s| s.clear())),
                ),
            )
        } else {
            Box::new(
                frus_widgets::AppBar::new()
                    .title(Text::new("Material 3 widgets"))
                    .action(
                        RichTooltip::new(IconButton::new(Icons::INFO).on_press({
                            let t = tip_open.clone();
                            move || t.set(!t.get())
                        }))
                        .title("About this page")
                        .text("Every widget here is themed from the scheme and can be restyled.")
                        .action("Got it", {
                            let t = tip_open.clone();
                            move || t.set(false)
                        })
                        .persistent(tip_open.get(), {
                            let t = tip_open.clone();
                            move || t.set(false)
                        }),
                    ),
            )
        };

        let split = SplitButton::new("Send", say(&said, "Send"))
            .icon(Icons::SEND)
            .menu(split_open.get(), {
                let o = split_open.clone();
                move || o.set(!o.get())
            })
            .item("Schedule send", {
                let o = split_open.clone();
                let s = said.clone();
                move || {
                    o.set(false);
                    s.set("Schedule send".into())
                }
            })
            .item("Save draft", {
                let o = split_open.clone();
                let s = said.clone();
                move || {
                    o.set(false);
                    s.set("Save draft".into())
                }
            });
        let groups = Flex::column()
            .gap(12.0)
            .child(
                ButtonGroup::single(view.get(), {
                    let v = view.clone();
                    move |i| v.set(Some(i))
                })
                .connected(true)
                .button("Day")
                .button("Week")
                .button("Month"),
            )
            .child(
                ButtonGroup::multiple(styles.get(), {
                    let st = styles.clone();
                    move |i| {
                        st.update(|on| {
                            if let Some(at) = on.iter().position(|&x| x == i) {
                                on.remove(at);
                            } else {
                                on.push(i);
                            }
                        })
                    }
                })
                .size(ButtonSize::Medium)
                .icon_button(Icons::FORMAT_BOLD, "Bold")
                .icon_button(Icons::FORMAT_ITALIC, "Italic")
                .icon_button(Icons::FORMAT_UNDERLINED, "Underline"),
            );

        let openers = Flex::row()
            .gap(8.0)
            .child(
                button("Sheet", {
                    let o = sheet_open.clone();
                    move || o.set(true)
                })
                .variant(Variant::Tonal),
            )
            .child(
                button("Dialog", {
                    let o = dialog_open.clone();
                    move || o.set(true)
                })
                .variant(Variant::Tonal),
            );

        let mut list = Flex::column();
        for key in 0..12u32 {
            let s = selection.clone();
            let s2 = selection.clone();
            let opened = said.clone();
            list = list.child(
                sel.item(
                    &key,
                    Container::new()
                        .height(56.0)
                        .padding_each(0.0, 16.0, 0.0, 16.0)
                        .alignment(frus_widgets::Alignment::CENTER_LEFT)
                        .child(Text::new(format!("Message {key}"))),
                    move || {
                        let mut taken = false;
                        s.update(|s| taken = s.tap(&key));
                        if !taken {
                            opened.set(format!("Opened message {key}"));
                        }
                    },
                    move || s2.update(|s| s.long_press(key)),
                ),
            );
        }

        let content = Flex::column()
            .gap(20.0)
            .padding(16.0)
            .child(heading("Split button"))
            .child(split)
            .child(heading("Button groups"))
            .child(groups)
            .child(heading("Sheets and dialogs"))
            .child(openers)
            .child(Text::new(said.get()).color(theme.scheme.primary))
            .child(heading("Selection mode — long-press a message"))
            .child(list)
            .child(Container::new().height(120.0));
        let scroll = SingleChildScrollView::new()
            .flex(1.0)
            .on_scroll({
                let h = hide.clone();
                move |position| {
                    h.update(|h| {
                        h.update_from(position);
                    })
                }
            })
            .child(content);

        let toolbar = [
            Icons::FORMAT_BOLD,
            Icons::FORMAT_ITALIC,
            Icons::FORMAT_UNDERLINED,
            Icons::EDIT,
        ]
        .into_iter()
        .fold(Toolbar::floating(), |bar, icon| {
            bar.item(IconButton::new(icon).on_press(say(&said, "Toolbar")))
        })
        .hidden(hide.get().hidden());
        let fab = FabMenu::new(fab_open.get(), {
            let o = fab_open.clone();
            move || o.set(!o.get())
        })
        .item(Icons::EDIT, "Compose", {
            let o = fab_open.clone();
            let s = said.clone();
            move || {
                o.set(false);
                s.set("Compose".into())
            }
        })
        .item(Icons::VIDEOCAM, "New meeting", {
            let o = fab_open.clone();
            let s = said.clone();
            move || {
                o.set(false);
                s.set("New meeting".into())
            }
        });

        // The page is the window's size: it sits under a dialog and a sheet, each of which
        // lays its body out as a share of the room it has.
        let window = frus_widgets::MediaQuery::of().size;
        let page = Stack::new()
            .width(window.width)
            .height(window.height)
            .fit(frus_widgets::StackFit::Expand)
            .layer(Flex::column().flex(1.0).child(top).child(scroll))
            .layer(Positioned::new(toolbar).left(16.0).bottom(16.0))
            .layer(Positioned::new(fab).right(16.0).bottom(16.0));

        let sheet = SideSheet::modal(sheet_open.get())
            .title("Filters")
            .on_dismiss({
                let o = sheet_open.clone();
                move || o.set(false)
            })
            .child(
                Flex::column()
                    .gap(8.0)
                    .child(Text::new("Price"))
                    .child(Text::new("Rating"))
                    .child(Text::new("Distance")),
            )
            .action(button("Apply", {
                let o = sheet_open.clone();
                move || o.set(false)
            }))
            .body(page);
        let dialog = FullScreenDialog::new(dialog_open.get())
            .title("New event")
            .on_close({
                let o = dialog_open.clone();
                move || o.set(false)
            })
            .action("Save", {
                let o = dialog_open.clone();
                let s = said.clone();
                move || {
                    o.set(false);
                    s.set("Event saved".into())
                }
            })
            .child(
                Flex::column()
                    .gap(12.0)
                    .padding_each(16.0, 0.0, 0.0, 0.0)
                    .child(Text::new("Title"))
                    .child(Text::new("Where and when")),
            )
            .body(sheet);
        Box::new(Container::new().color(theme.background).child(dialog)) as Box<dyn Widget>
    })
    .title("frus Material 3 widgets");
    frus_shell::run(app)
}

#[cfg(any(target_os = "android", target_arch = "wasm32"))]
fn main() {}
