// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`AnimatedSwitcher`]: moves between **two different children**, rather than between
//! two values of one.
//!
//! Every other implicit animation here moves a number — an opacity, a size, a pin — and
//! the widget carrying it stays the same widget throughout. A switcher is asked for
//! something else: the count was 3 and is now 4, and for a moment **both** are on the
//! screen, the 3 fading out while the 4 fades in. So something has to keep the 3.
//!
//! ## What is kept is the value, not the widget
//!
//! In the reference the switcher holds on to the old child *widget*, which works because
//! that widget outlives the frame it was made in. Here nothing does: the view is a pure
//! function of the application's state, the tree is rebuilt from it, and a boxed widget
//! can be neither kept nor cloned. What the application gave to *make* the child, though,
//! is plain data. So a switcher is handed a **value and a way to build a child from one**:
//!
//! ```ignore
//! AnimatedSwitcher::new(0.25, app.count, |n| text(n.to_string()))
//! ```
//!
//! and it is the values that the runtime keeps, under the switcher's identity. The child
//! on its way out is **built again from its value every frame** it is on the screen — it
//! is a real subtree, laid out, painted and animating in its own right, not a picture of
//! what it was. A value changing is what a switch *is*: equal values are the same child,
//! whatever else changed around them, which is the reference's key.
//!
//! ## Where it happens, and what it costs
//!
//! A new value can only be noticed while the tree is being **built** — that is the only
//! moment both it and the one before it are at hand — so the switcher does its work in
//! [`Widget::build_in`], which is handed its identity and the runtime. What it builds
//! carries the progress of each child as a plain number, through a **transition** of the
//! caller's choosing — a fade by default, any of the explicit transitions otherwise — so
//! while a switch is in flight the tree has to be built again each frame rather than only
//! painted. That is what a route transition already costs here, for the same reason; it
//! stops the frame the switch settles.
//!
//! The children on their way out take **no input** and are **not announced**: a tap on a
//! number that is fading away would be a tap on a state the application has already left,
//! and a screen reader reading it would be reading that state out.

use std::any::Any;
use std::cell::OnceCell;
use std::rc::Rc;

use frus_core::{Color, Curve, Rect, Scene, TextDirection};
use frus_layout::Style;

use crate::interaction::{Status, WidgetId};
use crate::motionpatterns::MotionPattern;
use crate::runtime::Runtime;
use crate::theme::Theme;
use crate::widget::Widget;

/// Builds a child from a value the switcher has kept, or `None` if the value is not the
/// kind this switcher builds from (a different switcher came to stand in the same place).
type Build<Msg> = Box<dyn Fn(&dyn Any) -> Option<Box<dyn Widget<Msg>>>>;
/// Wraps a child in its progress: `1` in place, `0` gone; told which way it is going.
type TransitionFn<Msg> = Box<dyn Fn(Box<dyn Widget<Msg>>, f32, Side) -> Box<dyn Widget<Msg>>>;

/// What a transition is told besides the progress: whether the child is leaving, and what
/// the theme it is drawn under says (milestone 785).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Side {
    /// The child is on its way out.
    pub(crate) leaving: bool,
    /// The layout reads right to left.
    pub(crate) rtl: bool,
    /// The colour a motion pattern stands its children on.
    pub(crate) fill: Color,
    /// The switch is a step forwards (a shared axis's direction).
    pub(crate) forward: bool,
}

/// A switcher playing a [`MotionPattern`].
#[derive(Clone, Copy, Debug)]
struct Patterned {
    pattern: MotionPattern,
    forward: bool,
    /// Whether the durations are the pattern's, read from the theme.
    themed: bool,
    fill: Option<Color>,
}
/// Arranges the children, the oldest first and the one arriving last.
type Arrange<Msg> = Box<dyn Fn(Vec<Box<dyn Widget<Msg>>>) -> Box<dyn Widget<Msg>>>;

/// **Moves between two different children** when the value it shows changes: the old one
/// leaves as the new one arrives — a fade by default.
///
/// ```
/// use frus_widgets::{AnimatedSwitcher, Text};
///
/// let count = 3;
/// let _shown: AnimatedSwitcher<()> =
///     AnimatedSwitcher::new(0.25, count, |n: &i32| Text::new(n.to_string()));
/// ```
///
/// It is handed the **value** a child is made from rather than the child, because a value
/// is what can be kept from one frame to the next — see the module documentation. Two
/// values that compare equal are the same child: nothing moves. A value that differs is a
/// switch, and the child that was showing leaves from wherever it had got to.
///
/// - [`AnimatedSwitcher::transition`] says **how** a child arrives and leaves. It is
///   handed the child and its progress, and any explicit transition fits:
///   `.transition(|child, t| ScaleTransition::new(t, child))`.
/// - [`AnimatedSwitcher::layout`] says **where** the children go while two are showing.
///   By default they overlap, centred, in a box as big as the largest of them.
/// - The first value is shown as it is, with no transition: an implicit animation adopts
///   what it is given on the frame it appears, here as everywhere.
///
/// An absent child is a value like any other: switch over an `Option` and build an empty
/// box for `None`, and a child fades out to nothing.
pub struct AnimatedSwitcher<Msg = crate::callback::Callback> {
    value: Rc<dyn Any>,
    same: fn(&dyn Any, &dyn Any) -> bool,
    build: Build<Msg>,
    timing: Timing,
    transition: TransitionFn<Msg>,
    arrange: Arrange<Msg>,
    patterned: Option<Patterned>,
    /// The arrangement, as the one-element slice [`Widget::children`] hands back. Composed
    /// once per tree, by the first walk that reaches this node.
    built: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

/// Whether two kept values are the same child. Values of two different kinds never are.
fn same<T: PartialEq + 'static>(a: &dyn Any, b: &dyn Any) -> bool {
    match (a.downcast_ref::<T>(), b.downcast_ref::<T>()) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

impl<Msg: Clone + 'static> AnimatedSwitcher<Msg> {
    /// Shows the child `build` makes from `value`, and moves over `duration` seconds to
    /// the child of each new value.
    pub fn new<T, W>(duration: f32, value: T, build: impl Fn(&T) -> W + 'static) -> Self
    where
        T: PartialEq + 'static,
        W: Widget<Msg> + 'static,
    {
        Self {
            value: Rc::new(value),
            same: same::<T>,
            build: Box::new(move |kept: &dyn Any| {
                kept.downcast_ref::<T>()
                    .map(|value| Box::new(build(value)) as Box<dyn Widget<Msg>>)
            }),
            timing: Timing {
                duration,
                reverse: duration,
                switch_in: Curve::Linear,
                switch_out: Curve::Linear,
            },
            transition: Box::new(|child, progress, _| {
                Box::new(crate::transitions::FadeTransition::new(progress, child))
            }),
            arrange: Box::new(|layers| Box::new(Overlap { layers })),
            patterned: None,
            built: OnceCell::new(),
        }
    }

    /// A switcher playing a **Material motion pattern** — fade through, shared axis or
    /// fade scale — for as long as the theme's motion tokens say (milestone 785).
    ///
    /// ```
    /// use frus_widgets::{AnimatedSwitcher, MotionPattern, SharedAxis, Text};
    ///
    /// let step = 2;
    /// let _steps: AnimatedSwitcher<()> = AnimatedSwitcher::patterned(
    ///     MotionPattern::SharedAxis(SharedAxis::X),
    ///     step,
    ///     |n: &i32| Text::new(format!("Step {n}")),
    /// );
    /// ```
    ///
    /// The children stand on the surface colour while they change, as the pattern asks;
    /// [`fill_color`](Self::fill_color) changes it, and `Color::TRANSPARENT` turns it off.
    pub fn patterned<T, W>(
        pattern: MotionPattern,
        value: T,
        build: impl Fn(&T) -> W + 'static,
    ) -> Self
    where
        T: PartialEq + 'static,
        W: Widget<Msg> + 'static,
    {
        let motion = crate::MotionTheme::M3;
        let mut switcher = Self::new(pattern.duration(&motion), value, build).pattern(pattern);
        switcher.timing.reverse = pattern.reverse_duration(&motion);
        if let Some(patterned) = switcher.patterned.as_mut() {
            patterned.themed = true;
        }
        switcher
    }

    /// Plays `pattern` between the children, over the durations this switcher was given:
    /// the child leaving and the child arriving each posed by the pattern, on the
    /// pattern's own curves. See [`patterned`](Self::patterned) for the theme's
    /// durations.
    pub fn pattern(mut self, pattern: MotionPattern) -> Self {
        self.timing.switch_in = Curve::Linear;
        self.timing.switch_out = Curve::Linear;
        self.patterned = Some(Patterned {
            pattern,
            forward: true,
            themed: false,
            fill: None,
        });
        self.transition = Box::new(move |child, t, side| {
            let entering = !side.leaving;
            // The leaving child's clock runs down from where it was: the pattern is told
            // how far the switch has gone.
            let progress = if entering { t } else { 1.0 - t };
            let pose = pattern.pose(progress, entering, side.forward);
            let (outer, inner) = match (pattern, entering) {
                (MotionPattern::FadeScale, _) => (Color::TRANSPARENT, Color::TRANSPARENT),
                // Fade through keeps the leaving child's fill outside its fade, so the
                // surface stays while it goes; the arriving one brings its own.
                (MotionPattern::FadeThrough, false) => (side.fill, Color::TRANSPARENT),
                _ => (Color::TRANSPARENT, side.fill),
            };
            crate::motionpatterns::posed(child, pose, side.rtl && pattern.mirrored(), outer, inner)
        });
        self
    }

    /// Whether this switch is a step **backwards**: a shared axis then moves the other
    /// way — the new child in from the start edge (from above, from larger), the old one
    /// out towards the end. `false` by default.
    pub fn reverse(mut self, reverse: bool) -> Self {
        if let Some(patterned) = self.patterned.as_mut() {
            patterned.forward = !reverse;
        }
        self
    }

    /// The colour a pattern stands its children on, instead of the theme's surface;
    /// `Color::TRANSPARENT` for none.
    pub fn fill_color(mut self, color: Color) -> Self {
        if let Some(patterned) = self.patterned.as_mut() {
            patterned.fill = Some(color);
        }
        self
    }

    /// How long a child takes to **leave**, when it is not as long as one takes to arrive.
    pub fn reverse_duration(mut self, seconds: f32) -> Self {
        self.timing.reverse = seconds;
        self
    }

    /// The curve a child **arrives** on. Linear by default, as in the reference.
    pub fn switch_in_curve(mut self, curve: Curve) -> Self {
        self.timing.switch_in = curve;
        self
    }

    /// The curve a child **leaves** on, read as its progress falls from where it was to
    /// nothing. Linear by default.
    pub fn switch_out_curve(mut self, curve: Curve) -> Self {
        self.timing.switch_out = curve;
        self
    }

    /// **How** a child arrives and leaves: handed the child and its progress — `1` in
    /// place, `0` gone — it returns the child as it should be drawn at that point. A fade
    /// by default.
    ///
    /// Both directions go through it, the leaving child with its progress falling, so a
    /// scale makes the old child shrink away as the new one grows.
    pub fn transition<W: Widget<Msg> + 'static>(
        mut self,
        transition: impl Fn(Box<dyn Widget<Msg>>, f32) -> W + 'static,
    ) -> Self {
        self.transition = Box::new(move |child, progress, _| Box::new(transition(child, progress)));
        self.patterned = None;
        self
    }

    /// **Where** the children go while more than one is showing: handed them oldest first,
    /// the arriving one last, it returns what holds them. By default they overlap, centred
    /// in a box as big as the largest.
    ///
    /// Each child arrives already keyed and wrapped in its transition, so the arrangement
    /// only has to place them — in the order given, if the arriving one is to be drawn on
    /// top.
    pub fn layout<W: Widget<Msg> + 'static>(
        mut self,
        layout: impl Fn(Vec<Box<dyn Widget<Msg>>>) -> W + 'static,
    ) -> Self {
        self.arrange = Box::new(move |layers| Box::new(layout(layers)));
        self
    }

    /// The children as they stand: from the runtime when a walk hands one over, and at rest
    /// otherwise — a bare traversal that reaches this node before any walk has is shown the
    /// value it was given, which is also what the first frame shows.
    fn compose(&self, at: Option<(WidgetId, &Runtime, &Theme)>) -> Vec<Box<dyn Widget<Msg>>> {
        let mut side = Side {
            leaving: false,
            rtl: false,
            fill: Color::TRANSPARENT,
            forward: self.patterned.is_none_or(|p| p.forward),
        };
        let shown = match at {
            Some((id, runtime, theme)) => {
                let mut timing = self.timing.clone();
                if let Some(patterned) = self.patterned {
                    if patterned.themed {
                        timing.duration = patterned.pattern.duration(&theme.motion);
                        timing.reverse = patterned.pattern.reverse_duration(&theme.motion);
                    }
                    side.fill = patterned.fill.unwrap_or(theme.scheme.surface);
                    if !patterned.pattern.fills() {
                        side.fill = Color::TRANSPARENT;
                    }
                }
                side.rtl = theme.direction == TextDirection::Rtl;
                runtime.switch_to(id, self.value.clone(), self.same, timing)
            }
            None => vec![Shown {
                serial: 0,
                value: self.value.clone(),
                progress: 1.0,
                leaving: false,
            }],
        };
        let layers = shown
            .into_iter()
            .filter_map(|shown| {
                let child = (self.build)(shown.value.as_ref())?;
                let side = Side {
                    leaving: shown.leaving,
                    ..side
                };
                let drawn = (self.transition)(child, shown.progress, side);
                // The same wrappers whether it is arriving or leaving, so that the subtree
                // keeps its identity — and whatever it retains — across the switch. A child
                // leaving is no longer the answer, to a finger or to a screen reader.
                let layer = crate::Keyed::new(
                    shown.serial,
                    crate::barrier::ExcludeSemantics::new(
                        crate::barrier::IgnorePointer::new(drawn).ignoring(shown.leaving),
                    )
                    .excluding(shown.leaving),
                );
                Some(Box::new(layer) as Box<dyn Widget<Msg>>)
            })
            .collect();
        vec![(self.arrange)(layers)]
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for AnimatedSwitcher<Msg> {
    fn build_in(&self, id: WidgetId, runtime: &Runtime, theme: &Theme) {
        self.built
            .get_or_init(|| self.compose(Some((id, runtime, theme))));
    }

    fn switches(&self) -> bool {
        true
    }

    fn style(&self) -> Style {
        // Nothing of its own: the arrangement's box is this node's box.
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
        "AnimatedSwitcher"
    }
}

/// The default arrangement: every child in one cell, centred, the box as big as the
/// largest — the reference's centred stack. A [`crate::Stack`] cannot be it here: its
/// layers are laid out apart from it, so a stack with no size of its own is nothing.
pub(crate) struct Overlap<Msg> {
    pub(crate) layers: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: Clone> Widget<Msg> for Overlap<Msg> {
    fn style(&self) -> Style {
        Style {
            overlap: true,
            align: frus_layout::Align::Center,
            ..Style::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.layers
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "AnimatedSwitcher"
    }
}

/// How long and on what curves a switcher's children move.
#[derive(Clone, Debug)]
pub(crate) struct Timing {
    pub(crate) duration: f32,
    pub(crate) reverse: f32,
    pub(crate) switch_in: Curve,
    pub(crate) switch_out: Curve,
}

/// One child as the tree is to be built with it.
pub(crate) struct Shown {
    pub(crate) serial: u64,
    pub(crate) value: Rc<dyn Any>,
    /// Through the curve: what the transition is handed.
    pub(crate) progress: f32,
    pub(crate) leaving: bool,
}

/// One child the runtime is keeping for a switcher.
struct Entry {
    /// Its identity among the switcher's children, for as long as it is on the screen.
    serial: u64,
    /// The key it was listed under, for a keyed list (`0` otherwise).
    key: u64,
    value: Rc<dyn Any>,
    /// Its own clock, linear: `0` gone, `1` in place. It runs up while the child arrives
    /// and down while it leaves, **from wherever it had got to** — a child cut off halfway
    /// in leaves from halfway, rather than jumping to full and fading from there.
    t: f32,
    leaving: bool,
    /// How long it still waits before it starts to arrive, in seconds: a staggered entrance
    /// (milestone 785).
    delay: f32,
}

/// What the runtime keeps for one switcher: the children on the screen, oldest first.
/// Only the last can be arriving; every one before it is leaving.
pub(crate) struct Switch {
    entries: Vec<Entry>,
    next: u64,
    timing: Timing,
    /// Moved since a tree was last built from it. What moved is baked into the children as
    /// numbers, so the frame has to be **built** again, not only painted.
    pub(crate) moved: bool,
}

/// The `t` at which `curve` reaches `value`, for a curve that rises from 0 to 1.
fn solve(curve: &Curve, value: f32) -> f32 {
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    for _ in 0..30 {
        let mid = 0.5 * (low + high);
        if curve.transform(mid) < value {
            low = mid;
        } else {
            high = mid;
        }
    }
    0.5 * (low + high)
}

impl Switch {
    /// A switcher seen for the first time: its value in place, with no transition — or,
    /// when it is to `appear`, arriving from nothing.
    fn mounted(value: Rc<dyn Any>, timing: Timing, appear: bool) -> Self {
        Self {
            entries: vec![Entry {
                serial: 0,
                key: 0,
                value,
                t: if appear { 0.0 } else { 1.0 },
                leaving: false,
                delay: 0.0,
            }],
            next: 1,
            timing,
            moved: appear,
        }
    }

    /// A keyed list seen for the first time: its items in place — or, when they are to
    /// `appear`, each arriving `stagger` seconds after the one before it.
    fn listed(items: Vec<(u64, Rc<dyn Any>)>, timing: Timing, appear: bool, stagger: f32) -> Self {
        let count = items.len() as u64;
        let entries = items
            .into_iter()
            .enumerate()
            .map(|(at, (key, value))| Entry {
                serial: at as u64,
                key,
                value,
                t: if appear { 0.0 } else { 1.0 },
                leaving: false,
                delay: if appear { stagger * at as f32 } else { 0.0 },
            })
            .collect();
        Self {
            entries,
            next: count,
            timing,
            moved: appear,
        }
    }

    fn shown(&self) -> Vec<Shown> {
        self.entries
            .iter()
            .map(|e| Shown {
                serial: e.serial,
                value: e.value.clone(),
                progress: if e.leaving {
                    self.timing.switch_out.transform(e.t)
                } else {
                    self.timing.switch_in.transform(e.t)
                },
                leaving: e.leaving,
            })
            .collect()
    }

    /// Whether it holds no child at all — or only children built from nothing.
    #[cfg(test)]
    pub(crate) fn is_empty_of_children(&self) -> bool {
        self.entries
            .iter()
            .all(|e| e.value.downcast_ref::<bool>() == Some(&false))
    }

    /// Takes in the items a keyed list was built with, and says what to show: the items,
    /// in their order, with each item that has gone still there **where it was** — after
    /// the item that was before it — on its way out. An item that comes back while it is
    /// leaving turns round from where it had got to, and keeps its identity.
    pub(crate) fn meet_list(
        &mut self,
        items: Vec<(u64, Rc<dyn Any>)>,
        timing: Timing,
        stagger: f32,
        still: bool,
    ) -> Vec<Shown> {
        self.timing = timing;
        let instant = still || self.timing.duration <= 0.0;
        let gone_at_once = still || self.timing.reverse <= 0.0;
        let listed = |key: u64| items.iter().any(|(k, _)| *k == key);
        // Each entry no longer listed, with what it came after: the nearest entry before
        // it that is still listed (`None`: the front).
        let mut anchors: Vec<(Option<u64>, Entry)> = Vec::new();
        let mut kept: Vec<Entry> = Vec::new();
        let mut before: Option<u64> = None;
        for mut entry in std::mem::take(&mut self.entries) {
            if listed(entry.key) {
                before = Some(entry.key);
                kept.push(entry);
            } else if !gone_at_once {
                entry.leaving = true;
                entry.delay = 0.0;
                anchors.push((before, entry));
            }
        }
        fn after(anchors: &mut Vec<(Option<u64>, Entry)>, key: Option<u64>) -> Vec<Entry> {
            let mut out = Vec::new();
            let mut at = 0;
            while at < anchors.len() {
                if anchors[at].0 == key {
                    out.push(anchors.remove(at).1);
                } else {
                    at += 1;
                }
            }
            out
        }
        let mut entries = after(&mut anchors, None);
        let mut arriving = 0;
        for (key, value) in items {
            let entry = match kept.iter().position(|e| e.key == key) {
                Some(at) => {
                    let mut entry = kept.remove(at);
                    entry.value = value;
                    entry.leaving = false;
                    if instant {
                        entry.t = 1.0;
                    }
                    entry
                }
                None => {
                    let delay = if instant {
                        0.0
                    } else {
                        stagger * arriving as f32
                    };
                    arriving += 1;
                    self.next += 1;
                    Entry {
                        serial: self.next,
                        key,
                        value,
                        t: if instant { 1.0 } else { 0.0 },
                        leaving: false,
                        delay,
                    }
                }
            };
            entries.push(entry);
            entries.extend(after(&mut anchors, Some(key)));
        }
        // What came after an item that is itself gone and was not found: at the end.
        entries.extend(anchors.into_iter().map(|(_, entry)| entry));
        self.entries = entries;
        self.moved = false;
        self.shown()
    }

    /// Takes in the value a tree was built with, and says what to show.
    pub(crate) fn meet(
        &mut self,
        value: Rc<dyn Any>,
        same: fn(&dyn Any, &dyn Any) -> bool,
        timing: Timing,
        still: bool,
        revive: bool,
    ) -> Vec<Shown> {
        self.timing = timing;
        let showing = self
            .entries
            .last()
            .is_some_and(|e| !e.leaving && same(e.value.as_ref(), value.as_ref()));
        if !showing {
            // A presence (milestone 785): a value coming back while it is still leaving
            // turns round from where it had got to, keeping its identity, rather than a
            // second copy arriving from nothing beside it.
            let back = if revive {
                self.entries
                    .iter()
                    .rposition(|e| e.leaving && same(e.value.as_ref(), value.as_ref()))
            } else {
                None
            };
            match back {
                Some(at) => {
                    let mut entry = self.entries.remove(at);
                    for other in &mut self.entries {
                        other.leaving = true;
                    }
                    if still {
                        self.entries.clear();
                        entry.t = 1.0;
                    } else {
                        // Where its exit had it, on the entrance's curve: the clock that
                        // shows it as it was, so turning round does not jump.
                        let shown = self.timing.switch_out.transform(entry.t);
                        entry.t = solve(&self.timing.switch_in, shown);
                    }
                    entry.leaving = false;
                    self.entries.push(entry);
                }
                None => self.arrive(value, still),
            }
        }
        self.moved = false;
        self.shown()
    }

    /// A new child: everything showing starts to leave, and it starts to arrive.
    fn arrive(&mut self, value: Rc<dyn Any>, still: bool) {
        for entry in &mut self.entries {
            entry.leaving = true;
        }
        // With the motion turned down, or no time to do it in, a child is simply gone —
        // and the new one simply there.
        if still || self.timing.reverse <= 0.0 {
            self.entries.clear();
        }
        let t = if still || self.timing.duration <= 0.0 {
            1.0
        } else {
            0.0
        };
        self.entries.push(Entry {
            serial: self.next,
            key: 0,
            value,
            t,
            leaving: false,
            delay: 0.0,
        });
        self.next += 1;
    }

    /// Steps every child's clock by `dt`, and lets go of the ones that have left. Returns
    /// whether anything moved.
    pub(crate) fn advance(&mut self, dt: f32, still: bool) -> bool {
        let step = |duration: f32, dt: f32| {
            if still || duration <= 0.0 {
                f32::INFINITY
            } else {
                dt / duration
            }
        };
        let mut moved = false;
        for entry in &mut self.entries {
            // A staggered entrance waits its turn first, and the wait takes frames too.
            let mut dt = dt;
            if !entry.leaving && entry.delay > 0.0 && dt > 0.0 {
                let waited = if still {
                    entry.delay
                } else {
                    dt.min(entry.delay)
                };
                entry.delay -= waited;
                dt = if still { dt } else { dt - waited };
                moved = true;
                if dt <= 0.0 {
                    continue;
                }
            }
            let (arrive, leave) = (
                step(self.timing.duration, dt),
                step(self.timing.reverse, dt),
            );
            let before = entry.t;
            entry.t = if entry.leaving {
                (entry.t - leave).max(0.0)
            } else {
                (entry.t + arrive).min(1.0)
            };
            // `dt` of nought moves nothing, however short the duration: it is the first
            // frame, not a step.
            if dt > 0.0 && entry.t != before {
                moved = true;
            } else {
                entry.t = before;
            }
        }
        let count = self.entries.len();
        self.entries.retain(|e| !e.leaving || e.t > 0.0);
        moved |= self.entries.len() != count;
        self.moved |= moved;
        moved
    }
}

impl Runtime {
    /// Hands a switcher what to show, noticing a new value — see [`Switch::meet`].
    pub(crate) fn switch_to(
        &self,
        id: WidgetId,
        value: Rc<dyn Any>,
        same: fn(&dyn Any, &dyn Any) -> bool,
        timing: Timing,
    ) -> Vec<Shown> {
        let still = self.still;
        let mut switchers = self.switchers.borrow_mut();
        switchers
            .entry(id)
            .or_insert_with(|| Switch::mounted(value.clone(), timing.clone(), false))
            .meet(value, same, timing, still, false)
    }

    /// Hands a presence what to show (milestone 785): a switch whose value coming back
    /// while it leaves turns round, and which may `appear` — arrive from nothing — the
    /// first time it is seen.
    pub(crate) fn presence_to(
        &self,
        id: WidgetId,
        value: Rc<dyn Any>,
        same: fn(&dyn Any, &dyn Any) -> bool,
        timing: Timing,
        appear: bool,
    ) -> Vec<Shown> {
        let still = self.still;
        let mut switchers = self.switchers.borrow_mut();
        switchers
            .entry(id)
            .or_insert_with(|| Switch::mounted(value.clone(), timing.clone(), appear && !still))
            .meet(value, same, timing, still, true)
    }

    /// Hands a keyed list what to show (milestone 785) — see [`Switch::meet_list`].
    pub(crate) fn list_to(
        &self,
        id: WidgetId,
        items: Vec<(u64, Rc<dyn Any>)>,
        timing: Timing,
        stagger: f32,
        appear: bool,
    ) -> Vec<Shown> {
        let still = self.still;
        let mut switchers = self.switchers.borrow_mut();
        match switchers.entry(id) {
            std::collections::hash_map::Entry::Occupied(mut kept) => {
                kept.get_mut().meet_list(items, timing, stagger, still)
            }
            std::collections::hash_map::Entry::Vacant(slot) => slot
                .insert(Switch::listed(items, timing, appear && !still, stagger))
                .shown(),
        }
    }

    /// Steps every [`AnimatedSwitcher`]'s children in and out, and forgets the switchers no
    /// longer in the tree. Returns whether a child is still moving.
    pub fn advance_switchers<Msg>(&mut self, root: &dyn Widget<Msg>, dt: f32) -> bool {
        fn find<Msg>(
            widget: &dyn Widget<Msg>,
            id: WidgetId,
            kept: &std::collections::HashMap<WidgetId, Switch>,
            present: &mut std::collections::HashSet<WidgetId>,
        ) {
            // Being at the identity is not enough: an identity is a position, and any
            // widget that comes to stand where the switcher stood has the same one.
            if widget.switches() && kept.contains_key(&id) {
                present.insert(id);
            }
            for (index, child) in widget.children().iter().enumerate() {
                find(
                    child.as_ref(),
                    crate::ui::child_id(id, index, child.as_ref()),
                    kept,
                    present,
                );
            }
        }
        let still = self.still;
        let switchers = self.switchers.get_mut();
        if switchers.is_empty() {
            return false;
        }
        // A switcher that has gone is forgotten, so that one appearing in its place later
        // is a mount — shown as it is — rather than a switch from what used to be there.
        let mut present = std::collections::HashSet::new();
        find(root, WidgetId::ROOT, switchers, &mut present);
        switchers.retain(|id, _| present.contains(id));
        self.memos.get_mut().retain(|id, _| present.contains(id));
        let mut moving = false;
        for switch in switchers.values_mut() {
            moving |= switch.advance(dt, still);
        }
        moving
    }

    /// Whether a switcher has moved since the tree was last built, so that the next frame
    /// has to **build** the tree again rather than only paint it: what moved is carried by
    /// the children as numbers.
    pub fn switching(&self) -> bool {
        self.switchers.borrow().values().any(|s| s.moved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{build_ui, Ui};
    use crate::{Container, Flex, Keyed, Text};
    use frus_core::{Color, Point, Primitive, Size};

    const SIZE: Size = Size::new(200.0, 100.0);

    fn red() -> Color {
        Color::rgb(1.0, 0.0, 0.0)
    }
    fn blue() -> Color {
        Color::rgb(0.0, 0.0, 1.0)
    }
    fn green() -> Color {
        Color::rgb(0.0, 1.0, 0.0)
    }

    /// Value 1 is a wide red tile, 2 a narrow blue one, 3 a green one — each clickable
    /// with its own number and labelled with its own word.
    fn tile(value: u8) -> Container<i32> {
        let (color, width, word) = match value {
            1 => (red(), 80.0, "one"),
            2 => (blue(), 40.0, "two"),
            _ => (green(), 40.0, "three"),
        };
        Container::new()
            .width(width)
            .height(20.0)
            .color(color)
            .on_click(value as i32)
            .child(Text::new(word))
    }

    fn switcher(value: u8) -> AnimatedSwitcher<i32> {
        AnimatedSwitcher::new(0.2, value, |v: &u8| tile(*v))
    }

    /// In a row, so the switcher's box is what its children make it.
    fn view(value: u8) -> Flex<i32> {
        Flex::row().child(switcher(value))
    }

    /// One turn of the shell's loop: the tree built with `value`, then the clocks stepped.
    fn step(rt: &mut Runtime, value: u8, dt: f32) {
        let tree = view(value);
        let _ = build_ui(&tree, SIZE, rt, &Theme::dark());
        rt.advance_switchers(&tree, dt);
    }

    /// What the next frame draws.
    fn look(rt: &Runtime, value: u8) -> Ui<i32> {
        build_ui(&view(value), SIZE, rt, &Theme::dark())
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

    /// How visible the tile of `color` is: `0` when it is not drawn at all.
    fn shown(ui: &Ui<i32>, color: Color) -> f32 {
        rects(ui)
            .iter()
            .filter(|(_, c, _)| *c == color)
            .map(|(_, _, opacity)| *opacity * color_alpha(color))
            .fold(0.0, f32::max)
    }

    fn color_alpha(color: Color) -> f32 {
        color.a
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    /// **The first value is shown as it is**: no fade in from nothing on the frame the
    /// switcher appears, the rule every implicit animation here keeps.
    #[test]
    fn the_first_value_is_shown_as_it_is() {
        let mut rt = Runtime::default();
        step(&mut rt, 1, 0.0);
        let ui = look(&rt, 1);
        assert_eq!(shown(&ui, red()), 1.0);
        assert_eq!(shown(&ui, blue()), 0.0);
    }

    /// **A new value crosses over**: at the change both are there, the old whole and the
    /// new not yet; halfway each is half; at the end only the new one is left in the tree.
    #[test]
    fn a_new_value_crosses_over_and_the_old_one_is_let_go() {
        let mut rt = Runtime::default();
        step(&mut rt, 1, 0.0);

        let ui = look(&rt, 2);
        assert_eq!(shown(&ui, red()), 1.0, "the old child, still whole");
        assert_eq!(shown(&ui, blue()), 0.0, "the new one, not yet");

        step(&mut rt, 2, 0.05);
        let ui = look(&rt, 2);
        assert!(close(shown(&ui, red()), 0.75), "{}", shown(&ui, red()));
        assert!(close(shown(&ui, blue()), 0.25), "{}", shown(&ui, blue()));

        step(&mut rt, 2, 0.05);
        let ui = look(&rt, 2);
        assert!(close(shown(&ui, red()), 0.5));
        assert!(close(shown(&ui, blue()), 0.5));

        step(&mut rt, 2, 0.1);
        let ui = look(&rt, 2);
        assert_eq!(shown(&ui, red()), 0.0, "gone from the tree");
        assert_eq!(shown(&ui, blue()), 1.0);
    }

    /// **The curves are the caller's**, one each way: the arriving child on the in-curve,
    /// the leaving one on the out-curve, read as its progress falls.
    #[test]
    fn each_direction_moves_on_its_own_curve() {
        let curved = |value: u8| {
            Flex::<i32>::row().child(
                AnimatedSwitcher::new(0.2, value, |v: &u8| tile(*v))
                    .switch_in_curve(Curve::Decelerate)
                    .switch_out_curve(Curve::Linear),
            )
        };
        let mut rt = Runtime::default();
        let tree = curved(1);
        let _ = build_ui(&tree, SIZE, &rt, &Theme::dark());
        rt.advance_switchers(&tree, 0.0);
        let tree = curved(2);
        let _ = build_ui(&tree, SIZE, &rt, &Theme::dark());
        rt.advance_switchers(&tree, 0.1);
        let ui = build_ui(&curved(2), SIZE, &rt, &Theme::dark());
        // Halfway: decelerating in is three quarters there, linear out is half gone.
        assert!(close(shown(&ui, blue()), 0.75), "{}", shown(&ui, blue()));
        assert!(close(shown(&ui, red()), 0.5), "{}", shown(&ui, red()));
    }

    /// **A child cut off on its way in leaves from where it had got to**, not from whole:
    /// the new value arrives while the last is still coming in, and the one that was
    /// arriving turns round at half rather than jumping to full and fading from there.
    #[test]
    fn a_child_cut_off_halfway_leaves_from_halfway() {
        let mut rt = Runtime::default();
        step(&mut rt, 1, 0.0);
        step(&mut rt, 2, 0.1); // blue half in
        step(&mut rt, 3, 0.0); // and now it leaves
        let ui = look(&rt, 3);
        assert!(close(shown(&ui, blue()), 0.5), "{}", shown(&ui, blue()));
        assert_eq!(shown(&ui, green()), 0.0, "the newest, not yet in");
        assert!(
            close(shown(&ui, red()), 0.5),
            "the first, still on its way out"
        );
    }

    /// **Equal values are the same child**: nothing moves, and nothing asks for the tree to
    /// be rebuilt.
    #[test]
    fn an_equal_value_is_not_a_switch() {
        let mut rt = Runtime::default();
        step(&mut rt, 1, 0.0);
        step(&mut rt, 1, 0.1);
        assert!(!rt.switching());
        assert_eq!(shown(&look(&rt, 1), red()), 1.0);
    }

    /// **While a switch moves, the frame has to be built, not only painted** — the progress
    /// is a number built into the children — and once it settles, it no longer does.
    #[test]
    fn a_switch_in_flight_asks_for_the_tree_to_be_built_and_then_stops() {
        let mut rt = Runtime::default();
        step(&mut rt, 1, 0.0);
        assert!(!rt.switching(), "at rest");
        step(&mut rt, 2, 0.05);
        assert!(rt.switching(), "moved since it was built");
        let _ = look(&rt, 2);
        assert!(!rt.switching(), "and built since");
        step(&mut rt, 2, 1.0);
        assert!(rt.switching(), "the step that ends it is a move too");
        step(&mut rt, 2, 0.05);
        assert!(!rt.switching(), "settled: painted only from here on");
    }

    /// **With motion turned down, it simply swaps**: the new child whole, the old one gone,
    /// on the frame the value changes.
    #[test]
    fn with_motion_turned_down_it_swaps_at_once() {
        let mut rt = Runtime::default();
        rt.still = true;
        step(&mut rt, 1, 0.0);
        let ui = look(&rt, 2);
        assert_eq!(shown(&ui, blue()), 1.0);
        assert_eq!(shown(&ui, red()), 0.0);
    }

    /// **The leaving child takes no input and is not announced.** The point is inside the
    /// wide red tile and outside the narrow blue one, so nothing but the red could answer
    /// it — and the red is a state the application has left.
    #[test]
    fn the_leaving_child_takes_no_input_and_is_not_announced() {
        let mut rt = Runtime::default();
        step(&mut rt, 1, 0.0);
        let before = look(&rt, 1);
        let only_red = Point::new(5.0, 10.0);
        let hit = before.hit(only_red).and_then(|id| before.msg_for(id));
        assert_eq!(
            hit,
            Some(1),
            "the fixture: the red tile answers there at rest"
        );

        step(&mut rt, 2, 0.1);
        let ui = look(&rt, 2);
        assert!(shown(&ui, red()) > 0.0, "the red is still on the screen");
        assert_eq!(ui.hit(only_red).and_then(|id| ui.msg_for(id)), None);
        let words: Vec<_> = ui
            .semantics()
            .iter()
            .filter_map(|(_, _, s)| s.label.clone())
            .collect();
        assert!(words.iter().any(|w| w.contains("two")), "{words:?}");
        assert!(!words.iter().any(|w| w.contains("one")), "{words:?}");
    }

    /// **While both show, the box is the larger of the two**, each centred in it; once the
    /// old one has gone, the box is the new one's.
    #[test]
    fn the_box_is_the_largest_child_while_both_show() {
        let mut rt = Runtime::default();
        step(&mut rt, 1, 0.0);
        step(&mut rt, 2, 0.1);
        let x_of = |ui: &Ui<i32>, color: Color| {
            rects(ui)
                .iter()
                .find(|(_, c, _)| *c == color)
                .map(|(r, _, _)| r.x)
                .unwrap()
        };
        let ui = look(&rt, 2);
        assert_eq!(x_of(&ui, red()), 0.0);
        assert_eq!(
            x_of(&ui, blue()),
            20.0,
            "the narrow one centred in the wide one's box"
        );

        step(&mut rt, 2, 1.0);
        assert_eq!(
            x_of(&look(&rt, 2), blue()),
            0.0,
            "and the box is its own again"
        );
    }

    /// **Behind a key, it still switches.** A key is a transparent wrapper: the walk hands
    /// it the identity it shares with the switcher, and it has to pass that on. Built
    /// through the older hook instead, the switcher never learns who it is and just shows
    /// the newest value whole.
    #[test]
    fn a_switcher_behind_a_key_still_switches() {
        let keyed = |value: u8| Flex::<i32>::row().child(Keyed::new(7u8, switcher(value)));
        let mut rt = Runtime::default();
        for (value, dt) in [(1, 0.0), (2, 0.1)] {
            let tree = keyed(value);
            let _ = build_ui(&tree, SIZE, &rt, &Theme::dark());
            rt.advance_switchers(&tree, dt);
        }
        let ui = build_ui(&keyed(2), SIZE, &rt, &Theme::dark());
        assert!(close(shown(&ui, red()), 0.5), "{}", shown(&ui, red()));
        assert!(close(shown(&ui, blue()), 0.5), "{}", shown(&ui, blue()));
    }

    /// **A switcher that goes is forgotten**, so one that appears in its place later is a
    /// mount — its value shown as it is — and not a switch from what used to be there.
    #[test]
    fn a_switcher_that_goes_is_forgotten() {
        let mut rt = Runtime::default();
        step(&mut rt, 1, 0.0);
        let without = Flex::<i32>::row().child(Container::new().width(10.0).height(10.0));
        let _ = build_ui(&without, SIZE, &rt, &Theme::dark());
        rt.advance_switchers(&without, 0.016);
        assert!(rt.switchers.borrow().is_empty(), "forgotten");

        step(&mut rt, 2, 0.0);
        let ui = look(&rt, 2);
        assert_eq!(shown(&ui, blue()), 1.0);
        assert_eq!(shown(&ui, red()), 0.0);
    }
}
