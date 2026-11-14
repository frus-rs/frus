// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`HoverCard`]: a card that opens when the pointer **rests** on something, and stays open
//! while the pointer is on the card (milestone 809).
//!
//! # Where "is it open" lives
//!
//! In the runtime, not in the application: whether the pointer has rested long enough is a
//! question of time, and an application should not need a timer to show a preview. The walk
//! that builds each frame knows where the pointer is ([`InputState::pointer`]) and what time it
//! is, and keeps one small record per card: whether the pointer is over the anchor or the
//! card, and since when. While a delay is running the frame asks for the next one; when
//! nothing is running nothing is drawn.
//!
//! An application that wants to say itself whether the card is open — a card opened by a
//! button, or kept open in a test — gives [`HoverCard::open`].
//!
//! [`InputState::pointer`]: crate::InputState::pointer

use std::rc::Rc;

use frus_core::{BorderRadius, BorderSide, Color, Insets, Point, Rect, Scene, ShapeBorder, Size};
use frus_layout::{Dimension, FlexDirection, Style};

use crate::interaction::Status;
use crate::portal::Placement;
use crate::theme::Theme;
use crate::transparent::Shared;
use crate::widget::Widget;

/// How long the pointer rests on the anchor before the card opens, in seconds, when neither
/// the card nor the theme says. Long enough that a pointer passing over a link on its way
/// somewhere else opens nothing.
pub const HOVER_CARD_OPEN_DELAY: f32 = 0.7;
/// How long the card stays once the pointer has left both the anchor and the card, in
/// seconds: room to cross the gap between them, or to come back.
pub const HOVER_CARD_CLOSE_DELAY: f32 = 0.3;
/// How long a finger rests on the anchor before the card opens: a long press.
pub const HOVER_CARD_TOUCH_DELAY: f32 = 0.5;

/// The room between the anchor and the card, in pixels, unless said otherwise.
const GAP: f32 = 6.0;
/// The room inside the card, round its content.
const PADDING: f32 = 16.0;
/// How wide a card may get, unless said otherwise.
const MAX_WIDTH: f32 = 320.0;
/// How far off the page the card sits: a menu's height, since it floats as a menu does.
const ELEVATION: f32 = 3.0;

/// **Which side of its anchor** a floating card opens on (milestone 809).
///
/// `Start` and `End` follow the text: in a right-to-left layout the start is the right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CardSide {
    /// Above the anchor.
    Top,
    /// Below the anchor — the default.
    #[default]
    Bottom,
    /// Before the anchor, on the side the text starts from.
    Start,
    /// After the anchor, on the side the text ends on.
    End,
}

/// **How a floating card lines up with its anchor** along the side it opens on.
///
/// On the top and the bottom, `Start` lines up the start edges (the left in left-to-right
/// text), `End` the end edges. On the start and the end sides, `Start` lines up the tops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CardAlign {
    /// The start edges, or the tops.
    #[default]
    Start,
    /// Centred on the anchor.
    Center,
    /// The end edges, or the bottoms.
    End,
}

/// **Where a card of `size` goes against `anchor`**, its top-left corner: on `side`, lined up
/// by `align`; **flipped** to the opposite side when it does not fit on its own and does on
/// the other; then **shifted** along so that it stays inside `window`.
///
/// `size` includes the gap between the anchor and the card, which the card keeps round itself.
pub(crate) fn place_card(
    anchor: Rect,
    size: Size,
    window: Rect,
    side: CardSide,
    align: CardAlign,
    rtl: bool,
) -> (f32, f32) {
    let (w, h) = (size.width, size.height);
    let (left, top) = (window.x, window.y);
    let (right, bottom) = (window.x + window.width, window.y + window.height);
    // The physical side: the start is the left, unless the text runs right to left.
    #[derive(Clone, Copy, PartialEq)]
    enum Physical {
        Top,
        Bottom,
        Left,
        Right,
    }
    let physical = match (side, rtl) {
        (CardSide::Top, _) => Physical::Top,
        (CardSide::Bottom, _) => Physical::Bottom,
        (CardSide::Start, false) | (CardSide::End, true) => Physical::Left,
        (CardSide::Start, true) | (CardSide::End, false) => Physical::Right,
    };
    let fits = |p: Physical| match p {
        Physical::Top => anchor.y - h >= top,
        Physical::Bottom => anchor.y + anchor.height + h <= bottom,
        Physical::Left => anchor.x - w >= left,
        Physical::Right => anchor.x + anchor.width + w <= right,
    };
    let opposite = match physical {
        Physical::Top => Physical::Bottom,
        Physical::Bottom => Physical::Top,
        Physical::Left => Physical::Right,
        Physical::Right => Physical::Left,
    };
    let physical = if !fits(physical) && fits(opposite) {
        opposite
    } else {
        physical
    };
    // Along the side: the start edge is the left in left-to-right text, and on the sides
    // themselves it is the top.
    let along = |start: f32, length: f32, extent: f32, mirrored: bool| match (align, mirrored) {
        (CardAlign::Center, _) => start + (length - extent) * 0.5,
        (CardAlign::Start, false) | (CardAlign::End, true) => start,
        (CardAlign::End, false) | (CardAlign::Start, true) => start + length - extent,
    };
    let (x, y) = match physical {
        Physical::Top => (along(anchor.x, anchor.width, w, rtl), anchor.y - h),
        Physical::Bottom => (
            along(anchor.x, anchor.width, w, rtl),
            anchor.y + anchor.height,
        ),
        Physical::Left => (anchor.x - w, along(anchor.y, anchor.height, h, false)),
        Physical::Right => (
            anchor.x + anchor.width,
            along(anchor.y, anchor.height, h, false),
        ),
    };
    // Shifted inside the window; a card larger than the window keeps its start corner on it.
    (x.min(right - w).max(left), y.min(bottom - h).max(top))
}

/// **The timing of a hover card**, as a widget gives it to the walk
/// ([`Widget::hover_card`](crate::Widget::hover_card)). Every delay is in seconds, and unset
/// is the theme's, then the framework's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HoverCardTiming {
    /// How long the pointer rests on the anchor before the card opens.
    pub open_delay: Option<f32>,
    /// How long the card stays once the pointer has left the anchor and the card.
    pub close_delay: Option<f32>,
    /// How long a finger rests on the anchor before the card opens.
    pub touch_delay: Option<f32>,
    /// Whether the card is open, when the application says so itself; `None` leaves it to
    /// the pointer.
    pub open: Option<bool>,
}

/// The delays of one card, resolved against the theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Delays {
    pub(crate) open: f32,
    pub(crate) close: f32,
    pub(crate) touch: f32,
}

impl HoverCardTiming {
    /// The caller's delays, then the theme's, then the framework's.
    pub(crate) fn resolve(&self, theme: &Theme) -> Delays {
        let t = &theme.widgets.hover_card;
        Delays {
            open: self
                .open_delay
                .or(t.open_delay)
                .unwrap_or(HOVER_CARD_OPEN_DELAY),
            close: self
                .close_delay
                .or(t.close_delay)
                .unwrap_or(HOVER_CARD_CLOSE_DELAY),
            touch: self
                .touch_delay
                .or(t.touch_delay)
                .unwrap_or(HOVER_CARD_TOUCH_DELAY),
        }
    }
}

/// **What the runtime keeps about one card** between frames: whether the pointer was over it
/// (the anchor or the card), since when, and whether it is open.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Intent {
    pub(crate) over: bool,
    pub(crate) since: f32,
    pub(crate) open: bool,
}

/// One frame of a card's timing, at time `now`: the pointer is at `pointer` (a finger if
/// `touch`), and `inside` says whether a point is over the anchor or the card. Returns whether
/// a delay is still **running**, and so whether another frame is needed.
///
/// - A mouse opens the card after resting `open` seconds, and closes it `close` seconds after
///   leaving both the anchor and the card.
/// - A finger opens it after resting `touch` seconds on the anchor — a long press — and the
///   card **stays** when the finger lifts: there is nothing to rest on a screen that is not
///   touched. The next touch outside the anchor and the card closes it, at once.
pub(crate) fn step(
    intent: &mut Intent,
    pointer: Option<Point>,
    touch: bool,
    now: f32,
    delays: Delays,
    inside: impl Fn(Point) -> bool,
) -> bool {
    if pointer.is_none() && touch {
        // A finger lifted: the card stays as it is, and the next touch starts afresh.
        if intent.over {
            intent.over = false;
            intent.since = now;
        }
        return false;
    }
    let over = pointer.is_some_and(inside);
    if over != intent.over {
        intent.over = over;
        intent.since = now;
    }
    let elapsed = now - intent.since;
    if over && !intent.open {
        let delay = if touch { delays.touch } else { delays.open };
        if elapsed >= delay {
            intent.open = true;
            return false;
        }
        return true;
    }
    if !over && intent.open {
        let delay = if touch { 0.0 } else { delays.close };
        if elapsed >= delay {
            intent.open = false;
            return false;
        }
        return true;
    }
    false
}

/// **What a caller said about a card's surface**, every word optional; the theme's, then
/// the framework's, fill in the rest. Shared by the hover card and the command palette.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SurfaceStyle {
    pub(crate) background: Option<Color>,
    pub(crate) shape: Option<ShapeBorder>,
    pub(crate) elevation: Option<f32>,
    pub(crate) shadow_color: Option<Color>,
    pub(crate) border: Option<BorderSide>,
    pub(crate) padding: Option<Insets>,
    pub(crate) width: Option<f32>,
    pub(crate) max_width: Option<f32>,
}

/// A surface's look, resolved for one theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceLook {
    pub(crate) background: Color,
    pub(crate) shape: ShapeBorder,
    pub(crate) elevation: f32,
    pub(crate) shadow_color: Color,
    pub(crate) border: Option<BorderSide>,
    pub(crate) padding: Insets,
    pub(crate) width: Option<f32>,
    pub(crate) max_width: Option<f32>,
}

impl SurfaceStyle {
    /// This style, with `other`'s answer wherever this one says nothing.
    pub(crate) fn or(self, other: SurfaceStyle) -> SurfaceStyle {
        SurfaceStyle {
            background: self.background.or(other.background),
            shape: self.shape.or(other.shape),
            elevation: self.elevation.or(other.elevation),
            shadow_color: self.shadow_color.or(other.shadow_color),
            border: self.border.or(other.border),
            padding: self.padding.or(other.padding),
            width: self.width.or(other.width),
            max_width: self.max_width.or(other.max_width),
        }
    }

    /// Fills what is still unset with the framework's: a menu's `surface_container`, the
    /// theme's corner, a menu's height, the scheme's shadow, a hairline in
    /// `outline_variant`, sixteen pixels of room, and at most 320 across.
    pub(crate) fn resolve(&self, theme: &Theme) -> SurfaceLook {
        SurfaceLook {
            background: self.background.unwrap_or(theme.scheme.surface_container),
            shape: self
                .shape
                .unwrap_or(ShapeBorder::rounded(BorderRadius::uniform(theme.radius))),
            elevation: self.elevation.unwrap_or(ELEVATION),
            shadow_color: self.shadow_color.unwrap_or(theme.scheme.shadow),
            border: self
                .border
                .or(Some(BorderSide::new(theme.scheme.outline_variant, 1.0))),
            padding: self.padding.unwrap_or(Insets::uniform(PADDING)),
            width: self.width,
            max_width: self.max_width,
        }
    }
}

/// **A floating surface**: one rounded panel with a shadow and a hairline, its content inside
/// it. What a hover card shows, and what a command palette is drawn on.
pub(crate) struct Surface<Msg> {
    pub(crate) children: Vec<Box<dyn Widget<Msg>>>,
    /// What the caller said, already merged over the widget's theme; resolved at layout.
    pub(crate) style: SurfaceStyle,
    /// The room kept **outside** the surface, transparent: a card's gap from its anchor.
    /// Unset, `themed_gap`'s answer.
    pub(crate) gap: Option<f32>,
    /// The gap when the caller gave none: the theme's, then the framework's.
    pub(crate) themed_gap: fn(&Theme) -> f32,
    /// What the surface says to a screen reader.
    pub(crate) semantics: Option<frus_core::SemanticsProperties>,
    /// Where the theme's word comes from: the hover card's, or the command palette's.
    pub(crate) themed: fn(&Theme) -> SurfaceStyle,
}

impl<Msg> Surface<Msg> {
    fn look(&self, theme: &Theme) -> SurfaceLook {
        self.style.or((self.themed)(theme)).resolve(theme)
    }

    fn margin(&self, theme: &Theme) -> f32 {
        self.gap
            .unwrap_or_else(|| (self.themed_gap)(theme))
            .max(0.0)
    }

    fn sizing(&self, theme: Option<&Theme>) -> Style {
        let fallback;
        let theme = match theme {
            Some(theme) => theme,
            None => {
                fallback = Theme::default();
                &fallback
            }
        };
        let look = self.look(theme);
        let margin = self.margin(theme);
        let border = look.border.map_or(0.0, |b| b.width);
        let pad = look.padding;
        Style {
            flex_direction: FlexDirection::Column,
            // A definite width, so that wrapped content is measured at the width it is drawn
            // at: a card that sized itself to its words measured them unwrapped and then drew
            // them wrapped, one line short (seen in the demo, milestone 809).
            width: Dimension::Length(
                look.width.unwrap_or(look.max_width.unwrap_or(MAX_WIDTH)) + 2.0 * margin,
            ),
            padding: Insets::new(
                pad.top + border + margin,
                pad.right + border + margin,
                pad.bottom + border + margin,
                pad.left + border + margin,
            ),
            ..Default::default()
        }
    }
}

impl<Msg: Clone> Widget<Msg> for Surface<Msg> {
    fn style(&self) -> Style {
        self.sizing(None)
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        self.sizing(Some(theme))
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let o = status.opacity;
        let look = self.look(theme);
        let m = self.margin(theme);
        let card = Rect::new(
            bounds.x + m,
            bounds.y + m,
            (bounds.width - 2.0 * m).max(0.0),
            (bounds.height - 2.0 * m).max(0.0),
        );
        let radius = look
            .shape
            .as_rounded(card)
            .map(|(_, r)| r)
            .unwrap_or(BorderRadius::ZERO);
        // A height casts only in a colour that shows (milestone 529).
        if look.elevation > 0.0 && look.shadow_color.a > 0.0 {
            frus_core::paint_elevation(
                scene,
                card,
                radius,
                look.elevation,
                look.shadow_color.fade(o),
            );
        }
        let shape = match look.border {
            Some(side) if side.width > 0.0 => look
                .shape
                .with_side(BorderSide::new(side.color.fade(o), side.width)),
            _ => look.shape,
        };
        scene.draw_shape(card, shape, look.background.fade(o));
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    /// A press on the surface stays there: it is not the page's behind it.
    fn opaque(&self) -> bool {
        true
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        // A surface answers nothing: whether what floats on it is enabled is its owner's.
        self.semantics.clone()
    }

    fn debug_name(&self) -> &'static str {
        "Surface"
    }
}

/// Defaults for [`HoverCard`] (milestone 809). Every field unset is the framework's.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HoverCardTheme {
    /// The card's surface. Unset, `surface_container`, a menu's.
    pub background: Option<Color>,
    /// What shape the card is. Unset, rounded by the theme's radius.
    pub shape: Option<ShapeBorder>,
    /// How far off the page the card sits. Unset, three, a menu's.
    pub elevation: Option<f32>,
    /// The colour of its shadow; [`Color::TRANSPARENT`] casts none.
    pub shadow_color: Option<Color>,
    /// Its outline. Unset, a hairline in `outline_variant`.
    pub border: Option<BorderSide>,
    /// The room inside the card, round its content. Unset, sixteen.
    pub padding: Option<Insets>,
    /// How wide the card is. Unset, [`max_width`](Self::max_width).
    pub width: Option<f32>,
    /// How wide the card is when no width is given. Unset, 320.
    pub max_width: Option<f32>,
    /// The room between the anchor and the card. Unset, six.
    pub gap: Option<f32>,
    /// How long the pointer rests before the card opens, in seconds. Unset, 0.7.
    pub open_delay: Option<f32>,
    /// How long the card stays after the pointer leaves it, in seconds. Unset, 0.3.
    pub close_delay: Option<f32>,
    /// How long a finger rests before the card opens, in seconds. Unset, 0.5.
    pub touch_delay: Option<f32>,
}

impl HoverCardTheme {
    fn surface(theme: &Theme) -> SurfaceStyle {
        let t = &theme.widgets.hover_card;
        SurfaceStyle {
            background: t.background,
            shape: t.shape,
            elevation: t.elevation,
            shadow_color: t.shadow_color,
            border: t.border,
            padding: t.padding,
            width: t.width,
            max_width: t.max_width,
        }
    }
}

/// **A card that opens when the pointer rests on its anchor** — a preview of a profile
/// behind a name, of a page behind a link.
///
/// ```
/// use frus_widgets::{column, HoverCard, Text};
///
/// let _card: HoverCard = HoverCard::new(Text::new("@ada"))
///     .content(column![Text::new("Ada Lovelace"), Text::new("Wrote the first program.")]);
/// ```
///
/// - With a **mouse**, the card opens once the pointer has rested on the anchor for
///   [`open_delay`](Self::open_delay) (0.7 s), stays while the pointer is on the anchor or on
///   the card, and closes [`close_delay`](Self::close_delay) (0.3 s) after it has left both.
///   A pointer that only passes over the anchor opens nothing.
/// - With a **finger**, a long press on the anchor opens it ([`touch_delay`](Self::touch_delay),
///   0.5 s); it stays when the finger lifts, and the next touch anywhere else closes it.
/// - It opens **below** the anchor, lined up with its start edge, unless told another
///   [`side`](Self::side) or [`align`](Self::align); where there is no room on that side it
///   opens on the other, and it is moved along to stay inside the window.
///
/// The card is **supplementary**: what it shows must be reachable another way, since a
/// keyboard and a screen reader do not rest on anything. Its content is read once it is open.
///
/// Every part of its look — surface, corner, height, shadow, outline, room, width, gap — and
/// every delay is the caller's, then [`HoverCardTheme`]'s, then the framework's.
pub struct HoverCard<Msg = crate::callback::Callback> {
    /// `[anchor]`, or `[anchor, card]` once there is content.
    children: Vec<Box<dyn Widget<Msg>>>,
    content: Option<Rc<dyn Widget<Msg>>>,
    style: SurfaceStyle,
    gap: Option<f32>,
    side: CardSide,
    align: CardAlign,
    timing: HoverCardTiming,
    enabled: bool,
}

impl<Msg: Clone + 'static> HoverCard<Msg> {
    /// A card that will open over `anchor`. Give it something to show with
    /// [`content`](Self::content).
    pub fn new(anchor: impl Widget<Msg> + 'static) -> Self {
        Self {
            children: vec![Box::new(anchor)],
            content: None,
            style: SurfaceStyle::default(),
            gap: None,
            side: CardSide::Bottom,
            align: CardAlign::Start,
            timing: HoverCardTiming::default(),
            enabled: true,
        }
    }

    /// What the card shows.
    #[must_use]
    pub fn content(mut self, content: impl Widget<Msg> + 'static) -> Self {
        self.content = Some(Rc::new(content));
        self.rebuild()
    }

    /// **Which side of the anchor** the card opens on. Below, unless told.
    #[must_use]
    pub fn side(mut self, side: CardSide) -> Self {
        self.side = side;
        self
    }

    /// **How the card lines up** with the anchor along that side. The start edges, unless told.
    #[must_use]
    pub fn align(mut self, align: CardAlign) -> Self {
        self.align = align;
        self
    }

    /// The room between the anchor and the card, over the theme's and six.
    #[must_use]
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = Some(gap);
        self.rebuild()
    }

    /// The card's surface, over the theme's and `surface_container`.
    #[must_use]
    pub fn background(mut self, color: Color) -> Self {
        self.style.background = Some(color);
        self.rebuild()
    }

    /// What shape the card is, over the theme's and the theme's corner.
    #[must_use]
    pub fn shape(mut self, shape: ShapeBorder) -> Self {
        self.style.shape = Some(shape);
        self.rebuild()
    }

    /// The shorthand for a rounded rectangle.
    #[must_use]
    pub fn radius(self, radius: impl Into<BorderRadius>) -> Self {
        self.shape(ShapeBorder::rounded(radius.into()))
    }

    /// How far off the page the card sits, over the theme's and three.
    #[must_use]
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.style.elevation = Some(elevation);
        self.rebuild()
    }

    /// The colour of its shadow; [`Color::TRANSPARENT`] casts none.
    #[must_use]
    pub fn shadow_color(mut self, color: Color) -> Self {
        self.style.shadow_color = Some(color);
        self.rebuild()
    }

    /// Its outline, over the theme's and a hairline in `outline_variant`;
    /// [`BorderSide::NONE`] draws none.
    #[must_use]
    pub fn border(mut self, side: BorderSide) -> Self {
        self.style.border = Some(side);
        self.rebuild()
    }

    /// The room inside the card, round its content.
    #[must_use]
    pub fn padding(mut self, padding: Insets) -> Self {
        self.style.padding = Some(padding);
        self.rebuild()
    }

    /// How wide the card is. Unset, the theme's, then [`max_width`](Self::max_width).
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.style.width = Some(width);
        self.rebuild()
    }

    /// How wide the card is when no [`width`](Self::width) is given, over the theme's and 320.
    #[must_use]
    pub fn max_width(mut self, width: f32) -> Self {
        self.style.max_width = Some(width);
        self.rebuild()
    }

    /// How long the pointer rests on the anchor before the card opens, in seconds.
    #[must_use]
    pub fn open_delay(mut self, seconds: f32) -> Self {
        self.timing.open_delay = Some(seconds.max(0.0));
        self
    }

    /// How long the card stays once the pointer has left the anchor and the card, in seconds.
    #[must_use]
    pub fn close_delay(mut self, seconds: f32) -> Self {
        self.timing.close_delay = Some(seconds.max(0.0));
        self
    }

    /// How long a finger rests on the anchor before the card opens, in seconds.
    #[must_use]
    pub fn touch_delay(mut self, seconds: f32) -> Self {
        self.timing.touch_delay = Some(seconds.max(0.0));
        self
    }

    /// **Says whether the card is open**, instead of the pointer: `true` keeps it open, `false`
    /// keeps it shut. For a card an application opens itself, and for tests.
    #[must_use]
    pub fn open(mut self, open: bool) -> Self {
        self.timing.open = Some(open);
        self
    }

    /// Whether the card opens at all. A disabled card is its anchor and nothing more.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.rebuild()
    }

    /// (Re)builds the card (child 1), so that the builders can be written in any order.
    fn rebuild(mut self) -> Self {
        self.children.truncate(1);
        if let (true, Some(content)) = (self.enabled, &self.content) {
            self.children.push(Box::new(Surface {
                children: vec![Box::new(Shared::new(content.clone()))],
                style: self.style,
                gap: self.gap,
                themed_gap: |theme| theme.widgets.hover_card.gap.unwrap_or(GAP),
                semantics: None,
                themed: HoverCardTheme::surface,
            }));
        }
        self
    }
}

impl<Msg: Clone> Widget<Msg> for HoverCard<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    /// A card is not a box round its anchor: it is its anchor, with something that may float
    /// over it. The anchor's structure is forwarded, its padding left to the anchor itself.
    fn style_themed(&self, theme: &Theme) -> Style {
        match self.children.first() {
            Some(anchor) => Style {
                padding: Insets::ZERO,
                ..anchor.style_themed(theme)
            },
            None => Style::default(),
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn overlay(&self) -> Option<(&dyn Widget<Msg>, Placement)> {
        self.children
            .get(1)
            .map(|card| (card.as_ref(), Placement::Card(self.side, self.align)))
    }

    fn hover_card(&self) -> Option<HoverCardTiming> {
        (self.enabled && self.children.len() > 1).then_some(self.timing)
    }

    /// An anchor whose card the application opens says whether it is open. One the pointer
    /// opens says nothing here: its state is the runtime's, and the card's content is read
    /// once it shows.
    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        // A card that is not enabled is no card: `hover_card` answers nothing then.
        let open = self.timing.open.filter(|_| self.hover_card().is_some())?;
        Some(frus_core::SemanticsProperties::new(frus_core::Role::None).expanded(open))
    }

    fn debug_name(&self) -> &'static str {
        "HoverCard"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Container, Runtime};
    use frus_core::Primitive;

    const WINDOW: Rect = Rect::new(0.0, 0.0, 400.0, 300.0);
    const MARK: Color = Color::rgb(0.9, 0.1, 0.1);

    fn place(anchor: Rect, side: CardSide, align: CardAlign, rtl: bool) -> (f32, f32) {
        place_card(anchor, Size::new(100.0, 50.0), WINDOW, side, align, rtl)
    }

    #[test]
    fn a_card_opens_on_its_side_lined_up_as_told() {
        let anchor = Rect::new(150.0, 100.0, 40.0, 20.0);
        let start = CardAlign::Start;
        assert_eq!(
            place(anchor, CardSide::Bottom, start, false),
            (150.0, 120.0)
        );
        assert_eq!(place(anchor, CardSide::Top, start, false), (150.0, 50.0));
        assert_eq!(place(anchor, CardSide::End, start, false), (190.0, 100.0));
        assert_eq!(place(anchor, CardSide::Start, start, false), (50.0, 100.0));
        assert_eq!(
            place(anchor, CardSide::Bottom, CardAlign::Center, false),
            (120.0, 120.0)
        );
        assert_eq!(
            place(anchor, CardSide::Bottom, CardAlign::End, false),
            (90.0, 120.0)
        );
    }

    /// Right to left, the start is the right: the start edges line up on the right, and the
    /// start side is the right one.
    #[test]
    fn right_to_left_the_start_is_the_right() {
        let anchor = Rect::new(150.0, 100.0, 40.0, 20.0);
        assert_eq!(
            place(anchor, CardSide::Bottom, CardAlign::Start, true),
            (90.0, 120.0)
        );
        assert_eq!(
            place(anchor, CardSide::Start, CardAlign::Start, true),
            (190.0, 100.0)
        );
    }

    /// No room below: above. No room at the window's right: moved back inside it. No room at
    /// the end: on the start side.
    #[test]
    fn a_card_flips_and_shifts_to_stay_in_the_window() {
        let low = Rect::new(150.0, 270.0, 40.0, 20.0);
        assert_eq!(
            place(low, CardSide::Bottom, CardAlign::Start, false),
            (150.0, 220.0)
        );
        let right = Rect::new(380.0, 10.0, 15.0, 20.0);
        assert_eq!(
            place(right, CardSide::Bottom, CardAlign::Start, false),
            (300.0, 30.0)
        );
        let edge = Rect::new(330.0, 100.0, 40.0, 20.0);
        assert_eq!(
            place(edge, CardSide::End, CardAlign::Start, false),
            (230.0, 100.0)
        );
    }

    const DELAYS: Delays = Delays {
        open: 0.7,
        close: 0.3,
        touch: 0.5,
    };

    fn inside(p: Point) -> bool {
        p.x < 10.0
    }

    /// A mouse opens it after resting, keeps it while over it, closes it after leaving; a
    /// pointer that only passes over opens nothing.
    #[test]
    fn a_mouse_rests_to_open_and_leaves_to_close() {
        let mut intent = Intent::default();
        let over = Some(Point::new(1.0, 1.0));
        let away = Some(Point::new(500.0, 500.0));
        assert!(
            step(&mut intent, over, false, 1.0, DELAYS, inside),
            "waiting"
        );
        assert!(step(&mut intent, over, false, 1.6, DELAYS, inside));
        assert!(!intent.open);
        assert!(!step(&mut intent, over, false, 1.75, DELAYS, inside));
        assert!(intent.open, "open after 0.7 s");
        assert!(
            step(&mut intent, away, false, 2.0, DELAYS, inside),
            "closing"
        );
        assert!(intent.open, "still open during the close delay");
        assert!(!step(&mut intent, over, false, 2.2, DELAYS, inside));
        assert!(intent.open, "back in time: it stays");
        step(&mut intent, away, false, 3.0, DELAYS, inside);
        assert!(!step(&mut intent, away, false, 3.31, DELAYS, inside));
        assert!(!intent.open, "closed 0.3 s after leaving");
        step(&mut intent, over, false, 4.0, DELAYS, inside);
        step(&mut intent, away, false, 4.2, DELAYS, inside);
        assert!(!step(&mut intent, away, false, 5.0, DELAYS, inside));
        assert!(!intent.open, "passing over opens nothing");
        // The mouse leaving the window is the pointer leaving the card.
        intent.open = true;
        step(&mut intent, None, false, 6.0, DELAYS, inside);
        step(&mut intent, None, false, 6.4, DELAYS, inside);
        assert!(!intent.open);
    }

    /// A finger opens it with a long press, the card stays when it lifts, and the next touch
    /// outside closes it at once.
    #[test]
    fn a_finger_long_presses_to_open_and_touches_elsewhere_to_close() {
        let mut intent = Intent::default();
        let on = Some(Point::new(1.0, 1.0));
        step(&mut intent, on, true, 1.0, DELAYS, inside);
        assert!(!step(&mut intent, on, true, 1.5, DELAYS, inside));
        assert!(intent.open, "a long press opens it");
        assert!(!step(&mut intent, None, true, 2.0, DELAYS, inside));
        assert!(!step(&mut intent, None, true, 9.0, DELAYS, inside));
        assert!(intent.open, "the lifted finger leaves it open");
        step(
            &mut intent,
            Some(Point::new(50.0, 1.0)),
            true,
            9.5,
            DELAYS,
            inside,
        );
        assert!(!intent.open, "a touch elsewhere closes it at once");
        // A quick tap, then another much later: neither opens it.
        step(&mut intent, on, true, 10.0, DELAYS, inside);
        step(&mut intent, None, true, 10.1, DELAYS, inside);
        step(&mut intent, on, true, 20.0, DELAYS, inside);
        assert!(!intent.open, "the second tap waits afresh");
    }

    fn card() -> HoverCard<()> {
        HoverCard::new(Container::<()>::new().width(40.0).height(20.0))
            .content(Container::<()>::new().width(100.0).height(50.0).color(MARK))
    }

    fn marked(ui: &crate::Ui<()>) -> Option<Rect> {
        ui.scene().primitives().iter().find_map(|p| match p {
            Primitive::Rect { rect, color, .. } if *color == MARK => Some(*rect),
            _ => None,
        })
    }

    fn frame(widget: &HoverCard<()>, runtime: &Runtime) -> crate::Ui<()> {
        build_ui(widget, Size::new(400.0, 300.0), runtime, &Theme::default())
    }

    /// **Through the walk**: the pointer on the anchor opens the card once the time has come,
    /// below the anchor and the gap; frames are asked for while it waits; the frame says when a
    /// pointer moving changes it; the pointer on the card keeps it; leaving closes it.
    #[test]
    fn the_walk_opens_the_card_after_the_pointer_has_rested() {
        let widget = card();
        let mut runtime = Runtime::default();
        let ui = frame(&widget, &runtime);
        assert!(marked(&ui).is_none(), "closed at first");
        assert!(ui.hover_cards_changed(Some(Point::new(10.0, 10.0)), false));
        assert!(!ui.hover_cards_changed(Some(Point::new(200.0, 200.0)), false));

        runtime.input.pointer = Some(Point::new(10.0, 10.0));
        runtime.time = 1.0;
        let ui = frame(&widget, &runtime);
        assert!(marked(&ui).is_none(), "not yet");
        assert!(ui.wants_full_rate_animation(), "a delay is running");
        runtime.time = 1.8;
        let ui = frame(&widget, &runtime);
        let shown = marked(&ui).expect("open after the delay");
        // The anchor's bottom, the gap, the hairline, the room inside.
        assert_eq!(shown.y, 20.0 + GAP + 1.0 + PADDING);
        assert!(!ui.wants_full_rate_animation(), "nothing is running");

        runtime.input.pointer = Some(Point::new(shown.x + 5.0, shown.y + 5.0));
        runtime.time = 3.0;
        assert!(marked(&frame(&widget, &runtime)).is_some(), "on the card");
        runtime.input.pointer = Some(Point::new(390.0, 290.0));
        runtime.time = 4.0;
        assert!(marked(&frame(&widget, &runtime)).is_some(), "closing");
        runtime.time = 4.5;
        assert!(marked(&frame(&widget, &runtime)).is_none(), "closed");
    }

    /// The application's word wins over the pointer's, and a disabled card never opens.
    #[test]
    fn open_overrules_the_pointer_and_disabled_shows_nothing() {
        let runtime = Runtime::default();
        assert!(marked(&frame(&card().open(true), &runtime)).is_some());
        let mut runtime = Runtime::default();
        runtime.input.pointer = Some(Point::new(10.0, 10.0));
        runtime.time = 50.0;
        let shut = card().open(false);
        frame(&shut, &runtime);
        runtime.time = 60.0;
        assert!(marked(&frame(&shut, &runtime)).is_none());
        assert!(Widget::<()>::overlay(&card().enabled(false)).is_none());
        assert_eq!(
            Widget::<()>::semantics(&card().open(true)).and_then(|s| s.expanded),
            Some(true)
        );
    }

    /// Every word about the surface is the caller's, then the theme's.
    #[test]
    fn the_surface_answers_to_its_theme_and_to_its_caller() {
        let runtime = Runtime::default();
        let painted = |ui: &crate::Ui<()>, colour: Color| {
            ui.scene().primitives().iter().any(|p| match p {
                Primitive::Rect { color, .. } => *color == colour,
                _ => false,
            })
        };
        let mut theme = Theme::default();
        let themed = Color::rgb(0.2, 0.4, 0.6);
        theme.widgets.hover_card.background = Some(themed);
        let size = Size::new(400.0, 300.0);
        let ui = build_ui(&card().open(true), size, &runtime, &theme);
        assert!(painted(&ui, themed), "the theme's surface");
        let own = Color::rgb(0.6, 0.6, 0.1);
        let ui = build_ui(&card().open(true).background(own), size, &runtime, &theme);
        assert!(painted(&ui, own), "the caller's");
        assert!(!painted(&ui, themed));
        let near = marked(&frame(&card().open(true).gap(0.0), &runtime)).unwrap();
        assert_eq!(near.y, 20.0 + 1.0 + PADDING, "a gap of its own");
    }
}
