// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **A second window** (milestone 827): the tasks, in a small window of their own beside the
//! main one, opened from the Stats section and from the window's menu bar.
//!
//! The window is a value of the shared [`Demo`]: opening it is setting that value, closing it
//! — its button here, or the window's own close button — is clearing it, and the application's
//! `windows` list follows. Both windows are built from the same tasks: one ticked here is
//! ticked there, and a task added here appears there. Each window has its own focus, its own
//! field and its own scroll position.

use crate::prelude::*;
use frus_widgets::{column, row, Semantics};

/// What the application calls the window.
pub(crate) const SUMMARY_WINDOW: &str = "summary";

/// The window, as the application lists it while it is open.
pub(crate) fn summary_window(demo: &Rc<Demo>) -> frus_shell::Window<Callback> {
    let close = demo.clone();
    frus_shell::Window::new(SUMMARY_WINDOW, on(move || close.show_summary(false)))
        .title("frus — Task summary")
        .size(380.0, 520.0)
}

/// What the window shows: how many tasks are left, a field to add one, and every task with
/// its checkbox.
pub(crate) fn summary(cx: &BuildContext, demo: &Rc<Demo>) -> Box<dyn Widget> {
    let theme = cx.theme().clone();
    let todos = demo.todos();
    let left = todos.iter().filter(|todo| !todo.done).count();
    let draft = cx.use_text_controller("");
    let add = {
        let (demo, draft) = (demo.clone(), draft.clone());
        on(move || {
            if demo.add(&draft.text()) {
                demo.save();
                draft.clear();
            }
        })
    };
    let field = TextField::new(draft.text())
        .placeholder("New task")
        .controller(&draft)
        .on_submit(add.clone());

    let mut list = Flex::column().gap(4.0);
    for todo in &todos {
        let (toggle, id) = (demo.clone(), todo.id);
        let label = if todo.done {
            text(todo.text.clone()).color(theme.muted).ellipsis()
        } else {
            text(todo.text.clone()).ellipsis()
        };
        list = list.child(keyed(
            todo.id,
            Semantics::merge(
                row![
                    Checkbox::new(todo.done).on_toggle(on_value(move |_: bool| {
                        toggle.toggle(id);
                        toggle.save();
                    })),
                    Expanded::new(label),
                ]
                .align(Align::Center)
                .gap(8.0),
            ),
        ));
    }

    let close = demo.clone();
    let content = column![
        text(match left {
            0 => "Nothing left to do".to_string(),
            1 => "1 task to do".to_string(),
            n => format!("{n} tasks to do"),
        })
        .size(22.0),
        text(format!("{} done of {}", todos.len() - left, todos.len()))
            .size(14.0)
            .color(theme.muted),
        row![Expanded::new(field), button("Add", add)]
            .align(Align::Center)
            .gap(8.0),
        Expanded::new(SingleChildScrollView::new().child(list)),
        button("Close this window", on(move || close.show_summary(false)))
            .variant(Variant::Outlined),
    ]
    .gap(12.0);
    // The whole window: the list takes what the rest leaves.
    let Size { width, height } = MediaQuery::of().size;
    Box::new(
        Container::new()
            .width(width)
            .height(height)
            .color(theme.background)
            .padding(16.0)
            .child(content),
    )
}
