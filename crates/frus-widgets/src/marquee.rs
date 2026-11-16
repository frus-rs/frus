// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`Marquee`]: a line of text, or any child, that **scrolls sideways in a loop when it is
//! wider than the room it has** — a song title in a player, a ticker of headlines, a long
//! file name in a narrow column (milestone 811).
//!
//! It is still when it fits: the box is the content's size, held to what it is offered, and
//! a content that is not wider than that is drawn like any other and asks for no frame.
//! When it overflows, the content waits a moment at its start, then runs towards where
//! reading starts at a steady speed, followed a gap behind by a copy of itself, so the loop
//! has no seam; when the copy reaches the start, the lap begins again with the same wait.
//!
//! Motion follows the reader's setting: with animations turned off, the content stays at
//! its start, cut at the box's edge.

use std::cell::{Cell, OnceCell};
use std::rc::Rc;

use frus_core::{AlignmentDirectional, AlignmentGeometry, Color, Rect, Scene, TextStyle};
use frus_layout::Style;

use crate::constraints::{AxisConstraint, ConstraintsTransform};
use crate::interaction::Status;
use crate::theme::Theme;
use crate::widget::Widget;

/// Default speed, in logical pixels per second.
pub const MARQUEE_VELOCITY: f32 = 30.0;
/// Default space between the content and its copy, in logical pixels.
pub const MARQUEE_GAP: f32 = 48.0;
/// Default wait at the start of every lap, in seconds.
pub const MARQUEE_PAUSE: f32 = 1.2;

/// Theme defaults for [`Marquee`] (`theme.widgets.marquee`); each `None` falls through to
/// the default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MarqueeTheme {
    /// Speed, in logical pixels per second. Unset, 30.
    pub velocity: Option<f32>,
    /// Space between the content and its copy. Unset, 48.
    pub gap: Option<f32>,
    /// Wait at the start of each lap, in seconds. Unset, 1.2.
    pub pause: Option<f32>,
    /// The style of a text marquee. Unset, the text style of the subtree it is in.
    pub text_style: Option<TextStyle>,
    /// The colour of a text marquee. Unset, the subtree's.
    pub text_color: Option<Color>,
}

/// **How a marquee moves**, resolved from the widget and its theme: what the walk reads
/// through [`Widget::marquee`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarqueeMotion {
    /// Speed, in logical pixels per second. Zero or less, it does not move.
    pub velocity: f32,
    /// Space between the content and the copy that follows it.
    pub gap: f32,
    /// Wait at the start of every lap, in seconds.
    pub pause: f32,
    /// Runs towards where reading **ends** rather than where it starts.
    pub reverse: bool,
}

impl Default for MarqueeMotion {
    fn default() -> Self {
        Self {
            velocity: MARQUEE_VELOCITY,
            gap: MARQUEE_GAP,
            pause: MARQUEE_PAUSE,
            reverse: false,
        }
    }
}

impl MarqueeMotion {
    /// Whether content `content` wide moves in a box `room` wide: only when it overflows,
    /// by more than half a pixel, at a speed above zero.
    ///
    /// ```
    /// use frus_widgets::MarqueeMotion;
    /// let motion = MarqueeMotion::default();
    /// assert!(motion.moves(100.0, 180.0));
    /// assert!(!motion.moves(100.0, 100.0));
    /// ```
    pub fn moves(&self, room: f32, content: f32) -> bool {
        self.velocity > 0.0 && content > room + 0.5
    }

    /// How far the content has travelled `elapsed` seconds after it started, for a lap
    /// `lap` long (the content's width plus the gap): `0` during each lap's pause, then
    /// growing at the speed up to `lap`, where the copy stands exactly where the content
    /// started, and the next lap begins.
    ///
    /// ```
    /// use frus_widgets::MarqueeMotion;
    /// let motion = MarqueeMotion { velocity: 10.0, gap: 0.0, pause: 1.0, reverse: false };
    /// assert_eq!(motion.travel(0.5, 50.0), 0.0); // waiting
    /// assert_eq!(motion.travel(3.0, 50.0), 20.0); // two seconds in
    /// assert_eq!(motion.travel(6.5, 50.0), 0.0); // the next lap's wait
    /// ```
    pub fn travel(&self, elapsed: f32, lap: f32) -> f32 {
        if self.velocity <= 0.0 || lap <= 0.0 {
            return 0.0;
        }
        let pause = self.pause.max(0.0);
        let period = pause + lap / self.velocity;
        let t = elapsed.max(0.0) % period;
        ((t - pause).max(0.0) * self.velocity).min(lap)
    }
}

/// What the marquee shows.
enum Content<Msg> {
    Text {
        text: String,
        style: Option<TextStyle>,
        color: Option<Color>,
    },
    Build(Rc<dyn Fn() -> Box<dyn Widget<Msg>>>),
}

/// Text, or any child, that **scrolls sideways in a loop when it does not fit**.
///
/// ```
/// use frus_widgets::{Marquee, Text};
///
/// // A line of text: still when it fits, scrolling when it does not.
/// let _title: Marquee<()> = Marquee::text("A very long song title that will not fit");
///
/// // Any child, built twice: once to show, once for the copy that follows it.
/// let _ticker: Marquee<()> = Marquee::builder(|| Text::new("Markets up 2 % · Rain tomorrow"))
///     .velocity(60.0)
///     .gap(80.0)
///     .pause(0.0);
/// ```
///
/// Its box is the content's size, held to what it is offered: in a row, give it the room
/// with an [`Expanded`](crate::Expanded). **Themed** through [`MarqueeTheme`]
/// (`theme.widgets.marquee`).
///
/// The copy is out of the semantics tree and takes no touches: a screen reader hears the
/// content once, and a tap lands on the content, not on its shadow.
pub struct Marquee<Msg = crate::callback::Callback> {
    content: Content<Msg>,
    velocity: Option<f32>,
    gap: Option<f32>,
    pause: Option<f32>,
    reverse: bool,
    motion: Cell<MarqueeMotion>,
    children: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> Marquee<Msg> {
    /// A marquee of one line of `text`, in the subtree's text style unless told.
    pub fn text(text: impl Into<String>) -> Self {
        Self::with(Content::Text {
            text: text.into(),
            style: None,
            color: None,
        })
    }

    /// A marquee of the child `build` makes. It is called twice: the content, and the copy
    /// that follows it a gap behind.
    pub fn builder<W: Widget<Msg> + 'static>(build: impl Fn() -> W + 'static) -> Self {
        Self::with(Content::Build(Rc::new(move || {
            Box::new(build()) as Box<dyn Widget<Msg>>
        })))
    }

    fn with(content: Content<Msg>) -> Self {
        Self {
            content,
            velocity: None,
            gap: None,
            pause: None,
            reverse: false,
            motion: Cell::new(MarqueeMotion::default()),
            children: OnceCell::new(),
        }
    }

    /// The speed, in logical pixels per second. Zero keeps it still.
    pub fn velocity(mut self, velocity: f32) -> Self {
        self.velocity = Some(velocity);
        self
    }

    /// The space between the content and the copy that follows it.
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = Some(gap);
        self
    }

    /// The wait at the start of every lap, in seconds.
    pub fn pause(mut self, seconds: f32) -> Self {
        self.pause = Some(seconds);
        self
    }

    /// Runs towards where reading ends (to the right, in a left-to-right language) instead
    /// of towards where it starts.
    pub fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// The style of a text marquee, over the theme's. Ignored by a built one.
    pub fn style(mut self, style: TextStyle) -> Self {
        if let Content::Text { style: s, .. } = &mut self.content {
            *s = Some(style);
        }
        self
    }

    /// The colour of a text marquee, over the theme's. Ignored by a built one.
    pub fn color(mut self, color: Color) -> Self {
        if let Content::Text { color: c, .. } = &mut self.content {
            *c = Some(color);
        }
        self
    }

    /// The motion this marquee has under `theme`.
    fn resolve(&self, theme: Option<&Theme>) -> MarqueeMotion {
        let t = theme.map(|t| t.widgets.marquee).unwrap_or_default();
        MarqueeMotion {
            velocity: self.velocity.or(t.velocity).unwrap_or(MARQUEE_VELOCITY),
            gap: self.gap.or(t.gap).unwrap_or(MARQUEE_GAP),
            pause: self.pause.or(t.pause).unwrap_or(MARQUEE_PAUSE),
            reverse: self.reverse,
        }
    }

    /// The content and its copy.
    fn build(&self, theme: Option<&Theme>) -> Vec<Box<dyn Widget<Msg>>> {
        let make = || -> Box<dyn Widget<Msg>> {
            match &self.content {
                Content::Text { text, style, color } => {
                    let t = theme.map(|t| t.widgets.marquee).unwrap_or_default();
                    let mut label = match style.or(t.text_style) {
                        Some(style) => crate::Text::styled(text.clone(), style),
                        None => crate::Text::new(text.clone()),
                    }
                    .no_wrap();
                    if let Some(color) = color.or(t.text_color) {
                        label = label.color(color);
                    }
                    Box::new(label)
                }
                Content::Build(build) => build(),
            }
        };
        let content = make();
        let copy = crate::ExcludeSemantics::new(crate::IgnorePointer::new(make()));
        vec![content, Box::new(copy)]
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for Marquee<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    fn build_themed(&self, theme: &Theme) {
        self.motion.set(self.resolve(Some(theme)));
        let _ = self.children.get_or_init(|| self.build(Some(theme)));
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.children.get_or_init(|| self.build(None))
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    /// The content at the width it asks for, as tall as it asks within what is offered;
    /// the box is that, held to the offer.
    fn constraints_transform(&self) -> Option<ConstraintsTransform> {
        Some(
            ConstraintsTransform::new()
                .width(AxisConstraint::Unbounded)
                .height(AxisConstraint::Loose)
                .report(false),
        )
    }

    /// At the start of the line, whichever side that is.
    fn alignment_geometry(&self) -> Option<AlignmentGeometry> {
        Some(AlignmentGeometry::Directional(
            AlignmentDirectional::CENTER_START,
        ))
    }

    fn clips_to_box(&self) -> bool {
        true
    }

    fn marquee(&self) -> Option<MarqueeMotion> {
        Some(self.motion.get())
    }

    fn debug_name(&self) -> &'static str {
        "Marquee"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Size};
    use frus_core::Primitive;

    const LONG: &str = "A headline far too long for the narrow box it has been given";

    /// Every text drawn, with where it starts.
    fn texts(ps: &[Primitive], out: &mut Vec<(f32, String)>) {
        for p in ps {
            match p {
                Primitive::Text { position, text, .. } => out.push((position.x, text.clone())),
                Primitive::Layer { primitives, .. } => texts(primitives, out),
                _ => {}
            }
        }
    }

    fn frame(
        make: &dyn Fn() -> Marquee<()>,
        width: f32,
        runtime: &Runtime,
    ) -> (Vec<(f32, String)>, bool) {
        let screen = crate::Flex::<()>::column()
            .align(frus_layout::Align::Start)
            .child(crate::SizedBox::new(make()).width(width));
        let ui = build_ui(&screen, Size::new(400.0, 200.0), runtime, &Theme::light());
        let mut found = Vec::new();
        texts(ui.scene().primitives(), &mut found);
        (found, ui.wants_animation())
    }

    #[test]
    fn the_lap_waits_runs_and_starts_again() {
        let m = MarqueeMotion {
            velocity: 20.0,
            gap: 10.0,
            pause: 1.0,
            reverse: false,
        };
        let lap = 90.0; // 80 of content and the gap
        assert_eq!(m.travel(0.0, lap), 0.0);
        assert_eq!(m.travel(0.99, lap), 0.0, "still waiting");
        assert!((m.travel(2.0, lap) - 20.0).abs() < 1e-3);
        // The lap is 1 s of wait and 4.5 s of travel: at its end the copy is at the start.
        assert!((m.travel(5.49, lap) - 89.8).abs() < 1e-2);
        assert_eq!(m.travel(5.6, lap), 0.0, "the next lap waits again");
        let still = MarqueeMotion { velocity: 0.0, ..m };
        assert_eq!(still.travel(3.0, lap), 0.0);
        assert!(!still.moves(10.0, 100.0));
    }

    /// **A text that fits is drawn once, where it starts, and asks for no frame.**
    #[test]
    fn a_text_that_fits_is_still() {
        let marquee = || Marquee::<()>::text("Short");
        let (found, animating) = frame(&marquee, 300.0, &Runtime::default());
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, 0.0);
        assert!(!animating, "nothing moves, nothing redraws");
    }

    /// **A text that overflows waits, then runs left with its copy behind it**, and keeps
    /// asking for frames while it does.
    #[test]
    fn an_overflowing_text_scrolls_with_its_copy() {
        let marquee = || {
            Marquee::<()>::text(LONG)
                .style(TextStyle::new(16.0))
                .velocity(50.0)
                .pause(1.0)
                .gap(40.0)
        };
        let mut runtime = Runtime::default();
        runtime.time = 0.0;
        let (start, animating) = frame(&marquee, 120.0, &runtime);
        assert!(animating, "an overflowing marquee asks for frames");
        let content = start.iter().find(|(_, t)| t == LONG).expect("the content");
        assert_eq!(content.0, 0.0, "it waits at its start: {start:?}");
        // Two seconds in: one of wait and one of travel at 50 px/s. Drawn every half
        // second on the way, as a running app would: a marquee unseen for over a second
        // starts its lap again.
        for t in [0.5, 1.0, 1.5] {
            runtime.time = t;
            let _ = frame(&marquee, 120.0, &runtime);
        }
        runtime.time = 2.0;
        let (later, _) = frame(&marquee, 120.0, &runtime);
        let xs: Vec<f32> = later
            .iter()
            .filter(|(_, t)| t == LONG)
            .map(|p| p.0)
            .collect();
        assert_eq!(xs.len(), 2, "the content and its copy: {later:?}");
        assert!((xs[0] + 50.0).abs() < 0.5, "it ran 50 px left: {xs:?}");
        let width = frus_text::measure_resolved(LONG, &TextStyle::new(16.0).resolved()).width;
        assert!(
            (xs[1] - (xs[0] + width + 40.0)).abs() < 2.0,
            "the copy follows a gap behind: {xs:?}, content {width}"
        );
    }

    /// **Reduced motion keeps it at its start**, and nothing asks for a frame.
    #[test]
    fn reduced_motion_keeps_it_still() {
        let marquee = || Marquee::<()>::text(LONG).pause(0.0);
        let mut runtime = Runtime::default();
        runtime.still = true;
        runtime.time = 3.0;
        let (found, animating) = frame(&marquee, 120.0, &runtime);
        assert!(!animating);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, 0.0);
    }

    /// **Reversed, it runs to the right**, its copy ahead of it on the left.
    #[test]
    fn reversed_it_runs_right() {
        let marquee = || {
            Marquee::<()>::text(LONG)
                .pause(0.0)
                .velocity(10.0)
                .reverse(true)
        };
        let mut runtime = Runtime::default();
        runtime.time = 0.0;
        let _ = frame(&marquee, 120.0, &runtime);
        runtime.time = 1.0;
        let (found, _) = frame(&marquee, 120.0, &runtime);
        let xs: Vec<f32> = found.iter().map(|p| p.0).collect();
        assert!(xs.iter().any(|x| (x - 10.0).abs() < 0.5), "{xs:?}");
        assert!(
            xs.iter().any(|x| *x < -100.0),
            "the copy on the left: {xs:?}"
        );
    }

    /// **The theme sets the motion** a marquee did not choose.
    #[test]
    fn the_theme_sets_the_defaults() {
        let mut theme = Theme::light();
        theme.widgets.marquee.velocity = Some(99.0);
        theme.widgets.marquee.pause = Some(0.25);
        let marquee = Marquee::<()>::text("x").gap(5.0);
        let motion = marquee.resolve(Some(&theme));
        assert_eq!(
            (motion.velocity, motion.gap, motion.pause),
            (99.0, 5.0, 0.25)
        );
        assert_eq!(marquee.resolve(None).velocity, MARQUEE_VELOCITY);
    }

    /// **The copy is not read out**: a screen reader hears the content once.
    #[test]
    fn the_copy_is_out_of_the_semantics_tree() {
        let marquee = Marquee::<()>::text(LONG);
        let kids = Widget::<()>::children(&marquee);
        assert_eq!(kids.len(), 2);
        let hidden = |w: &dyn Widget<()>| w.barrier().is_some_and(|b| b.semantics);
        assert!(hidden(kids[1].as_ref()), "the copy is hidden from readers");
        assert!(!hidden(kids[0].as_ref()));
    }
}
