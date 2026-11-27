// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Keys and clicks a row of a list answers itself** (milestone 855).
//!
//! A row of a virtualised list is built for the frame it shows in and let go after it, so
//! the shell cannot ask it anything once the frame is drawn — only what the frame kept. A
//! focus stop that wants the keys a composite widget uses (a tree's arrows, its Home and End,
//! its type-ahead) hands the frame a closure, [`Widget::on_row_key`](crate::Widget::on_row_key),
//! which the shell calls for each key while that stop has the focus. Its answer can send
//! messages **and move the focus to another row of the same list**, named by its index and
//! its key, built or not: the list scrolls the row into view and the focus is on it when it
//! is built.
//!
//! A click with Shift or the platform's own modifier held (Cmd on an Apple keyboard, Ctrl
//! elsewhere) is asked of [`Widget::on_modified_click`](crate::Widget::on_modified_click) in
//! the same way — the frame keeps the closure — so a row can extend or toggle a selection.

use std::rc::Rc;

use crate::shortcuts::KeyStroke;
use crate::TargetPlatform;

/// The modifiers held for a click (milestone 855).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClickModifiers {
    /// Shift.
    pub shift: bool,
    /// The **platform's own modifier**: Cmd on an Apple keyboard, Ctrl on any other — the
    /// key that adds an item to a selection.
    pub primary: bool,
    /// Alt / Option.
    pub alt: bool,
}

impl ClickModifiers {
    /// Whether nothing is held.
    pub const fn none(&self) -> bool {
        !(self.shift || self.primary || self.alt)
    }
}

/// A key pressed while a row has the focus, as [`Widget::on_row_key`](crate::Widget::on_row_key)
/// is handed it (milestone 855).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowKey {
    /// The key and the modifiers held, as pressed: `ctrl` and `meta` as they are, `primary`
    /// never set — ask [`RowKey::primary`] for the platform's own modifier.
    pub stroke: KeyStroke,
    /// Whose keyboard conventions the keys follow.
    pub keyboard: TargetPlatform,
    /// When it was pressed, in seconds on a clock that only goes forward — what a
    /// type-ahead measures the pause between two letters with.
    pub at: f32,
}

impl RowKey {
    /// Whether the **platform's own modifier** is held: Cmd on an Apple keyboard, Ctrl on
    /// any other.
    pub fn primary(&self) -> bool {
        if self.keyboard.is_apple() {
            self.stroke.meta
        } else {
            self.stroke.ctrl
        }
    }
}

/// A row of the same list, for the focus to go to (milestone 855): where it is and its key —
/// the [`Widget::key`](crate::Widget::key) of the widget the list builds at `index`, which
/// must be the focus stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowTarget {
    /// Its index in the list.
    pub index: usize,
    /// Its key.
    pub key: u64,
}

/// What a row answers to a key (milestone 855): the messages to send, and the row the focus
/// goes to.
pub struct RowKeyAnswer<Msg> {
    /// Sent in order.
    pub messages: Vec<Msg>,
    /// Another row of the list the focused row is in, for the focus to move to. The list
    /// scrolls it into view first.
    pub focus: Option<RowTarget>,
}

impl<Msg> RowKeyAnswer<Msg> {
    /// The key is taken and nothing else happens.
    pub fn taken() -> Self {
        Self {
            messages: Vec::new(),
            focus: None,
        }
    }

    /// Also sends `message`.
    pub fn send(mut self, message: Option<Msg>) -> Self {
        self.messages.extend(message);
        self
    }

    /// The focus goes to `target`.
    pub fn focus(mut self, target: RowTarget) -> Self {
        self.focus = Some(target);
        self
    }
}

impl<Msg> std::fmt::Debug for RowKeyAnswer<Msg> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RowKeyAnswer")
            .field("messages", &self.messages.len())
            .field("focus", &self.focus)
            .finish()
    }
}

/// The closure a row hands the frame for its keys; `None` lets the key go on as if there were
/// none.
pub type RowKeyHandler<Msg> = Rc<dyn Fn(RowKey) -> Option<RowKeyAnswer<Msg>>>;

/// The closure a target hands the frame for a click, told the modifiers held.
pub type ModifiedClick<Msg> = Rc<dyn Fn(ClickModifiers) -> Option<Msg>>;
