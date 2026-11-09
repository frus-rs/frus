// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The **Android accessibility tree** (milestone 782): what TalkBack is shown of a frame, and
//! what its requests become.
//!
//! The frame's semantic nodes ([`frus_widgets::Ui::semantics`]) and its scrollable areas
//! become flat rows of plain data — one per virtual view, with its parent, its box in the
//! window's pixels, its class name, its words and its flags — which the Java half
//! (`FrusAccessibility`, in the bundled dex) turns into `AccessibilityNodeInfo`s when the
//! screen reader asks. Everything decided here is decided away from a device, and tested:
//! the Java side applies the rows unchanged.
//!
//! The events a screen reader needs to follow a changing screen (focus moved, a box
//! ticked, a value moved, text typed, an area scrolled, the content changed) are found by
//! comparing a frame's rows with the last ones published.

use std::collections::{HashMap, HashSet};

use frus_widgets::{Point, Rect, Role, SemanticsProperties, Toggled, WidgetId};

/// The flags of a row: the node's states, as the Java half sets them on the node. Ours, not
/// Android's — the Java side reads each bit and calls the matching setter.
pub(crate) mod flag {
    /// The node can be checked.
    pub const CHECKABLE: i32 = 1;
    /// The node is checked.
    pub const CHECKED: i32 = 1 << 1;
    /// The node can be used.
    pub const ENABLED: i32 = 1 << 2;
    /// The node can take input focus.
    pub const FOCUSABLE: i32 = 1 << 3;
    /// The node has the input focus.
    pub const FOCUSED: i32 = 1 << 4;
    /// The node answers a press.
    pub const CLICKABLE: i32 = 1 << 5;
    /// The node's text can be edited.
    pub const EDITABLE: i32 = 1 << 6;
    /// The node scrolls.
    pub const SCROLLABLE: i32 = 1 << 7;
    /// The node is a heading.
    pub const HEADING: i32 = 1 << 8;
    /// The node's text is a password.
    pub const PASSWORD: i32 = 1 << 9;
    /// The node is the selected one of its group (a tab).
    pub const SELECTED: i32 = 1 << 10;
    /// Some of the node is on the screen.
    pub const VISIBLE: i32 = 1 << 11;
    /// The node carries a range (its `range` row is meaningful).
    pub const RANGE: i32 = 1 << 12;
    /// The node's value can be set directly (`ACTION_SET_PROGRESS`).
    pub const SET_PROGRESS: i32 = 1 << 13;
}

/// The standard actions of `AccessibilityNodeInfo`, Android's own bit values: the Java half
/// hands the mask to `addAction` as it is.
pub(crate) mod action {
    /// `ACTION_FOCUS`.
    pub const FOCUS: i32 = 0x0000_0001;
    /// `ACTION_CLEAR_FOCUS`.
    pub const CLEAR_FOCUS: i32 = 0x0000_0002;
    /// `ACTION_CLICK`.
    pub const CLICK: i32 = 0x0000_0010;
    /// `ACTION_SCROLL_FORWARD`.
    pub const SCROLL_FORWARD: i32 = 0x0000_1000;
    /// `ACTION_SCROLL_BACKWARD`.
    pub const SCROLL_BACKWARD: i32 = 0x0000_2000;
    /// `ACTION_EXPAND`.
    pub const EXPAND: i32 = 0x0004_0000;
    /// `ACTION_COLLAPSE`.
    pub const COLLAPSE: i32 = 0x0008_0000;
    /// `ACTION_SET_TEXT`.
    pub const SET_TEXT: i32 = 0x0020_0000;
    /// `AccessibilityAction.ACTION_SET_PROGRESS`'s id, `android.R.id.accessibilityActionSetProgress`:
    /// not a bit of the mask, an action of its own.
    pub const SET_PROGRESS: i32 = 0x0102_003d;
}

/// Android's accessibility event types this side raises.
pub(crate) mod event {
    /// `TYPE_VIEW_CLICKED`.
    pub const CLICKED: i32 = 0x0000_0001;
    /// `TYPE_VIEW_SELECTED`.
    pub const SELECTED: i32 = 0x0000_0004;
    /// `TYPE_VIEW_FOCUSED`.
    pub const FOCUSED: i32 = 0x0000_0008;
    /// `TYPE_VIEW_TEXT_CHANGED`.
    pub const TEXT_CHANGED: i32 = 0x0000_0010;
    /// `TYPE_WINDOW_CONTENT_CHANGED`.
    pub const CONTENT_CHANGED: i32 = 0x0000_0800;
    /// `TYPE_VIEW_SCROLLED`.
    pub const SCROLLED: i32 = 0x0000_1000;
}

/// The parent id of a row under the host view itself (`View.NO_ID`, which is what
/// `AccessibilityNodeProvider.HOST_VIEW_ID` is too).
pub(crate) const HOST: i32 = -1;

/// One virtual view, as plain data.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AndroidNode {
    /// The virtual view id: positive, stable for as long as the widget stays on screen.
    pub id: i32,
    /// The parent's virtual id, or [`HOST`].
    pub parent: i32,
    /// `[left, top, right, bottom]`, in the window's physical pixels.
    pub bounds: [i32; 4],
    /// The Android class the node reads as (`android.widget.Button`, …): it is what decides
    /// the role a screen reader announces.
    pub class_name: &'static str,
    /// The node's text: a label's words, a field's contents.
    pub text: Option<String>,
    /// The node's content description: what names a control.
    pub description: Option<String>,
    /// The hint: the name of an editable field, read with its contents.
    pub hint: Option<String>,
    /// The [`flag`] bits.
    pub flags: i32,
    /// The [`action`] mask.
    pub actions: i32,
    /// `(min, max, current)` of a slider or a progress bar.
    pub range: Option<(f32, f32, f32)>,
    /// `(x, y, max_x, max_y)` of a scrollable area, in physical pixels.
    pub scroll: Option<[i32; 4]>,
}

impl AndroidNode {
    fn has(&self, bit: i32) -> bool {
        self.flags & bit != 0
    }
}

/// A frame's rows, in reading order: a parent always comes before its children, and the
/// children of a parent are read in the order they appear.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AndroidTree {
    /// The rows.
    pub nodes: Vec<AndroidNode>,
}

impl AndroidTree {
    /// The row of virtual view `id`.
    pub(crate) fn node(&self, id: i32) -> Option<&AndroidNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    /// The node holding the input focus.
    fn focused(&self) -> Option<i32> {
        self.nodes
            .iter()
            .find(|node| node.has(flag::FOCUSED))
            .map(|node| node.id)
    }
}

/// A scrollable area of the frame, as the tree needs it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollArea {
    /// The area's identity.
    pub id: WidgetId,
    /// Its viewport, in the layout's pixels.
    pub viewport: Rect,
    /// How far it scrolls across and down.
    pub max: (f32, f32),
    /// Where it rests now.
    pub offset: (f32, f32),
}

impl ScrollArea {
    /// Something to scroll: an area whose content fits never reads as scrollable.
    fn scrolls(&self) -> bool {
        self.max.0 > 0.5 || self.max.1 > 0.5
    }

    /// Scrolls across rather than down.
    fn horizontal(&self) -> bool {
        self.max.1 <= 0.5 && self.max.0 > 0.5
    }

    fn area(&self) -> f32 {
        self.viewport.width * self.viewport.height
    }
}

/// The virtual view ids: a stable, positive number per widget, for as long as the widget is
/// on screen. A node keeps its id from frame to frame, which is what lets the screen
/// reader's focus stay on it while the screen changes around it.
#[derive(Debug, Default)]
pub(crate) struct VirtualIds {
    by_widget: HashMap<WidgetId, i32>,
    by_virtual: HashMap<i32, WidgetId>,
    last: i32,
}

impl VirtualIds {
    /// The id of `widget`, given one if it has none.
    pub(crate) fn id(&mut self, widget: WidgetId) -> i32 {
        if let Some(&id) = self.by_widget.get(&widget) {
            return id;
        }
        // Positive, and never one still in use: past `i32::MAX` the count starts again.
        loop {
            self.last = if self.last == i32::MAX {
                1
            } else {
                self.last + 1
            };
            if !self.by_virtual.contains_key(&self.last) {
                break;
            }
        }
        self.by_widget.insert(widget, self.last);
        self.by_virtual.insert(self.last, widget);
        self.last
    }

    /// The widget behind virtual view `id`.
    pub(crate) fn widget(&self, id: i32) -> Option<WidgetId> {
        self.by_virtual.get(&id).copied()
    }

    /// Forgets the widgets no longer on screen.
    fn retain(&mut self, live: &HashSet<WidgetId>) {
        self.by_widget.retain(|widget, _| live.contains(widget));
        self.by_virtual.retain(|_, widget| live.contains(widget));
    }
}

/// The class a role reads as. TalkBack names a node's role from its class: a `Button`
/// is "button", a `CheckBox` "checkbox", a `Switch` "switch", a `SeekBar` "slider", an
/// `EditText` "edit box", an `ActionBar$Tab` "tab".
fn class_name(role: Role) -> &'static str {
    match role {
        Role::None | Role::ListItem => "android.view.View",
        Role::Label | Role::Heading => "android.widget.TextView",
        Role::Button | Role::Link => "android.widget.Button",
        Role::CheckBox => "android.widget.CheckBox",
        Role::Switch => "android.widget.Switch",
        Role::RadioButton => "android.widget.RadioButton",
        Role::Slider => "android.widget.SeekBar",
        Role::TextInput => "android.widget.EditText",
        Role::Image => "android.widget.ImageView",
        Role::Tab => "android.app.ActionBar$Tab",
        Role::ProgressBar => "android.widget.ProgressBar",
    }
}

/// `rect`, in the layout's pixels, as `[left, top, right, bottom]` in the window's.
fn to_pixels(rect: Rect, scale: f32) -> [i32; 4] {
    [
        (rect.x * scale).round() as i32,
        (rect.y * scale).round() as i32,
        ((rect.x + rect.width) * scale).round() as i32,
        ((rect.y + rect.height) * scale).round() as i32,
    ]
}

fn joined(first: Option<&String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(a), Some(b)) if !b.is_empty() => Some(format!("{a}, {b}")),
        (Some(a), _) => Some(a.clone()),
        (None, b) => b,
    }
}

/// The row of one semantic node: its role's class, its words where the screen reader
/// looks for them, its states and what may be done to it.
fn to_row(
    id: i32,
    parent: i32,
    rect: Rect,
    sem: &SemanticsProperties,
    focused: bool,
    scale: f32,
) -> AndroidNode {
    let enabled = !sem.disabled;
    let mut flags = 0;
    let mut actions = 0;
    if enabled {
        flags |= flag::ENABLED;
    }
    if rect.width > 0.0 && rect.height > 0.0 {
        flags |= flag::VISIBLE;
    }
    let (mut text, mut description, mut hint) = (None, None, None);
    match sem.role {
        // Static text speaks its words as its text: that is a `TextView`'s.
        Role::Label => text = sem.value.clone().or_else(|| sem.label.clone()),
        Role::Heading => {
            text = sem.label.clone().or_else(|| sem.value.clone());
            flags |= flag::HEADING;
        }
        // A field reads its contents, then what it is for: its name is its hint, as an
        // Android field's label is.
        Role::TextInput => {
            text = sem.readable_value().filter(|value| !value.is_empty());
            hint = sem.label.clone();
            flags |= flag::EDITABLE | flag::FOCUSABLE;
            if sem.obscured {
                flags |= flag::PASSWORD;
            }
            if enabled {
                actions |= action::CLICK | action::SET_TEXT;
            }
        }
        // A range speaks its own value, as a percentage or as the value it was given: the
        // description names it.
        Role::Slider | Role::ProgressBar => description = sem.label.clone(),
        _ => description = joined(sem.label.as_ref(), sem.readable_value()),
    }
    if let Some((min, value, max)) = sem.range {
        flags |= flag::RANGE;
        if sem.role == Role::Slider && enabled {
            flags |= flag::FOCUSABLE | flag::SET_PROGRESS;
            actions |= action::SCROLL_FORWARD | action::SCROLL_BACKWARD;
        }
        // A range whose value is words rather than a number says them too.
        if let Some(words) = sem.value.as_ref().filter(|_| sem.role == Role::Slider) {
            if words.parse::<f32>().is_err() {
                description = joined(description.as_ref(), Some(words.clone()));
            }
        }
        let range = (min, max, value.clamp(min.min(max), max.max(min)));
        return AndroidNode {
            id,
            parent,
            bounds: to_pixels(rect, scale),
            class_name: class_name(sem.role),
            text,
            description,
            hint,
            flags: finish(flags, focused),
            actions: finish_actions(actions, flags, focused),
            range: Some(range),
            scroll: None,
        };
    }
    match (sem.role, sem.toggled) {
        (_, Toggled::None) => {}
        // A tab is the selected one, not a ticked one.
        (Role::Tab, toggled) => {
            if toggled == Toggled::True {
                flags |= flag::SELECTED;
            }
        }
        // Partly on reads as on: Android has no third state before API 36.
        (_, Toggled::True | Toggled::Mixed) => flags |= flag::CHECKABLE | flag::CHECKED,
        (_, Toggled::False) => flags |= flag::CHECKABLE,
    }
    if sem.clickable && enabled {
        flags |= flag::CLICKABLE | flag::FOCUSABLE;
        actions |= action::CLICK;
        // A control that opens something offers to open or to close it, which is how
        // TalkBack says "expanded" or "collapsed" (milestone 786). Either is a press.
        match sem.expanded {
            Some(true) => actions |= action::COLLAPSE,
            Some(false) => actions |= action::EXPAND,
            None => {}
        }
    }
    AndroidNode {
        id,
        parent,
        bounds: to_pixels(rect, scale),
        class_name: class_name(sem.role),
        text,
        description,
        hint,
        flags: finish(flags, focused),
        actions: finish_actions(actions, flags, focused),
        range: None,
        scroll: None,
    }
}

fn finish(flags: i32, focused: bool) -> i32 {
    if focused {
        flags | flag::FOCUSED
    } else {
        flags
    }
}

/// A node that can take the focus offers to; one that has it, to give it up.
fn finish_actions(actions: i32, flags: i32, focused: bool) -> i32 {
    if flags & flag::ENABLED == 0 {
        return 0;
    }
    if focused {
        actions | action::CLEAR_FOCUS
    } else if flags & flag::FOCUSABLE != 0 {
        actions | action::FOCUS
    } else {
        actions
    }
}

/// The row of a scrollable area: a scroll view, which TalkBack scrolls a page at a time
/// when its reading reaches the end of what shows, and which it tells the person about.
fn area_row(id: i32, parent: i32, area: &ScrollArea, scale: f32) -> AndroidNode {
    let horizontal = area.horizontal();
    let (offset, max) = if horizontal {
        (area.offset.0, area.max.0)
    } else {
        (area.offset.1, area.max.1)
    };
    let mut actions = 0;
    if offset < max - 0.5 {
        actions |= action::SCROLL_FORWARD;
    }
    if offset > 0.5 {
        actions |= action::SCROLL_BACKWARD;
    }
    let px = |v: f32| (v * scale).round() as i32;
    AndroidNode {
        id,
        parent,
        bounds: to_pixels(area.viewport, scale),
        class_name: if horizontal {
            "android.widget.HorizontalScrollView"
        } else {
            "android.widget.ScrollView"
        },
        text: None,
        description: None,
        hint: None,
        flags: flag::ENABLED | flag::SCROLLABLE | flag::VISIBLE,
        actions,
        range: None,
        scroll: Some([
            px(area.offset.0),
            px(area.offset.1),
            px(area.max.0),
            px(area.max.1),
        ]),
    }
}

/// Builds the frame's rows.
///
/// `nodes` are the frame's semantic nodes in reading order; `areas` its scrollable areas;
/// `focus` the widget with the input focus; `scale` the layout's pixels to the window's.
///
/// A node sits under the innermost area that scrolls and whose viewport holds its centre,
/// and an area under the innermost larger one holding its own, so that TalkBack finds the
/// area to scroll when its reading reaches the last row that shows. An area comes in
/// where its first node is read. An area that is also a node (a multi-line field scrolls
/// its own text) is left out: the field answers for itself.
pub(crate) fn build(
    nodes: &[(WidgetId, Rect, SemanticsProperties)],
    areas: &[ScrollArea],
    focus: Option<WidgetId>,
    scale: f32,
    ids: &mut VirtualIds,
) -> AndroidTree {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let node_ids: HashSet<WidgetId> = nodes.iter().map(|(id, ..)| *id).collect();
    let areas: Vec<&ScrollArea> = areas
        .iter()
        .filter(|area| area.scrolls() && !node_ids.contains(&area.id))
        .collect();
    // The innermost area holding `point`, other than `except`, and larger than `within`.
    let innermost = |point: Point, except: Option<WidgetId>, within: f32| {
        areas
            .iter()
            .filter(|area| {
                Some(area.id) != except && area.area() > within && area.viewport.contains(point)
            })
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .copied()
    };
    let center = |rect: Rect| Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);

    let mut tree = AndroidTree::default();
    let mut emitted: HashSet<WidgetId> = HashSet::new();
    let mut live: HashSet<WidgetId> = HashSet::new();
    for (widget, rect, sem) in nodes {
        // The chain of areas around the node, innermost first.
        let mut chain = Vec::new();
        let mut at = innermost(center(*rect), None, 0.0);
        while let Some(area) = at {
            if chain.iter().any(|a: &&ScrollArea| a.id == area.id) || chain.len() > 16 {
                break;
            }
            chain.push(area);
            at = innermost(center(area.viewport), Some(area.id), area.area());
        }
        // Outermost first: each area under the one before it.
        let mut parent = HOST;
        for area in chain.iter().rev() {
            let id = ids.id(area.id);
            if emitted.insert(area.id) {
                live.insert(area.id);
                tree.nodes.push(area_row(id, parent, area, scale));
            }
            parent = id;
        }
        if !emitted.insert(*widget) {
            continue;
        }
        live.insert(*widget);
        let id = ids.id(*widget);
        tree.nodes.push(to_row(
            id,
            parent,
            *rect,
            sem,
            focus == Some(*widget),
            scale,
        ));
    }
    ids.retain(&live);
    tree
}

/// An event for the screen reader: `kind` is an Android event type, `id` the virtual view
/// it comes from ([`HOST`] for the host), and the rest what that type carries.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AndroidEvent {
    /// The [`event`] type.
    pub kind: i32,
    /// The virtual view it comes from.
    pub id: i32,
    /// Four numbers, by type: a text change's start, removed and added counts; a scroll's
    /// `x, y, max_x, max_y`.
    pub numbers: [i32; 4],
    /// A text change's text before it.
    pub before: Option<String>,
}

impl AndroidEvent {
    fn simple(kind: i32, id: i32) -> Self {
        Self {
            kind,
            id,
            numbers: [0; 4],
            before: None,
        }
    }
}

/// The edit that turns `before` into `after`, in UTF-16 units as Android counts them:
/// `(start, removed, added)` — the common start and end kept, the middle replaced.
fn text_change(before: &str, after: &str) -> Option<(i32, i32, i32)> {
    if before == after {
        return None;
    }
    let a: Vec<u16> = before.encode_utf16().collect();
    let b: Vec<u16> = after.encode_utf16().collect();
    let start = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[start..]
        .iter()
        .rev()
        .zip(b[start..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    Some((
        start as i32,
        (a.len() - start - tail) as i32,
        (b.len() - start - tail) as i32,
    ))
}

/// What changed between the rows last published and these, as events.
///
/// - anything at all: the window's content changed, which makes TalkBack read the tree again;
/// - the input focus moved to a node: it is focused;
/// - a box was ticked or unticked: it is clicked, which is when TalkBack says its new state;
/// - a range moved: it is selected, which is when TalkBack says its new value;
/// - a field's text changed: what was removed and added, which TalkBack reads out;
/// - an area scrolled: where to, which TalkBack turns into "showing items…" or a sound.
pub(crate) fn diff(old: &AndroidTree, new: &AndroidTree) -> Vec<AndroidEvent> {
    let mut events = Vec::new();
    if old == new {
        return events;
    }
    events.push(AndroidEvent::simple(event::CONTENT_CHANGED, HOST));
    let focus = new.focused();
    if focus.is_some() && focus != old.focused() {
        events.push(AndroidEvent::simple(event::FOCUSED, focus.unwrap_or(HOST)));
    }
    for node in &new.nodes {
        let Some(was) = old.node(node.id) else {
            continue;
        };
        if was.has(flag::CHECKABLE)
            && node.has(flag::CHECKABLE)
            && was.has(flag::CHECKED) != node.has(flag::CHECKED)
        {
            events.push(AndroidEvent::simple(event::CLICKED, node.id));
        }
        if let (Some(a), Some(b)) = (was.range, node.range) {
            if a.2 != b.2 {
                events.push(AndroidEvent::simple(event::SELECTED, node.id));
            }
        }
        if node.has(flag::EDITABLE) {
            let before = was.text.clone().unwrap_or_default();
            let after = node.text.as_deref().unwrap_or_default();
            if let Some((start, removed, added)) = text_change(&before, after) {
                events.push(AndroidEvent {
                    kind: event::TEXT_CHANGED,
                    id: node.id,
                    numbers: [start, removed, added, 0],
                    before: Some(before),
                });
            }
        }
        if let (Some(a), Some(b)) = (was.scroll, node.scroll) {
            if a[..2] != b[..2] {
                events.push(AndroidEvent {
                    kind: event::SCROLLED,
                    id: node.id,
                    numbers: b,
                    before: None,
                });
            }
        }
    }
    events
}

/// What the screen reader asked of a node, for the shell to carry out.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum A11yAction {
    /// Press it.
    Click(WidgetId),
    /// Give it the input focus.
    Focus(WidgetId),
    /// Take the input focus away from it.
    ClearFocus(WidgetId),
    /// Put this text in the field.
    SetValue(WidgetId, String),
    /// Scroll the area a page, forward or back.
    Scroll(WidgetId, bool),
    /// Move the slider a step, up or down.
    Adjust(WidgetId, bool),
    /// Move the slider to this fraction of its range.
    SetFraction(WidgetId, f32),
}

/// A request the Java half passed on: the virtual view, the Android action, and its
/// argument — a text for set text, a value for set progress.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Request {
    /// The virtual view.
    pub id: i32,
    /// The Android action.
    pub action: i32,
    /// Set text's text.
    pub text: Option<String>,
    /// Set progress's value.
    pub value: f32,
}

/// The shell's action for `request`, against the rows it was asked about. A node gone since,
/// or an action it does not offer, is nothing.
pub(crate) fn to_action(
    request: &Request,
    tree: &AndroidTree,
    ids: &VirtualIds,
) -> Option<A11yAction> {
    let node = tree.node(request.id)?;
    let widget = ids.widget(request.id)?;
    if request.action == action::SET_PROGRESS {
        if node.flags & flag::SET_PROGRESS == 0 {
            return None;
        }
        let (min, max, _) = node.range?;
        let span = max - min;
        let fraction = if span.abs() > f32::EPSILON {
            ((request.value - min) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        return Some(A11yAction::SetFraction(widget, fraction));
    }
    if node.actions & request.action == 0 {
        return None;
    }
    let forward = request.action == action::SCROLL_FORWARD;
    Some(match request.action {
        action::CLICK | action::EXPAND | action::COLLAPSE => A11yAction::Click(widget),
        action::FOCUS => A11yAction::Focus(widget),
        action::CLEAR_FOCUS => A11yAction::ClearFocus(widget),
        action::SET_TEXT => A11yAction::SetValue(widget, request.text.clone().unwrap_or_default()),
        action::SCROLL_FORWARD | action::SCROLL_BACKWARD if node.scroll.is_some() => {
            A11yAction::Scroll(widget, forward)
        }
        action::SCROLL_FORWARD | action::SCROLL_BACKWARD => A11yAction::Adjust(widget, forward),
        _ => return None,
    })
}

/// Where a page's scroll takes an area resting at `offset`: one viewport further, forward or
/// back, along the axis it scrolls, never past its ends.
pub(crate) fn page_target(
    viewport: Rect,
    max: (f32, f32),
    offset: (f32, f32),
    forward: bool,
) -> (f32, f32) {
    let sign = if forward { 1.0 } else { -1.0 };
    let horizontal = max.1 <= 0.5 && max.0 > 0.5;
    if horizontal {
        (
            (offset.0 + sign * viewport.width).clamp(0.0, max.0.max(0.0)),
            offset.1,
        )
    } else {
        (
            offset.0,
            (offset.1 + sign * viewport.height).clamp(0.0, max.1.max(0.0)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wid(raw: u64) -> WidgetId {
        WidgetId::from_u64(raw)
    }

    fn sem(role: Role) -> SemanticsProperties {
        SemanticsProperties::new(role)
    }

    fn one(sem: SemanticsProperties) -> AndroidNode {
        let mut ids = VirtualIds::default();
        let tree = build(
            &[(wid(1), Rect::new(10.0, 20.0, 100.0, 40.0), sem)],
            &[],
            None,
            1.0,
            &mut ids,
        );
        tree.nodes[0].clone()
    }

    /// **Each role reads as the class TalkBack names it by**: a button, a checkbox, a
    /// switch, a slider, an edit box, an image, a tab, a progress bar, text.
    #[test]
    fn roles_read_as_their_android_class() {
        for (role, class) in [
            (Role::Button, "android.widget.Button"),
            (Role::CheckBox, "android.widget.CheckBox"),
            (Role::Switch, "android.widget.Switch"),
            (Role::RadioButton, "android.widget.RadioButton"),
            (Role::Slider, "android.widget.SeekBar"),
            (Role::TextInput, "android.widget.EditText"),
            (Role::Image, "android.widget.ImageView"),
            (Role::Tab, "android.app.ActionBar$Tab"),
            (Role::ProgressBar, "android.widget.ProgressBar"),
            (Role::Label, "android.widget.TextView"),
            (Role::Heading, "android.widget.TextView"),
            (Role::ListItem, "android.view.View"),
        ] {
            assert_eq!(one(sem(role)).class_name, class, "{role:?}");
        }
    }

    /// **Words go where the reader looks for them**: text speaks its words as its text, a
    /// heading too and is marked one, a control is named by its description, an image by
    /// its description, and a field reads its contents with its name as the hint.
    #[test]
    fn words_go_where_the_reader_looks() {
        let text = one(sem(Role::Label).label("Water the plants"));
        assert_eq!(text.text.as_deref(), Some("Water the plants"));
        assert_eq!(text.description, None);
        assert_eq!(text.flags & flag::HEADING, 0);

        let heading = one(sem(Role::Heading).label("Today"));
        assert_eq!(heading.text.as_deref(), Some("Today"));
        assert_ne!(heading.flags & flag::HEADING, 0);

        let button = one(sem(Role::Button).label("Save").clickable());
        assert_eq!(button.description.as_deref(), Some("Save"));
        assert_eq!(button.text, None);

        let badge = one(sem(Role::Button).label("Inbox").value("3 new").clickable());
        assert_eq!(badge.description.as_deref(), Some("Inbox, 3 new"));

        let image = one(sem(Role::Image).label("A red fox"));
        assert_eq!(image.description.as_deref(), Some("A red fox"));

        let field = one(sem(Role::TextInput).label("Name").value("Ada"));
        assert_eq!(field.text.as_deref(), Some("Ada"));
        assert_eq!(field.hint.as_deref(), Some("Name"));
        assert_eq!(field.description, None);
        // An empty field has no text: its hint is what is read.
        let empty = one(sem(Role::TextInput).label("Name").value(""));
        assert_eq!(empty.text, None);
    }

    /// **A control that opens something offers the other state** (milestone 786): a
    /// collapsed one can be expanded, an expanded one collapsed, which is what TalkBack
    /// reads as "collapsed" or "expanded" — and either action is the press it stands for.
    #[test]
    fn an_opening_control_offers_to_expand_or_collapse() {
        let shut = one(sem(Role::Button).label("More").clickable().expanded(false));
        assert_ne!(shut.actions & action::EXPAND, 0);
        assert_eq!(shut.actions & action::COLLAPSE, 0);
        let open = one(sem(Role::Button).label("More").clickable().expanded(true));
        assert_ne!(open.actions & action::COLLAPSE, 0);
        assert_eq!(open.actions & action::EXPAND, 0);
        let plain = one(sem(Role::Button).label("Save").clickable());
        assert_eq!(plain.actions & (action::EXPAND | action::COLLAPSE), 0);

        let mut ids = VirtualIds::default();
        let nodes = [(
            wid(1),
            Rect::new(0.0, 0.0, 50.0, 50.0),
            sem(Role::Button).label("More").clickable().expanded(false),
        )];
        let tree = build(&nodes, &[], None, 1.0, &mut ids);
        let request = Request {
            id: ids.by_widget[&wid(1)],
            action: action::EXPAND,
            text: None,
            value: 0.0,
        };
        assert_eq!(
            to_action(&request, &tree, &ids),
            Some(A11yAction::Click(wid(1)))
        );
    }

    /// **States and actions**: a box is checkable and checked, a tab selected, a button
    /// clickable, a field editable with set text, a disabled control offers nothing.
    #[test]
    fn states_and_actions_follow_the_annotation() {
        let ticked = one(sem(Role::CheckBox).label("Wifi").toggled(true).clickable());
        assert_ne!(ticked.flags & flag::CHECKABLE, 0);
        assert_ne!(ticked.flags & flag::CHECKED, 0);
        assert_ne!(ticked.actions & action::CLICK, 0);
        assert_ne!(ticked.actions & action::FOCUS, 0);

        let off = one(sem(Role::Switch).toggled(false).clickable());
        assert_ne!(off.flags & flag::CHECKABLE, 0);
        assert_eq!(off.flags & flag::CHECKED, 0);

        let mixed = one(sem(Role::CheckBox).maybe_toggled(None));
        assert_ne!(mixed.flags & flag::CHECKED, 0, "partly on reads as on");

        let tab = one(sem(Role::Tab).label("Home").toggled(true).clickable());
        assert_ne!(tab.flags & flag::SELECTED, 0);
        assert_eq!(
            tab.flags & flag::CHECKABLE,
            0,
            "a tab is selected, not ticked"
        );

        let field = one(sem(Role::TextInput).label("Name"));
        assert_ne!(field.flags & flag::EDITABLE, 0);
        assert_ne!(field.actions & action::SET_TEXT, 0);
        assert_ne!(field.actions & action::CLICK, 0);

        let disabled = one(sem(Role::Button).label("Send").clickable().disabled(true));
        assert_eq!(disabled.flags & flag::ENABLED, 0);
        assert_eq!(disabled.flags & flag::CLICKABLE, 0);
        assert_eq!(disabled.actions, 0);
        let frozen = one(sem(Role::TextInput).disabled(true));
        assert_eq!(frozen.actions & action::SET_TEXT, 0);
    }

    /// **A password is never handed over**: the field is a password and its text dots.
    #[test]
    fn a_password_field_reads_dots() {
        let field = one(sem(Role::TextInput)
            .label("Password")
            .value("s3cret")
            .obscured(true));
        assert_ne!(field.flags & flag::PASSWORD, 0);
        assert_eq!(field.text.as_deref(), Some("\u{2022}".repeat(6).as_str()));
    }

    /// **A slider is a range that can be moved**: its range, a step either way, and its value
    /// set outright; a progress bar is only the range.
    #[test]
    fn a_slider_can_be_adjusted() {
        let slider = one(sem(Role::Slider).label("Volume").range(0.0, 30.0, 100.0));
        assert_eq!(slider.range, Some((0.0, 100.0, 30.0)));
        assert_ne!(slider.flags & flag::RANGE, 0);
        assert_ne!(slider.flags & flag::SET_PROGRESS, 0);
        assert_ne!(slider.actions & action::SCROLL_FORWARD, 0);
        assert_ne!(slider.actions & action::SCROLL_BACKWARD, 0);
        assert_eq!(slider.description.as_deref(), Some("Volume"));
        // A value in words is said as well; a number is left to the range.
        let words = one(sem(Role::Slider)
            .label("Size")
            .value("Large")
            .range(0.0, 2.0, 2.0));
        assert_eq!(words.description.as_deref(), Some("Size, Large"));

        let bar = one(sem(Role::ProgressBar).label("Upload").range(0.0, 0.5, 1.0));
        assert_eq!(bar.range, Some((0.0, 1.0, 0.5)));
        assert_eq!(
            bar.actions & (action::SCROLL_FORWARD | action::SCROLL_BACKWARD),
            0
        );
        assert_eq!(bar.flags & flag::SET_PROGRESS, 0);
    }

    /// **Bounds are the window's pixels**: the layout's box times the scale, rounded.
    #[test]
    fn bounds_are_physical_pixels() {
        let mut ids = VirtualIds::default();
        let tree = build(
            &[(
                wid(1),
                Rect::new(10.0, 20.5, 100.0, 40.0),
                sem(Role::Button),
            )],
            &[],
            None,
            2.75,
            &mut ids,
        );
        assert_eq!(tree.nodes[0].bounds, [28, 56, 303, 166]);
        assert_ne!(tree.nodes[0].flags & flag::VISIBLE, 0);
        // A node clipped out of sight is in the tree, not visible.
        let hidden = build(
            &[(wid(2), Rect::new(10.0, 20.0, 0.0, 0.0), sem(Role::Button))],
            &[],
            None,
            1.0,
            &mut ids,
        );
        assert_eq!(hidden.nodes[0].flags & flag::VISIBLE, 0);
    }

    /// **The focused node is marked, and can give the focus up**.
    #[test]
    fn the_focused_node_is_marked() {
        let mut ids = VirtualIds::default();
        let nodes = [
            (
                wid(1),
                Rect::new(0.0, 0.0, 10.0, 10.0),
                sem(Role::TextInput),
            ),
            (
                wid(2),
                Rect::new(0.0, 20.0, 10.0, 10.0),
                sem(Role::TextInput),
            ),
        ];
        let tree = build(&nodes, &[], Some(wid(2)), 1.0, &mut ids);
        assert_eq!(tree.nodes[0].flags & flag::FOCUSED, 0);
        assert_ne!(tree.nodes[1].flags & flag::FOCUSED, 0);
        assert_ne!(tree.nodes[1].actions & action::CLEAR_FOCUS, 0);
        assert_eq!(tree.nodes[1].actions & action::FOCUS, 0);
    }

    /// **Ids are stable and reversible**: a widget keeps its id across frames, ids are
    /// positive, and one gone from the screen is forgotten.
    #[test]
    fn ids_are_stable_while_on_screen() {
        let mut ids = VirtualIds::default();
        let frame = |ids: &mut VirtualIds, which: &[u64]| {
            let nodes: Vec<_> = which
                .iter()
                .map(|&raw| (wid(raw), Rect::new(0.0, 0.0, 5.0, 5.0), sem(Role::Button)))
                .collect();
            build(&nodes, &[], None, 1.0, ids)
        };
        let first = frame(&mut ids, &[7, 8]);
        let second = frame(&mut ids, &[8, 9]);
        assert_eq!(first.nodes[1].id, second.nodes[0].id, "8 keeps its id");
        assert!(second.nodes.iter().all(|node| node.id > 0));
        assert_eq!(ids.widget(second.nodes[1].id), Some(wid(9)));
        assert_eq!(ids.widget(first.nodes[0].id), None, "7 is forgotten");
    }

    /// **The count starts again past the largest id, skipping ids still in use.**
    #[test]
    fn ids_wrap_around_without_reuse() {
        let mut ids = VirtualIds {
            last: i32::MAX - 1,
            ..VirtualIds::default()
        };
        let a = ids.id(wid(1));
        assert_eq!(a, i32::MAX);
        ids.by_virtual.insert(1, wid(99));
        let b = ids.id(wid(2));
        assert_eq!(b, 2, "1 is taken");
    }

    fn area(id: u64, viewport: Rect, max: (f32, f32), offset: (f32, f32)) -> ScrollArea {
        ScrollArea {
            id: wid(id),
            viewport,
            max,
            offset,
        }
    }

    /// **Rows sit inside the area that scrolls them**: the list is a scroll view with the
    /// rows inside it, read where its first row is, the title outside it before, and a
    /// sideways strip inside the list nests in it.
    #[test]
    fn nodes_nest_in_their_scroll_area() {
        let mut ids = VirtualIds::default();
        let nodes = [
            (
                wid(1),
                Rect::new(0.0, 0.0, 300.0, 40.0),
                sem(Role::Heading).label("Inbox"),
            ),
            (
                wid(2),
                Rect::new(0.0, 50.0, 300.0, 40.0),
                sem(Role::Button).clickable(),
            ),
            (
                wid(3),
                Rect::new(0.0, 100.0, 50.0, 40.0),
                sem(Role::Button).clickable(),
            ),
            (
                wid(4),
                Rect::new(0.0, 150.0, 300.0, 40.0),
                sem(Role::Button).clickable(),
            ),
        ];
        let areas = [
            area(
                10,
                Rect::new(0.0, 45.0, 300.0, 200.0),
                (0.0, 800.0),
                (0.0, 120.0),
            ),
            area(
                11,
                Rect::new(0.0, 95.0, 300.0, 50.0),
                (400.0, 0.0),
                (0.0, 0.0),
            ),
            // Nothing to scroll: no node of its own.
            area(12, Rect::new(0.0, 0.0, 300.0, 40.0), (0.0, 0.0), (0.0, 0.0)),
        ];
        let tree = build(&nodes, &areas, None, 1.0, &mut ids);
        let id = |raw| ids.by_widget[&wid(raw)];
        let order: Vec<i32> = tree.nodes.iter().map(|n| n.id).collect();
        assert_eq!(order, vec![id(1), id(10), id(2), id(11), id(3), id(4)]);
        let parent = |raw| tree.node(id(raw)).unwrap().parent;
        assert_eq!(parent(1), HOST);
        assert_eq!(parent(10), HOST);
        assert_eq!(parent(2), id(10));
        assert_eq!(parent(11), id(10));
        assert_eq!(parent(3), id(11));
        assert_eq!(parent(4), id(10));
        assert!(!ids.by_widget.contains_key(&wid(12)));

        let list = tree.node(id(10)).unwrap();
        assert_eq!(list.class_name, "android.widget.ScrollView");
        assert_ne!(list.flags & flag::SCROLLABLE, 0);
        assert_ne!(list.actions & action::SCROLL_FORWARD, 0);
        assert_ne!(list.actions & action::SCROLL_BACKWARD, 0);
        assert_eq!(list.scroll, Some([0, 120, 0, 800]));
        let strip = tree.node(id(11)).unwrap();
        assert_eq!(strip.class_name, "android.widget.HorizontalScrollView");
        assert_eq!(strip.actions, action::SCROLL_FORWARD, "at its start");
    }

    /// **A field that scrolls its own text is one node**, not a scroll view around itself.
    #[test]
    fn a_field_answers_for_its_own_scroll() {
        let mut ids = VirtualIds::default();
        let rect = Rect::new(0.0, 0.0, 200.0, 80.0);
        let tree = build(
            &[(wid(5), rect, sem(Role::TextInput))],
            &[area(5, rect, (0.0, 300.0), (0.0, 0.0))],
            None,
            1.0,
            &mut ids,
        );
        assert_eq!(tree.nodes.len(), 1);
        assert_eq!(tree.nodes[0].parent, HOST);
    }

    fn kinds(events: &[AndroidEvent]) -> Vec<i32> {
        events.iter().map(|e| e.kind).collect()
    }

    /// **Nothing changed, nothing said; a change is a content change.**
    #[test]
    fn an_unchanged_tree_raises_nothing() {
        let mut ids = VirtualIds::default();
        let nodes = [(
            wid(1),
            Rect::new(0.0, 0.0, 10.0, 10.0),
            sem(Role::Label).label("A"),
        )];
        let a = build(&nodes, &[], None, 1.0, &mut ids);
        assert!(diff(&a, &a.clone()).is_empty());
        let moved = [(
            wid(1),
            Rect::new(0.0, 5.0, 10.0, 10.0),
            sem(Role::Label).label("A"),
        )];
        let b = build(&moved, &[], None, 1.0, &mut ids);
        assert_eq!(kinds(&diff(&a, &b)), vec![event::CONTENT_CHANGED]);
        // The first tree published is a change from nothing.
        assert_eq!(
            kinds(&diff(&AndroidTree::default(), &a)),
            vec![event::CONTENT_CHANGED]
        );
    }

    /// **The events a screen reader follows the screen by**: the focus moving, a box
    /// ticked, a value moved, text typed, an area scrolled.
    #[test]
    fn changes_raise_their_events() {
        let mut ids = VirtualIds::default();
        let r = |y| Rect::new(0.0, y, 100.0, 20.0);
        let frame = |ids: &mut VirtualIds, checked, value: f32, text: &str, offset, focus| {
            let nodes = [
                (
                    wid(1),
                    r(0.0),
                    sem(Role::CheckBox).toggled(checked).clickable(),
                ),
                (wid(2), r(30.0), sem(Role::Slider).range(0.0, value, 10.0)),
                (
                    wid(3),
                    r(60.0),
                    sem(Role::TextInput).label("Name").value(text),
                ),
            ];
            let areas = [area(
                9,
                Rect::new(0.0, 0.0, 100.0, 100.0),
                (0.0, 500.0),
                (0.0, offset),
            )];
            build(&nodes, &areas, focus, 1.0, ids)
        };
        let before = frame(&mut ids, false, 2.0, "Ad", 0.0, None);
        let after = frame(&mut ids, true, 3.0, "Ada", 40.0, Some(wid(3)));
        let events = diff(&before, &after);
        let id = |raw| ids.by_widget[&wid(raw)];
        let find = |kind| events.iter().find(|e| e.kind == kind).cloned().unwrap();
        assert_eq!(events[0].kind, event::CONTENT_CHANGED);
        assert_eq!(find(event::FOCUSED).id, id(3));
        assert_eq!(find(event::CLICKED).id, id(1));
        assert_eq!(find(event::SELECTED).id, id(2));
        let typed = find(event::TEXT_CHANGED);
        assert_eq!(typed.id, id(3));
        assert_eq!(typed.numbers[..3], [2, 0, 1]);
        assert_eq!(typed.before.as_deref(), Some("Ad"));
        let scrolled = find(event::SCROLLED);
        assert_eq!(scrolled.id, id(9));
        assert_eq!(scrolled.numbers, [0, 40, 0, 500]);
        // The focus staying where it is raises no second focus event.
        let again = frame(&mut ids, true, 3.0, "Ada!", 40.0, Some(wid(3)));
        assert!(!kinds(&diff(&after, &again)).contains(&event::FOCUSED));
    }

    /// **A text change counts in UTF-16, as Android does**, and keeps the common ends.
    #[test]
    fn text_changes_count_utf16_units() {
        assert_eq!(text_change("abc", "abc"), None);
        assert_eq!(text_change("", "hi"), Some((0, 0, 2)));
        assert_eq!(text_change("hello", "help"), Some((3, 2, 1)));
        assert_eq!(text_change("ab", "a\u{1F600}b"), Some((1, 0, 2)));
        assert_eq!(text_change("aXb", "ab"), Some((1, 1, 0)));
    }

    /// **A request becomes the shell's action**, and only one the node offers.
    #[test]
    fn requests_become_actions() {
        let mut ids = VirtualIds::default();
        let nodes = [
            (
                wid(1),
                Rect::new(0.0, 0.0, 10.0, 10.0),
                sem(Role::Button).clickable(),
            ),
            (
                wid(2),
                Rect::new(0.0, 20.0, 10.0, 10.0),
                sem(Role::Slider).range(10.0, 15.0, 20.0),
            ),
            (
                wid(3),
                Rect::new(0.0, 40.0, 10.0, 10.0),
                sem(Role::TextInput),
            ),
            (
                wid(4),
                Rect::new(0.0, 60.0, 10.0, 10.0),
                sem(Role::Label).label("x"),
            ),
        ];
        let areas = [area(
            9,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            (0.0, 500.0),
            (0.0, 0.0),
        )];
        let tree = build(&nodes, &areas, None, 1.0, &mut ids);
        let id = |raw| ids.by_widget[&wid(raw)];
        let ask = |raw, action, text: Option<&str>, value| {
            to_action(
                &Request {
                    id: id(raw),
                    action,
                    text: text.map(str::to_string),
                    value,
                },
                &tree,
                &ids,
            )
        };
        assert_eq!(
            ask(1, action::CLICK, None, 0.0),
            Some(A11yAction::Click(wid(1)))
        );
        assert_eq!(
            ask(1, action::FOCUS, None, 0.0),
            Some(A11yAction::Focus(wid(1)))
        );
        assert_eq!(
            ask(3, action::SET_TEXT, Some("Ada"), 0.0),
            Some(A11yAction::SetValue(wid(3), "Ada".into()))
        );
        assert_eq!(
            ask(2, action::SCROLL_FORWARD, None, 0.0),
            Some(A11yAction::Adjust(wid(2), true))
        );
        assert_eq!(
            ask(2, action::SCROLL_BACKWARD, None, 0.0),
            Some(A11yAction::Adjust(wid(2), false))
        );
        assert_eq!(
            ask(2, action::SET_PROGRESS, None, 17.5),
            Some(A11yAction::SetFraction(wid(2), 0.75))
        );
        assert_eq!(
            ask(9, action::SCROLL_FORWARD, None, 0.0),
            Some(A11yAction::Scroll(wid(9), true))
        );
        // At its start, the list cannot go back.
        assert_eq!(ask(9, action::SCROLL_BACKWARD, None, 0.0), None);
        // Text cannot be pressed, a button cannot be typed into or set.
        assert_eq!(ask(4, action::CLICK, None, 0.0), None);
        assert_eq!(ask(1, action::SET_TEXT, Some("x"), 0.0), None);
        assert_eq!(ask(1, action::SET_PROGRESS, None, 1.0), None);
        // A node no longer there is nothing.
        let gone = Request {
            id: 4242,
            action: action::CLICK,
            text: None,
            value: 0.0,
        };
        assert_eq!(to_action(&gone, &tree, &ids), None);
    }

    /// **A page is one viewport along the area's axis**, never past its ends.
    #[test]
    fn a_page_scroll_stays_inside_the_area() {
        let viewport = Rect::new(0.0, 0.0, 300.0, 200.0);
        assert_eq!(
            page_target(viewport, (0.0, 500.0), (0.0, 0.0), true),
            (0.0, 200.0)
        );
        assert_eq!(
            page_target(viewport, (0.0, 500.0), (0.0, 400.0), true),
            (0.0, 500.0)
        );
        assert_eq!(
            page_target(viewport, (0.0, 500.0), (0.0, 100.0), false),
            (0.0, 0.0)
        );
        assert_eq!(
            page_target(viewport, (900.0, 0.0), (0.0, 0.0), true),
            (300.0, 0.0)
        );
    }
}
