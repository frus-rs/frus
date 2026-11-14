// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`SwipeActions`]: a list row that slides aside to show what can be done with it
//! (milestone 810).
//!
//! It is a [`Dismissible`](crate::Dismissible) that **stays**: the same shared gesture — a
//! sideways drag is the row's, an up-and-down one the list's — the same fling estimate,
//! the same item following the finger over what it uncovers. What differs is where it
//! comes to rest: open on its actions, or closed, and never gone.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use frus_core::{Color, Rect, Role, Scene, SemanticsProperties, TextStyle};
use frus_layout::{Align, Dimension, Justify, Style};

use crate::dismiss::{DismissDirection, DismissSpec, RevealRequest, RevealSpec};
use crate::icons::IconData;
use crate::interaction::{Status, WidgetId};
use crate::semantics::Semantics;
use crate::shortcuts::{KeyStroke, ShortcutKey};
use crate::stack::Stack;
use crate::theme::Theme;
use crate::widget::Widget;
use crate::{Container, Flex, Icon, Positioned, Text};

/// The width of one action, unless the row or its theme says otherwise.
pub const SWIPE_ACTION_WIDTH: f32 = 80.0;
/// How far a swipe must go, as a share of the row, to perform the primary action.
pub const SWIPE_FULL_THRESHOLD: f32 = 0.6;
/// A row's height, unless it is given one.
pub const SWIPE_ROW_HEIGHT: f32 = 64.0;

/// Defaults for [`SwipeActions`]. Each field unset is the default named beside it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SwipeActionsTheme {
    /// One action's width. Unset, [`SWIPE_ACTION_WIDTH`].
    pub action_width: Option<f32>,
    /// The share of the row a full swipe crosses. Unset, [`SWIPE_FULL_THRESHOLD`].
    pub full_threshold: Option<f32>,
    /// An action's fill, when it names none. Unset, the scheme's `secondary_container`.
    pub background: Option<Color>,
    /// An action's content, when it names none. Unset, the scheme's
    /// `on_secondary_container`.
    pub foreground: Option<Color>,
    /// An action's label type. Unset, the type scale's `label_medium`.
    pub label_style: Option<TextStyle>,
    /// An action's icon size. Unset, 22.
    pub icon_size: Option<f32>,
}

/// One action of a [`SwipeActions`] row.
///
/// ```
/// use frus_widgets::{Color, Icons, SwipeAction};
///
/// #[derive(Clone)]
/// enum Msg { Delete(u32) }
///
/// let _delete: SwipeAction<Msg> = SwipeAction::new("Delete", Msg::Delete(7))
///     .icon(Icons::DELETE)
///     .colors(Color::rgb(0.73, 0.1, 0.1), Color::WHITE);
/// ```
#[derive(Clone)]
pub struct SwipeAction<Msg = crate::callback::Callback> {
    label: String,
    icon: Option<IconData>,
    background: Option<Color>,
    foreground: Option<Color>,
    message: Msg,
}

impl<Msg> SwipeAction<Msg> {
    /// An action called `label`, sending `message` when performed.
    pub fn new(label: impl Into<String>, message: impl Into<Msg>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            background: None,
            foreground: None,
            message: message.into(),
        }
    }

    /// An icon above the label.
    #[must_use]
    pub fn icon(mut self, icon: IconData) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Its fill and its content's colour, over the theme's.
    #[must_use]
    pub fn colors(mut self, background: Color, foreground: Color) -> Self {
        self.background = Some(background);
        self.foreground = Some(foreground);
        self
    }
}

/// **A list row with actions behind it**, uncovered by a sideways swipe: *Archive* on one
/// side, *Delete* on the other.
///
/// ```
/// use frus_widgets::{Icons, SwipeAction, SwipeActions, Text};
///
/// #[derive(Clone)]
/// enum Msg { Archive(u32), Delete(u32), Pin(u32) }
///
/// let id = 7;
/// let _row: SwipeActions<Msg> = SwipeActions::new(Text::new("A message from Ada"))
///     .leading(SwipeAction::new("Pin", Msg::Pin(id)).icon(Icons::PUSH_PIN))
///     .trailing(SwipeAction::new("Delete", Msg::Delete(id)).icon(Icons::DELETE))
///     .trailing(SwipeAction::new("Archive", Msg::Archive(id)).icon(Icons::ARCHIVE))
///     .group(1)
///     .height(72.0);
/// ```
///
/// - **Leading** actions are on the reading start side — the left, or the right in a
///   right-to-left interface — and uncovered by a swipe towards the end; **trailing** ones
///   the other way. The first of each side is its **primary** action and sits at the edge.
/// - Let go past half the actions and the row rests **open**; short of it, or flung back,
///   it springs closed. A tap on an action performs it and closes the row; a tap anywhere
///   else just closes it.
/// - A swipe past [`full_threshold`](SwipeActions::full_threshold) of the row performs the
///   primary action at once and springs back ([`full_swipe`](SwipeActions::full_swipe),
///   on by default).
/// - Rows given the same [`group`](SwipeActions::group) are open one at a time: starting to
///   swipe one closes the other.
/// - With the keyboard, the focused row opens with the arrow keys and closes with Escape,
///   and Tab reaches its actions once they show. A screen reader is offered every action
///   by name in its actions menu, without swiping.
///
/// Like a [`Dismissible`](crate::Dismissible) it lays its actions under its child, so it
/// needs a height: [`height`](SwipeActions::height), [`SWIPE_ROW_HEIGHT`] when unset.
pub struct SwipeActions<Msg = crate::callback::Callback> {
    child: RefCell<Option<Box<dyn Widget<Msg>>>>,
    leading: Vec<SwipeAction<Msg>>,
    trailing: Vec<SwipeAction<Msg>>,
    full_leading: bool,
    full_trailing: bool,
    group: Option<u64>,
    action_width: Option<f32>,
    full_threshold: Option<f32>,
    width: Dimension,
    height: f32,
    /// `[left actions, right actions, child]`, made on the first build, when the reading
    /// direction is known.
    children: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
    /// The swipe as the frame lays it out: physical sides.
    spec: Cell<DismissSpec>,
    rtl: Cell<bool>,
    id: Rc<Cell<Option<WidgetId>>>,
    keys: Rc<dyn Fn(KeyStroke) -> Option<Msg>>,
}

impl<Msg: Clone + 'static> SwipeActions<Msg> {
    /// A row showing `child`, with no actions yet.
    pub fn new(child: impl Widget<Msg> + 'static) -> Self {
        let id: Rc<Cell<Option<WidgetId>>> = Rc::new(Cell::new(None));
        let at = id.clone();
        // The keyboard's way in: the arrows open the row the way they point, Escape closes
        // it. Nothing is sent; the row moves on the next frame.
        let keys = Rc::new(move |stroke: KeyStroke| -> Option<Msg> {
            let id = at.get()?;
            if stroke.ctrl || stroke.alt || stroke.meta {
                return None;
            }
            let request = match stroke.key {
                // The row moves the way the arrow points, uncovering the other side.
                ShortcutKey::Left => RevealRequest::Open(false),
                ShortcutKey::Right => RevealRequest::Open(true),
                ShortcutKey::Escape => RevealRequest::Close,
                _ => return None,
            };
            crate::dismiss::request_reveal(id, request);
            None
        });
        Self {
            child: RefCell::new(Some(Box::new(child))),
            leading: Vec::new(),
            trailing: Vec::new(),
            full_leading: true,
            full_trailing: true,
            group: None,
            action_width: None,
            full_threshold: None,
            width: Dimension::Auto,
            height: SWIPE_ROW_HEIGHT,
            children: OnceCell::new(),
            spec: Cell::new(DismissSpec::default()),
            rtl: Cell::new(false),
            id,
            keys,
        }
    }

    /// Adds an action on the reading start side. The first is the primary one.
    #[must_use]
    pub fn leading(mut self, action: SwipeAction<Msg>) -> Self {
        self.leading.push(action);
        self
    }

    /// Adds an action on the reading end side. The first is the primary one.
    #[must_use]
    pub fn trailing(mut self, action: SwipeAction<Msg>) -> Self {
        self.trailing.push(action);
        self
    }

    /// Whether a swipe all the way performs the primary action: of the leading side, and
    /// of the trailing side. On for both by default.
    #[must_use]
    pub fn full_swipe(mut self, leading: bool, trailing: bool) -> Self {
        self.full_leading = leading;
        self.full_trailing = trailing;
        self
    }

    /// The rows of which only one is open at a time: give every row of a list the same
    /// number.
    #[must_use]
    pub fn group(mut self, group: u64) -> Self {
        self.group = Some(group);
        self
    }

    /// One action's width, over the theme's.
    #[must_use]
    pub fn action_width(mut self, width: f32) -> Self {
        self.action_width = Some(width);
        self
    }

    /// The share of the row a full swipe crosses, over the theme's.
    #[must_use]
    pub fn full_threshold(mut self, threshold: f32) -> Self {
        self.full_threshold = Some(threshold.clamp(0.1, 1.0));
        self
    }

    /// The row's height. Unset, [`SWIPE_ROW_HEIGHT`].
    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// A fixed width. Unset, the room on offer.
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.width = Dimension::Length(width);
        self
    }

    /// The actions on the physical left and right, primary at the edge: leading on the
    /// left in a left-to-right interface.
    fn sides(&self, rtl: bool) -> (&[SwipeAction<Msg>], &[SwipeAction<Msg>], bool, bool) {
        if rtl {
            (
                &self.trailing,
                &self.leading,
                self.full_trailing,
                self.full_leading,
            )
        } else {
            (
                &self.leading,
                &self.trailing,
                self.full_leading,
                self.full_trailing,
            )
        }
    }

    /// The message of action `index`, counting the leading ones first.
    fn action(&self, index: usize) -> Option<&SwipeAction<Msg>> {
        self.leading.iter().chain(&self.trailing).nth(index)
    }
}

/// One side's actions, laid out against its physical edge, primary outermost, over a band
/// of the primary's colour that a full swipe uncovers.
fn panel<Msg: Clone + 'static>(
    actions: &[SwipeAction<Msg>],
    left: bool,
    width: f32,
    theme: &Theme,
) -> Box<dyn Widget<Msg>> {
    let t = theme.widgets.swipe_actions;
    let s = &theme.scheme;
    let background = |a: &SwipeAction<Msg>| {
        a.background
            .or(t.background)
            .unwrap_or(s.secondary_container)
    };
    let mut stack = Stack::new();
    if let Some(primary) = actions.first() {
        stack = stack.layer(Positioned::fill(
            Container::new().color(background(primary)),
        ));
    }
    for (i, action) in actions.iter().enumerate() {
        let fg = action
            .foreground
            .or(t.foreground)
            .unwrap_or(s.on_secondary_container);
        let style = t
            .label_style
            .unwrap_or(crate::theme::type_scale(Some(theme)).label_medium)
            .color(fg);
        let mut column = Flex::column()
            .gap(4.0)
            .align(Align::Center)
            .justify(Justify::Center);
        if let Some(icon) = action.icon {
            column = column.child(Icon::new(icon).size(t.icon_size.unwrap_or(22.0)).color(fg));
        }
        column = column.child(Text::styled(action.label.clone(), style).no_wrap());
        let button = Semantics::new(
            SemanticsProperties::new(Role::Button)
                .label(action.label.clone())
                .clickable(),
            Container::new()
                .color(background(action))
                .alignment(frus_core::Alignment::CENTER)
                .on_click(action.message.clone())
                .child(column),
        )
        .merging(true);
        let offset = i as f32 * width;
        let placed = Positioned::new(button).top(0.0).bottom(0.0).width(width);
        stack = stack.layer(if left {
            placed.left(offset)
        } else {
            placed.right(offset)
        });
    }
    Box::new(stack)
}

impl<Msg: Clone + 'static> Widget<Msg> for SwipeActions<Msg> {
    /// Learns the reading direction and the theme, lays the two sides out against their
    /// physical edges, and remembers who it is for the keyboard.
    fn build_in(&self, id: WidgetId, _runtime: &crate::runtime::Runtime, theme: &Theme) {
        self.id.set(Some(id));
        let rtl = theme.direction == frus_core::TextDirection::Rtl;
        self.rtl.set(rtl);
        let t = theme.widgets.swipe_actions;
        let width = self
            .action_width
            .or(t.action_width)
            .unwrap_or(SWIPE_ACTION_WIDTH);
        let (left, right, full_left, full_right) = self.sides(rtl);
        self.spec.set(DismissSpec {
            reveal: Some(RevealSpec {
                left: left.len() as f32 * width,
                right: right.len() as f32 * width,
                full_left: full_left && !left.is_empty(),
                full_right: full_right && !right.is_empty(),
                full_threshold: self
                    .full_threshold
                    .or(t.full_threshold)
                    .unwrap_or(SWIPE_FULL_THRESHOLD),
                group: self.group,
            }),
            ..DismissSpec::default()
        });
        self.children.get_or_init(|| {
            let child = self
                .child
                .borrow_mut()
                .take()
                .unwrap_or_else(|| Box::new(Flex::column()));
            vec![
                panel(left, true, width, theme),
                panel(right, false, width, theme),
                child,
            ]
        });
    }

    fn style(&self) -> Style {
        Style {
            width: self.width,
            height: Dimension::Length(self.height),
            ..Default::default()
        }
    }

    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        crate::widget::FillAxes {
            horizontal: matches!(self.width, Dimension::Auto),
            vertical: false,
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.children.get().map_or(&[], Vec::as_slice)
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn stack(&self) -> bool {
        true
    }

    fn dismissible(&self) -> Option<DismissSpec> {
        Some(self.spec.get())
    }

    /// A full swipe's primary action: the side the row uncovered, read back into leading
    /// and trailing.
    fn on_dismissed(&self, direction: DismissDirection) -> Option<Msg> {
        // Moved to the right uncovers the left side: leading, unless right to left.
        let uncovered_left = direction == DismissDirection::ToEnd;
        let leading = uncovered_left != self.rtl.get();
        let side = if leading {
            &self.leading
        } else {
            &self.trailing
        };
        side.first().map(|a| a.message.clone())
    }

    /// The row and what is in it, read as one item, offering every action by name.
    fn describes(&self) -> Option<crate::semantics::Description> {
        let mut props = SemanticsProperties::new(Role::ListItem);
        for action in self.leading.iter().chain(&self.trailing) {
            props = props.custom_action(action.label.clone());
        }
        Some(crate::semantics::Description {
            props,
            merging: true,
        })
    }

    fn on_custom_action(&self, index: usize) -> Option<Msg> {
        self.action(index).map(|a| a.message.clone())
    }

    fn focusable(&self) -> bool {
        !(self.leading.is_empty() && self.trailing.is_empty())
    }

    fn on_keystroke(&self) -> Option<Rc<dyn Fn(KeyStroke) -> Option<Msg>>> {
        Some(self.keys.clone())
    }

    fn debug_name(&self) -> &'static str {
        "SwipeActions"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dismiss::{DismissPhase, Dismissable};
    use crate::{build_ui, Icons, Runtime, Size};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Pin,
        Delete,
        Archive,
    }

    const W: f32 = 400.0;

    fn row() -> SwipeActions<Msg> {
        SwipeActions::new(Text::new("A message from Ada"))
            .leading(SwipeAction::new("Pin", Msg::Pin).icon(Icons::PUSH_PIN))
            .trailing(SwipeAction::new("Delete", Msg::Delete).icon(Icons::DELETE))
            .trailing(SwipeAction::new("Archive", Msg::Archive))
            .group(1)
            .height(72.0)
    }

    fn item(row: &SwipeActions<Msg>, theme: &Theme) -> Dismissable {
        row.build_in(WidgetId::ROOT, &Runtime::default(), theme);
        Dismissable {
            id: WidgetId::ROOT,
            rect: Rect::new(0.0, 0.0, W, 72.0),
            spec: row.spec.get(),
        }
    }

    fn settle(runtime: &mut Runtime, items: &[Dismissable]) -> Vec<(WidgetId, DismissDirection)> {
        let mut fired = Vec::new();
        for _ in 0..60 {
            let (_, done) = runtime.advance_dismiss(items, 1.0 / 60.0);
            fired.extend(done);
        }
        fired
    }

    /// **The sides are physical and follow the reading direction**: leading on the left
    /// left to right, on the right right to left.
    #[test]
    fn the_sides_mirror_in_rtl() {
        let ltr = item(&row(), &Theme::default()).spec.reveal.unwrap();
        assert_eq!((ltr.left, ltr.right), (80.0, 160.0));
        let rtl = item(&row(), &Theme::default().rtl()).spec.reveal.unwrap();
        assert_eq!((rtl.left, rtl.right), (160.0, 80.0));
        // A full swipe to the right performs the left side's primary: Pin left to right,
        // Delete right to left.
        let r = row();
        r.build_in(WidgetId::ROOT, &Runtime::default(), &Theme::default());
        assert_eq!(r.on_dismissed(DismissDirection::ToEnd), Some(Msg::Pin));
        let r = row();
        r.build_in(WidgetId::ROOT, &Runtime::default(), &Theme::default().rtl());
        assert_eq!(r.on_dismissed(DismissDirection::ToEnd), Some(Msg::Delete));
    }

    /// **Past half its actions the row rests open; short of it, it springs back.**
    #[test]
    fn a_swipe_opens_or_springs_back() {
        let it = item(&row(), &Theme::default());
        let mut runtime = Runtime::default();
        runtime.swipe_drag(&it, -100.0);
        runtime.swipe_release(&it, 0.0, 0.0, false);
        settle(&mut runtime, &[it]);
        let state = runtime.dismiss.get(&it.id).copied().expect("open");
        assert_eq!(state.phase(), DismissPhase::Open);
        assert!(
            (state.progress() + 160.0 / W).abs() < 1e-4,
            "rests on its actions"
        );
        assert_eq!(runtime.open_swipe_rows(), vec![it.id]);

        let mut runtime = Runtime::default();
        runtime.swipe_drag(&it, -60.0);
        runtime.swipe_release(&it, 0.0, 0.0, false);
        settle(&mut runtime, &[it]);
        assert!(runtime.dismiss.is_empty(), "sprang back and let go");
    }

    /// **A full swipe performs the primary action and springs back**; a row with no
    /// full swipe stops at its actions however far the finger goes.
    #[test]
    fn a_full_swipe_performs_the_primary_and_closes() {
        let it = item(&row(), &Theme::default());
        let mut runtime = Runtime::default();
        runtime.swipe_drag(&it, -300.0);
        runtime.swipe_release(&it, 0.0, 0.0, false);
        let fired = settle(&mut runtime, &[it]);
        assert_eq!(fired, vec![(it.id, DismissDirection::ToStart)]);
        let r = row();
        r.build_in(WidgetId::ROOT, &Runtime::default(), &Theme::default());
        assert_eq!(r.on_dismissed(DismissDirection::ToStart), Some(Msg::Delete));
        assert!(runtime.dismiss.is_empty(), "and closed again");

        let short = item(&row().full_swipe(false, false), &Theme::default());
        let mut runtime = Runtime::default();
        runtime.swipe_drag(&short, -300.0);
        assert!(
            (runtime.swipe_progress(short.id) + 160.0 / W).abs() < 1e-4,
            "stops at its actions"
        );
        runtime.swipe_release(&short, 0.0, 0.0, false);
        assert!(
            settle(&mut runtime, &[short]).is_empty(),
            "nothing performed"
        );
        // A side with no actions refuses the swipe outright.
        let one_sided = item(
            &SwipeActions::new(Text::new("x")).trailing(SwipeAction::new("Delete", Msg::Delete)),
            &Theme::default(),
        );
        let mut runtime = Runtime::default();
        runtime.swipe_drag(&one_sided, 120.0);
        assert_eq!(runtime.swipe_progress(one_sided.id), 0.0);
    }

    /// **One open row per group**: starting to swipe another closes the first.
    #[test]
    fn only_one_row_of_a_group_is_open() {
        let theme = Theme::default();
        let a = item(&row(), &theme);
        let mut b = item(&row(), &theme);
        b.id = WidgetId::ROOT.child(1);
        b.rect.y = 72.0;
        let mut runtime = Runtime::default();
        runtime.swipe_drag(&a, -120.0);
        runtime.swipe_release(&a, 0.0, 0.0, false);
        settle(&mut runtime, &[a, b]);
        assert_eq!(runtime.open_swipe_rows(), vec![a.id]);
        runtime.swipe_drag(&b, -120.0);
        runtime.swipe_release(&b, 0.0, 0.0, false);
        settle(&mut runtime, &[a, b]);
        assert_eq!(
            runtime.open_swipe_rows(),
            vec![b.id],
            "a closed when b opened"
        );
        runtime.close_swipe_rows();
        settle(&mut runtime, &[a, b]);
        assert!(runtime.open_swipe_rows().is_empty());
    }

    /// **The keyboard opens and closes the row**: the arrow points the way the row moves.
    #[test]
    fn the_arrows_open_and_escape_closes() {
        let r = row();
        let it = item(&r, &Theme::default());
        let keys = r.on_keystroke().expect("a listener");
        assert_eq!(
            keys(KeyStroke::new(ShortcutKey::Left)),
            None,
            "sends nothing"
        );
        let mut runtime = Runtime::default();
        settle(&mut runtime, &[it]);
        assert!((runtime.swipe_progress(it.id) + 160.0 / W).abs() < 1e-4);
        keys(KeyStroke::new(ShortcutKey::Escape));
        settle(&mut runtime, &[it]);
        assert_eq!(runtime.swipe_progress(it.id), 0.0);
    }

    /// **A screen reader is offered every action by name**, on one node for the row, and
    /// performing one sends its message.
    #[test]
    fn the_actions_are_named_actions_of_the_row() {
        let r = row();
        let ui = build_ui(
            &r,
            Size::new(W, 72.0),
            &Runtime::default(),
            &Theme::default(),
        );
        let (id, _, props) = ui
            .semantics()
            .iter()
            .find(|(_, _, s)| !s.custom_actions.is_empty())
            .expect("a row with actions")
            .clone();
        assert_eq!(props.custom_actions, ["Pin", "Delete", "Archive"]);
        assert_eq!(props.label.as_deref(), Some("A message from Ada"));
        let widget = crate::ui::find_widget(&r, id).expect("the row");
        assert_eq!(widget.on_custom_action(1), Some(Msg::Delete));
        assert_eq!(widget.on_custom_action(3), None);
    }
}
