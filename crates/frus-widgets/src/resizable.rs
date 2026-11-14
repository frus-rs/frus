// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`ResizablePanels`], [`ResizablePanel`] and [`ResizablePanelsState`]: panels side by side,
//! or one above the other, with a handle between each two that moves the boundary
//! (milestone 809).
//!
//! # Where the sizes live
//!
//! In the application, as a [`ResizablePanelsState`]: each panel's **share** of the group's
//! room, and the drag under way if there is one. The group is sent the new state on every
//! movement of a handle, by the pointer or the keyboard; an application that remembers the
//! layout between runs saves [`sizes`](ResizablePanelsState::sizes) once
//! [`is_dragging`](ResizablePanelsState::is_dragging) is false again, and gives them back to
//! [`ResizablePanelsState::new`] at the next start.

use std::cell::Cell;
use std::rc::Rc;

use frus_core::{Color, Rect, Scene, Size};
use frus_layout::{Align, Dimension, FlexDirection, Style};

use crate::flex::Flex;
use crate::interaction::{Cursor, Status};
use crate::shortcuts::{KeyStroke, ShortcutKey};
use crate::theme::Theme;
use crate::transparent::Shared;
use crate::widget::Widget;

/// How wide a handle is across, where a press takes it, in pixels.
const HANDLE: f32 = 8.0;
/// How thick the line drawn down the middle of a handle is, at rest.
const LINE: f32 = 1.0;
/// How thick it is under the pointer, held, or with the focus.
const ACTIVE_LINE: f32 = 3.0;
/// How far one arrow key moves a boundary: five per cent of the group.
const STEP: f32 = 0.05;
/// A share's rounding: no boundary moves for less than this.
const EPS: f32 = 1e-4;

/// **What a panel may be**, as a share of its group's room: no smaller than `min` unless it
/// collapses, no larger than `max`, and `collapsed` once collapsed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Limits {
    pub(crate) min: f32,
    pub(crate) max: f32,
    pub(crate) collapsible: bool,
    pub(crate) collapsed: f32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 1.0,
            collapsible: false,
            collapsed: 0.0,
        }
    }
}

/// **Where the boundary between panels `i` and `i + 1` settles** when the first is asked to be
/// `want` of the room the two share, `pair`: inside both panels' limits, and — for a panel
/// that collapses — collapsed once it is asked to be less than half its least size.
fn settle(want: f32, pair: f32, a: Limits, b: Limits) -> f32 {
    let want_b = pair - want;
    if b.collapsible && want_b < b.min * 0.5 && pair - b.collapsed <= a.max + EPS {
        return pair - b.collapsed;
    }
    if a.collapsible && want < a.min * 0.5 && pair - a.collapsed <= b.max + EPS {
        return a.collapsed;
    }
    let lo = a.min.max(pair - b.max);
    let hi = a.max.min(pair - b.min);
    if lo > hi {
        // The limits cannot both be kept: the first panel's are.
        return want.clamp(a.min, a.max.min(pair));
    }
    want.clamp(lo, hi)
}

/// The shares after the boundary after panel `handle` has moved by `delta` (a share; positive
/// grows the panel before it) from where it was in `from`.
pub(crate) fn resize(from: &[f32], limits: &[Limits], handle: usize, delta: f32) -> Vec<f32> {
    let mut sizes = from.to_vec();
    if handle + 1 >= sizes.len() {
        return sizes;
    }
    let (i, j) = (handle, handle + 1);
    let pair = from[i] + from[j];
    let a = settle(from[i] + delta, pair, limits[i], limits[j]);
    sizes[i] = a;
    sizes[j] = pair - a;
    sizes
}

/// `sizes` as shares of one: as many as there are panels, none negative, adding up to one.
/// A list of the wrong length is replaced by equal shares.
pub(crate) fn normalized(sizes: &[f32], count: usize) -> Vec<f32> {
    if count == 0 {
        return Vec::new();
    }
    let usable = sizes.len() == count && sizes.iter().all(|s| s.is_finite() && *s >= 0.0);
    let total: f32 = if usable { sizes.iter().sum() } else { 0.0 };
    if !usable || total <= EPS {
        return vec![1.0 / count as f32; count];
    }
    sizes.iter().map(|s| s / total).collect()
}

/// The drag of one handle under way.
#[derive(Clone, Debug, PartialEq)]
struct Drag {
    handle: usize,
    /// The shares when the drag began: every movement is measured from them, so a panel
    /// held at its least size follows the pointer again only once the pointer is back.
    from: Vec<f32>,
    /// How far the boundary has been dragged since, as a share.
    moved: f32,
}

/// **The sizes of a [`ResizablePanels`]**: each panel's share of the group's room, and the drag
/// under way. Kept by the application, and given back to it, changed, through the group's
/// message.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResizablePanelsState {
    sizes: Vec<f32>,
    drag: Option<Drag>,
}

impl ResizablePanelsState {
    /// The panels at these shares of the group's room — `[0.25, 0.75]` gives the first a
    /// quarter. They are scaled to add up to one; a list that does not have one share per
    /// panel is replaced by equal shares. Empty is equal shares from the start.
    pub fn new(sizes: impl Into<Vec<f32>>) -> Self {
        Self {
            sizes: sizes.into(),
            drag: None,
        }
    }

    /// Each panel's share of the group's room, as last set: what to save to restore the
    /// layout later.
    pub fn sizes(&self) -> &[f32] {
        &self.sizes
    }

    /// Whether a handle is being dragged. A layout worth saving is one that is not.
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }
}

/// **One panel of a [`ResizablePanels`]**: what it shows, and how small and how large it may
/// get, as shares of the group's room.
pub struct ResizablePanel<Msg = crate::callback::Callback> {
    child: Rc<dyn Widget<Msg>>,
    limits: Limits,
    label: Option<String>,
}

impl<Msg: 'static> ResizablePanel<Msg> {
    /// A panel showing `child`, free to take any share.
    pub fn new(child: impl Widget<Msg> + 'static) -> Self {
        Self {
            child: Rc::new(child),
            limits: Limits::default(),
            label: None,
        }
    }

    /// **The least share** it may have, from `0` to `1`. Its handles stop there — unless it
    /// [`collapses`](Self::collapsible).
    #[must_use]
    pub fn min(mut self, share: f32) -> Self {
        self.limits.min = share.clamp(0.0, 1.0);
        self
    }

    /// **The largest share** it may have, from `0` to `1`.
    #[must_use]
    pub fn max(mut self, share: f32) -> Self {
        self.limits.max = share.clamp(0.0, 1.0);
        self
    }

    /// Whether it **collapses**: dragged to less than half its least share, it snaps to its
    /// [`collapsed_size`](Self::collapsed_size), and dragged back past half its least share it
    /// opens at that share. Enter on a handle next to it collapses and opens it.
    #[must_use]
    pub fn collapsible(mut self, collapsible: bool) -> Self {
        self.limits.collapsible = collapsible;
        self
    }

    /// The share it keeps once collapsed: nothing, unless told — a strip of icons, say.
    #[must_use]
    pub fn collapsed_size(mut self, share: f32) -> Self {
        self.limits.collapsed = share.clamp(0.0, 1.0);
        self
    }

    /// What the panel is called, for the handles' announcement: "Sidebar".
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

/// Defaults for [`ResizablePanels`] (milestone 809). Every field unset is the framework's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ResizablePanelsTheme {
    /// How wide a handle is across, where a press takes it. Unset, eight pixels.
    pub handle_size: Option<f32>,
    /// How thick the line down a handle is at rest. Unset, one pixel.
    pub line_width: Option<f32>,
    /// The line's colour at rest. Unset, `outline_variant`.
    pub line_color: Option<Color>,
    /// How thick the line is under the pointer, while dragged and with the focus. Unset, three.
    pub active_line_width: Option<f32>,
    /// Its colour then. Unset, `primary`.
    pub active_color: Option<Color>,
    /// Whether a grip — a short rounded bar across the line's middle — is drawn. Unset, no.
    pub grip: Option<bool>,
    /// The grip's colour. Unset, `outline`.
    pub grip_color: Option<Color>,
    /// How far one arrow key moves a boundary, as a share of the group. Unset, five per cent.
    pub keyboard_step: Option<f32>,
}

/// What a caller said about the handles, every word optional.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct HandleStyle {
    size: Option<f32>,
    line_width: Option<f32>,
    line_color: Option<Color>,
    active_line_width: Option<f32>,
    active_color: Option<Color>,
    grip: Option<bool>,
    grip_color: Option<Color>,
    step: Option<f32>,
}

impl HandleStyle {
    fn size(&self, theme: &Theme) -> f32 {
        self.size
            .or(theme.widgets.resizable_panels.handle_size)
            .unwrap_or(HANDLE)
            .max(0.0)
    }

    fn step(&self, theme: &Theme) -> f32 {
        self.step
            .or(theme.widgets.resizable_panels.keyboard_step)
            .unwrap_or(STEP)
    }
}

type OnChange<Msg> = Rc<dyn Fn(ResizablePanelsState) -> Msg>;

/// One panel as the build needs it: what it shows, its limits, its name.
type PanelParts<Msg> = (Rc<dyn Widget<Msg>>, Limits, Option<String>);

/// What the panels are built from that only the theme knows: the direction of the text, a
/// handle's size and a key's step. Learnt in [`Widget::expand`], which runs before the build.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Env {
    rtl: bool,
    thickness: f32,
    step: f32,
}

impl Default for Env {
    fn default() -> Self {
        Self {
            rtl: false,
            thickness: HANDLE,
            step: STEP,
        }
    }
}

/// **Panels side by side, or one above the other, with a handle between each two** that moves
/// the boundary: a sidebar beside a document, an editor above a console.
///
/// ```
/// use frus_widgets::{ResizablePanel, ResizablePanels, ResizablePanelsState, Text};
///
/// #[derive(Clone)]
/// enum Msg { Resized(ResizablePanelsState) }
///
/// let state = ResizablePanelsState::new([0.3, 0.7]);
/// let panels = ResizablePanels::horizontal(&state, Msg::Resized)
///     .panel(ResizablePanel::new(Text::new("Files")).min(0.15).max(0.5).collapsible(true))
///     .panel(ResizablePanel::new(Text::new("Editor")).min(0.3));
/// # let _: ResizablePanels<Msg> = panels;
/// ```
///
/// - **A handle is dragged** with the pointer or a finger; it stops where either panel beside
///   it reaches its least or largest share, and a panel that
///   [collapses](ResizablePanel::collapsible) snaps shut once dragged under half its least share.
/// - **A handle takes the focus**, and then the arrow keys move it by
///   [`keyboard_step`](Self::keyboard_step) (towards the start or the end of the text, in a
///   right-to-left layout too), Home and End take the panel before it to its least and its
///   largest share, and Enter collapses and opens a collapsible panel beside it.
/// - **It takes the room it is given**, and shares it out: the handles first, then the panels
///   by their shares. A panel's content is clipped to its panel.
///
/// A handle reads to a screen reader as a splitter, with the share of the panel before it as
/// its value. Every part of its look is the caller's, then [`ResizablePanelsTheme`]'s, then
/// the framework's.
pub struct ResizablePanels<Msg = crate::callback::Callback> {
    vertical: bool,
    state: ResizablePanelsState,
    on_change: OnChange<Msg>,
    panels: Vec<ResizablePanel<Msg>>,
    style: HandleStyle,
    /// What the theme says that the panels are built from, learnt before they are built.
    env: Rc<Cell<Env>>,
    build: Box<dyn Fn(Size) -> Box<dyn Widget<Msg>>>,
}

impl<Msg: Clone + 'static> ResizablePanels<Msg> {
    /// Panels **side by side**, at the shares `state` holds, sending `on_change` the state they
    /// should have next.
    pub fn horizontal<R1: crate::callback::IntoMsg<Msg>>(
        state: &ResizablePanelsState,
        on_change: impl Fn(ResizablePanelsState) -> R1 + 'static,
    ) -> Self {
        Self::along(false, state, on_change)
    }

    /// Panels **one above the other**, at the shares `state` holds.
    pub fn vertical<R1: crate::callback::IntoMsg<Msg>>(
        state: &ResizablePanelsState,
        on_change: impl Fn(ResizablePanelsState) -> R1 + 'static,
    ) -> Self {
        Self::along(true, state, on_change)
    }

    fn along<R1: crate::callback::IntoMsg<Msg>>(
        vertical: bool,
        state: &ResizablePanelsState,
        on_change: impl Fn(ResizablePanelsState) -> R1 + 'static,
    ) -> Self {
        let on_change = crate::callback::handler1(on_change);
        Self {
            vertical,
            state: state.clone(),
            on_change: Rc::new(on_change),
            panels: Vec::new(),
            style: HandleStyle::default(),
            env: Rc::new(Cell::new(Env::default())),
            build: Box::new(|_| Box::new(crate::SizedBox::empty())),
        }
        .rebuilt()
    }

    /// Adds a panel, after the others.
    #[must_use]
    pub fn panel(mut self, panel: ResizablePanel<Msg>) -> Self {
        self.panels.push(panel);
        self.rebuilt()
    }

    /// How wide a handle is across, where a press takes it, over the theme's and eight.
    #[must_use]
    pub fn handle_size(mut self, size: f32) -> Self {
        self.style.size = Some(size);
        self.rebuilt()
    }

    /// How thick the line down a handle is at rest, over the theme's and one.
    #[must_use]
    pub fn line_width(mut self, width: f32) -> Self {
        self.style.line_width = Some(width);
        self.rebuilt()
    }

    /// The line's colour at rest, over the theme's and `outline_variant`.
    #[must_use]
    pub fn line_color(mut self, color: Color) -> Self {
        self.style.line_color = Some(color);
        self.rebuilt()
    }

    /// How thick the line is under the pointer, dragged or focused, over the theme's and three.
    #[must_use]
    pub fn active_line_width(mut self, width: f32) -> Self {
        self.style.active_line_width = Some(width);
        self.rebuilt()
    }

    /// The line's colour then, over the theme's and `primary`.
    #[must_use]
    pub fn active_color(mut self, color: Color) -> Self {
        self.style.active_color = Some(color);
        self.rebuilt()
    }

    /// Whether a grip is drawn across the middle of each handle.
    #[must_use]
    pub fn grip(mut self, grip: bool) -> Self {
        self.style.grip = Some(grip);
        self.rebuilt()
    }

    /// The grip's colour, over the theme's and `outline`.
    #[must_use]
    pub fn grip_color(mut self, color: Color) -> Self {
        self.style.grip_color = Some(color);
        self.rebuilt()
    }

    /// How far one arrow key moves a boundary, as a share of the group, over the theme's and
    /// five per cent.
    #[must_use]
    pub fn keyboard_step(mut self, share: f32) -> Self {
        self.style.step = Some(share.abs());
        self.rebuilt()
    }

    /// Makes the layout again from what has been said so far: a panel or a style said after
    /// another reaches it.
    fn rebuilt(mut self) -> Self {
        let vertical = self.vertical;
        let panels: Vec<PanelParts<Msg>> = self
            .panels
            .iter()
            .map(|p| (p.child.clone(), p.limits, p.label.clone()))
            .collect();
        let state = self.state.clone();
        let on_change = self.on_change.clone();
        let style = self.style;
        let env = self.env.clone();
        self.build = Box::new(move |size| {
            build_group(
                vertical,
                &panels,
                &state,
                &on_change,
                style,
                env.get(),
                size,
            )
        });
        self
    }
}

/// The group laid out in `size`: the panels at their shares of what the handles leave, and a
/// handle between each two.
fn build_group<Msg: Clone + 'static>(
    vertical: bool,
    panels: &[PanelParts<Msg>],
    state: &ResizablePanelsState,
    on_change: &OnChange<Msg>,
    style: HandleStyle,
    env: Env,
    size: Size,
) -> Box<dyn Widget<Msg>> {
    let Env {
        rtl,
        thickness,
        step,
    } = env;
    let count = panels.len();
    let sizes = normalized(&state.sizes, count);
    let limits: Vec<Limits> = panels.iter().map(|p| p.1).collect();
    let mut flex = if vertical {
        Flex::column()
    } else {
        Flex::row()
    };
    flex = flex.align(Align::Stretch);
    let main = if vertical { size.height } else { size.width };
    let free = (main - thickness * count.saturating_sub(1) as f32).max(0.0);
    for (index, (child, _, _)) in panels.iter().enumerate() {
        let extent = (sizes[index] * free).max(0.0);
        flex = flex.child(PanelBox {
            children: vec![Box::new(Shared::new(child.clone()))],
            vertical,
            extent,
        });
        if index + 1 < count {
            flex = flex.child(Handle {
                vertical,
                rtl,
                index,
                sizes: sizes.clone(),
                limits: limits.clone(),
                drag: state.drag.clone().filter(|d| d.handle == index),
                free,
                on_change: on_change.clone(),
                style,
                thickness,
                label: panels[index].2.clone(),
                keys: handle_keys(vertical, rtl, index, &sizes, &limits, step, on_change),
            });
        }
    }
    Box::new(flex)
}

/// The keys a focused handle answers, each with the state it leads to.
fn handle_keys<Msg: Clone + 'static>(
    vertical: bool,
    rtl: bool,
    index: usize,
    sizes: &[f32],
    limits: &[Limits],
    step: f32,
    on_change: &OnChange<Msg>,
) -> Vec<(KeyStroke, Msg)> {
    let to = |sizes: Vec<f32>| on_change(ResizablePanelsState { sizes, drag: None });
    let by = |delta: f32| to(resize(sizes, limits, index, delta));
    let (a, b) = (limits[index], limits[index + 1]);
    let pair = sizes[index] + sizes[index + 1];
    // Towards the end of the text grows the panel before the handle: the right in
    // left-to-right text, the left in right-to-left, and down in a column.
    let (grow, shrink) = match (vertical, rtl) {
        (true, _) => (ShortcutKey::Down, ShortcutKey::Up),
        (false, false) => (ShortcutKey::Right, ShortcutKey::Left),
        (false, true) => (ShortcutKey::Left, ShortcutKey::Right),
    };
    let mut keys = vec![
        (KeyStroke::new(grow), by(step)),
        (KeyStroke::new(shrink), by(-step)),
        (KeyStroke::new(ShortcutKey::Home), by(-pair)),
        (KeyStroke::new(ShortcutKey::End), by(pair)),
    ];
    // Enter shuts the panel before the handle if it collapses, else the one after; and opens
    // whichever of the two is shut.
    let toggled = if a.collapsible {
        Some(if sizes[index] <= a.collapsed + EPS {
            // Open: at its least share, or half the pair, whichever is more.
            let want = a.min.max(pair * 0.5);
            resize(sizes, limits, index, want - sizes[index])
        } else {
            let mut next = sizes.to_vec();
            next[index] = a.collapsed;
            next[index + 1] = pair - a.collapsed;
            next
        })
    } else if b.collapsible {
        Some(if sizes[index + 1] <= b.collapsed + EPS {
            let want = b.min.max(pair * 0.5);
            resize(sizes, limits, index, sizes[index + 1] - want)
        } else {
            let mut next = sizes.to_vec();
            next[index + 1] = b.collapsed;
            next[index] = pair - b.collapsed;
            next
        })
    } else {
        None
    };
    if let Some(next) = toggled {
        keys.push((KeyStroke::new(ShortcutKey::Enter), to(next)));
    }
    keys
}

impl<Msg: Clone + 'static> Widget<Msg> for ResizablePanels<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn layout_builder(&self) -> Option<&dyn Fn(Size) -> Box<dyn Widget<Msg>>> {
        Some(&*self.build)
    }

    /// The group takes all the room it is given, and shares it out.
    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        crate::widget::FillAxes::BOTH
    }

    /// The direction of the text is learnt here, before the panels are built: it decides which
    /// way a drag and an arrow key move a boundary.
    fn expand(&self, _id: crate::interaction::WidgetId, _runtime: &crate::Runtime, theme: &Theme) {
        self.env.set(Env {
            rtl: theme.direction.is_rtl(),
            thickness: self.style.size(theme),
            step: self.style.step(theme),
        });
    }

    fn debug_name(&self) -> &'static str {
        "ResizablePanels"
    }
}

/// One panel's box: its share of the room along the group, all of it across, and its content
/// clipped to it.
struct PanelBox<Msg> {
    children: Vec<Box<dyn Widget<Msg>>>,
    vertical: bool,
    extent: f32,
}

impl<Msg: Clone> Widget<Msg> for PanelBox<Msg> {
    fn style(&self) -> Style {
        let along = Dimension::Length(self.extent);
        Style {
            flex_direction: FlexDirection::Column,
            width: if self.vertical {
                Dimension::Auto
            } else {
                along
            },
            height: if self.vertical {
                along
            } else {
                Dimension::Auto
            },
            flex_shrink: 0.0,
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn clips_to_box(&self) -> bool {
        true
    }

    fn debug_name(&self) -> &'static str {
        "ResizablePanel"
    }
}

/// The handle between panels `index` and `index + 1`.
struct Handle<Msg> {
    vertical: bool,
    rtl: bool,
    index: usize,
    sizes: Vec<f32>,
    limits: Vec<Limits>,
    /// This handle's drag, if it is the one being dragged.
    drag: Option<Drag>,
    /// The room the panels share, in pixels: what a drag's pixels are a share of.
    free: f32,
    on_change: OnChange<Msg>,
    style: HandleStyle,
    thickness: f32,
    label: Option<String>,
    keys: Vec<(KeyStroke, Msg)>,
}

impl<Msg: Clone> Handle<Msg> {
    fn sizing(&self, theme: Option<&Theme>) -> Style {
        let thickness = theme.map_or(self.thickness, |t| self.style.size(t));
        let across = Dimension::Length(thickness);
        Style {
            width: if self.vertical {
                Dimension::Auto
            } else {
                across
            },
            height: if self.vertical {
                across
            } else {
                Dimension::Auto
            },
            flex_shrink: 0.0,
            ..Default::default()
        }
    }

    fn send(&self, sizes: Vec<f32>, drag: Option<Drag>) -> Msg {
        (self.on_change)(ResizablePanelsState { sizes, drag })
    }
}

impl<Msg: Clone> Widget<Msg> for Handle<Msg> {
    fn style(&self) -> Style {
        self.sizing(None)
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        self.sizing(Some(theme))
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let o = status.opacity;
        let t = &theme.widgets.resizable_panels;
        let s = &self.style;
        let active = status
            .hover_progress
            .max(status.press_progress)
            .max(if status.focused { 1.0 } else { 0.0 })
            .max(if self.drag.is_some() { 1.0 } else { 0.0 })
            .clamp(0.0, 1.0);
        let rest_width = s.line_width.or(t.line_width).unwrap_or(LINE);
        let active_width = s
            .active_line_width
            .or(t.active_line_width)
            .unwrap_or(ACTIVE_LINE);
        let rest = s
            .line_color
            .or(t.line_color)
            .unwrap_or(theme.scheme.outline_variant);
        let lit = s.active_color.or(t.active_color).unwrap_or(theme.primary);
        let width = rest_width + (active_width - rest_width) * active;
        let colour = rest.lerp(lit, active);
        let line = if self.vertical {
            Rect::new(
                bounds.x,
                bounds.y + (bounds.height - width) * 0.5,
                bounds.width,
                width,
            )
        } else {
            Rect::new(
                bounds.x + (bounds.width - width) * 0.5,
                bounds.y,
                width,
                bounds.height,
            )
        };
        if width > 0.0 && colour.a > 0.0 {
            scene.fill_rect(line, colour.fade(o));
        }
        if s.grip.or(t.grip).unwrap_or(false) {
            let grip = s
                .grip_color
                .or(t.grip_color)
                .unwrap_or(theme.scheme.outline);
            let (long, short) = (24.0, 4.0);
            let rect = if self.vertical {
                Rect::new(
                    bounds.x + (bounds.width - long) * 0.5,
                    bounds.y + (bounds.height - short) * 0.5,
                    long,
                    short,
                )
            } else {
                Rect::new(
                    bounds.x + (bounds.width - short) * 0.5,
                    bounds.y + (bounds.height - long) * 0.5,
                    short,
                    long,
                )
            };
            scene.draw_rect(rect, grip.fade(o), short * 0.5, 0.0, Color::TRANSPARENT);
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn focusable(&self) -> bool {
        true
    }

    /// A press on the handle is the handle's, and so is the pointer over it: it lights up.
    fn opaque(&self) -> bool {
        true
    }

    /// The handle shows the focus by lighting its line, as it does under the pointer.
    fn draws_own_focus(&self) -> bool {
        true
    }

    fn shortcut_callbacks(&self) -> &[(KeyStroke, Msg)] {
        &self.keys
    }

    fn pan_axis(&self) -> Option<crate::PanAxis> {
        Some(if self.vertical {
            crate::PanAxis::Vertical
        } else {
            crate::PanAxis::Horizontal
        })
    }

    fn on_pan(&self, event: crate::PanEvent) -> Option<Msg> {
        match event {
            crate::PanEvent::Start { .. } => Some(self.send(
                self.sizes.clone(),
                Some(Drag {
                    handle: self.index,
                    from: self.sizes.clone(),
                    moved: 0.0,
                }),
            )),
            crate::PanEvent::Update { delta, .. } => {
                if self.free <= 0.0 {
                    return None;
                }
                let along = if self.vertical { delta.y } else { delta.x };
                // Right to left, the panel before the handle is on its right: a drag to the
                // right shrinks it.
                let along = if self.rtl && !self.vertical {
                    -along
                } else {
                    along
                };
                let drag = self.drag.clone().unwrap_or(Drag {
                    handle: self.index,
                    from: self.sizes.clone(),
                    moved: 0.0,
                });
                let moved = drag.moved + along / self.free;
                let sizes = resize(&drag.from, &self.limits, self.index, moved);
                Some(self.send(sizes, Some(Drag { moved, ..drag })))
            }
            crate::PanEvent::End { .. } => Some(self.send(self.sizes.clone(), None)),
        }
    }

    fn cursor_icon(&self, _x: f32, _y: f32, _w: f32, _h: f32) -> Option<Cursor> {
        Some(if self.vertical {
            Cursor::ResizeRow
        } else {
            Cursor::ResizeColumn
        })
    }

    /// A splitter, its value the share of the panel before it, in per cent.
    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        let a = self.limits[self.index];
        let label = match &self.label {
            Some(label) => format!("Resize {label}"),
            None => "Resize".to_string(),
        };
        let percent = (self.sizes[self.index] * 100.0).round();
        Some(
            frus_core::SemanticsProperties::new(frus_core::Role::Splitter)
                .label(label)
                .range(
                    if a.collapsible { a.collapsed } else { a.min } * 100.0,
                    percent,
                    a.max * 100.0,
                )
                .value(format!("{percent:.0}%")),
        )
    }

    fn debug_name(&self) -> &'static str {
        "ResizeHandle"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Container, Runtime};
    use frus_core::Primitive;

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Resized(ResizablePanelsState),
    }

    const A: Color = Color::rgb(0.9, 0.1, 0.1);
    const B: Color = Color::rgb(0.1, 0.9, 0.1);

    fn lim(min: f32, max: f32, collapsible: bool) -> Limits {
        Limits {
            min,
            max,
            collapsible,
            collapsed: 0.0,
        }
    }

    fn close(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    #[test]
    fn a_boundary_stops_at_the_limits_of_both_panels() {
        let limits = [lim(0.2, 0.6, false), lim(0.3, 1.0, false)];
        let from = [0.4, 0.6];
        assert!(close(&resize(&from, &limits, 0, 0.1), &[0.5, 0.5]));
        assert!(
            close(&resize(&from, &limits, 0, 0.5), &[0.6, 0.4]),
            "its max"
        );
        assert!(
            close(&resize(&from, &limits, 0, -0.5), &[0.2, 0.8]),
            "its min"
        );
        let tight = [lim(0.0, 1.0, false), lim(0.5, 1.0, false)];
        assert!(
            close(&resize(&from, &tight, 0, 0.3), &[0.5, 0.5]),
            "the other's min"
        );
    }

    /// A collapsible panel snaps shut under half its least share, and opens again past it —
    /// measured from where the drag began, so small steps add up.
    #[test]
    fn a_collapsible_panel_snaps_shut_and_open() {
        let limits = [lim(0.2, 1.0, true), lim(0.0, 1.0, false)];
        let from = [0.3, 0.7];
        assert!(
            close(&resize(&from, &limits, 0, -0.15), &[0.2, 0.8]),
            "held at min"
        );
        assert!(
            close(&resize(&from, &limits, 0, -0.21), &[0.0, 1.0]),
            "shut"
        );
        let shut = [0.0, 1.0];
        assert!(
            close(&resize(&shut, &limits, 0, 0.05), &[0.0, 1.0]),
            "still shut"
        );
        assert!(
            close(&resize(&shut, &limits, 0, 0.12), &[0.2, 0.8]),
            "open at its min"
        );
    }

    #[test]
    fn sizes_are_shares_of_one() {
        assert!(close(&normalized(&[1.0, 3.0], 2), &[0.25, 0.75]));
        assert!(close(&normalized(&[0.5], 2), &[0.5, 0.5]), "wrong length");
        assert!(close(&normalized(&[], 3), &[1.0 / 3.0; 3]));
        assert!(close(&normalized(&[0.0, 0.0], 2), &[0.5, 0.5]));
    }

    fn group(state: &ResizablePanelsState) -> ResizablePanels<Msg> {
        ResizablePanels::horizontal(state, Msg::Resized)
            .panel(
                ResizablePanel::new(Container::<Msg>::new().color(A))
                    .min(0.2)
                    .collapsible(true)
                    .label("Files"),
            )
            .panel(ResizablePanel::new(Container::<Msg>::new().color(B)))
    }

    fn rect_of(ui: &crate::Ui<Msg>, colour: Color) -> Option<Rect> {
        ui.scene().primitives().iter().find_map(|p| match p {
            Primitive::Rect { rect, color, .. } if *color == colour => Some(*rect),
            _ => None,
        })
    }

    fn frame(widget: &ResizablePanels<Msg>, theme: &Theme) -> crate::Ui<Msg> {
        build_ui(widget, Size::new(408.0, 200.0), &Runtime::default(), theme)
    }

    /// The panels share what the handle leaves, by their shares, and fill the height.
    #[test]
    fn the_panels_share_the_room_the_handles_leave() {
        let ui = frame(
            &group(&ResizablePanelsState::new([0.25, 0.75])),
            &Theme::default(),
        );
        let a = rect_of(&ui, A).expect("the first panel");
        let b = rect_of(&ui, B).expect("the second");
        assert_eq!((a.x, a.width, a.height), (0.0, 100.0, 200.0));
        assert_eq!((b.x, b.width), (108.0, 300.0));
        // The handle is a splitter, its value the first panel's share.
        let splitter = ui
            .semantics()
            .iter()
            .find(|(_, _, s)| s.role == frus_core::Role::Splitter)
            .map(|(_, _, s)| s.clone())
            .expect("a splitter");
        assert_eq!(splitter.label.as_deref(), Some("Resize Files"));
        assert_eq!(splitter.range.map(|r| r.1), Some(25.0));
    }

    /// Right to left, the first panel is on the right.
    #[test]
    fn right_to_left_the_first_panel_is_on_the_right() {
        let theme = Theme {
            direction: frus_core::TextDirection::Rtl,
            ..Theme::default()
        };
        let ui = frame(&group(&ResizablePanelsState::new([0.25, 0.75])), &theme);
        let a = rect_of(&ui, A).unwrap();
        assert_eq!((a.x, a.width), (308.0, 100.0));
    }

    fn handle(rtl: bool, drag: Option<Drag>) -> Handle<Msg> {
        Handle {
            vertical: false,
            rtl,
            index: 0,
            sizes: vec![0.25, 0.75],
            limits: vec![lim(0.2, 1.0, true), Limits::default()],
            drag,
            free: 400.0,
            on_change: Rc::new(Msg::Resized),
            style: HandleStyle::default(),
            thickness: 8.0,
            label: None,
            keys: Vec::new(),
        }
    }

    fn sizes_of(msg: Option<Msg>) -> (Vec<f32>, bool) {
        match msg {
            Some(Msg::Resized(state)) => (state.sizes().to_vec(), state.is_dragging()),
            None => panic!("no message"),
        }
    }

    /// A drag moves the boundary by its pixels as a share of the room; right to left, a drag
    /// to the right shrinks the first panel; the end says the drag is over.
    #[test]
    fn a_drag_moves_the_boundary() {
        let start = handle(false, None).on_pan(crate::PanEvent::Start {
            local: frus_core::Point::new(4.0, 50.0),
        });
        let (sizes, dragging) = sizes_of(start);
        assert!(dragging && close(&sizes, &[0.25, 0.75]));
        let drag = Some(Drag {
            handle: 0,
            from: vec![0.25, 0.75],
            moved: 0.0,
        });
        let update = |rtl: bool, dx: f32| {
            handle(rtl, drag.clone()).on_pan(crate::PanEvent::Update {
                local: frus_core::Point::new(4.0, 50.0),
                delta: frus_core::Point::new(dx, 0.0),
            })
        };
        assert!(close(&sizes_of(update(false, 40.0)).0, &[0.35, 0.65]));
        assert!(close(&sizes_of(update(true, 40.0)).0, &[0.2, 0.8]), "rtl");
        let (_, dragging) = sizes_of(handle(false, drag.clone()).on_pan(crate::PanEvent::End {
            velocity: frus_core::Point::new(0.0, 0.0),
        }));
        assert!(!dragging, "the drag is over: a layout worth saving");
    }

    /// The keys a focused handle answers: the arrows by the step (mirrored right to left),
    /// Home and End to the ends, Enter to collapse.
    #[test]
    fn the_keyboard_moves_and_collapses() {
        let limits = [lim(0.2, 1.0, true), Limits::default()];
        let on: OnChange<Msg> = Rc::new(Msg::Resized);
        let keys = |rtl| handle_keys(false, rtl, 0, &[0.25, 0.75], &limits, 0.05, &on);
        let find = |rtl, key| {
            keys(rtl)
                .into_iter()
                .find(|(stroke, _)| stroke.key == key)
                .map(|(_, msg)| sizes_of(Some(msg)).0)
                .unwrap()
        };
        assert!(close(&find(false, ShortcutKey::Right), &[0.3, 0.7]));
        assert!(close(&find(false, ShortcutKey::Left), &[0.2, 0.8]));
        assert!(close(&find(true, ShortcutKey::Left), &[0.3, 0.7]), "rtl");
        assert!(close(&find(false, ShortcutKey::End), &[1.0, 0.0]));
        assert!(
            close(&find(false, ShortcutKey::Home), &[0.0, 1.0]),
            "collapses"
        );
        assert!(
            close(&find(false, ShortcutKey::Enter), &[0.0, 1.0]),
            "Enter shuts it"
        );
        let shut = handle_keys(false, false, 0, &[0.0, 1.0], &limits, 0.05, &on);
        let reopened = shut
            .into_iter()
            .find(|(s, _)| s.key == ShortcutKey::Enter)
            .map(|(_, m)| sizes_of(Some(m)).0)
            .unwrap();
        assert!(close(&reopened, &[0.5, 0.5]), "and opens it");
    }

    /// The handle's look is the caller's, then the theme's.
    #[test]
    fn the_handle_answers_to_its_theme_and_to_its_caller() {
        let line = Color::rgb(0.4, 0.1, 0.7);
        let mut theme = Theme::default();
        theme.widgets.resizable_panels.line_color = Some(line);
        theme.widgets.resizable_panels.handle_size = Some(12.0);
        let ui = frame(&group(&ResizablePanelsState::new([0.5, 0.5])), &theme);
        assert!(rect_of(&ui, line).is_some(), "the theme's line");
        assert_eq!(
            rect_of(&ui, A).unwrap().width,
            198.0,
            "the theme's handle size"
        );
        let own = Color::rgb(0.1, 0.4, 0.4);
        let ui = frame(
            &group(&ResizablePanelsState::new([0.5, 0.5])).line_color(own),
            &theme,
        );
        assert!(rect_of(&ui, own).is_some() && rect_of(&ui, line).is_none());
    }
}
