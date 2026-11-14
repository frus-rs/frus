// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Overlays and panels (milestone 809): a command palette over the screen (Ctrl+K), hover
//! cards on the names in the sidebar, a context menu on the canvas, and the three areas in
//! panels whose boundaries can be dragged.

use std::time::Duration;

use frus_widgets::host;
use frus_widgets::{
    CommandPalette, CommandPaletteState, ContextMenu, ContextMenuState, HoverCard, KeyStroke,
    MenuItem, PaletteCommand, ResizablePanel, ResizablePanels, ResizablePanelsState, ShortcutKey,
    SubmenuButton, Text,
};

use crate::prelude::*;

/// The people the sidebar lists, each with what their card says.
const PEOPLE: [(&str, &str, &str); 4] = [
    (
        "Ada",
        "Ada Lovelace",
        "Wrote the first program, for an engine never built.",
    ),
    (
        "Grace",
        "Grace Hopper",
        "Built the first compiler and named the bug.",
    ),
    (
        "Alan",
        "Alan Turing",
        "Asked what it means for a machine to think.",
    ),
    (
        "Edsger",
        "Edsger Dijkstra",
        "Found the shortest path, and the case against goto.",
    ),
];

/// The screen.
pub(crate) struct OverlaysPage;

/// What the screen keeps.
pub(crate) struct OverlaysState {
    palette: CommandPaletteState,
    /// Whether the palette searches the people, fetched after each key, instead of commands.
    people_mode: bool,
    found: Vec<usize>,
    loading: bool,
    menu: ContextMenuState,
    columns: ResizablePanelsState,
    rows: ResizablePanelsState,
    wrap: bool,
    sort: usize,
    log: Vec<String>,
}

impl StatefulWidget for OverlaysPage {
    type State = OverlaysState;

    fn create_state(&self) -> OverlaysState {
        OverlaysState {
            palette: CommandPaletteState::closed(),
            people_mode: false,
            found: Vec::new(),
            loading: false,
            menu: ContextMenuState::closed(),
            columns: ResizablePanelsState::new([0.28, 0.72]),
            rows: ResizablePanelsState::new([0.65, 0.35]),
            wrap: true,
            sort: 0,
            log: vec!["Press Ctrl+K, right-click the canvas, rest on a name.".into()],
        }
    }
}

impl OverlaysState {
    fn said(&mut self, what: impl Into<String>) {
        self.log.insert(0, what.into());
        self.log.truncate(6);
    }
}

/// A command that closes the palette and says what it did.
fn run(cx: &StateContext<OverlaysState>, what: &'static str) -> Callback {
    cx.callback(move |s| {
        s.palette = CommandPaletteState::closed();
        s.said(format!("Ran: {what}"));
    })
}

/// A menu row's message: close the menu, say what was chosen.
fn chose(cx: &StateContext<OverlaysState>, what: &'static str) -> Callback {
    cx.callback(move |s| {
        s.menu = ContextMenuState::closed();
        s.said(format!("Menu: {what}"));
    })
}

impl State for OverlaysState {
    type Widget = OverlaysPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();

        // The sidebar: a name each, with its card.
        let sidebar = PEOPLE.iter().fold(
            Flex::column()
                .gap(8.0)
                .padding(16.0)
                .child(text("People").size(18.0).color(theme.on_surface)),
            |list, (short, name, about)| {
                list.child(
                    HoverCard::new(
                        Container::new()
                            .padding(6.0)
                            .child(text(format!("@{short}")).size(15.0).color(theme.primary)),
                    )
                    .content(
                        Flex::column()
                            .gap(6.0)
                            .child(text(*name).size(16.0).weight(FontWeight::Bold))
                            .child(Text::new(*about).wrap()),
                    ),
                )
            },
        );

        // The canvas, and its menu.
        let canvas = ContextMenu::new(
            Container::new()
                .flex(1.0)
                .color(theme.scheme.surface_container_low)
                .padding(16.0)
                .child(
                    text("Canvas: right-click, or press and hold, for its menu")
                        .size(15.0)
                        .color(theme.muted),
                ),
            &self.menu,
            cx.handler(|s, state: ContextMenuState| s.menu = state),
        )
        .item(MenuItem::icon(Icons::CONTENT_CUT, "Cut", chose(cx, "Cut")).shortcut("Ctrl+X"))
        .item(MenuItem::icon(Icons::CONTENT_COPY, "Copy", chose(cx, "Copy")).shortcut("Ctrl+C"))
        .item(MenuItem::new("Paste", chose(cx, "Paste")).enabled(false))
        .divider()
        .item(MenuItem::checked(
            "Word wrap",
            self.wrap,
            cx.callback(|s| {
                s.wrap = !s.wrap;
                s.menu = ContextMenuState::closed();
            }),
        ))
        .submenu(["Name", "Date", "Size"].iter().enumerate().fold(
            SubmenuButton::new("Sort by"),
            |sub, (i, label)| {
                sub.item(MenuItem::radio(
                    *label,
                    self.sort == i,
                    cx.callback(move |s| {
                        s.sort = i;
                        s.menu = ContextMenuState::closed();
                    }),
                ))
            },
        ))
        .submenu(
            SubmenuButton::new("Share")
                .item(MenuItem::new("Email", chose(cx, "Email")))
                .item(MenuItem::new("Link", chose(cx, "Link"))),
        );

        let console = self
            .log
            .iter()
            .fold(Flex::column().gap(4.0).padding(12.0), |list, line| {
                list.child(text(line.clone()).size(13.0).color(theme.on_surface))
            });
        let main = ResizablePanels::vertical(
            &self.rows,
            cx.handler(|s, state: ResizablePanelsState| s.rows = state),
        )
        .panel(ResizablePanel::new(canvas).min(0.3).label("Canvas"))
        .panel(
            ResizablePanel::new(console)
                .min(0.15)
                .collapsible(true)
                .label("Console"),
        );
        let panels = ResizablePanels::horizontal(
            &self.columns,
            cx.handler(|s, state: ResizablePanelsState| {
                if !state.is_dragging() {
                    s.said(format!(
                        "Sidebar at {:.0}%",
                        state.sizes().first().copied().unwrap_or(0.0) * 100.0
                    ));
                }
                s.columns = state;
            }),
        )
        .panel(
            ResizablePanel::new(sidebar)
                .min(0.15)
                .max(0.5)
                .collapsible(true)
                .label("Sidebar"),
        )
        .panel(ResizablePanel::new(main).min(0.3));

        let body = Flex::column()
            .flex(1.0)
            .child(
                Flex::row()
                    .gap(12.0)
                    .padding(12.0)
                    .child(
                        button(
                            "Command palette (Ctrl+K)",
                            cx.callback(|s| {
                                s.people_mode = false;
                                s.palette = CommandPaletteState::opened();
                            }),
                        )
                        .variant(Variant::Filled),
                    )
                    .child(
                        button(
                            "Search people",
                            cx.callback(|s| {
                                s.people_mode = true;
                                s.found = (0..PEOPLE.len()).collect();
                                s.palette = CommandPaletteState::opened();
                            }),
                        )
                        .variant(Variant::Outlined),
                    ),
            )
            .child(Expanded::new(panels));

        let palette = if self.people_mode {
            // An asynchronous source: the people are "fetched" a moment after each key.
            let handle = cx.handle();
            CommandPalette::new(body, &self.palette, move |state: CommandPaletteState| {
                let handle = handle.clone();
                Callback::new(move || {
                    let query = state.query().to_string();
                    let changed = handle.read(|s| s.palette.query() != query);
                    let next = state.clone();
                    handle.set_state(|s| {
                        s.palette = next.clone();
                        if changed {
                            s.loading = true;
                        }
                    });
                    if changed {
                        let handle = handle.clone();
                        host::after(Duration::from_millis(400), move || {
                            let query = query.clone();
                            handle.set_state(|s| {
                                if s.palette.query() == query {
                                    s.found = (0..PEOPLE.len())
                                        .filter(|&i| {
                                            frus_widgets::fuzzy_match(&query, PEOPLE[i].1).is_some()
                                        })
                                        .collect();
                                    s.loading = false;
                                }
                            })
                        });
                    }
                })
            })
            .placeholder("Search people…")
            .filter(false)
            .loading(self.loading)
            .commands(
                self.found.iter().map(|&i| {
                    PaletteCommand::new(PEOPLE[i].1, run(cx, PEOPLE[i].1)).group("People")
                }),
            )
        } else {
            CommandPalette::new(
                body,
                &self.palette,
                cx.handler(|s, state: CommandPaletteState| s.palette = state),
            )
            .commands([
                PaletteCommand::new("New file", run(cx, "New file"))
                    .group("File")
                    .icon(Icons::ADD)
                    .shortcut("Ctrl+N"),
                PaletteCommand::new("Open file", run(cx, "Open file"))
                    .group("File")
                    .icon(Icons::FOLDER_OPEN)
                    .shortcut("Ctrl+O"),
                PaletteCommand::new("Save", run(cx, "Save"))
                    .group("File")
                    .icon(Icons::SAVE)
                    .shortcut("Ctrl+S"),
                PaletteCommand::new(
                    "Toggle sidebar",
                    cx.callback(|s| {
                        let shut = s.columns.sizes().first().is_some_and(|w| *w < 0.01);
                        s.columns =
                            ResizablePanelsState::new(if shut { [0.28, 0.72] } else { [0.0, 1.0] });
                        s.palette = CommandPaletteState::closed();
                    }),
                )
                .group("View")
                .shortcut("Ctrl+B"),
                PaletteCommand::new("Go to settings", run(cx, "Go to settings"))
                    .group("View")
                    .keywords(["preferences", "options"]),
                PaletteCommand::new("Print", run(cx, "Print"))
                    .group("File")
                    .enabled(false),
            ])
        };
        let palette = palette.open_on(KeyStroke::new(ShortcutKey::Char('k')).ctrl());

        let router = cx.router();
        Scaffold::new()
            .background(theme.background)
            .app_bar(
                AppBar::new()
                    .title(Text::new("Overlays and panels"))
                    .leading(IconButton::new(Icons::ARROW_BACK).on_press(on(move || {
                        router.pop();
                    })))
                    .build(),
            )
            .body(palette)
            .build()
    }
}
