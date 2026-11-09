// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`Toolbar`]: the docked and floating toolbars, horizontal or vertical, that step out of
//! the way while a page is scrolled (milestone 786) — and [`HideOnScroll`], which decides
//! when.

use frus_core::{BorderRadius, Color};
use frus_layout::{Align, Dimension, FlexDirection, Justify, Style};

use crate::lazy::Lazy;
use crate::scrollposition::ScrollPosition;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::widget::Widget;

/// A toolbar's thickness: its height lying down, its width standing up.
pub const TOOLBAR_EXTENT: f32 = 64.0;
/// The room inside a floating toolbar, around its items.
pub const FLOATING_TOOLBAR_PADDING: f32 = 8.0;
/// The room at either end of a docked toolbar.
pub const DOCKED_TOOLBAR_PADDING: f32 = 16.0;
/// The gap between a floating toolbar's items.
pub const TOOLBAR_ITEM_GAP: f32 = 4.0;
/// How far off the page a floating toolbar sits.
pub const FLOATING_TOOLBAR_ELEVATION: f32 = 6.0;

/// Where a toolbar is.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToolbarKind {
    /// Across the whole edge of the window, flush with it, its actions spread out.
    #[default]
    Docked,
    /// A pill floating over the page, its actions together.
    Floating,
}

/// Defaults for [`Toolbar`]. Each field unset is the kind's own.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToolbarTheme {
    /// A docked toolbar's surface. Unset, `surface_container`.
    pub docked_background: Option<Color>,
    /// A floating toolbar's surface. Unset, `surface_container`, or `primary_container`
    /// when vibrant.
    pub floating_background: Option<Color>,
    /// A floating toolbar's height off the page. Unset, six.
    pub elevation: Option<f32>,
    /// The shadow it casts, alpha included. Unset, the scheme's shadow at 30 %.
    pub shadow_color: Option<Color>,
    /// The thickness. Unset, 64.
    pub extent: Option<f32>,
    /// The gap between a floating toolbar's items. Unset, four.
    pub item_gap: Option<f32>,
    /// How long it takes to step out of the way or come back, in seconds. Unset, the
    /// theme's `medium2` motion token.
    pub hide_duration: Option<f32>,
}

struct Parts<Msg> {
    kind: ToolbarKind,
    vertical: bool,
    vibrant: bool,
    hidden: bool,
    items: Vec<Box<dyn Widget<Msg>>>,
    background: Option<Color>,
    elevation: Option<f32>,
    extent: Option<f32>,
    item_gap: Option<f32>,
    justify: Option<Justify>,
}

/// **A bar of actions for the page in front of it**, docked to an edge or floating over
/// it, lying down or standing up.
///
/// ```
/// # use frus_widgets::{IconButton, Icons, Toolbar};
/// # #[derive(Clone)] enum Msg { Bold, Italic }
/// let scrolled_down = false;
/// Toolbar::<Msg>::floating()
///     .item(IconButton::new(Icons::FORMAT_BOLD).on_press(Msg::Bold))
///     .item(IconButton::new(Icons::FORMAT_ITALIC).on_press(Msg::Italic))
///     .hidden(scrolled_down);
/// ```
///
/// - **Docked**: 64 high, the whole width of what holds it, `surface_container`, the
///   actions spread across it with 16 at either end. Put it at the bottom of a page.
/// - **Floating**: a 64-high pill in `surface_container` (or `primary_container` when
///   [`vibrant`](Self::vibrant)), six off the page, the actions four apart inside eight
///   of room. Position it over the page, usually 16 from an edge, in a
///   [`Stack`](crate::Stack).
/// - [`vertical`](Self::vertical) stands either kind up: 64 wide, the actions in a
///   column.
///
/// [`hidden`](Self::hidden) slides it off the edge it belongs to — down for a toolbar
/// lying down, towards the end edge for one standing up — and back, over the theme's
/// `medium2` duration. [`HideOnScroll`] says when, from the page's scroll position.
///
/// The toolbar is a group of its items; each item says what it is.
pub struct Toolbar<Msg = crate::callback::Callback> {
    inner: Lazy<Parts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> Toolbar<Msg> {
    fn of(kind: ToolbarKind) -> Self {
        Self {
            inner: Lazy::new(
                Parts {
                    kind,
                    vertical: false,
                    vibrant: false,
                    hidden: false,
                    items: Vec::new(),
                    background: None,
                    elevation: None,
                    extent: None,
                    item_gap: None,
                    justify: None,
                },
                build::<Msg>,
            ),
        }
    }

    /// A toolbar docked across an edge.
    pub fn docked() -> Self {
        Self::of(ToolbarKind::Docked)
    }

    /// A toolbar floating over the page.
    pub fn floating() -> Self {
        Self::of(ToolbarKind::Floating)
    }

    fn p(&mut self) -> &mut Parts<Msg> {
        self.inner.parts_mut()
    }

    /// Adds an action — an [`IconButton`](crate::IconButton), a button, anything.
    #[must_use]
    pub fn item(mut self, item: impl Widget<Msg> + 'static) -> Self {
        self.p().items.push(Box::new(item));
        self
    }

    /// Stands it up: a column of actions, 64 wide.
    #[must_use]
    pub fn vertical(mut self, vertical: bool) -> Self {
        self.p().vertical = vertical;
        self
    }

    /// A floating toolbar in the strong container colour rather than the neutral one.
    #[must_use]
    pub fn vibrant(mut self, vibrant: bool) -> Self {
        self.p().vibrant = vibrant;
        self
    }

    /// Slides it out of the way (`true`) or back (`false`), animated.
    #[must_use]
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.p().hidden = hidden;
        self
    }

    /// The surface, over the theme's and the kind's.
    #[must_use]
    pub fn background(mut self, color: Color) -> Self {
        self.p().background = Some(color);
        self
    }

    /// A floating toolbar's height off the page.
    #[must_use]
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.p().elevation = Some(elevation);
        self
    }

    /// The thickness. 64 by default.
    #[must_use]
    pub fn extent(mut self, extent: f32) -> Self {
        self.p().extent = Some(extent);
        self
    }

    /// The gap between a floating toolbar's items.
    #[must_use]
    pub fn item_gap(mut self, gap: f32) -> Self {
        self.p().item_gap = Some(gap);
        self
    }

    /// How the actions share the length. A docked toolbar spreads them out, a floating one
    /// keeps them together.
    #[must_use]
    pub fn justify(mut self, justify: Justify) -> Self {
        self.p().justify = Some(justify);
        self
    }
}

crate::composite::composite_widget!(Toolbar);

fn build<Msg: Clone + 'static>(p: Parts<Msg>) -> Box<dyn Widget<Msg>> {
    Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
        let t = theme.widgets.toolbar;
        let extent = p.extent.or(t.extent).unwrap_or(TOOLBAR_EXTENT);
        let floating = p.kind == ToolbarKind::Floating;
        let direction = if p.vertical {
            FlexDirection::Column
        } else {
            FlexDirection::Row
        };
        let (width, height) = match (floating, p.vertical) {
            (false, false) => (Dimension::Auto, Dimension::Length(extent)),
            (false, true) => (Dimension::Length(extent), Dimension::Auto),
            (true, false) => (Dimension::Auto, Dimension::Length(extent)),
            (true, true) => (Dimension::Length(extent), Dimension::Auto),
        };
        let pad = if floating {
            FLOATING_TOOLBAR_PADDING
        } else {
            DOCKED_TOOLBAR_PADDING
        };
        let padding = if p.vertical {
            frus_core::Insets::new(pad, 0.0, pad, 0.0)
        } else {
            frus_core::Insets::new(0.0, pad, 0.0, pad)
        };
        let gap = if floating {
            p.item_gap.or(t.item_gap).unwrap_or(TOOLBAR_ITEM_GAP)
        } else {
            0.0
        };
        let background = p.background.unwrap_or(if floating {
            t.floating_background.unwrap_or(if p.vibrant {
                theme.scheme.primary_container
            } else {
                theme.scheme.surface_container
            })
        } else {
            t.docked_background
                .unwrap_or(theme.scheme.surface_container)
        });
        let style = Style {
            flex_direction: direction,
            width,
            height,
            padding,
            gap,
            align: Align::Center,
            justify: p.justify.unwrap_or(if floating {
                Justify::Center
            } else {
                Justify::SpaceEvenly
            }),
            ..Default::default()
        };
        let mut surface = crate::composite::Surface::new(style, background);
        if !floating {
            // Docked: the whole edge it is docked to.
            surface = surface.fill(if p.vertical {
                crate::widget::FillAxes::HEIGHT
            } else {
                crate::widget::FillAxes::WIDTH
            });
        }
        if floating {
            let elevation = p
                .elevation
                .or(t.elevation)
                .unwrap_or(FLOATING_TOOLBAR_ELEVATION);
            let shadow = t
                .shadow_color
                .unwrap_or(theme.scheme.shadow.with_alpha(0.3));
            surface = surface
                .radius(BorderRadius::uniform(extent * 0.5))
                .elevation(elevation, shadow);
        }
        for item in p.items {
            surface = surface.child_boxed(item);
        }
        // Out of the way: down for a toolbar lying down, towards the end for one standing
        // up. A floating one goes a little further, past the room it floats in.
        let distance = if floating { 1.5 } else { 1.0 };
        let (dx, dy) = match (p.hidden, p.vertical) {
            (false, _) => (0.0, 0.0),
            (true, false) => (0.0, distance),
            (true, true) if theme.direction == frus_core::TextDirection::Rtl => (-distance, 0.0),
            (true, true) => (distance, 0.0),
        };
        let duration = t.hide_duration.unwrap_or(theme.motion.medium2);
        Box::new(crate::AnimatedSlide::new(
            dx,
            dy,
            duration,
            theme.motion.standard.clone(),
            surface,
        )) as Box<dyn Widget<Msg>>
    }))
}

/// **Decides when a toolbar steps out of the way**: hidden while the page is scrolled
/// towards its end, shown again as soon as it is scrolled back, and always shown at the
/// very top.
///
/// ```
/// # use frus_widgets::HideOnScroll;
/// let mut hide = HideOnScroll::new();
/// hide.update(0.0);
/// hide.update(120.0); // scrolled down
/// assert!(hide.hidden());
/// hide.update(100.0); // and back up a little
/// assert!(!hide.hidden());
/// ```
///
/// Keep one in the page's state and feed it the scroll offset from the scroll view's
/// `on_scroll`; hand [`hidden`](Self::hidden) to [`Toolbar::hidden`].
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct HideOnScroll {
    last: Option<f32>,
    hidden: bool,
    threshold: f32,
}

impl HideOnScroll {
    /// A fresh one: shown, with a few pixels of slack before a scroll counts.
    pub fn new() -> Self {
        Self {
            last: None,
            hidden: false,
            threshold: 4.0,
        }
    }

    /// How far a scroll must go, in one step, before it hides or shows the toolbar.
    #[must_use]
    pub fn threshold(mut self, pixels: f32) -> Self {
        self.threshold = pixels;
        self
    }

    /// Takes the page's new offset along its scroll axis; returns whether the answer
    /// changed.
    pub fn update(&mut self, offset: f32) -> bool {
        let before = self.hidden;
        if let Some(last) = self.last {
            let delta = offset - last;
            if offset <= 0.0 {
                self.hidden = false;
            } else if delta > self.threshold {
                self.hidden = true;
            } else if delta < -self.threshold {
                self.hidden = false;
            }
            if delta.abs() > self.threshold || offset <= 0.0 {
                self.last = Some(offset);
            }
        } else {
            self.last = Some(offset);
        }
        before != self.hidden
    }

    /// [`update`](Self::update), from a scroll view's position along its vertical axis.
    pub fn update_from(&mut self, position: ScrollPosition) -> bool {
        self.update(position.offset.1)
    }

    /// Whether the toolbar should be out of the way.
    pub fn hidden(&self) -> bool {
        self.hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Size};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {}

    fn item() -> crate::Container<Msg> {
        crate::Container::new().width(48.0).height(48.0)
    }

    fn rects(toolbar: Toolbar<Msg>, size: Size) -> Vec<frus_core::Rect> {
        let column = crate::Flex::column().flex(1.0).child(toolbar);
        let ui = build_ui(&column, size, &Runtime::default(), &Theme::default());
        let mut boxes: Vec<_> = ui
            .scene()
            .primitives()
            .iter()
            .filter_map(|p| match p {
                frus_core::Primitive::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        boxes.dedup();
        boxes
    }

    /// **Docked, it spans the width at 64 high; floating, it hugs its items in a pill.**
    #[test]
    fn docked_spans_the_edge_and_floating_hugs_its_items() {
        let docked = Toolbar::docked().item(item()).item(item()).item(item());
        let surface = *rects(docked, Size::new(360.0, 200.0)).last().unwrap();
        assert_eq!((surface.width, surface.height), (360.0, TOOLBAR_EXTENT));

        let floating = Toolbar::floating().item(item()).item(item()).item(item());
        let surface = *rects(floating, Size::new(360.0, 200.0)).last().unwrap();
        assert_eq!(surface.height, TOOLBAR_EXTENT);
        assert_eq!(
            surface.width,
            3.0 * 48.0 + 2.0 * TOOLBAR_ITEM_GAP + 2.0 * FLOATING_TOOLBAR_PADDING
        );

        let standing = Toolbar::floating().vertical(true).item(item()).item(item());
        let surface = *rects(standing, Size::new(360.0, 400.0)).last().unwrap();
        assert_eq!(surface.width, TOOLBAR_EXTENT);
        assert_eq!(
            surface.height,
            2.0 * 48.0 + TOOLBAR_ITEM_GAP + 2.0 * FLOATING_TOOLBAR_PADDING
        );
    }

    /// **Down hides it, up shows it, the top always shows it**, and jitter under the
    /// threshold changes nothing.
    #[test]
    fn hide_on_scroll_follows_the_direction() {
        let mut hide = HideOnScroll::new();
        assert!(!hide.update(0.0));
        assert!(!hide.update(2.0), "under the threshold");
        assert!(hide.update(40.0));
        assert!(hide.hidden());
        assert!(!hide.update(80.0), "still going down");
        assert!(hide.update(60.0), "back up shows it");
        assert!(!hide.hidden());
        hide.update(200.0);
        assert!(hide.hidden());
        hide.update(0.0);
        assert!(!hide.hidden(), "the top always shows it");
    }
}
