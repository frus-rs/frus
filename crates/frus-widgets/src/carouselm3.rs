// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`Carousel`]: the Material 3 carousel — a row of items that **change size as they
//! scroll**, shrinking and being masked at the edges, and coming to rest on an item.
//!
//! Four layouts, one widget:
//!
//! - **multi-browse** ([`Carousel::multi_browse`]): a large item, then smaller ones, so
//!   several can be browsed at a glance;
//! - **hero** ([`Carousel::hero`]): one large item in the middle with a sliver of its
//!   neighbours either side;
//! - **uncontained** ([`Carousel::uncontained`]): items of one size running off the edge,
//!   the one leaving shrinking as it goes;
//! - **full screen** ([`Carousel::full_screen`]): one item filling the view, vertically.
//!
//! An item is **masked**, not squeezed: its content keeps the size of a large item and the
//! rounded mask around it narrows, so a photo is cropped rather than distorted.
//!
//! The scrolling is a paged view's — drag, fling, the scroll wheel, the edges — with one
//! stop per item. The items are drawn over it from the offset of the frame being drawn, so
//! nothing lags the finger.

use std::rc::Rc;

use frus_core::{Color, Rect, Scene, Size};
use frus_layout::{Dimension, Style};

use crate::interaction::Status;
use crate::scroll::Axis;
use crate::shortcuts::{KeyStroke, ShortcutKey};
use crate::theme::Theme;
use crate::widget::Widget;

/// An item's corner radius: the extra-large shape.
pub const CAROUSEL_RADIUS: f32 = 28.0;
/// The room around each item, which makes the gap between two.
pub const CAROUSEL_ITEM_PADDING: f32 = 4.0;
/// The carousel's height unless told.
pub const CAROUSEL_HEIGHT: f32 = 200.0;

/// Theme defaults for [`Carousel`]. Each `None` falls through to the default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarouselTheme {
    /// An item's surface, behind its content. Unset, `surface`.
    pub background: Option<Color>,
    /// The room around each item. Unset, four.
    pub padding: Option<f32>,
    /// An item's corner radius. Unset, twenty-eight.
    pub radius: Option<f32>,
    /// The press and hover wash over an item. Unset, `on_surface`.
    pub overlay: Option<Color>,
    /// How small an item at the edge gets before it scrolls away. Unset, zero.
    pub shrink_extent: Option<f32>,
}

/// Where one item is this frame: its index, where it starts along the scroll (from the
/// view's leading edge) and how long it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarouselItem {
    /// The item's index.
    pub index: usize,
    /// Its leading edge, from the view's, in logical pixels. Negative when it is partly
    /// scrolled off.
    pub start: f32,
    /// Its length along the scroll.
    pub extent: f32,
}

/// What decides the items' sizes.
#[derive(Clone, Debug, PartialEq)]
pub enum CarouselSizing {
    /// Every item `extent` long; the one leaving shrinks to `shrink` before it scrolls off.
    Fixed {
        /// The items' length.
        extent: f32,
    },
    /// The view is shared out by `weights`, one per slot: `[3, 2, 1]` is a large item, a
    /// medium one and a small one.
    Weighted {
        /// One weight per visible slot.
        weights: Vec<f32>,
        /// Whether every item comes to rest in the largest slot, even the first and the
        /// last (a centred hero), leaving the slots before or after them empty.
        consume_max_weight: bool,
    },
}

impl CarouselSizing {
    /// The distance between two resting positions.
    pub fn step(&self, viewport: f32) -> f32 {
        match self {
            CarouselSizing::Fixed { extent } => extent.min(viewport).max(1.0),
            CarouselSizing::Weighted { weights, .. } => {
                let sum: f32 = weights.iter().sum();
                if sum <= 0.0 || weights.is_empty() {
                    viewport.max(1.0)
                } else {
                    (viewport * weights[0] / sum).max(1.0)
                }
            }
        }
    }

    /// How many resting positions there are for `count` items.
    pub fn stops(&self, viewport: f32, count: usize) -> usize {
        if count == 0 {
            return 0;
        }
        match self {
            CarouselSizing::Fixed { extent } => {
                let m = extent.min(viewport).max(1.0);
                let max = (count as f32 * m - viewport).max(0.0);
                (max / m - 1e-3).ceil().max(0.0) as usize + 1
            }
            CarouselSizing::Weighted {
                weights,
                consume_max_weight,
            } => {
                if *consume_max_weight {
                    count
                } else {
                    count.saturating_sub(weights.len()) + 1
                }
            }
        }
    }

    /// The furthest the view scrolls.
    pub fn max_offset(&self, viewport: f32, count: usize) -> f32 {
        match self {
            CarouselSizing::Fixed { extent } => {
                let m = extent.min(viewport).max(1.0);
                (count as f32 * m - viewport).max(0.0)
            }
            CarouselSizing::Weighted { .. } => {
                self.stops(viewport, count).saturating_sub(1) as f32 * self.step(viewport)
            }
        }
    }

    /// The item at rest at `offset`: the large one.
    pub fn index_at(&self, viewport: f32, count: usize, offset: f32) -> usize {
        let step = self.step(viewport);
        let stops = self.stops(viewport, count);
        ((offset / step).round().max(0.0) as usize).min(stops.saturating_sub(1))
    }

    /// Where every visible item is when the view has scrolled `offset`.
    ///
    /// ```
    /// use frus_widgets::{CarouselItem, CarouselSizing};
    /// let multi = CarouselSizing::Weighted { weights: vec![3.0, 2.0, 1.0], consume_max_weight: false };
    /// let items = multi.layout(0.0, 600.0, 10, 0.0);
    /// assert_eq!(items[0], CarouselItem { index: 0, start: 0.0, extent: 300.0 });
    /// assert_eq!(items[1], CarouselItem { index: 1, start: 300.0, extent: 200.0 });
    /// assert_eq!(items[2], CarouselItem { index: 2, start: 500.0, extent: 100.0 });
    /// ```
    pub fn layout(
        &self,
        offset: f32,
        viewport: f32,
        count: usize,
        shrink: f32,
    ) -> Vec<CarouselItem> {
        if count == 0 || viewport <= 0.0 {
            return Vec::new();
        }
        let offset = offset.clamp(0.0, self.max_offset(viewport, count));
        match self {
            CarouselSizing::Fixed { extent } => fixed_layout(
                offset,
                viewport,
                extent.min(viewport).max(1.0),
                shrink,
                count,
            ),
            CarouselSizing::Weighted {
                weights,
                consume_max_weight,
            } => weighted_layout(
                offset,
                viewport,
                weights,
                shrink,
                count,
                *consume_max_weight,
            ),
        }
    }
}

/// Snaps an index from a float that is a whisker off a whole number.
fn snapped_floor(x: f32) -> i64 {
    let r = x.round();
    if (x - r).abs() < 1e-4 {
        r as i64
    } else {
        x.floor() as i64
    }
}

/// Items of one length `m`: the first visible one shrinks as it leaves, down to the
/// larger of what is left over at the far end and `min`, then scrolls off; the last one
/// takes what room is left, at least that much.
fn fixed_layout(s: f32, v: f32, m: f32, min: f32, count: usize) -> Vec<CarouselItem> {
    let first = snapped_floor(s / m).max(0);
    let off = s - first as f32 * m;
    let eff_min = (v % m).max(min).min(m);
    let last = {
        let actual = (s + v) / m - 1.0;
        let r = actual.round();
        if (actual - r).abs() < 1e-4 {
            r.max(0.0) as i64
        } else {
            actual.ceil().max(0.0) as i64
        }
    };
    let mut out = Vec::new();
    for i in first..=last {
        if i < 0 || i as usize >= count {
            continue;
        }
        let fi = i as f32;
        let (start, extent) = if i == first {
            let e = (m - off).max(eff_min);
            if e <= eff_min {
                (m * fi - eff_min + m - s, e)
            } else {
                (0.0, e)
            }
        } else if i == last {
            (m * fi - s, (s + v - m * fi).clamp(eff_min, m))
        } else {
            (m * fi - s, m)
        };
        if start < v && start + extent > 0.0 {
            out.push(CarouselItem {
                index: i as usize,
                start,
                extent,
            });
        }
    }
    out
}

/// The view shared out by `weights`: as the first item leaves, each item behind it grows
/// toward the slot in front, so that one stop later every item is one slot on.
fn weighted_layout(
    s: f32,
    v: f32,
    weights: &[f32],
    shrink: f32,
    count: usize,
    consume_max: bool,
) -> Vec<CarouselItem> {
    let sum: f32 = weights.iter().sum();
    if weights.is_empty() || sum <= 0.0 {
        return Vec::new();
    }
    let unit = v / sum;
    let max_w = weights.iter().cloned().fold(f32::MIN, f32::max);
    let min_w = weights.iter().cloned().fold(f32::MAX, f32::min);
    let first_ext = weights[0] * unit;
    let max_ext = max_w * unit;
    let min_ext = min_w * unit;
    let eff_shrink = shrink.clamp(0.0, min_ext);
    let smaller_before_max = weights.iter().take_while(|w| **w != max_w).count() as i64;
    let idx = snapped_floor(s / first_ext);
    let first = if consume_max {
        idx - smaller_before_max
    } else {
        idx
    };
    let off = s - idx as f32 * first_ext;
    let dist = first_ext - off;
    let progress = off / first_ext;
    let len = weights.len() as i64;

    let mut out = Vec::new();
    // The first item: what is left of it, or the shrink extent while it slides off.
    let (mut pos, extent) = if dist <= eff_shrink {
        (dist - eff_shrink, eff_shrink)
    } else {
        (0.0, dist.max(eff_shrink))
    };
    let mut push = |i: i64, start: f32, extent: f32| {
        if i >= 0 && (i as usize) < count && start < v && start + extent > 0.0 {
            out.push(CarouselItem {
                index: i as usize,
                start,
                extent,
            });
        }
    };
    push(first, pos, extent);
    pos = dist;
    let mut i = first + 1;
    while pos < v && i < first + len + 2 {
        let k = i - first;
        let e = if k < len {
            let k = k as usize;
            let grow = (weights[k - 1] - weights[k]) / max_w;
            weights[k] * unit + grow * progress * max_ext
        } else {
            (v - pos).max(eff_shrink)
        };
        push(i, pos, e);
        pos += e;
        i += 1;
    }
    out
}

/// One item's state for its builder: how long it is now and how long at its largest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarouselItemState {
    /// The item's length along the scroll this frame.
    pub extent: f32,
    /// The length of the largest slot: what the content is laid out at.
    pub full_extent: f32,
}

impl CarouselItemState {
    /// How open the item is: one at its full length, less as it narrows.
    pub fn openness(&self) -> f32 {
        if self.full_extent <= 0.0 {
            1.0
        } else {
            (self.extent / self.full_extent).clamp(0.0, 1.0)
        }
    }
}

type ItemBuilder<Msg> = Rc<dyn Fn(usize) -> Box<dyn Widget<Msg>>>;

/// The **Material 3 carousel**: items that change size as they scroll, are masked at the
/// edges and come to rest on an item.
///
/// ```
/// use frus_widgets::{Carousel, Container, Color};
///
/// let photos: Carousel<()> = Carousel::multi_browse(12, |i| {
///     Container::new().color(Color::rgb(0.2 + 0.05 * i as f32, 0.4, 0.7))
/// })
/// .height(220.0);
/// ```
///
/// It can be **controlled**: give it the item to show with [`index`](Self::index) and hear
/// where it came to rest with [`on_index_changed`](Self::on_index_changed). With both, the
/// arrow keys, Home and End move it while it has the focus.
///
/// **Themed** through [`CarouselTheme`] (`theme.widgets.carousel`); each value has a
/// builder that outranks the theme.
pub struct Carousel<Msg = crate::callback::Callback> {
    count: usize,
    build: ItemBuilder<Msg>,
    sizing: CarouselSizing,
    axis: Axis,
    width: Dimension,
    height: Dimension,
    index: Option<usize>,
    on_index_changed: Option<Rc<dyn Fn(usize) -> Msg>>,
    on_tap: Option<Rc<dyn Fn(usize) -> Msg>>,
    background: Option<Color>,
    padding: Option<f32>,
    radius: Option<f32>,
    shrink_extent: Option<f32>,
    snapping: bool,
    semantics_label: Option<String>,
    children: std::cell::OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> Carousel<Msg> {
    fn with_sizing<W: Widget<Msg> + 'static>(
        count: usize,
        sizing: CarouselSizing,
        build: impl Fn(usize) -> W + 'static,
    ) -> Self {
        Self {
            count,
            build: Rc::new(move |i| Box::new(build(i)) as Box<dyn Widget<Msg>>),
            sizing,
            axis: Axis::Horizontal,
            width: Dimension::Percent(1.0),
            height: Dimension::Length(CAROUSEL_HEIGHT),
            index: None,
            on_index_changed: None,
            on_tap: None,
            background: None,
            padding: None,
            radius: None,
            shrink_extent: None,
            snapping: true,
            semantics_label: None,
            children: std::cell::OnceCell::new(),
        }
    }

    /// A **multi-browse** carousel: a large item, a medium one and a small one share the
    /// view (weights `[3, 2, 1]`), and each comes to rest in the large slot in turn.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::multi_browse(8, |i| Text::new(format!("{i}")));
    /// ```
    pub fn multi_browse<W: Widget<Msg> + 'static>(
        count: usize,
        build: impl Fn(usize) -> W + 'static,
    ) -> Self {
        Self::with_sizing(
            count,
            CarouselSizing::Weighted {
                weights: vec![3.0, 2.0, 1.0],
                consume_max_weight: false,
            },
            build,
        )
    }

    /// A **hero** carousel: one large item in the middle, a sliver of each neighbour at the
    /// sides (weights `[1, 7, 1]`); every item, the first and the last too, comes to rest in
    /// the middle.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::hero(5, |i| Text::new(format!("{i}")));
    /// ```
    pub fn hero<W: Widget<Msg> + 'static>(
        count: usize,
        build: impl Fn(usize) -> W + 'static,
    ) -> Self {
        Self::with_sizing(
            count,
            CarouselSizing::Weighted {
                weights: vec![1.0, 7.0, 1.0],
                consume_max_weight: true,
            },
            build,
        )
    }

    /// An **uncontained** carousel: every item `extent` long, running off the far edge; the
    /// one leaving shrinks before it goes.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::uncontained(20, 180.0, |i| Text::new(format!("{i}")));
    /// ```
    pub fn uncontained<W: Widget<Msg> + 'static>(
        count: usize,
        extent: f32,
        build: impl Fn(usize) -> W + 'static,
    ) -> Self {
        Self::with_sizing(count, CarouselSizing::Fixed { extent }, build)
    }

    /// A **full-screen** carousel: one item fills the view and the next comes up from
    /// below. Give it the screen's height ([`height`](Self::height) or a flexible parent).
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::full_screen(3, |i| Text::new(format!("{i}"))).height(640.0);
    /// ```
    pub fn full_screen<W: Widget<Msg> + 'static>(
        count: usize,
        build: impl Fn(usize) -> W + 'static,
    ) -> Self {
        let mut c = Self::with_sizing(count, CarouselSizing::Fixed { extent: f32::MAX }, build);
        c.axis = Axis::Vertical;
        c.height = Dimension::Percent(1.0);
        c.padding = Some(0.0);
        c
    }

    /// Any weights: one per slot, the largest the item at rest.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::multi_browse(9, |i| Text::new(format!("{i}"))).weights(&[1.0, 2.0, 3.0, 2.0, 1.0], false);
    /// ```
    pub fn weights(mut self, weights: &[f32], consume_max_weight: bool) -> Self {
        let weights: Vec<f32> = weights.iter().map(|w| w.max(0.0)).collect();
        if weights.iter().any(|w| *w > 0.0) {
            self.sizing = CarouselSizing::Weighted {
                weights,
                consume_max_weight,
            };
        }
        self
    }

    /// Scrolls along `axis`; horizontal unless told (vertical for full screen).
    ///
    /// ```
    /// # use frus_widgets::{Axis, Carousel, Text};
    /// let _c: Carousel<()> = Carousel::uncontained(4, 120.0, |i| Text::new(format!("{i}"))).axis(Axis::Vertical);
    /// ```
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// The carousel's width; the parent's unless told.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::hero(4, |i| Text::new(format!("{i}"))).width(360.0);
    /// ```
    pub fn width(mut self, width: f32) -> Self {
        self.width = Dimension::Length(width);
        self
    }

    /// The carousel's height; two hundred unless told.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::hero(4, |i| Text::new(format!("{i}"))).height(260.0);
    /// ```
    pub fn height(mut self, height: f32) -> Self {
        self.height = Dimension::Length(height);
        self
    }

    /// The item to show: the view glides there whenever this changes.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::hero(4, |i| Text::new(format!("{i}"))).index(2);
    /// ```
    pub fn index(mut self, index: usize) -> Self {
        self.index = Some(index);
        self
    }

    /// Hears the item the view came to rest on. With [`index`](Self::index), the keyboard
    /// moves the carousel too.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<usize> = Carousel::hero(4, |i| Text::new(format!("{i}"))).on_index_changed(|i| i);
    /// ```
    pub fn on_index_changed<R: crate::callback::IntoMsg<Msg>>(
        mut self,
        message: impl Fn(usize) -> R + 'static,
    ) -> Self {
        self.on_index_changed = Some(Rc::new(crate::callback::handler1(message)));
        self
    }

    /// Makes items tappable: `on_tap(index)`, with the press wash over the item.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<usize> = Carousel::hero(4, |i| Text::new(format!("{i}"))).on_tap(|i| i);
    /// ```
    pub fn on_tap<R: crate::callback::IntoMsg<Msg>>(
        mut self,
        message: impl Fn(usize) -> R + 'static,
    ) -> Self {
        self.on_tap = Some(Rc::new(crate::callback::handler1(message)));
        self
    }

    /// The items' surface, behind their content.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Color, Text};
    /// let _c: Carousel<()> = Carousel::hero(4, |i| Text::new(format!("{i}"))).background(Color::rgb(0.9, 0.9, 0.95));
    /// ```
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// The room around each item; half the gap between two.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::hero(4, |i| Text::new(format!("{i}"))).padding(6.0);
    /// ```
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = Some(padding.max(0.0));
        self
    }

    /// The items' corner radius.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::hero(4, |i| Text::new(format!("{i}"))).radius(16.0);
    /// ```
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius.max(0.0));
        self
    }

    /// How small an item at the edge gets before it scrolls away.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::uncontained(9, 200.0, |i| Text::new(format!("{i}"))).shrink_extent(56.0);
    /// ```
    pub fn shrink_extent(mut self, extent: f32) -> Self {
        self.shrink_extent = Some(extent.max(0.0));
        self
    }

    /// Whether a release comes to rest on an item; it does unless told.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::uncontained(9, 200.0, |i| Text::new(format!("{i}"))).snapping(false);
    /// ```
    pub fn snapping(mut self, snapping: bool) -> Self {
        self.snapping = snapping;
        self
    }

    /// What the carousel is, for screen readers.
    ///
    /// ```
    /// # use frus_widgets::{Carousel, Text};
    /// let _c: Carousel<()> = Carousel::hero(4, |i| Text::new(format!("{i}"))).semantics_label("Featured");
    /// ```
    pub fn semantics_label(mut self, label: impl Into<String>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }

    /// Builds the tree: a layout builder that knows the view's size, a paged view of empty
    /// stops for the scrolling, and the items drawn over it from its offset.
    fn make(&self) -> Box<dyn Widget<Msg>> {
        let count = self.count;
        let sizing = self.sizing.clone();
        let axis = self.axis;
        let build = self.build.clone();
        let requested = self.index;
        let on_index = self.on_index_changed.clone();
        let on_tap = self.on_tap.clone();
        let (background, padding, radius, shrink, snapping) = (
            self.background,
            self.padding,
            self.radius,
            self.shrink_extent,
            self.snapping,
        );
        let label = self.semantics_label.clone();
        let layout = crate::LayoutBuilder::new(move |size: Size| {
            let (sizing, build, on_index, on_tap, label) = (
                sizing.clone(),
                build.clone(),
                on_index.clone(),
                on_tap.clone(),
                label.clone(),
            );
            crate::themebuilder::ThemeBuilder::boxed(move |theme: &Theme| {
                let t = theme.widgets.carousel;
                let vertical = matches!(axis, Axis::Vertical);
                let along = if vertical { size.height } else { size.width };
                let sizing = match sizing {
                    CarouselSizing::Fixed { extent } if extent >= f32::MAX / 2.0 => {
                        CarouselSizing::Fixed { extent: along }
                    }
                    other => other,
                };
                let stops = sizing.stops(along, count).max(1);
                let step = sizing.step(along);
                let mut pages = crate::PageView::new(stops, |_| crate::Container::<Msg>::new())
                    .axis(axis)
                    .viewport_fraction((step / along.max(1.0)).min(1.0))
                    .pad_ends(true)
                    .page_snapping(snapping)
                    .width(size.width)
                    .height(size.height);
                if let Some(i) = requested {
                    pages = pages.page(i.min(stops - 1));
                }
                if let Some(on) = &on_index {
                    let on = on.clone();
                    pages = pages.on_page_changed(move |p| on(p));
                }
                let draw = Rc::new(Overlay {
                    count,
                    sizing,
                    vertical,
                    rtl: theme.direction.is_rtl() && !vertical,
                    build,
                    on_tap,
                    background: background.or(t.background).unwrap_or(theme.scheme.surface),
                    overlay: t.overlay.unwrap_or(theme.scheme.on_surface),
                    padding: padding.or(t.padding).unwrap_or(CAROUSEL_ITEM_PADDING),
                    radius: radius.or(t.radius).unwrap_or(CAROUSEL_RADIUS),
                    shrink: shrink.or(t.shrink_extent).unwrap_or(0.0),
                });
                let overlay = crate::ScrollOverlay::new(pages, move |offset, size| {
                    draw.items(if vertical { offset.1 } else { offset.0 }, size)
                });
                let mut root: Box<dyn Widget<Msg>> = Box::new(overlay);
                // The keyboard: only a controlled carousel can be moved by it, since the
                // offset it glides to is the application's index.
                if let (Some(i), Some(on)) = (requested, &on_index) {
                    let last = stops - 1;
                    let i = i.min(last);
                    let (back, fwd) = if vertical {
                        (ShortcutKey::Up, ShortcutKey::Down)
                    } else if theme.direction.is_rtl() {
                        (ShortcutKey::Right, ShortcutKey::Left)
                    } else {
                        (ShortcutKey::Left, ShortcutKey::Right)
                    };
                    let focus = crate::Focus::new(BoxedChild(root));
                    root = Box::new(
                        crate::CallbackShortcuts::new(focus)
                            .bind(KeyStroke::new(back), on(i.saturating_sub(1)))
                            .bind(KeyStroke::new(fwd), on((i + 1).min(last)))
                            .bind(KeyStroke::new(ShortcutKey::Home), on(0))
                            .bind(KeyStroke::new(ShortcutKey::End), on(last)),
                    );
                }
                let mut sem = crate::Semantics::new(
                    frus_core::SemanticsProperties::new(frus_core::Role::None),
                    BoxedChild(root),
                );
                if let Some(label) = label {
                    sem = sem.label(label);
                }
                Box::new(sem) as Box<dyn Widget<Msg>>
            })
        });
        Box::new(layout.flex(1.0))
    }
}

/// A boxed widget as a widget: a pass-through with the child's box.
struct BoxedChild<Msg>(Box<dyn Widget<Msg>>);

impl<Msg: 'static> Widget<Msg> for BoxedChild<Msg> {
    fn style(&self) -> Style {
        self.0.style()
    }
    fn style_themed(&self, theme: &Theme) -> Style {
        self.0.style_themed(theme)
    }
    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        std::slice::from_ref(&self.0)
    }
    fn paint(&self, _: Rect, _: Status, _: &Theme, _: &mut Scene) {}
    fn on_click(&self) -> Option<Msg> {
        None
    }
}

/// What draws the items over the paged view, its look resolved against the theme.
struct Overlay<Msg> {
    count: usize,
    sizing: CarouselSizing,
    vertical: bool,
    rtl: bool,
    build: ItemBuilder<Msg>,
    on_tap: Option<Rc<dyn Fn(usize) -> Msg>>,
    background: Color,
    overlay: Color,
    padding: f32,
    radius: f32,
    shrink: f32,
}

impl<Msg: Clone + 'static> Overlay<Msg> {
    fn items(&self, offset: f32, size: Size) -> Box<dyn Widget<Msg>> {
        let along = if self.vertical {
            size.height
        } else {
            size.width
        };
        let across = if self.vertical {
            size.width
        } else {
            size.height
        };
        let pad = self.padding;
        let full = match &self.sizing {
            CarouselSizing::Fixed { extent } => extent.min(along),
            CarouselSizing::Weighted { weights, .. } => {
                let sum: f32 = weights.iter().sum();
                let max = weights.iter().cloned().fold(0.0, f32::max);
                if sum > 0.0 {
                    along * max / sum
                } else {
                    along
                }
            }
        };
        let mut stack = crate::Stack::new().width(size.width).height(size.height);
        for item in self.sizing.layout(offset, along, self.count, self.shrink) {
            let len = (item.extent - 2.0 * pad).max(0.0);
            let start = item.start + pad;
            let content_len = (full - 2.0 * pad).max(0.0);
            let cross = (across - 2.0 * pad).max(0.0);
            let (cw, ch) = if self.vertical {
                (cross, content_len)
            } else {
                (content_len, cross)
            };
            // The content keeps the full item's size; the mask around it narrows.
            let content = crate::OverflowBox::new(BoxedChild((self.build)(item.index)))
                .width(cw)
                .height(ch);
            let mut surface = crate::Container::new()
                .color(self.background)
                .radius(self.radius)
                .clip()
                .child(content);
            surface = if self.vertical {
                surface.width(cross).height(len)
            } else {
                surface.width(len).height(cross)
            };
            let body: Box<dyn Widget<Msg>> = match &self.on_tap {
                Some(on_tap) => Box::new(
                    crate::InkWell::new()
                        .color(self.overlay)
                        .radius(self.radius)
                        .on_click(on_tap(item.index))
                        .child(surface),
                ),
                None => Box::new(surface),
            };
            let placed = if self.vertical {
                crate::Positioned::new(BoxedChild(body))
                    .top(start)
                    .left(pad)
            } else if self.rtl {
                crate::Positioned::new(BoxedChild(body))
                    .right(start)
                    .top(pad)
            } else {
                crate::Positioned::new(BoxedChild(body))
                    .left(start)
                    .top(pad)
            };
            stack = stack.layer(placed);
        }
        Box::new(stack)
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for Carousel<Msg> {
    fn style(&self) -> Style {
        Style {
            width: self.width,
            height: self.height,
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.children.get_or_init(|| vec![self.make()])
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "Carousel"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn multi() -> CarouselSizing {
        CarouselSizing::Weighted {
            weights: vec![3.0, 2.0, 1.0],
            consume_max_weight: false,
        }
    }

    fn hero() -> CarouselSizing {
        CarouselSizing::Weighted {
            weights: vec![1.0, 7.0, 1.0],
            consume_max_weight: true,
        }
    }

    fn total(items: &[CarouselItem]) -> f32 {
        items.iter().map(|i| i.extent).sum()
    }

    #[test]
    fn multi_browse_at_rest_is_large_medium_small() {
        let items = multi().layout(0.0, 600.0, 10, 0.0);
        let got: Vec<(usize, f32, f32)> =
            items.iter().map(|i| (i.index, i.start, i.extent)).collect();
        assert_eq!(
            got,
            vec![(0, 0.0, 300.0), (1, 300.0, 200.0), (2, 500.0, 100.0)]
        );
        // One stop on, everything has moved up a slot.
        let items = multi().layout(300.0, 600.0, 10, 0.0);
        let got: Vec<(usize, f32, f32)> =
            items.iter().map(|i| (i.index, i.start, i.extent)).collect();
        assert_eq!(
            got,
            vec![(1, 0.0, 300.0), (2, 300.0, 200.0), (3, 500.0, 100.0)]
        );
    }

    #[test]
    fn half_way_between_two_stops_the_items_grow_toward_the_slot_in_front() {
        let items = multi().layout(150.0, 600.0, 10, 0.0);
        let got: Vec<(usize, f32, f32)> =
            items.iter().map(|i| (i.index, i.start, i.extent)).collect();
        assert_eq!(
            got,
            vec![
                (0, 0.0, 150.0),
                (1, 150.0, 250.0),
                (2, 400.0, 150.0),
                (3, 550.0, 50.0)
            ]
        );
        // The view is always exactly filled.
        for s in [0.0, 37.0, 150.0, 299.0, 450.0, 1200.0] {
            let items = multi().layout(s, 600.0, 10, 0.0);
            assert!((total(&items) - 600.0).abs() < 1e-3, "at {s}: {items:?}");
        }
    }

    #[test]
    fn a_shrinking_item_stops_at_the_shrink_extent_and_slides_off() {
        // 40 of the first item left, but it may not get smaller than 60: it keeps 60 and
        // its leading 20 are off screen.
        let items = multi().layout(260.0, 600.0, 10, 60.0);
        assert_eq!(items[0].index, 0);
        assert!((items[0].extent - 60.0).abs() < 1e-3);
        assert!((items[0].start + 20.0).abs() < 1e-3);
        assert!((items[1].start - 40.0).abs() < 1e-3);
    }

    #[test]
    fn multi_browse_ends_with_every_slot_filled() {
        let m = multi();
        assert_eq!(m.stops(600.0, 10), 8);
        assert_eq!(m.max_offset(600.0, 10), 7.0 * 300.0);
        let items = m.layout(m.max_offset(600.0, 10), 600.0, 10, 0.0);
        let idx: Vec<usize> = items.iter().map(|i| i.index).collect();
        assert_eq!(idx, vec![7, 8, 9]);
        assert!((total(&items) - 600.0).abs() < 1e-3);
        // Past the end it is held there.
        assert_eq!(m.layout(99_999.0, 600.0, 10, 0.0), items);
    }

    #[test]
    fn a_hero_rests_every_item_in_the_middle() {
        let h = hero();
        // Unit 40: [40, 280, 40]; the first item rests in the middle with nothing before.
        let items = h.layout(0.0, 360.0, 5, 0.0);
        let got: Vec<(usize, f32, f32)> =
            items.iter().map(|i| (i.index, i.start, i.extent)).collect();
        assert_eq!(got, vec![(0, 40.0, 280.0), (1, 320.0, 40.0)]);
        assert_eq!(h.stops(360.0, 5), 5);
        assert_eq!(h.step(360.0), 40.0);
        // At the last stop the last item is in the middle and nothing follows.
        let items = h.layout(4.0 * 40.0, 360.0, 5, 0.0);
        let got: Vec<(usize, f32, f32)> =
            items.iter().map(|i| (i.index, i.start, i.extent)).collect();
        assert_eq!(got, vec![(3, 0.0, 40.0), (4, 40.0, 280.0)]);
        assert_eq!(h.index_at(360.0, 5, 121.0), 3);
    }

    #[test]
    fn uncontained_items_run_off_the_end_and_the_leaving_one_shrinks() {
        let u = CarouselSizing::Fixed { extent: 200.0 };
        // 500 wide: two whole items and half of the third.
        let items = u.layout(0.0, 500.0, 10, 0.0);
        let got: Vec<(usize, f32, f32)> =
            items.iter().map(|i| (i.index, i.start, i.extent)).collect();
        assert_eq!(
            got,
            vec![(0, 0.0, 200.0), (1, 200.0, 200.0), (2, 400.0, 100.0)]
        );
        // 50 on: the first shrinks to 150 rather than sliding, the others move up.
        let items = u.layout(50.0, 500.0, 10, 0.0);
        assert_eq!((items[0].start, items[0].extent), (0.0, 150.0));
        assert_eq!((items[1].start, items[1].extent), (150.0, 200.0));
        // The last visible one takes at least the leftover (100), up to a whole item.
        assert_eq!(items[2].start, 350.0);
        assert_eq!(items[2].extent, 150.0);
        // 100 on: the first has reached the leftover size (100); from there it slides off.
        let items = u.layout(100.0, 500.0, 10, 0.0);
        assert_eq!((items[0].start, items[0].extent), (0.0, 100.0));
        let items = u.layout(150.0, 500.0, 10, 0.0);
        assert!((items[0].start + 50.0).abs() < 1e-3 && (items[0].extent - 100.0).abs() < 1e-3);
        assert_eq!(items[1].start, 50.0);
        // Stops: one per item until the last ones are all in view.
        assert_eq!(u.max_offset(500.0, 10), 1500.0);
        assert_eq!(u.stops(500.0, 10), 9);
    }

    #[test]
    fn full_screen_is_one_item_per_view() {
        let f = CarouselSizing::Fixed { extent: 800.0 };
        let items = f.layout(0.0, 800.0, 3, 0.0);
        assert_eq!(items.len(), 1);
        assert_eq!(f.stops(800.0, 3), 3);
        let items = f.layout(400.0, 800.0, 3, 0.0);
        assert_eq!(items.len(), 2);
        assert!((total(&items) - 800.0).abs() < 1e-3);
    }

    #[test]
    fn the_resting_item_is_the_nearest_stop() {
        let m = multi();
        assert_eq!(m.index_at(600.0, 10, 0.0), 0);
        assert_eq!(m.index_at(600.0, 10, 149.0), 0);
        assert_eq!(m.index_at(600.0, 10, 151.0), 1);
        assert_eq!(m.index_at(600.0, 10, 1e9), 7);
    }

    #[test]
    fn openness_reads_the_mask() {
        let s = CarouselItemState {
            extent: 50.0,
            full_extent: 200.0,
        };
        assert_eq!(s.openness(), 0.25);
    }
}
