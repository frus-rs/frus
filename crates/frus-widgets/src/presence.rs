// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Presence**: a child that animates out before it goes (milestone 785).
//!
//! The view here is rebuilt from the application's state every frame, so a child the
//! state no longer has is simply not in the next tree — there is nothing left to animate.
//! What can be kept is the **value** the child was made from, as
//! [`AnimatedSwitcher`](crate::AnimatedSwitcher) keeps its values: the runtime holds it
//! under the widget's identity while the child plays its exit, then lets it go, with the
//! subtree and everything the subtree kept.
//!
//! - [`AnimatedPresence`]: one child, shown while it is wanted, with an `enter` and an
//!   `exit` [`Transition`].
//! - [`PresenceList`]: a keyed list whose removed items leave where they were, the items
//!   under them closing the gap, and whose new items arrive — staggered, if asked.
//! - [`Staggered`]: children that arrive one after another when they first appear.
//!
//! A child on its way out takes no input and is not announced: a tap on it would be a tap
//! on a state the application has already left. Once its exit is over it is gone from the
//! tree. With the motion turned down (reduced motion), children come and go at once.

use std::any::Any;
use std::cell::{OnceCell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use frus_core::{Curve, Rect, Scene, TextDirection};
use frus_layout::Style;

use crate::interaction::{Status, WidgetId};
use crate::motionpatterns::{MotionPose, MotionPreset, Transition};
use crate::runtime::Runtime;
use crate::switcher::{Shown, Timing};
use crate::theme::Theme;
use crate::widget::Widget;

type Build<Msg> = Box<dyn Fn(&dyn Any) -> Option<Box<dyn Widget<Msg>>>>;

fn same<T: PartialEq + 'static>(a: &dyn Any, b: &dyn Any) -> bool {
    match (a.downcast_ref::<T>(), b.downcast_ref::<T>()) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// The timing a pair of transitions plays on under `theme`: the enter's duration and
/// curve, the exit's — its curve read in time, so an accelerating exit starts slowly.
fn timing(enter: &Transition, exit: &Transition, theme: Option<&Theme>) -> Timing {
    let motion = theme.map_or(&crate::MotionTheme::M3, |theme| &*theme.motion);
    let (duration, switch_in) = enter.resolve(motion);
    let (reverse, out) = exit.resolve(motion);
    Timing {
        duration,
        reverse,
        switch_in,
        // The runtime hands a leaving child its clock running down, `1` to `0`. Flipped,
        // the curve describes the exit as it unfolds in time.
        switch_out: Curve::Flipped(Box::new(out)),
    }
}

/// Wraps a child in its transition: the same chain whatever its state, so that it keeps
/// its identity from arriving to leaving and back.
fn present<Msg: Clone + 'static>(
    child: Box<dyn Widget<Msg>>,
    shown: &Shown,
    enter: &Transition,
    exit: &Transition,
    rtl: bool,
) -> Box<dyn Widget<Msg>> {
    let transition = if shown.leaving { exit } else { enter };
    let pose: MotionPose = transition.pose(shown.progress);
    let dx = if rtl { -pose.dx } else { pose.dx };
    let mut inner: Box<dyn Widget<Msg>> = Box::new(
        crate::Transform::translate(dx, pose.dy)
            .and_scale(pose.scale)
            .child(child),
    );
    if let Some(axis) = enter.size.or(exit.size) {
        let factor = if transition.size.is_some() {
            shown.progress
        } else {
            1.0
        };
        inner = Box::new(
            crate::SizeTransition::new(factor.clamp(0.0, 1.0), inner)
                .axis(axis)
                .axis_alignment(-1.0),
        );
    }
    let drawn = crate::Opacity::new(pose.opacity.clamp(0.0, 1.0), inner);
    Box::new(crate::Keyed::new(
        shown.serial,
        crate::barrier::ExcludeSemantics::new(
            crate::barrier::IgnorePointer::new(drawn).ignoring(shown.leaving),
        )
        .excluding(shown.leaving),
    ))
}

fn rtl(theme: Option<&Theme>) -> bool {
    theme.is_some_and(|theme| theme.direction == TextDirection::Rtl)
}

/// **One child that animates in when it appears and out before it goes.**
///
/// ```
/// use frus_widgets::{AnimatedPresence, MotionPreset, Text, Transition};
///
/// let saved = true;
/// let _note: AnimatedPresence<()> = AnimatedPresence::new(saved, Text::new("Saved"))
///     .enter(Transition::preset(MotionPreset::EmphasizedEnter).slide(0.0, 16.0))
///     .exit(Transition::preset(MotionPreset::EmphasizedExit).scale(0.9));
/// ```
///
/// [`new`](Self::new) takes a flag and the child, built every frame like any other.
/// [`of`](Self::of) takes an `Option` and a way to build a child from its value, for a
/// child whose data is gone the moment it is removed — a deleted message, a dismissed
/// error: the presence keeps the value for as long as the exit lasts. A different value
/// takes the place of the one before, the old one leaving as the new one arrives.
///
/// The child is shown as it is the first time the presence is built, unless
/// [`appear`](Self::appear) asks for an entrance. While it leaves it takes no input and is
/// not announced; once it has left it is gone from the tree, with what it kept.
pub struct AnimatedPresence<Msg = crate::callback::Callback> {
    value: Rc<dyn Any>,
    same: fn(&dyn Any, &dyn Any) -> bool,
    build: Build<Msg>,
    enter: Transition,
    exit: Transition,
    appear: bool,
    built: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> AnimatedPresence<Msg> {
    /// Shows `child` while `visible`, fading it in and out on the standard enter and exit
    /// presets.
    pub fn new(visible: bool, child: impl Widget<Msg> + 'static) -> Self {
        let child: RefCell<Option<Box<dyn Widget<Msg>>>> = RefCell::new(Some(Box::new(child)));
        Self::with_build(
            Rc::new(visible),
            same::<bool>,
            Box::new(move |value: &dyn Any| {
                if *value.downcast_ref::<bool>()? {
                    child.borrow_mut().take()
                } else {
                    None
                }
            }),
        )
    }

    /// Shows the child `build` makes from `value` while there is one; keeps the value for
    /// as long as its child takes to leave.
    pub fn of<T, W>(value: Option<T>, build: impl Fn(&T) -> W + 'static) -> Self
    where
        T: PartialEq + 'static,
        W: Widget<Msg> + 'static,
    {
        Self::with_build(
            Rc::new(value),
            same::<Option<T>>,
            Box::new(move |kept: &dyn Any| {
                let value = kept.downcast_ref::<Option<T>>()?.as_ref()?;
                Some(Box::new(build(value)) as Box<dyn Widget<Msg>>)
            }),
        )
    }

    fn with_build(
        value: Rc<dyn Any>,
        same: fn(&dyn Any, &dyn Any) -> bool,
        build: Build<Msg>,
    ) -> Self {
        Self {
            value,
            same,
            build,
            enter: Transition::preset(MotionPreset::StandardEnter),
            exit: Transition::preset(MotionPreset::StandardExit),
            appear: false,
            built: OnceCell::new(),
        }
    }

    /// How the child arrives. A fade on [`MotionPreset::StandardEnter`] by default.
    pub fn enter(mut self, transition: Transition) -> Self {
        self.enter = transition;
        self
    }

    /// How the child leaves. A fade on [`MotionPreset::StandardExit`] by default.
    pub fn exit(mut self, transition: Transition) -> Self {
        self.exit = transition;
        self
    }

    /// Whether a child there the first time the presence is built plays its entrance too.
    /// `false` by default: what is on the screen when a page opens is simply there.
    pub fn appear(mut self, appear: bool) -> Self {
        self.appear = appear;
        self
    }

    fn compose(&self, at: Option<(WidgetId, &Runtime, &Theme)>) -> Vec<Box<dyn Widget<Msg>>> {
        let theme = at.map(|(_, _, theme)| theme);
        let shown = match at {
            Some((id, runtime, theme)) => runtime.presence_to(
                id,
                self.value.clone(),
                self.same,
                timing(&self.enter, &self.exit, Some(theme)),
                self.appear,
            ),
            None => vec![Shown {
                serial: 0,
                value: self.value.clone(),
                progress: 1.0,
                leaving: false,
            }],
        };
        let rtl = rtl(theme);
        // The child built from the current value is the newest entry: build it last, so a
        // flag's single child goes to the entry that stands for it.
        let layers = shown
            .iter()
            .filter_map(|shown| {
                let child = (self.build)(shown.value.as_ref())?;
                Some(present(child, shown, &self.enter, &self.exit, rtl))
            })
            .collect();
        vec![Box::new(crate::switcher::Overlap { layers })]
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for AnimatedPresence<Msg> {
    fn build_in(&self, id: WidgetId, runtime: &Runtime, theme: &Theme) {
        self.built
            .get_or_init(|| self.compose(Some((id, runtime, theme))));
    }

    fn switches(&self) -> bool {
        true
    }

    fn style(&self) -> Style {
        Style::default()
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.built.get_or_init(|| self.compose(None))
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "AnimatedPresence"
    }
}

fn hash_key<K: Hash>(key: &K) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

/// **A list whose items animate in and out**: an item removed from the list plays its
/// exit where it was — the items under it closing up as it goes — and an item added plays
/// its entrance.
///
/// ```
/// use frus_widgets::{PresenceList, Text};
///
/// #[derive(Clone, PartialEq)]
/// struct Task { id: u32, title: String }
///
/// let tasks = vec![Task { id: 1, title: "Write".into() }, Task { id: 2, title: "Ship".into() }];
/// let _list: PresenceList<()> =
///     PresenceList::new(tasks, |task| task.id, |task| Text::new(task.title.clone()))
///         .stagger(0.05);
/// ```
///
/// Items are told apart by their **key**, not their place, so an item moved stays the same
/// item and keeps its state. An item is kept, by value, for as long as its exit lasts.
/// By default an item fades and unfolds in on [`MotionPreset::StandardEnter`] and fades
/// and folds away on [`MotionPreset::StandardExit`]; [`enter`](Self::enter) and
/// [`exit`](Self::exit) change that. Items that arrive together arrive
/// [`stagger`](Self::stagger) seconds apart.
///
/// Put the room between items inside them (a padding) rather than in a gap: an item
/// folding away takes its own room with it, and a gap would stay until it has gone.
pub struct PresenceList<Msg = crate::callback::Callback> {
    items: Vec<(u64, Rc<dyn Any>)>,
    build: Build<Msg>,
    enter: Transition,
    exit: Transition,
    stagger: f32,
    appear: bool,
    horizontal: bool,
    built: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> PresenceList<Msg> {
    /// A column of the children `build` makes from `items`, each told apart by `key`.
    pub fn new<T, K, W>(
        items: impl IntoIterator<Item = T>,
        key: impl Fn(&T) -> K,
        build: impl Fn(&T) -> W + 'static,
    ) -> Self
    where
        T: 'static,
        K: Hash,
        W: Widget<Msg> + 'static,
    {
        Self {
            items: items
                .into_iter()
                .map(|item| (hash_key(&key(&item)), Rc::new(item) as Rc<dyn Any>))
                .collect(),
            build: Box::new(move |kept: &dyn Any| {
                let item = kept.downcast_ref::<T>()?;
                Some(Box::new(build(item)) as Box<dyn Widget<Msg>>)
            }),
            enter: Transition::preset(MotionPreset::StandardEnter).size(),
            exit: Transition::preset(MotionPreset::StandardExit).size(),
            stagger: 0.0,
            appear: false,
            horizontal: false,
            built: OnceCell::new(),
        }
    }

    /// How an item arrives.
    pub fn enter(mut self, transition: Transition) -> Self {
        self.enter = transition;
        self
    }

    /// How an item leaves.
    pub fn exit(mut self, transition: Transition) -> Self {
        self.exit = transition;
        self
    }

    /// How long each item that arrives with others waits after the one before it, in
    /// seconds: a staggered entrance. `0` by default, all at once.
    pub fn stagger(mut self, seconds: f32) -> Self {
        self.stagger = seconds.max(0.0);
        self
    }

    /// Whether the items there the first time the list is built play their entrance —
    /// staggered, with [`stagger`](Self::stagger). `false` by default.
    pub fn appear(mut self, appear: bool) -> Self {
        self.appear = appear;
        self
    }

    /// Lays the items out in a row rather than a column.
    pub fn horizontal(mut self) -> Self {
        self.horizontal = true;
        self
    }

    fn compose(&self, at: Option<(WidgetId, &Runtime, &Theme)>) -> Vec<Box<dyn Widget<Msg>>> {
        let theme = at.map(|(_, _, theme)| theme);
        let shown = match at {
            Some((id, runtime, theme)) => runtime.list_to(
                id,
                self.items.clone(),
                timing(&self.enter, &self.exit, Some(theme)),
                self.stagger,
                self.appear,
            ),
            None => self
                .items
                .iter()
                .enumerate()
                .map(|(at, (_, value))| Shown {
                    serial: at as u64,
                    value: value.clone(),
                    progress: 1.0,
                    leaving: false,
                })
                .collect(),
        };
        let rtl = rtl(theme);
        let mut flex = if self.horizontal {
            crate::Flex::row()
        } else {
            crate::Flex::column()
        };
        for shown in &shown {
            if let Some(child) = (self.build)(shown.value.as_ref()) {
                flex = flex.child_boxed(present(child, shown, &self.enter, &self.exit, rtl));
            }
        }
        vec![Box::new(flex)]
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for PresenceList<Msg> {
    fn build_in(&self, id: WidgetId, runtime: &Runtime, theme: &Theme) {
        self.built
            .get_or_init(|| self.compose(Some((id, runtime, theme))));
    }

    fn switches(&self) -> bool {
        true
    }

    fn style(&self) -> Style {
        Style::default()
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.built.get_or_init(|| self.compose(None))
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "PresenceList"
    }
}

/// **Children that arrive one after another** when they first appear: a staggered
/// entrance for a list, a grid of cards, the fields of a form.
///
/// ```
/// use frus_widgets::{MotionPreset, Staggered, Text, Transition};
///
/// let _intro: Staggered<()> = Staggered::column()
///     .child(Text::new("Welcome"))
///     .child(Text::new("Three things to know"))
///     .child(Text::new("Let's start"))
///     .interval(0.06)
///     .enter(Transition::preset(MotionPreset::EmphasizedEnter).slide(0.0, 24.0));
/// ```
///
/// Each child waits [`interval`](Self::interval) seconds after the one before it (50 ms by
/// default), then plays [`enter`](Self::enter) — a fade and a 16 px rise on
/// [`MotionPreset::EmphasizedEnter`] by default. It plays once, when the children are first
/// built; a child added later arrives the same way, after the others added with it. A
/// child taken away goes at once: for exits, see [`PresenceList`].
pub struct Staggered<Msg = crate::callback::Callback> {
    children: RefCell<Vec<Option<Box<dyn Widget<Msg>>>>>,
    enter: Transition,
    interval: f32,
    horizontal: bool,
    gap: f32,
    built: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> Staggered<Msg> {
    /// Children one under another.
    pub fn column() -> Self {
        Self {
            children: RefCell::new(Vec::new()),
            enter: Transition::preset(MotionPreset::EmphasizedEnter).slide(0.0, 16.0),
            interval: 0.05,
            horizontal: false,
            gap: 0.0,
            built: OnceCell::new(),
        }
    }

    /// Children side by side.
    pub fn row() -> Self {
        Self {
            horizontal: true,
            ..Self::column()
        }
    }

    /// Adds a child.
    pub fn child(self, child: impl Widget<Msg> + 'static) -> Self {
        self.children.borrow_mut().push(Some(Box::new(child)));
        self
    }

    /// Adds the children of `children`.
    pub fn children(self, children: impl IntoIterator<Item = Box<dyn Widget<Msg>>>) -> Self {
        self.children
            .borrow_mut()
            .extend(children.into_iter().map(Some));
        self
    }

    /// How long each child waits after the one before it, in seconds.
    pub fn interval(mut self, seconds: f32) -> Self {
        self.interval = seconds.max(0.0);
        self
    }

    /// How each child arrives.
    pub fn enter(mut self, transition: Transition) -> Self {
        self.enter = transition;
        self
    }

    /// The room between two children, in logical pixels.
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    fn compose(&self, at: Option<(WidgetId, &Runtime, &Theme)>) -> Vec<Box<dyn Widget<Msg>>> {
        let theme = at.map(|(_, _, theme)| theme);
        let count = self.children.borrow().len();
        let items: Vec<(u64, Rc<dyn Any>)> = (0..count)
            .map(|at| (at as u64, Rc::new(at) as Rc<dyn Any>))
            .collect();
        // Leaving takes no time: a child taken away has no value to be built again from.
        let exit = Transition::none();
        let shown = match at {
            Some((id, runtime, theme)) => runtime.list_to(
                id,
                items,
                timing(&self.enter, &exit, Some(theme)),
                self.interval,
                true,
            ),
            None => items
                .into_iter()
                .enumerate()
                .map(|(at, (_, value))| Shown {
                    serial: at as u64,
                    value,
                    progress: 1.0,
                    leaving: false,
                })
                .collect(),
        };
        let rtl = rtl(theme);
        let mut flex = if self.horizontal {
            crate::Flex::row()
        } else {
            crate::Flex::column()
        }
        .gap(self.gap);
        let mut children = self.children.borrow_mut();
        for shown in &shown {
            let Some(&index) = shown.value.downcast_ref::<usize>() else {
                continue;
            };
            if let Some(child) = children.get_mut(index).and_then(Option::take) {
                flex = flex.child_boxed(present(child, shown, &self.enter, &exit, rtl));
            }
        }
        vec![Box::new(flex)]
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for Staggered<Msg> {
    fn build_in(&self, id: WidgetId, runtime: &Runtime, theme: &Theme) {
        self.built
            .get_or_init(|| self.compose(Some((id, runtime, theme))));
    }

    fn switches(&self) -> bool {
        true
    }

    fn style(&self) -> Style {
        Style::default()
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.built.get_or_init(|| self.compose(None))
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "Staggered"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{build_ui, Ui};
    use crate::{Container, Flex, Size};
    use frus_core::{Color, Point, Primitive};

    const SIZE: Size = Size::new(200.0, 200.0);

    fn red() -> Color {
        Color::rgb(1.0, 0.0, 0.0)
    }

    fn tile(color: Color, msg: i32) -> Container<i32> {
        Container::new()
            .width(100.0)
            .height(20.0)
            .color(color)
            .on_click(msg)
    }

    /// Every filled rectangle, with the opacity it is drawn at through the layers above it.
    fn rects(ui: &Ui<i32>) -> Vec<(Rect, Color, f32)> {
        fn walk(primitives: &[Primitive], opacity: f32, out: &mut Vec<(Rect, Color, f32)>) {
            for primitive in primitives {
                match primitive {
                    Primitive::Rect { rect, color, .. } => out.push((*rect, *color, opacity)),
                    Primitive::Layer {
                        primitives,
                        opacity: layer,
                        ..
                    } => walk(primitives, opacity * layer, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(ui.scene().primitives(), 1.0, &mut out);
        out
    }

    fn opacity_of(ui: &Ui<i32>, color: Color) -> f32 {
        rects(ui)
            .iter()
            .filter(|(_, c, _)| *c == color)
            .map(|(_, _, o)| *o)
            .fold(0.0, f32::max)
    }

    fn message_at(ui: &Ui<i32>, point: Point) -> Option<i32> {
        ui.hit(point).and_then(|id| ui.msg_for(id))
    }

    fn presence(visible: bool) -> Flex<i32> {
        Flex::column().child(AnimatedPresence::new(visible, tile(red(), 7)))
    }

    /// One turn of the loop: built, then the clocks stepped.
    fn step(rt: &mut Runtime, tree: &dyn Widget<i32>, dt: f32) -> Ui<i32> {
        let ui = build_ui(tree, SIZE, rt, &Theme::dark());
        rt.advance_switchers(tree, dt);
        ui
    }

    /// **A child removed plays its exit, takes no tap while it does, and is then gone**:
    /// no picture, no hit, nothing kept for it in the runtime.
    #[test]
    fn a_removed_child_animates_out_then_is_gone_and_untappable() {
        let mut rt = Runtime::default();
        let ui = step(&mut rt, &presence(true), 0.0);
        assert_eq!(opacity_of(&ui, red()), 1.0, "shown as it is at first");
        assert_eq!(message_at(&ui, Point::new(10.0, 10.0)), Some(7));

        // Hidden: the exit is 200 ms on the standard exit preset.
        step(&mut rt, &presence(false), 0.0);
        step(&mut rt, &presence(false), 0.1);
        let ui = step(&mut rt, &presence(false), 0.0);
        let mid = opacity_of(&ui, red());
        assert!(mid > 0.0 && mid < 1.0, "halfway out: {mid}");
        assert_eq!(
            message_at(&ui, Point::new(10.0, 10.0)),
            None,
            "a leaving child takes no tap"
        );

        step(&mut rt, &presence(false), 0.15);
        let ui = step(&mut rt, &presence(false), 0.0);
        assert!(
            rects(&ui).iter().all(|(_, c, _)| *c != red()),
            "gone from the picture"
        );
        assert_eq!(
            message_at(&ui, Point::new(10.0, 10.0)),
            None,
            "and from input"
        );
        let switches = rt.switchers.borrow();
        assert_eq!(switches.len(), 1);
        assert!(
            switches.values().next().unwrap().is_empty_of_children(),
            "nothing of it is kept"
        );
    }

    /// **Shown again while it leaves, it turns round** from where it was, instead of a
    /// second copy arriving from nothing.
    #[test]
    fn a_child_shown_again_while_leaving_turns_round() {
        let mut rt = Runtime::default();
        step(&mut rt, &presence(true), 0.0);
        step(&mut rt, &presence(false), 0.0);
        step(&mut rt, &presence(false), 0.1);
        let leaving = opacity_of(&step(&mut rt, &presence(false), 0.0), red());
        let back = step(&mut rt, &presence(true), 0.0);
        assert_eq!(
            rects(&back).iter().filter(|(_, c, _)| *c == red()).count(),
            1,
            "one child, not two"
        );
        assert!(
            (opacity_of(&back, red()) - leaving).abs() < 0.2,
            "from where it was: {leaving} then {}",
            opacity_of(&back, red())
        );
        assert_eq!(message_at(&back, Point::new(10.0, 10.0)), Some(7));
    }

    /// **A value is kept for as long as its exit**: `of` builds the child of a value the
    /// application no longer has.
    #[test]
    fn a_value_is_kept_while_its_child_leaves() {
        let view = |value: Option<u8>| {
            Flex::column().child(AnimatedPresence::of(value, |_: &u8| tile(red(), 1)))
        };
        let mut rt = Runtime::default();
        step(&mut rt, &view(Some(3)), 0.0);
        step(&mut rt, &view(None), 0.0);
        step(&mut rt, &view(None), 0.05);
        let ui = step(&mut rt, &view(None), 0.0);
        assert!(opacity_of(&ui, red()) > 0.0, "built from the kept value");
    }

    /// **With the motion turned down, a child goes at once.**
    #[test]
    fn with_reduced_motion_a_child_goes_at_once() {
        let mut rt = Runtime::default();
        rt.still = true;
        step(&mut rt, &presence(true), 0.0);
        let ui = step(&mut rt, &presence(false), 0.0);
        assert_eq!(opacity_of(&ui, red()), 0.0);
    }

    fn color(n: u8) -> Color {
        let shade = n as f32 / 10.0;
        Color::rgb(shade, 0.0, 1.0 - shade)
    }

    fn list(items: &[u8]) -> PresenceList<i32> {
        PresenceList::new(items.to_vec(), |n| *n, |n: &u8| tile(color(*n), *n as i32))
    }

    fn y_of(ui: &Ui<i32>, n: u8) -> Option<f32> {
        rects(ui)
            .iter()
            .find(|(_, c, _)| *c == color(n))
            .map(|(r, _, _)| r.y)
    }

    /// **An item removed from a list leaves where it was**, the items under it moving up
    /// as it folds away; then it is gone, and the item under it takes its place.
    #[test]
    fn a_list_item_removed_folds_away_where_it_was() {
        let mut rt = Runtime::default();
        let ui = step(&mut rt, &list(&[1, 2, 3]), 0.0);
        assert_eq!(y_of(&ui, 3), Some(40.0));
        step(&mut rt, &list(&[1, 3]), 0.0);
        step(&mut rt, &list(&[1, 3]), 0.1);
        let ui = step(&mut rt, &list(&[1, 3]), 0.0);
        let y2 = y_of(&ui, 2).expect("still there, leaving");
        let y3 = y_of(&ui, 3).unwrap();
        assert_eq!(y2, 20.0, "in its place");
        assert!(y3 > 20.0 && y3 < 40.0, "the one under it moving up: {y3}");
        assert_eq!(message_at(&ui, Point::new(10.0, 22.0)), None);
        step(&mut rt, &list(&[1, 3]), 0.2);
        let ui = step(&mut rt, &list(&[1, 3]), 0.0);
        assert_eq!(y_of(&ui, 2), None, "gone");
        assert_eq!(y_of(&ui, 3), Some(20.0));
        assert_eq!(message_at(&ui, Point::new(10.0, 25.0)), Some(3));
    }

    /// **Items arriving together are staggered**: the second waits its turn.
    #[test]
    fn items_arriving_together_arrive_one_after_another() {
        let mut rt = Runtime::default();
        let staggered = |items: &[u8]| list(items).stagger(0.1);
        step(&mut rt, &staggered(&[1]), 0.0);
        step(&mut rt, &staggered(&[1, 2, 3]), 0.0);
        step(&mut rt, &staggered(&[1, 2, 3]), 0.08);
        let ui = step(&mut rt, &staggered(&[1, 2, 3]), 0.0);
        assert!(
            opacity_of(&ui, color(2)) > 0.0,
            "the first to arrive has started"
        );
        assert_eq!(opacity_of(&ui, color(3)), 0.0, "the second waits");
        step(&mut rt, &staggered(&[1, 2, 3]), 0.1);
        let ui = step(&mut rt, &staggered(&[1, 2, 3]), 0.0);
        assert!(opacity_of(&ui, color(3)) > 0.0, "then it starts");
    }

    /// **A staggered entrance plays the first time**, child by child.
    #[test]
    fn staggered_children_appear_one_after_another() {
        let view = || {
            Staggered::column()
                .child(tile(color(1), 1))
                .child(tile(color(2), 2))
                .interval(0.1)
        };
        let mut rt = Runtime::default();
        let ui = step(&mut rt, &view(), 0.0);
        assert_eq!(opacity_of(&ui, color(1)), 0.0, "not yet");
        step(&mut rt, &view(), 0.05);
        let ui = step(&mut rt, &view(), 0.0);
        assert!(opacity_of(&ui, color(1)) > 0.0);
        assert_eq!(opacity_of(&ui, color(2)), 0.0);
        for _ in 0..40 {
            step(&mut rt, &view(), 0.05);
        }
        let ui = step(&mut rt, &view(), 0.0);
        assert_eq!(opacity_of(&ui, color(2)), 1.0, "all in place");
    }
}
