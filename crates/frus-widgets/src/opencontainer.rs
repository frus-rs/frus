// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`OpenContainer`]: **the container transform** (milestone 785) — a card that grows into
//! a full page, and shrinks back into the card.
//!
//! The closed container sits in the layout like any card. Opened, it leaves a placeholder
//! of its own size behind and floats above everything, its bounds, corners, elevation and
//! colour moving from the card's to the window's, its closed content giving way to its
//! open content — as [`container_transform`] says, which is where the numbers are.
//!
//! Where the card is comes from the frame before: the closed container records the box it
//! was painted in, and that box is where the transform starts and where it ends again.

use std::any::Any;
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use frus_core::{Alignment, BorderRadius, Color, Rect, Scene};
use frus_layout::Style;

use crate::interaction::{Status, WidgetId};
use crate::motionpatterns::{container_transform, ContainerColors, ContainerTransitionType};
use crate::runtime::Runtime;
use crate::switcher::Timing;
use crate::theme::Theme;
use crate::widget::Widget;

/// Defaults for [`OpenContainer`], on [`WidgetThemes`](crate::WidgetThemes): each `None`
/// is the framework's own.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OpenContainerTheme {
    /// The closed container's colour. Unset, the scheme's `surface_container_low`, a
    /// card's.
    pub closed_color: Option<Color>,
    /// The open container's colour. Unset, the scheme's `surface`.
    pub open_color: Option<Color>,
    /// The colour a fade through passes through. Unset, the scheme's `surface`.
    pub middle_color: Option<Color>,
    /// The closed container's elevation. Unset, 1.
    pub closed_elevation: Option<f32>,
    /// The open container's elevation. Unset, 4.
    pub open_elevation: Option<f32>,
    /// The closed container's corner radius. Unset, 12, a card's.
    pub closed_radius: Option<f32>,
    /// The open container's corner radius. Unset, 0.
    pub open_radius: Option<f32>,
    /// The scrim over the page while the container moves, at its full strength. Unset,
    /// the scheme's `scrim` at 54 %.
    pub scrim_color: Option<Color>,
    /// How the two contents give way. Unset, [`ContainerTransitionType::Fade`].
    pub transition_type: Option<ContainerTransitionType>,
}

/// A content, when there is one.
type Content<Msg> = Option<Box<dyn Widget<Msg>>>;

/// What an [`OpenContainer`] keeps between frames.
struct Memo {
    /// Where the closed container was last painted.
    rect: Cell<Option<Rect>>,
    /// Whether it is open, for a container that keeps the answer itself.
    open: Cell<bool>,
}

/// Where the contents come from.
enum Contents<Msg> {
    /// Built by the application, told whether it is open.
    Given {
        open: bool,
        closed: RefCell<Option<Box<dyn Widget<Msg>>>>,
        opened: RefCell<Option<Box<dyn Widget<Msg>>>>,
        on_open: Option<Msg>,
        on_close: Option<Msg>,
    },
    /// Built here, from builders handed the message that opens or closes it.
    Built {
        closed: Box<dyn Fn(Msg) -> Box<dyn Widget<Msg>>>,
        opened: Box<dyn Fn(Msg) -> Box<dyn Widget<Msg>>>,
        signal: fn(Rc<Memo>, bool) -> Msg,
    },
}

/// **A card that grows into a page**: the container transform.
///
/// The simplest form keeps whether it is open to itself, and hands each builder the
/// [`Callback`](crate::Callback) that moves it:
///
/// ```
/// use frus_widgets::{Button, OpenContainer, Text};
///
/// let _album = OpenContainer::builder(
///     |open| Button::new("Album").on_press(open),
///     |close| Button::new("Back to the albums").on_press(close),
/// );
/// ```
///
/// A tap anywhere on the closed container opens it too. An application that keeps the
/// answer in its own state says whether it is open, and what to send:
///
/// ```
/// use frus_widgets::{OpenContainer, Text};
///
/// #[derive(Clone)]
/// enum Msg { Open, Close }
///
/// let open = false;
/// let _detail: OpenContainer<Msg> =
///     OpenContainer::new(open, Text::new("Album"), Text::new("All the tracks"))
///         .on_open(Msg::Open)
///         .on_close(Msg::Close);
/// ```
///
/// Escape and the system's Back close an open container. Its colours, elevations, corner
/// radii and scrim come from the theme's [`OpenContainerTheme`], each overridable here;
/// it moves for the theme's `medium2` duration (300 ms) on the "fast out, slow in" curve.
/// With the motion turned down it opens and closes at once.
pub struct OpenContainer<Msg = crate::callback::Callback> {
    contents: Contents<Msg>,
    style: OpenContainerTheme,
    duration: Option<f32>,
    tappable: bool,
    built: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

fn same_bool(a: &dyn Any, b: &dyn Any) -> bool {
    a.downcast_ref::<bool>() == b.downcast_ref::<bool>()
}

impl OpenContainer<crate::callback::Callback> {
    /// A container that keeps whether it is open itself: `closed` is handed the callback
    /// that opens it, `opened` the one that closes it.
    pub fn builder<C, O>(
        closed: impl Fn(crate::Callback) -> C + 'static,
        opened: impl Fn(crate::Callback) -> O + 'static,
    ) -> Self
    where
        C: Widget + 'static,
        O: Widget + 'static,
    {
        Self::with(Contents::Built {
            closed: Box::new(move |open| Box::new(closed(open))),
            opened: Box::new(move |close| Box::new(opened(close))),
            signal: |memo, open| {
                crate::Callback::new(move || {
                    memo.open.set(open);
                    crate::component::request_rebuild();
                })
            },
        })
    }
}

impl<Msg: Clone + 'static> OpenContainer<Msg> {
    /// A container the application opens and closes: `open` says which it is, `closed`
    /// is its content as a card and `opened` as a page.
    pub fn new(
        open: bool,
        closed: impl Widget<Msg> + 'static,
        opened: impl Widget<Msg> + 'static,
    ) -> Self {
        Self::with(Contents::Given {
            open,
            closed: RefCell::new(Some(Box::new(closed))),
            opened: RefCell::new(Some(Box::new(opened))),
            on_open: None,
            on_close: None,
        })
    }

    fn with(contents: Contents<Msg>) -> Self {
        Self {
            contents,
            style: OpenContainerTheme::default(),
            duration: None,
            tappable: true,
            built: OnceCell::new(),
        }
    }

    /// What a tap on the closed container sends.
    pub fn on_open(mut self, message: impl Into<Msg>) -> Self {
        if let Contents::Given { on_open, .. } = &mut self.contents {
            *on_open = Some(message.into());
        }
        self
    }

    /// What Escape and the system's Back send while it is open.
    pub fn on_close(mut self, message: impl Into<Msg>) -> Self {
        if let Contents::Given { on_close, .. } = &mut self.contents {
            *on_close = Some(message.into());
        }
        self
    }

    /// Whether a tap anywhere on the closed container opens it. `true` by default; turn it
    /// off when only a button inside should.
    pub fn tappable(mut self, tappable: bool) -> Self {
        self.tappable = tappable;
        self
    }

    /// How the two contents give way to each other.
    pub fn transition_type(mut self, kind: ContainerTransitionType) -> Self {
        self.style.transition_type = Some(kind);
        self
    }

    /// How long it takes to open or close, in seconds, instead of the theme's `medium2`.
    pub fn duration(mut self, seconds: f32) -> Self {
        self.duration = Some(seconds.max(0.0));
        self
    }

    /// The closed container's colour.
    pub fn closed_color(mut self, color: Color) -> Self {
        self.style.closed_color = Some(color);
        self
    }

    /// The open container's colour.
    pub fn open_color(mut self, color: Color) -> Self {
        self.style.open_color = Some(color);
        self
    }

    /// The colour a fade through passes through.
    pub fn middle_color(mut self, color: Color) -> Self {
        self.style.middle_color = Some(color);
        self
    }

    /// The closed container's elevation.
    pub fn closed_elevation(mut self, elevation: f32) -> Self {
        self.style.closed_elevation = Some(elevation);
        self
    }

    /// The open container's elevation.
    pub fn open_elevation(mut self, elevation: f32) -> Self {
        self.style.open_elevation = Some(elevation);
        self
    }

    /// The closed container's corner radius.
    pub fn closed_radius(mut self, radius: f32) -> Self {
        self.style.closed_radius = Some(radius);
        self
    }

    /// The open container's corner radius.
    pub fn open_radius(mut self, radius: f32) -> Self {
        self.style.open_radius = Some(radius);
        self
    }

    /// The scrim over the page behind, at its full strength.
    pub fn scrim_color(mut self, color: Color) -> Self {
        self.style.scrim_color = Some(color);
        self
    }

    fn compose(&self, at: Option<(WidgetId, &Runtime, &Theme)>) -> Vec<Box<dyn Widget<Msg>>> {
        let Some((id, runtime, theme)) = at else {
            // A bare traversal, before any walk: the closed container, at rest.
            return vec![self.closed_surface(None, None, &Theme::default())];
        };
        let memo = runtime.memo(id, || Memo {
            rect: Cell::new(None),
            open: Cell::new(false),
        });
        let open = match &self.contents {
            Contents::Given { open, .. } => *open,
            Contents::Built { .. } => memo.open.get(),
        };
        let duration = self.duration.unwrap_or(theme.motion.medium2);
        let shown = runtime.presence_to(
            id,
            Rc::new(open),
            same_bool,
            Timing {
                duration,
                reverse: duration,
                switch_in: frus_core::Curve::Linear,
                switch_out: frus_core::Curve::Linear,
            },
            false,
        );
        // How far open, and which way it is going: the clock of the "open" entry.
        let (value, opening) = shown
            .iter()
            .find(|shown| shown.value.downcast_ref::<bool>() == Some(&true))
            .map_or((0.0, false), |shown| (shown.progress, !shown.leaving));
        let (on_open, on_close) = self.messages(&memo);
        if value <= 0.0 && !opening {
            return vec![self.closed_surface(Some(memo), on_open, theme)];
        }
        let (closed, opened) = self.contents(on_open, on_close.clone());
        let window = crate::MediaQuery::of().size;
        let window = Rect::new(0.0, 0.0, window.width, window.height);
        let from =
            memo.rect
                .get()
                .unwrap_or(Rect::new(window.width * 0.5, window.height * 0.5, 0.0, 0.0));
        let placeholder = Probe {
            memo: memo.clone(),
            children: vec![Box::new(
                crate::SizedBox::<Msg>::empty()
                    .width(from.width)
                    .height(from.height),
            )],
        };
        let morph = self.morph(theme, value, opening, from, window, closed, opened);
        vec![Box::new(Portal {
            children: vec![Box::new(placeholder), morph],
            on_close,
        })]
    }

    /// What opens it and what closes it.
    fn messages(&self, memo: &Rc<Memo>) -> (Option<Msg>, Option<Msg>) {
        match &self.contents {
            Contents::Given {
                on_open, on_close, ..
            } => (on_open.clone(), on_close.clone()),
            Contents::Built { signal, .. } => (
                Some(signal(memo.clone(), true)),
                Some(signal(memo.clone(), false)),
            ),
        }
    }

    /// The two contents, built or taken.
    fn contents(
        &self,
        on_open: Option<Msg>,
        on_close: Option<Msg>,
    ) -> (Content<Msg>, Content<Msg>) {
        match &self.contents {
            Contents::Given { closed, opened, .. } => {
                (closed.borrow_mut().take(), opened.borrow_mut().take())
            }
            Contents::Built { closed, opened, .. } => (on_open.map(closed), on_close.map(opened)),
        }
    }

    fn colors(&self, theme: &Theme) -> (ContainerColors, OpenContainerTheme) {
        let themed = &theme.widgets.open_container;
        let style = &self.style;
        let colors = ContainerColors {
            closed: style
                .closed_color
                .or(themed.closed_color)
                .unwrap_or(theme.scheme.surface_container_low),
            open: style
                .open_color
                .or(themed.open_color)
                .unwrap_or(theme.scheme.surface),
            middle: style
                .middle_color
                .or(themed.middle_color)
                .unwrap_or(theme.scheme.surface),
        };
        let resolved = OpenContainerTheme {
            closed_elevation: Some(
                style
                    .closed_elevation
                    .or(themed.closed_elevation)
                    .unwrap_or(1.0),
            ),
            open_elevation: Some(
                style
                    .open_elevation
                    .or(themed.open_elevation)
                    .unwrap_or(4.0),
            ),
            closed_radius: Some(style.closed_radius.or(themed.closed_radius).unwrap_or(12.0)),
            open_radius: Some(style.open_radius.or(themed.open_radius).unwrap_or(0.0)),
            scrim_color: Some(
                style
                    .scrim_color
                    .or(themed.scrim_color)
                    .unwrap_or(theme.scheme.scrim.with_alpha(0x8A as f32 / 255.0)),
            ),
            transition_type: Some(
                style
                    .transition_type
                    .or(themed.transition_type)
                    .unwrap_or_default(),
            ),
            ..OpenContainerTheme::default()
        };
        (colors, resolved)
    }

    /// The closed container at rest: a card that opens when tapped, recording where it is.
    fn closed_surface(
        &self,
        memo: Option<Rc<Memo>>,
        on_open: Option<Msg>,
        theme: &Theme,
    ) -> Box<dyn Widget<Msg>> {
        let (colors, style) = self.colors(theme);
        let radius = style.closed_radius.unwrap_or(12.0);
        let content: Box<dyn Widget<Msg>> = match &self.contents {
            Contents::Given { closed, .. } => closed
                .borrow_mut()
                .take()
                .unwrap_or_else(|| Box::new(crate::SizedBox::empty())),
            Contents::Built { closed, .. } => match on_open.clone() {
                Some(open) => closed(open),
                None => Box::new(crate::SizedBox::empty()),
            },
        };
        let mut surface = crate::Container::new()
            .color(colors.closed)
            .radius(radius)
            .clip()
            .child(content);
        if let Some(open) = on_open.filter(|_| self.tappable) {
            surface = surface.on_click(open);
        }
        let elevated = Elevated {
            elevation: style.closed_elevation.unwrap_or(1.0),
            radius,
            children: vec![Box::new(surface)],
        };
        match memo {
            Some(memo) => Box::new(Probe {
                memo,
                children: vec![Box::new(elevated)],
            }),
            None => Box::new(elevated),
        }
    }

    /// The container between its two states, over the whole window.
    #[allow(clippy::too_many_arguments)]
    fn morph(
        &self,
        theme: &Theme,
        value: f32,
        opening: bool,
        from: Rect,
        window: Rect,
        closed: Option<Box<dyn Widget<Msg>>>,
        opened: Option<Box<dyn Widget<Msg>>>,
    ) -> Box<dyn Widget<Msg>> {
        let (colors, style) = self.colors(theme);
        let kind = style.transition_type.unwrap_or_default();
        let frame = container_transform(value, opening, kind, colors);
        let t = frame.bounds;
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        let rect = Rect::new(
            lerp(from.x, window.x),
            lerp(from.y, window.y),
            lerp(from.width, window.width),
            lerp(from.height, window.height),
        );
        let radius = lerp(
            style.closed_radius.unwrap_or(12.0),
            style.open_radius.unwrap_or(0.0),
        );
        let elevation = lerp(
            style.closed_elevation.unwrap_or(1.0),
            style.open_elevation.unwrap_or(4.0),
        );
        let settled = opening && value >= 1.0;
        // Each content is laid out at its own size — the card's, the window's — and scaled
        // to the container's width from its top left corner, as it fits the width.
        let fitted = |child: Box<dyn Widget<Msg>>, size: Rect, opacity: f32, inert: bool| {
            let scale = if size.width > 0.0 {
                rect.width / size.width
            } else {
                1.0
            };
            crate::Positioned::new(
                crate::Transform::scale_from(scale, Alignment::TOP_LEFT).child(
                    crate::Opacity::new(
                        opacity.clamp(0.0, 1.0),
                        crate::barrier::ExcludeSemantics::new(
                            crate::barrier::IgnorePointer::new(child).ignoring(inert),
                        )
                        .excluding(inert),
                    ),
                ),
            )
            .left(0.0)
            .top(0.0)
            .width(size.width)
            .height(size.height)
        };
        let mut inside = crate::Stack::new().width(rect.width).height(rect.height);
        // Once it is open, the closed content is gone: nothing of it is drawn or touched.
        if let Some(closed) = closed.filter(|_| !settled) {
            inside = inside.layer(fitted(closed, from, frame.closed_opacity, true));
        }
        if let Some(opened) = opened {
            // On the open colour: a page whose own background is transparent still covers
            // the closed content as it fades in, rather than letting both show at once.
            let backed: Box<dyn Widget<Msg>> = Box::new(
                crate::Container::new()
                    .width(window.width)
                    .height(window.height)
                    .color(colors.open)
                    .child(opened),
            );
            inside = inside.layer(fitted(backed, window, frame.open_opacity, !settled));
        }
        let surface = Elevated {
            elevation,
            radius,
            children: vec![Box::new(
                crate::Container::new()
                    .color(frame.color)
                    .radius(radius)
                    .clip()
                    .child(inside),
            )],
        };
        let scrim = style
            .scrim_color
            .unwrap_or(Color::TRANSPARENT)
            .fade(frame.scrim);
        Box::new(
            crate::Stack::new()
                .width(window.width)
                .height(window.height)
                // The page behind takes no input while the container is over it.
                .layer(crate::Positioned::fill(crate::barrier::AbsorbPointer::new(
                    crate::Container::new().color(scrim),
                )))
                .layer(crate::Positioned::from_rect(surface, rect)),
        )
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for OpenContainer<Msg> {
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
        "OpenContainer"
    }
}

/// The placeholder left in the layout, and the container floating above the window: a
/// modal overlay with no scrim of the framework's own — the container paints its own —
/// that Escape and the system's Back close.
struct Portal<Msg> {
    children: Vec<Box<dyn Widget<Msg>>>,
    on_close: Option<Msg>,
}

impl<Msg: Clone> Widget<Msg> for Portal<Msg> {
    fn style(&self) -> Style {
        Style {
            flex_direction: frus_layout::FlexDirection::Column,
            ..Style::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn overlay(&self) -> Option<(&dyn Widget<Msg>, crate::Placement)> {
        self.children
            .get(1)
            .map(|content| (content.as_ref(), crate::Placement::Center))
    }

    fn overlay_dismiss(&self) -> Option<Msg> {
        self.on_close.clone()
    }

    fn overlay_scrim(&self, _theme: &Theme) -> Option<Color> {
        Some(Color::TRANSPARENT)
    }

    fn on_key(&self, key: &crate::Key) -> crate::KeyResponse<Msg> {
        match (key, &self.on_close) {
            (crate::Key::Escape, Some(message)) => {
                crate::KeyResponse::Handled(Some(message.clone()))
            }
            _ => crate::KeyResponse::Ignored,
        }
    }
}

/// Records where its child is painted, for the transform to start from.
struct Probe<Msg> {
    memo: Rc<Memo>,
    children: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: Clone> Widget<Msg> for Probe<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {
        self.memo.rect.set(Some(bounds));
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }
}

/// Paints a shadow at `elevation` under its child's rounded box.
struct Elevated<Msg> {
    elevation: f32,
    radius: f32,
    children: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: Clone> Widget<Msg> for Elevated<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, bounds: Rect, _status: Status, theme: &Theme, scene: &mut Scene) {
        if self.elevation > 0.0 {
            frus_core::paint_elevation(
                scene,
                bounds,
                BorderRadius::from(self.radius),
                self.elevation,
                theme.scheme.shadow,
            );
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }
}

impl Runtime {
    /// What a widget keeps between frames beside its switch (milestone 785): made by
    /// `make` the first time, the same value after, let go with the widget.
    pub(crate) fn memo<T: 'static>(&self, id: WidgetId, make: impl FnOnce() -> T) -> Rc<T> {
        let mut memos = self.memos.borrow_mut();
        if let Some(kept) = memos
            .get(&id)
            .and_then(|kept| kept.clone().downcast::<T>().ok())
        {
            return kept;
        }
        let made = Rc::new(make());
        memos.insert(id, made.clone());
        made
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{build_ui, Ui};
    use crate::{Container, Flex, Size};
    use frus_core::{Point, Primitive};

    const WINDOW: Size = Size::new(400.0, 300.0);

    fn card() -> Color {
        Color::rgb(1.0, 0.0, 0.0)
    }

    fn page() -> Color {
        Color::rgb(0.0, 0.0, 1.0)
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Open,
        Close,
        Inside,
    }

    fn view(open: bool) -> Flex<Msg> {
        Flex::column().padding(20.0).child(
            OpenContainer::new(
                open,
                Container::new().width(100.0).height(50.0).color(card()),
                Container::new()
                    .width(400.0)
                    .height(300.0)
                    .color(page())
                    .on_click(Msg::Inside),
            )
            .on_open(Msg::Open)
            .on_close(Msg::Close),
        )
    }

    fn surface() -> crate::SurfaceGuard {
        crate::MediaQuery {
            size: WINDOW,
            ..crate::MediaQuery::of()
        }
        .install()
    }

    fn step(rt: &mut Runtime, tree: &dyn Widget<Msg>, dt: f32) -> Ui<Msg> {
        let ui = build_ui(tree, WINDOW, rt, &Theme::dark());
        rt.advance_switchers(tree, dt);
        ui
    }

    /// The box the colour `c` is painted in, through the transforms above it.
    fn box_of(ui: &Ui<Msg>, c: Color) -> Option<Rect> {
        fn walk(primitives: &[Primitive], c: Color, m: frus_core::Affine) -> Option<Rect> {
            primitives.iter().find_map(|p| match p {
                Primitive::Rect { rect, color, .. } if *color == c => Some(m.apply_rect(*rect)),
                Primitive::Layer {
                    primitives,
                    transform,
                    ..
                } => {
                    let inner = transform.map_or(m, |t| m.then(t.affine));
                    walk(primitives, c, inner)
                }
                _ => None,
            })
        }
        walk(ui.scene().primitives(), c, frus_core::Affine::IDENTITY)
    }

    fn message_at(ui: &Ui<Msg>, point: Point) -> Option<Msg> {
        ui.hit(point).and_then(|id| ui.msg_for(id))
    }

    /// **Closed, it is a card that opens when tapped**; opened, the container grows from
    /// the card's box to the window's on the "fast out, slow in" curve, and once open the
    /// page is the window's and takes the taps; closed again, it shrinks back into the card.
    #[test]
    fn the_container_grows_from_the_card_to_the_window_and_back() {
        let _surface = surface();
        let mut rt = Runtime::default();
        let ui = step(&mut rt, &view(false), 0.0);
        let closed = box_of(&ui, card()).expect("the card is drawn");
        assert_eq!((closed.x, closed.y, closed.width), (20.0, 20.0, 100.0));
        assert_eq!(message_at(&ui, Point::new(50.0, 40.0)), Some(Msg::Open));

        // Opened: a third of the way through its 300 ms.
        step(&mut rt, &view(true), 0.0);
        step(&mut rt, &view(true), 0.1);
        let ui = step(&mut rt, &view(true), 0.0);
        let frame = container_transform(
            1.0 / 3.0,
            true,
            ContainerTransitionType::Fade,
            ContainerColors {
                closed: card(),
                open: page(),
                middle: page(),
            },
        );
        let page_box = box_of(&ui, page()).expect("the page is drawn, scaled");
        let expected_width = 100.0 + (400.0 - 100.0) * frame.bounds;
        assert!(
            (page_box.width - expected_width).abs() < 1.0,
            "{} for {expected_width}",
            page_box.width
        );
        assert!(
            page_box.x > 0.0 && page_box.x < 20.0,
            "on its way: {}",
            page_box.x
        );
        assert_eq!(
            message_at(&ui, Point::new(200.0, 200.0)),
            None,
            "nothing is tapped while it moves"
        );

        for _ in 0..30 {
            step(&mut rt, &view(true), 0.02);
        }
        let ui = step(&mut rt, &view(true), 0.0);
        let open = box_of(&ui, page()).unwrap();
        assert_eq!((open.x, open.y, open.width), (0.0, 0.0, 400.0));
        assert_eq!(message_at(&ui, Point::new(200.0, 200.0)), Some(Msg::Inside));

        step(&mut rt, &view(false), 0.0);
        for _ in 0..30 {
            step(&mut rt, &view(false), 0.02);
        }
        let ui = step(&mut rt, &view(false), 0.0);
        assert_eq!(box_of(&ui, page()), None, "the page is gone");
        assert_eq!(box_of(&ui, card()).map(|r| r.x), Some(20.0));
        assert_eq!(message_at(&ui, Point::new(50.0, 40.0)), Some(Msg::Open));
    }

    /// **A container that keeps its own state** opens from the callback its closed builder
    /// is handed; with the motion turned down it is open at once.
    #[test]
    fn a_built_container_opens_from_its_callback() {
        let _surface = surface();
        let view = || {
            Flex::column().child(OpenContainer::builder(
                |open| crate::Button::new("open").on_press(open),
                |close| crate::Button::new("close").on_press(close),
            ))
        };
        let mut rt = Runtime::default();
        rt.still = true;
        let ui = build_ui(&view(), WINDOW, &rt, &Theme::dark());
        rt.advance_switchers(&view(), 0.0);
        let open = ui
            .hit(Point::new(10.0, 10.0))
            .and_then(|id| ui.msg_for(id))
            .expect("the card is tappable");
        open.call();
        let ui = build_ui(&view(), WINDOW, &rt, &Theme::dark());
        assert!(
            ui.over_overlay(Point::new(390.0, 290.0)),
            "open at once, over the window"
        );
    }
}
