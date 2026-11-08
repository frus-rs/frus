// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Slivers (milestone 780): a scroll view made of pieces that each lay themselves out
//! along the scroll — a collapsing app bar, then a grid, then a list, all in one area that
//! scrolls as one.
//!
//! [`CustomScrollView`] holds the pieces. Each one is a [`Sliver`]: [`SliverList`],
//! [`SliverGrid`], [`SliverToBoxAdapter`], [`SliverPadding`], [`SliverFillRemaining`],
//! [`SliverPersistentHeader`] and [`SliverAppBar`]. The lists and grids are **lazy**: an
//! item outside the viewport and its cache extent is never built.
//!
//! ```
//! use frus_widgets::{
//!     CustomScrollView, FlexibleSpaceBar, SliverAppBar, SliverGrid, SliverList, Text,
//! };
//!
//! let _page: CustomScrollView = CustomScrollView::new()
//!     .sliver(
//!         SliverAppBar::new()
//!             .pinned(true)
//!             .expanded_height(200.0)
//!             .flexible_space(FlexibleSpaceBar::new().title(Text::new("Photos"))),
//!     )
//!     .sliver(SliverGrid::count(3, 30, |i| Text::new(format!("Tile {i}"))))
//!     .sliver(SliverList::new(1000, 56.0, |i| Text::new(format!("Row {i}"))));
//! ```
//!
//! The geometry follows the usual sliver protocol: every piece is told how far into it the
//! view has scrolled and how much of the viewport is left, and answers how much it scrolls
//! (`scroll_extent`), how much it paints (`paint_extent`) and how much room it takes from
//! the pieces after it (`layout_extent`). A pinned header paints more than it takes, which
//! is how the content after it goes underneath.

use std::rc::Rc;

use frus_core::{Alignment, Color, Insets, Rect, Scene, TextStyle};
use frus_layout::{Dimension, Style};

use crate::interaction::Status;
use crate::physics::ScrollPhysics;
use crate::scroll::Axis;
use crate::theme::Theme;
use crate::widget::Widget;

/// How far past each end of the viewport a [`CustomScrollView`] builds its lazy children by
/// default, in logical pixels: rows about to scroll in are ready when they arrive.
pub const DEFAULT_CACHE_EXTENT: f32 = 250.0;

/// How long a floating header that snaps takes to finish showing or hiding, in seconds.
pub(crate) const SNAP_DURATION: f32 = 0.2;

// ---------------------------------------------------------------------------------------
// The view
// ---------------------------------------------------------------------------------------

/// **A scroll view made of slivers** — an app bar that collapses, a grid, a list, a footer
/// that fills what is left, scrolling as one.
///
/// ```
/// use frus_widgets::{CustomScrollView, SliverAppBar, SliverList, Text};
///
/// let _inbox: CustomScrollView = CustomScrollView::new()
///     .sliver(SliverAppBar::new().title(Text::new("Inbox")).floating(true).snap(true))
///     .sliver(SliverList::new(500, 72.0, |i| Text::new(format!("Message {i}"))));
/// ```
///
/// It scrolls with the same machinery as [`ListView`](crate::ListView): the platform's
/// physics and scrollbar, a finger, a wheel, the keyboard, a [`ScrollTo`](crate::ScrollTo)
/// addressed through a key, and the offset kept across a restart with
/// [`restoration_id`](Self::restoration_id). Unsized, it takes all the room on offer.
pub struct CustomScrollView<Msg = crate::callback::Callback> {
    pub(crate) slivers: Vec<Sliver<Msg>>,
    pub(crate) axis: Axis,
    pub(crate) cache_extent: f32,
    width: Dimension,
    height: Dimension,
    sized: (bool, bool),
    flex_grow: f32,
    physics: Option<ScrollPhysics>,
    keyboard_dismiss: Option<crate::KeyboardDismissBehavior>,
    scrollbars: Option<crate::physics::Scrollbars>,
    thumb_visibility: Option<bool>,
    primary: Option<bool>,
    on_scroll: Option<Box<dyn Fn(crate::ScrollPosition) -> Msg>>,
    grain: f32,
    restoration: Option<Vec<String>>,
}

impl<Msg> CustomScrollView<Msg> {
    /// An empty view, scrolling down; add pieces with [`sliver`](Self::sliver).
    pub fn new() -> Self {
        Self {
            slivers: Vec::new(),
            axis: Axis::Vertical,
            cache_extent: DEFAULT_CACHE_EXTENT,
            width: Dimension::Auto,
            height: Dimension::Length(200.0),
            sized: (false, false),
            flex_grow: 0.0,
            physics: None,
            keyboard_dismiss: None,
            scrollbars: None,
            thumb_visibility: None,
            primary: None,
            on_scroll: None,
            grain: 0.0,
            restoration: None,
        }
    }

    /// Adds a piece after the others.
    ///
    /// ```
    /// # use frus_widgets::{CustomScrollView, SliverToBoxAdapter, Text};
    /// let _v: CustomScrollView = CustomScrollView::new().sliver(SliverToBoxAdapter::new(Text::new("Hi")));
    /// ```
    pub fn sliver(mut self, sliver: impl Into<Sliver<Msg>>) -> Self {
        self.slivers.push(sliver.into());
        self
    }

    /// Adds several pieces, in order.
    ///
    /// ```
    /// # use frus_widgets::{CustomScrollView, Sliver, SliverToBoxAdapter, Text};
    /// let pieces: Vec<Sliver> = vec![SliverToBoxAdapter::new(Text::new("A")).into()];
    /// let _v: CustomScrollView = CustomScrollView::new().slivers(pieces);
    /// ```
    pub fn slivers(mut self, slivers: impl IntoIterator<Item = Sliver<Msg>>) -> Self {
        self.slivers.extend(slivers);
        self
    }

    /// Which way it scrolls: down (the default) or across. [`Axis::Both`] reads as down.
    ///
    /// ```
    /// # use frus_widgets::{Axis, CustomScrollView};
    /// let _shelf: CustomScrollView = CustomScrollView::new().axis(Axis::Horizontal);
    /// ```
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = match axis {
            Axis::Horizontal => Axis::Horizontal,
            _ => Axis::Vertical,
        };
        self
    }

    /// How far past each end of the viewport the lazy pieces build their items;
    /// [`DEFAULT_CACHE_EXTENT`] unless told.
    ///
    /// ```
    /// # use frus_widgets::CustomScrollView;
    /// let _v: CustomScrollView = CustomScrollView::new().cache_extent(0.0);
    /// ```
    pub fn cache_extent(mut self, extent: f32) -> Self {
        self.cache_extent = extent.max(0.0);
        self
    }

    /// Overrides how the view behaves at its edges and after a fling; unset, the
    /// platform's.
    ///
    /// ```
    /// # use frus_widgets::{CustomScrollView, ScrollPhysics};
    /// let _v: CustomScrollView = CustomScrollView::new().physics(ScrollPhysics::BOUNCING);
    /// ```
    pub fn physics(mut self, physics: ScrollPhysics) -> Self {
        self.physics = Some(physics);
        self
    }

    /// What a finger dragging the view does to the focus; unset, the scroll behaviour
    /// in force decides.
    ///
    /// ```
    /// # use frus_widgets::{CustomScrollView, KeyboardDismissBehavior};
    /// let _v: CustomScrollView =
    ///     CustomScrollView::new().keyboard_dismiss_behavior(KeyboardDismissBehavior::OnDrag);
    /// ```
    pub fn keyboard_dismiss_behavior(mut self, behavior: crate::KeyboardDismissBehavior) -> Self {
        self.keyboard_dismiss = Some(behavior);
        self
    }

    /// Whether this view draws a scrollbar, over the application's answer.
    ///
    /// ```
    /// # use frus_widgets::{CustomScrollView, Scrollbars};
    /// let _v: CustomScrollView = CustomScrollView::new().scrollbars(Scrollbars::Never);
    /// ```
    pub fn scrollbars(mut self, scrollbars: crate::physics::Scrollbars) -> Self {
        self.scrollbars = Some(scrollbars);
        self
    }

    /// Keeps the scrollbar on screen rather than letting it fade.
    ///
    /// ```
    /// # use frus_widgets::CustomScrollView;
    /// let _v: CustomScrollView = CustomScrollView::new().thumb_visibility(true);
    /// ```
    pub fn thumb_visibility(mut self, visible: bool) -> Self {
        self.thumb_visibility = Some(visible);
        self
    }

    /// Whether this is the page's primary scroll view: a primary view answers a drag even
    /// when everything fits. Unset, a vertical view is primary and a horizontal one is not.
    ///
    /// ```
    /// # use frus_widgets::CustomScrollView;
    /// let _v: CustomScrollView = CustomScrollView::new().primary(false);
    /// ```
    pub fn primary(mut self, primary: bool) -> Self {
        self.primary = Some(primary);
        self
    }

    /// Sets the viewport width, in logical pixels.
    ///
    /// ```
    /// # use frus_widgets::CustomScrollView;
    /// let _v: CustomScrollView = CustomScrollView::new().width(320.0);
    /// ```
    pub fn width(mut self, width: f32) -> Self {
        self.width = Dimension::Length(width);
        self.sized.0 = true;
        self
    }

    /// Sets the viewport height, in logical pixels.
    ///
    /// ```
    /// # use frus_widgets::CustomScrollView;
    /// let _v: CustomScrollView = CustomScrollView::new().height(480.0);
    /// ```
    pub fn height(mut self, height: f32) -> Self {
        self.height = Dimension::Length(height);
        self.sized.1 = true;
        self
    }

    /// Flex growth factor along the parent's main axis.
    ///
    /// ```
    /// # use frus_widgets::CustomScrollView;
    /// let _v: CustomScrollView = CustomScrollView::new().flex(1.0);
    /// ```
    pub fn flex(mut self, grow: f32) -> Self {
        self.flex_grow = grow;
        self
    }

    /// Says where the view is whenever it moves.
    ///
    /// ```
    /// # use frus_widgets::CustomScrollView;
    /// let _v: CustomScrollView<f32> = CustomScrollView::new().on_scroll(|p| p.y().pixels);
    /// ```
    pub fn on_scroll<R: crate::callback::IntoMsg<Msg>>(
        mut self,
        on_scroll: impl Fn(crate::ScrollPosition) -> R + 'static,
    ) -> Self {
        self.on_scroll = Some(Box::new(crate::callback::handler1(on_scroll)));
        self
    }

    /// Reports no more often than every `px` of movement, and always where it comes to rest.
    ///
    /// ```
    /// # use frus_widgets::CustomScrollView;
    /// let _v: CustomScrollView<f32> =
    ///     CustomScrollView::new().on_scroll(|p| p.y().pixels).notify_every(100.0);
    /// ```
    pub fn notify_every(mut self, px: f32) -> Self {
        self.grain = px.max(0.0);
        self
    }

    /// Keeps how far the view is scrolled across the end of the process, under `id`.
    ///
    /// ```
    /// # use frus_widgets::CustomScrollView;
    /// let _v: CustomScrollView = CustomScrollView::new().restoration_id("feed");
    /// ```
    pub fn restoration_id(mut self, id: &str) -> Self {
        self.restoration = Some(crate::restoration::path_here(id));
        self
    }
}

impl<Msg> Default for CustomScrollView<Msg> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for CustomScrollView<Msg> {
    /// Registers the offset under its restoration id, if it has one.
    fn build_in(
        &self,
        id: crate::interaction::WidgetId,
        runtime: &crate::runtime::Runtime,
        theme: &Theme,
    ) {
        if let Some(path) = &self.restoration {
            runtime.restoration.register_scroll(path.clone(), id);
        }
        self.build_themed(theme)
    }

    fn style(&self) -> Style {
        Style {
            width: self.width,
            height: self.height,
            flex_grow: self.flex_grow,
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    /// All the room on offer on an axis nobody sized, as a list takes.
    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        if self.flex_grow > 0.0 {
            return crate::widget::FillAxes::NONE;
        }
        crate::widget::FillAxes {
            horizontal: !self.sized.0,
            vertical: !self.sized.1,
        }
    }

    fn soft_extent(&self) -> crate::widget::FillAxes {
        crate::widget::FillAxes {
            horizontal: false,
            vertical: !self.sized.1,
        }
    }

    fn sliver_view(&self) -> Option<&CustomScrollView<Msg>> {
        Some(self)
    }

    fn scroll_axis(&self) -> Axis {
        self.axis
    }

    fn scroll_physics(&self) -> Option<ScrollPhysics> {
        self.physics.or_else(|| {
            self.primary
                .unwrap_or(self.axis == Axis::Vertical)
                .then_some(ScrollPhysics::ALWAYS)
        })
    }

    fn keyboard_dismiss_behavior(&self) -> Option<crate::KeyboardDismissBehavior> {
        self.keyboard_dismiss
    }

    fn scrollbars(&self) -> Option<crate::physics::Scrollbars> {
        self.scrollbars
    }

    fn thumb_visibility(&self) -> Option<bool> {
        self.thumb_visibility
    }

    fn on_scroll(&self, position: crate::ScrollPosition) -> Option<Msg> {
        self.on_scroll.as_ref().map(|report| report(position))
    }

    fn scroll_grain(&self) -> f32 {
        self.grain
    }

    fn debug_name(&self) -> &'static str {
        "CustomScrollView"
    }
}

// ---------------------------------------------------------------------------------------
// The pieces
// ---------------------------------------------------------------------------------------

/// One piece of a [`CustomScrollView`]. Made from any of the `Sliver*` builders with
/// `.into()`, which [`CustomScrollView::sliver`] does for you.
///
/// ```
/// # use frus_widgets::{Sliver, SliverToBoxAdapter, Text};
/// let _piece: Sliver = SliverToBoxAdapter::new(Text::new("Header")).into();
/// ```
pub struct Sliver<Msg = crate::callback::Callback> {
    pub(crate) kind: SliverKind<Msg>,
}

pub(crate) enum SliverKind<Msg> {
    Box(Box<dyn Widget<Msg>>),
    List(SliverList<Msg>),
    Grid(SliverGrid<Msg>),
    Padding(Insets, Box<Sliver<Msg>>),
    Fill(SliverFillRemaining<Msg>),
    Header(HeaderSpec<Msg>),
}

/// **A single widget as a piece** of a [`CustomScrollView`]: a banner above a list, a
/// "load more" button below it. It scrolls as one box, as long as it is.
///
/// ```
/// # use frus_widgets::{CustomScrollView, SliverToBoxAdapter, Text};
/// let _v: CustomScrollView = CustomScrollView::new().sliver(SliverToBoxAdapter::new(Text::new("Today")));
/// ```
pub struct SliverToBoxAdapter<Msg = crate::callback::Callback> {
    child: Box<dyn Widget<Msg>>,
}

impl<Msg> SliverToBoxAdapter<Msg> {
    /// `child`, across the whole width of the view and as long as it needs.
    pub fn new(child: impl Widget<Msg> + 'static) -> Self {
        Self {
            child: Box::new(child),
        }
    }
}

impl<Msg> From<SliverToBoxAdapter<Msg>> for Sliver<Msg> {
    fn from(adapter: SliverToBoxAdapter<Msg>) -> Self {
        Sliver {
            kind: SliverKind::Box(adapter.child),
        }
    }
}

/// Where a [`SliverList`]'s items come from.
pub(crate) enum ListItems<Msg> {
    /// Built from their index, as they come into view.
    Built(Box<dyn Fn(usize) -> Box<dyn Widget<Msg>>>),
    /// Given whole.
    Owned(Vec<Box<dyn Widget<Msg>>>),
}

/// **A lazy list as a piece** of a [`CustomScrollView`]: only the items in view, and in
/// the cache extent around it, are built.
///
/// ```
/// # use frus_widgets::{CustomScrollView, SliverList, Text};
/// let _v: CustomScrollView = CustomScrollView::new()
///     .sliver(SliverList::new(10_000, 48.0, |i| Text::new(format!("Row {i}"))));
/// ```
pub struct SliverList<Msg = crate::callback::Callback> {
    pub(crate) count: usize,
    /// The extent of every item, or, for a list of items of any extent, `None`.
    pub(crate) item_extent: Option<f32>,
    /// The guess for an item not measured yet.
    pub(crate) estimate: f32,
    pub(crate) items: ListItems<Msg>,
}

impl<Msg> SliverList<Msg> {
    /// `count` items, each `item_extent` long along the scroll, built by `build(index)` as
    /// they come into view.
    pub fn new<W: Widget<Msg> + 'static>(
        count: usize,
        item_extent: f32,
        build: impl Fn(usize) -> W + 'static,
    ) -> Self {
        Self {
            count,
            item_extent: Some(item_extent.max(0.0)),
            estimate: item_extent.max(0.0),
            items: ListItems::Built(Self::scoped(build)),
        }
    }

    /// `count` items **of any extent**, built by `build(index)` as they come into view
    /// and measured the first time they are; the ones not seen yet are counted at the
    /// mean of those that were.
    ///
    /// ```
    /// # use frus_widgets::{SliverList, Text};
    /// let _feed: SliverList = SliverList::builder(200, |i| Text::new("word ".repeat(i % 40)).wrap());
    /// ```
    pub fn builder<W: Widget<Msg> + 'static>(
        count: usize,
        build: impl Fn(usize) -> W + 'static,
    ) -> Self {
        Self {
            count,
            item_extent: None,
            estimate: 56.0,
            items: ListItems::Built(Self::scoped(build)),
        }
    }

    /// A list of the widgets given, each as long as it needs. Fine for a few dozen
    /// rows; a long list wants [`SliverList::builder`], which builds only what is seen.
    ///
    /// ```
    /// # use frus_widgets::{SliverList, Text, Widget};
    /// let rows: Vec<Box<dyn Widget>> = vec![Box::new(Text::new("One")), Box::new(Text::new("Two"))];
    /// let _list: SliverList = SliverList::children(rows);
    /// ```
    pub fn children(children: impl IntoIterator<Item = Box<dyn Widget<Msg>>>) -> Self {
        let children: Vec<_> = children.into_iter().collect();
        Self {
            count: children.len(),
            item_extent: None,
            estimate: 56.0,
            items: ListItems::Owned(children),
        }
    }

    /// Adds one widget to a list made by [`SliverList::children`]; a list built from an
    /// index has nowhere to put it and ignores it.
    ///
    /// ```
    /// # use frus_widgets::{SliverList, Text};
    /// let _list: SliverList = SliverList::children([]).child(Text::new("One")).child(Text::new("Two"));
    /// ```
    pub fn child(mut self, child: impl Widget<Msg> + 'static) -> Self {
        if let ListItems::Owned(children) = &mut self.items {
            children.push(Box::new(child));
            self.count = children.len();
        }
        self
    }

    /// The extent to count an item at before any has been measured, for a list of items
    /// of any extent; 56 unless told.
    ///
    /// ```
    /// # use frus_widgets::{SliverList, Text};
    /// let _l: SliverList = SliverList::builder(10, |_| Text::new("x")).estimated_extent(72.0);
    /// ```
    pub fn estimated_extent(mut self, extent: f32) -> Self {
        if self.item_extent.is_none() {
            self.estimate = extent.max(0.0);
        }
        self
    }

    fn scoped<W: Widget<Msg> + 'static>(
        build: impl Fn(usize) -> W + 'static,
    ) -> Box<dyn Fn(usize) -> Box<dyn Widget<Msg>>> {
        let rows = crate::restoration::ItemScopes::here("row");
        Box::new(move |index| rows.build(index, || Box::new(build(index)) as Box<dyn Widget<Msg>>))
    }
}

impl<Msg> From<SliverList<Msg>> for Sliver<Msg> {
    fn from(list: SliverList<Msg>) -> Self {
        Sliver {
            kind: SliverKind::List(list),
        }
    }
}

/// How a [`SliverGrid`] decides its columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum GridColumns {
    /// This many.
    Count(usize),
    /// As many as it takes for none to be wider than this.
    MaxExtent(f32),
}

/// **A lazy grid as a piece** of a [`CustomScrollView`]: only the rows in view, and in the
/// cache extent around it, are built.
///
/// ```
/// # use frus_widgets::{CustomScrollView, SliverGrid, Text};
/// let _v: CustomScrollView = CustomScrollView::new()
///     .sliver(SliverGrid::extent(160.0, 400, |i| Text::new(format!("{i}"))).gap(8.0).aspect(1.0));
/// ```
///
/// Tiles are square unless [`aspect`](Self::aspect) or [`tile_height`](Self::tile_height)
/// says otherwise.
pub struct SliverGrid<Msg = crate::callback::Callback> {
    pub(crate) columns: GridColumns,
    pub(crate) count: usize,
    pub(crate) build: Box<dyn Fn(usize) -> Box<dyn Widget<Msg>>>,
    pub(crate) row_gap: f32,
    pub(crate) column_gap: f32,
    pub(crate) aspect: Option<f32>,
    pub(crate) tile_height: Option<f32>,
}

impl<Msg> SliverGrid<Msg> {
    /// `count` tiles in `columns` columns, built by `build(index)` as their row comes into
    /// view.
    pub fn count<W: Widget<Msg> + 'static>(
        columns: usize,
        count: usize,
        build: impl Fn(usize) -> W + 'static,
    ) -> Self {
        Self::with(GridColumns::Count(columns.max(1)), count, build)
    }

    /// `count` tiles in as many columns as it takes for none to be wider than
    /// `max_tile_width`: a 500 px grid at `extent(150.0)` is four columns of 125.
    pub fn extent<W: Widget<Msg> + 'static>(
        max_tile_width: f32,
        count: usize,
        build: impl Fn(usize) -> W + 'static,
    ) -> Self {
        Self::with(
            GridColumns::MaxExtent(max_tile_width.max(f32::MIN_POSITIVE)),
            count,
            build,
        )
    }

    fn with<W: Widget<Msg> + 'static>(
        columns: GridColumns,
        count: usize,
        build: impl Fn(usize) -> W + 'static,
    ) -> Self {
        let cells = crate::restoration::ItemScopes::here("cell");
        Self {
            columns,
            count,
            build: Box::new(move |index| {
                cells.build(index, || Box::new(build(index)) as Box<dyn Widget<Msg>>)
            }),
            row_gap: 0.0,
            column_gap: 0.0,
            aspect: None,
            tile_height: None,
        }
    }

    /// The same gap between rows and between columns.
    ///
    /// ```
    /// # use frus_widgets::{SliverGrid, Text};
    /// let _g: SliverGrid = SliverGrid::count(2, 8, |_| Text::new("x")).gap(8.0);
    /// ```
    pub fn gap(mut self, gap: f32) -> Self {
        self.row_gap = gap.max(0.0);
        self.column_gap = gap.max(0.0);
        self
    }

    /// The gap between rows.
    ///
    /// ```
    /// # use frus_widgets::{SliverGrid, Text};
    /// let _g: SliverGrid = SliverGrid::count(2, 8, |_| Text::new("x")).row_gap(4.0);
    /// ```
    pub fn row_gap(mut self, gap: f32) -> Self {
        self.row_gap = gap.max(0.0);
        self
    }

    /// The gap between columns.
    ///
    /// ```
    /// # use frus_widgets::{SliverGrid, Text};
    /// let _g: SliverGrid = SliverGrid::count(2, 8, |_| Text::new("x")).column_gap(4.0);
    /// ```
    pub fn column_gap(mut self, gap: f32) -> Self {
        self.column_gap = gap.max(0.0);
        self
    }

    /// A tile's width over its height: `2.0` is twice as wide as it is tall.
    ///
    /// ```
    /// # use frus_widgets::{SliverGrid, Text};
    /// let _g: SliverGrid = SliverGrid::count(2, 8, |_| Text::new("x")).aspect(1.5);
    /// ```
    pub fn aspect(mut self, ratio: f32) -> Self {
        self.aspect = Some(ratio);
        self
    }

    /// A tile's height, whatever its width.
    ///
    /// ```
    /// # use frus_widgets::{SliverGrid, Text};
    /// let _g: SliverGrid = SliverGrid::count(2, 8, |_| Text::new("x")).tile_height(120.0);
    /// ```
    pub fn tile_height(mut self, height: f32) -> Self {
        self.tile_height = Some(height.max(0.0));
        self
    }
}

impl<Msg> From<SliverGrid<Msg>> for Sliver<Msg> {
    fn from(grid: SliverGrid<Msg>) -> Self {
        Sliver {
            kind: SliverKind::Grid(grid),
        }
    }
}

/// **Room around another piece**, inside the scroll: the room before it scrolls in ahead
/// of it and the room after it follows it.
///
/// ```
/// # use frus_widgets::{CustomScrollView, SliverGrid, SliverPadding, Text};
/// let _v: CustomScrollView = CustomScrollView::new()
///     .sliver(SliverPadding::new(SliverGrid::count(2, 10, |_| Text::new("x"))).padding(16.0));
/// ```
pub struct SliverPadding<Msg = crate::callback::Callback> {
    padding: Insets,
    sliver: Sliver<Msg>,
}

impl<Msg> SliverPadding<Msg> {
    /// `sliver`, with no room around it until [`padding`](Self::padding) says.
    pub fn new(sliver: impl Into<Sliver<Msg>>) -> Self {
        Self {
            padding: Insets::ZERO,
            sliver: sliver.into(),
        }
    }

    /// The same room on every side.
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = Insets::uniform(padding);
        self
    }

    /// The room one side at a time.
    ///
    /// ```
    /// # use frus_widgets::{SliverList, SliverPadding, Text};
    /// let _p: SliverPadding = SliverPadding::new(SliverList::new(3, 40.0, |_| Text::new("x")))
    ///     .padding_each(8.0, 16.0, 88.0, 16.0);
    /// ```
    pub fn padding_each(mut self, top: f32, right: f32, bottom: f32, left: f32) -> Self {
        self.padding = Insets::new(top, right, bottom, left);
        self
    }
}

impl<Msg> From<SliverPadding<Msg>> for Sliver<Msg> {
    fn from(padding: SliverPadding<Msg>) -> Self {
        Sliver {
            kind: SliverKind::Padding(padding.padding, Box::new(padding.sliver)),
        }
    }
}

/// **The rest of the viewport**: a piece as long as what the pieces before it leave of the
/// viewport — an empty-state message centred under a short list, a footer at the bottom
/// of a page that does not fill the screen.
///
/// ```
/// # use frus_widgets::{Center, CustomScrollView, SliverFillRemaining, SliverToBoxAdapter, Text};
/// let _v: CustomScrollView = CustomScrollView::new()
///     .sliver(SliverToBoxAdapter::new(Text::new("Results")))
///     .sliver(SliverFillRemaining::new(Center::new(Text::new("Nothing found"))));
/// ```
pub struct SliverFillRemaining<Msg = crate::callback::Callback> {
    pub(crate) child: Option<Box<dyn Widget<Msg>>>,
    pub(crate) has_scroll_body: bool,
}

impl<Msg> SliverFillRemaining<Msg> {
    /// `child`, as long as what is left of the viewport.
    pub fn new(child: impl Widget<Msg> + 'static) -> Self {
        Self {
            child: Some(Box::new(child)),
            has_scroll_body: true,
        }
    }

    /// The same room, with nothing in it.
    ///
    /// ```
    /// # use frus_widgets::SliverFillRemaining;
    /// let _gap: SliverFillRemaining = SliverFillRemaining::empty();
    /// ```
    pub fn empty() -> Self {
        Self {
            child: None,
            has_scroll_body: true,
        }
    }

    /// `true` (the default): the child is exactly what is left, and squeezed into it if
    /// it is longer. `false`: what is left **or the child's own length**, whichever is
    /// more, so a footer taller than the room scrolls rather than being cut.
    ///
    /// ```
    /// # use frus_widgets::{SliverFillRemaining, Text};
    /// let _f: SliverFillRemaining = SliverFillRemaining::new(Text::new("Footer")).has_scroll_body(false);
    /// ```
    pub fn has_scroll_body(mut self, has: bool) -> Self {
        self.has_scroll_body = has;
        self
    }
}

impl<Msg> From<SliverFillRemaining<Msg>> for Sliver<Msg> {
    fn from(fill: SliverFillRemaining<Msg>) -> Self {
        Sliver {
            kind: SliverKind::Fill(fill),
        }
    }
}

/// What a header is told when it is built: how far it has been collapsed, and whether
/// content is scrolling under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliverHeaderState {
    /// How far it has been scrolled into, from `0` (fully expanded) up to `max_extent`.
    pub shrink_offset: f32,
    /// The extent it is being laid out at now: `max_extent - shrink_offset`, never under
    /// `min_extent`.
    pub extent: f32,
    /// Its smallest extent.
    pub min_extent: f32,
    /// Its largest extent.
    pub max_extent: f32,
    /// Whether content is scrolling under it — a cue to lift it with a shadow or a tint.
    pub overlaps_content: bool,
    /// Its extent across the scroll: the width of a vertical view.
    pub cross_extent: f32,
}

impl SliverHeaderState {
    /// How collapsed it is, from `0.0` (at `max_extent`) to `1.0` (at `min_extent`).
    ///
    /// ```
    /// # use frus_widgets::SliverHeaderState;
    /// let s = SliverHeaderState { shrink_offset: 50.0, extent: 150.0, min_extent: 100.0,
    ///     max_extent: 200.0, overlaps_content: false, cross_extent: 400.0 };
    /// assert_eq!(s.progress(), 0.5);
    /// ```
    pub fn progress(&self) -> f32 {
        let range = self.max_extent - self.min_extent;
        if range <= 0.0 {
            return 1.0;
        }
        ((self.max_extent - self.extent) / range).clamp(0.0, 1.0)
    }
}

/// How a header moves with the scroll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeaderMode {
    /// Scrolls away with the content.
    Scrolling,
    /// Collapses to its smallest extent, then stays.
    Pinned,
    /// Scrolls away, and comes back as soon as the content is scrolled back.
    Floating,
    /// Comes back as soon as the content is scrolled back, and never goes below its
    /// smallest extent.
    FloatingPinned,
}

impl HeaderMode {
    pub(crate) fn of(pinned: bool, floating: bool) -> Self {
        match (pinned, floating) {
            (false, false) => HeaderMode::Scrolling,
            (true, false) => HeaderMode::Pinned,
            (false, true) => HeaderMode::Floating,
            (true, true) => HeaderMode::FloatingPinned,
        }
    }

    pub(crate) fn floats(self) -> bool {
        matches!(self, HeaderMode::Floating | HeaderMode::FloatingPinned)
    }
}

type HeaderBuild<Msg> = Box<dyn Fn(SliverHeaderState) -> Box<dyn Widget<Msg>>>;
type HeaderExtents = Box<dyn Fn(&Theme) -> (f32, f32)>;

/// A header, as the view lays it out: its extents, how it moves, and how it is built.
pub(crate) struct HeaderSpec<Msg> {
    /// `(min, max)` extents, asked once a frame of the theme in force.
    pub(crate) extents: HeaderExtents,
    pub(crate) pinned: bool,
    pub(crate) floating: bool,
    pub(crate) snap: bool,
    pub(crate) build: HeaderBuild<Msg>,
}

impl<Msg> HeaderSpec<Msg> {
    pub(crate) fn mode(&self) -> HeaderMode {
        HeaderMode::of(self.pinned, self.floating)
    }
}

/// **A header that collapses** between two extents as the view scrolls, built from a
/// [`SliverHeaderState`] every frame — a section title that sticks, a search field that floats
/// back as soon as the reader scrolls up.
///
/// ```
/// # use frus_widgets::{Container, CustomScrollView, SliverPersistentHeader, Text};
/// let _v: CustomScrollView = CustomScrollView::new().sliver(
///     SliverPersistentHeader::new(48.0, 120.0, |state| {
///         Container::new().child(Text::new(format!("{:.0}%", state.progress() * 100.0)))
///     })
///     .pinned(true),
/// );
/// ```
///
/// Alone, it scrolls away. [`pinned`](Self::pinned) stops it at `min_extent` at the top;
/// [`floating`](Self::floating) brings it back as soon as the content is scrolled back;
/// [`snap`](Self::snap) finishes a floating header's showing or hiding once the scroll
/// stops. The widget it builds is given exactly the current extent.
pub struct SliverPersistentHeader<Msg = crate::callback::Callback> {
    spec: HeaderSpec<Msg>,
}

impl<Msg> SliverPersistentHeader<Msg> {
    /// A header between `min_extent` and `max_extent`, built by `build`.
    pub fn new<W: Widget<Msg> + 'static>(
        min_extent: f32,
        max_extent: f32,
        build: impl Fn(SliverHeaderState) -> W + 'static,
    ) -> Self {
        let min = min_extent.max(0.0);
        let max = max_extent.max(min);
        Self {
            spec: HeaderSpec {
                extents: Box::new(move |_| (min, max)),
                pinned: false,
                floating: false,
                snap: false,
                build: Box::new(move |state| Box::new(build(state)) as Box<dyn Widget<Msg>>),
            },
        }
    }

    /// Stops at `min_extent` at the top of the view rather than scrolling away.
    pub fn pinned(mut self, pinned: bool) -> Self {
        self.spec.pinned = pinned;
        self
    }

    /// Comes back as soon as the content is scrolled back, wherever the view is.
    pub fn floating(mut self, floating: bool) -> Self {
        self.spec.floating = floating;
        self
    }

    /// For a floating header: once the scroll stops, finishes showing it or hiding it,
    /// whichever way the scroll was going.
    pub fn snap(mut self, snap: bool) -> Self {
        self.spec.snap = snap;
        self
    }
}

impl<Msg> From<SliverPersistentHeader<Msg>> for Sliver<Msg> {
    fn from(header: SliverPersistentHeader<Msg>) -> Self {
        Sliver {
            kind: SliverKind::Header(header.spec),
        }
    }
}

// ---------------------------------------------------------------------------------------
// The app bar
// ---------------------------------------------------------------------------------------

/// A widget shared between the bars a [`SliverAppBar`] builds, one a frame.
struct Shared<Msg> {
    inner: Rc<dyn Widget<Msg>>,
}

impl<Msg> Shared<Msg> {
    fn restyle(&self, base: Style) -> Style {
        base
    }
}

crate::transparent::forward_transparent!(Shared {
    fn key(&self) -> Option<u64> {
        self.inner.key()
    }
    fn positioned(&self) -> Option<crate::positioned::Positioning> {
        self.inner.positioned()
    }
    fn theme_override(&self, inherited: &crate::theme::Theme) -> Option<Box<crate::theme::Theme>> {
        self.inner.theme_override(inherited)
    }
    fn media_override(&self, inherited: crate::MediaQuery) -> Option<crate::MediaQuery> {
        self.inner.media_override(inherited)
    }
    fn scaffold_override(&self) -> Option<crate::ScaffoldInfo> {
        self.inner.scaffold_override()
    }
    fn scroll_behavior_override(&self) -> Option<crate::ScrollBehavior> {
        self.inner.scroll_behavior_override()
    }
    fn autofill_group(&self) -> bool {
        self.inner.autofill_group()
    }
    fn area_toolbar(
        &self,
        context: crate::ToolbarContext,
    ) -> Option<Option<&dyn crate::widget::Widget<Msg>>> {
        self.inner.area_toolbar(context)
    }
});

fn shared<Msg: 'static>(widget: &Rc<dyn Widget<Msg>>) -> Shared<Msg> {
    Shared {
        inner: widget.clone(),
    }
}

/// What an expanded [`SliverAppBar`] shows behind its toolbar: a title that shrinks into
/// the toolbar as the bar collapses, over a background that fades out.
///
/// ```
/// # use frus_widgets::{Container, FlexibleSpaceBar, Text};
/// # use frus_core::Color;
/// let _space: FlexibleSpaceBar =
///     FlexibleSpaceBar::new().title(Text::new("Paris")).background(Container::new().color(Color::rgb(0.1, 0.3, 0.6)));
/// ```
pub struct FlexibleSpaceBar<Msg = crate::callback::Callback> {
    title: Option<Rc<dyn Widget<Msg>>>,
    background: Option<Rc<dyn Widget<Msg>>>,
    center_title: Option<bool>,
    expanded_title_scale: f32,
    title_padding: Option<(f32, f32)>,
    title_style: Option<TextStyle>,
}

impl<Msg> Default for FlexibleSpaceBar<Msg> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Msg> FlexibleSpaceBar<Msg> {
    /// An empty space: no title, no background.
    pub fn new() -> Self {
        Self {
            title: None,
            background: None,
            center_title: None,
            expanded_title_scale: 1.5,
            title_padding: None,
            title_style: None,
        }
    }

    /// The title, drawn [`expanded_title_scale`](Self::expanded_title_scale) times larger
    /// at the bottom of the expanded bar and at its toolbar size once it has collapsed.
    pub fn title(mut self, title: impl Widget<Msg> + 'static) -> Self {
        self.title = Some(Rc::new(title));
        self
    }

    /// What fills the expanded bar behind the title — a picture, a gradient. It moves up
    /// at half the scroll's pace and fades out as the bar reaches its toolbar.
    ///
    /// ```
    /// # use frus_widgets::{Container, FlexibleSpaceBar};
    /// let _s: FlexibleSpaceBar = FlexibleSpaceBar::new().background(Container::new());
    /// ```
    pub fn background(mut self, background: impl Widget<Msg> + 'static) -> Self {
        self.background = Some(Rc::new(background));
        self
    }

    /// Centres the title; unset, the platform's convention decides, as for an app bar.
    ///
    /// ```
    /// # use frus_widgets::{FlexibleSpaceBar, Text};
    /// let _s: FlexibleSpaceBar = FlexibleSpaceBar::new().title(Text::new("A")).center_title(true);
    /// ```
    pub fn center_title(mut self, centered: bool) -> Self {
        self.center_title = Some(centered);
        self
    }

    /// How much larger the title is when the bar is fully expanded; `1.5` unless told.
    ///
    /// ```
    /// # use frus_widgets::FlexibleSpaceBar;
    /// let _s: FlexibleSpaceBar = FlexibleSpaceBar::new().expanded_title_scale(2.0);
    /// ```
    pub fn expanded_title_scale(mut self, scale: f32) -> Self {
        self.expanded_title_scale = scale.max(1.0);
        self
    }

    /// The room between the title and the bar's start edge and bottom edge: unset, 72
    /// at the start beside a leading button (16 without one) and 16 at the bottom.
    ///
    /// ```
    /// # use frus_widgets::FlexibleSpaceBar;
    /// let _s: FlexibleSpaceBar = FlexibleSpaceBar::new().title_padding(24.0, 12.0);
    /// ```
    pub fn title_padding(mut self, start: f32, bottom: f32) -> Self {
        self.title_padding = Some((start, bottom));
        self
    }

    /// The title's type at toolbar size; unset, the app bar theme's title style, else the
    /// theme's `title_large`.
    ///
    /// ```
    /// # use frus_widgets::FlexibleSpaceBar;
    /// # use frus_core::TextStyle;
    /// let _s: FlexibleSpaceBar = FlexibleSpaceBar::new().title_style(TextStyle::new(20.0));
    /// ```
    pub fn title_style(mut self, style: TextStyle) -> Self {
        self.title_style = Some(style);
        self
    }
}

/// What a custom flexible space is told when it is built; see
/// [`SliverAppBar::flexible_space_builder`].
pub type FlexibleSpaceState = SliverHeaderState;

enum Space<Msg> {
    None,
    Bar(FlexibleSpaceBar<Msg>),
    Custom(Rc<dyn Widget<Msg>>),
    Built(Rc<dyn Fn(FlexibleSpaceState) -> Box<dyn Widget<Msg>>>),
}

/// **An app bar as a piece** of a [`CustomScrollView`]: it scrolls away with the content,
/// stays as a toolbar ([`pinned`](Self::pinned)), comes back as soon as the reader scrolls
/// up ([`floating`](Self::floating), [`snap`](Self::snap)), and collapses from
/// [`expanded_height`](Self::expanded_height) to its toolbar as it goes.
///
/// ```
/// use frus_widgets::{CustomScrollView, FlexibleSpaceBar, IconButton, Icons, SliverAppBar, SliverList, Text};
///
/// let _page: CustomScrollView = CustomScrollView::new()
///     .sliver(
///         SliverAppBar::new()
///             .pinned(true)
///             .expanded_height(220.0)
///             .action(IconButton::new(Icons::SEARCH))
///             .flexible_space(FlexibleSpaceBar::new().title(Text::new("Lisbon"))),
///     )
///     .sliver(SliverList::new(100, 56.0, |i| Text::new(format!("Place {i}"))));
/// ```
///
/// **Themed**: the surface is the app bar theme's `background`, else the scheme's
/// `surface`; once content scrolls under it, the theme's `scrolled_under_background`, else
/// `surface_container`, lifted to `scrolled_under_elevation` (a shadow only where the
/// theme or [`shadow_color`](Self::shadow_color) names one). Every one of them is
/// overridden by the builder method of the same name.
pub struct SliverAppBar<Msg = crate::callback::Callback> {
    title: Option<Rc<dyn Widget<Msg>>>,
    leading: Option<Rc<dyn Widget<Msg>>>,
    actions: Vec<Rc<dyn Widget<Msg>>>,
    space: Space<Msg>,
    expanded_height: Option<f32>,
    collapsed_height: Option<f32>,
    toolbar_height: Option<f32>,
    pinned: bool,
    floating: bool,
    snap: bool,
    primary: bool,
    force_elevated: bool,
    center_title: Option<bool>,
    background: Option<Color>,
    scrolled_under_background: Option<Color>,
    foreground: Option<Color>,
    elevation: Option<f32>,
    scrolled_under_elevation: Option<f32>,
    shadow_color: Option<Color>,
    automatically_imply_leading: bool,
}

impl<Msg: Clone + 'static> Default for SliverAppBar<Msg> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Msg: Clone + 'static> SliverAppBar<Msg> {
    /// A bar with no title that scrolls away with the content.
    pub fn new() -> Self {
        Self {
            title: None,
            leading: None,
            actions: Vec::new(),
            space: Space::None,
            expanded_height: None,
            collapsed_height: None,
            toolbar_height: None,
            pinned: false,
            floating: false,
            snap: false,
            primary: true,
            force_elevated: false,
            center_title: None,
            background: None,
            scrolled_under_background: None,
            foreground: None,
            elevation: None,
            scrolled_under_elevation: None,
            shadow_color: None,
            automatically_imply_leading: true,
        }
    }

    /// The title in the toolbar. For a title that shrinks into the toolbar as an
    /// expanded bar collapses, give it to a [`FlexibleSpaceBar`] instead.
    ///
    /// ```
    /// # use frus_widgets::{SliverAppBar, Text};
    /// let _b: SliverAppBar = SliverAppBar::new().title(Text::new("Inbox"));
    /// ```
    pub fn title(mut self, title: impl Widget<Msg> + 'static) -> Self {
        self.title = Some(Rc::new(title));
        self
    }

    /// The widget at the start of the toolbar; unset, the shell's menu or back button.
    ///
    /// ```
    /// # use frus_widgets::{IconButton, Icons, SliverAppBar};
    /// let _b: SliverAppBar = SliverAppBar::new().leading(IconButton::new(Icons::MENU));
    /// ```
    pub fn leading(mut self, leading: impl Widget<Msg> + 'static) -> Self {
        self.leading = Some(Rc::new(leading));
        self
    }

    /// Whether a bar with no leading of its own takes the shell's menu or back button.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// let _b: SliverAppBar = SliverAppBar::new().automatically_imply_leading(false);
    /// ```
    pub fn automatically_imply_leading(mut self, imply: bool) -> Self {
        self.automatically_imply_leading = imply;
        self
    }

    /// Adds a widget at the end of the toolbar — an icon button, usually.
    ///
    /// ```
    /// # use frus_widgets::{IconButton, Icons, SliverAppBar};
    /// let _b: SliverAppBar = SliverAppBar::new().action(IconButton::new(Icons::SEARCH));
    /// ```
    pub fn action(mut self, action: impl Widget<Msg> + 'static) -> Self {
        self.actions.push(Rc::new(action));
        self
    }

    /// The expanded bar's [`FlexibleSpaceBar`]: a title that collapses into the toolbar
    /// over a background.
    ///
    /// ```
    /// # use frus_widgets::{FlexibleSpaceBar, SliverAppBar, Text};
    /// let _b: SliverAppBar = SliverAppBar::new()
    ///     .expanded_height(200.0)
    ///     .flexible_space(FlexibleSpaceBar::new().title(Text::new("Paris")));
    /// ```
    pub fn flexible_space(mut self, space: FlexibleSpaceBar<Msg>) -> Self {
        self.space = Space::Bar(space);
        self
    }

    /// Any widget behind the toolbar, filling the bar at whatever extent it is.
    ///
    /// ```
    /// # use frus_widgets::{Container, SliverAppBar};
    /// let _b: SliverAppBar = SliverAppBar::new().expanded_height(160.0).flexible_space_widget(Container::new());
    /// ```
    pub fn flexible_space_widget(mut self, space: impl Widget<Msg> + 'static) -> Self {
        self.space = Space::Custom(Rc::new(space));
        self
    }

    /// A space behind the toolbar built every frame from how collapsed the bar is: the
    /// whole collapse, done your way.
    ///
    /// ```
    /// # use frus_widgets::{Container, SliverAppBar};
    /// # use frus_core::Color;
    /// let _b: SliverAppBar = SliverAppBar::new()
    ///     .expanded_height(160.0)
    ///     .flexible_space_builder(|s| Container::new().color(Color::rgb(0.0, 0.0, s.progress())));
    /// ```
    pub fn flexible_space_builder<W: Widget<Msg> + 'static>(
        mut self,
        build: impl Fn(FlexibleSpaceState) -> W + 'static,
    ) -> Self {
        self.space = Space::Built(Rc::new(move |state| {
            Box::new(build(state)) as Box<dyn Widget<Msg>>
        }));
        self
    }

    /// The bar's extent fully expanded, toolbar and status bar included. Unset, it is
    /// as tall as its toolbar and does not collapse.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// let _b: SliverAppBar = SliverAppBar::new().expanded_height(240.0);
    /// ```
    pub fn expanded_height(mut self, height: f32) -> Self {
        self.expanded_height = Some(height.max(0.0));
        self
    }

    /// The extent a pinned bar collapses to; unset, the toolbar (and the status bar
    /// above it, for a primary bar).
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// let _b: SliverAppBar = SliverAppBar::new().pinned(true).collapsed_height(80.0);
    /// ```
    pub fn collapsed_height(mut self, height: f32) -> Self {
        self.collapsed_height = Some(height.max(0.0));
        self
    }

    /// The toolbar's height; unset, the app bar theme's, else the platform toolbar height.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// let _b: SliverAppBar = SliverAppBar::new().toolbar_height(56.0);
    /// ```
    pub fn toolbar_height(mut self, height: f32) -> Self {
        self.toolbar_height = Some(height.max(0.0));
        self
    }

    /// Stays at the top as a toolbar once collapsed, rather than scrolling away.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// let _b: SliverAppBar = SliverAppBar::new().pinned(true);
    /// ```
    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// Comes back as soon as the reader scrolls up, wherever the content is.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// let _b: SliverAppBar = SliverAppBar::new().floating(true);
    /// ```
    pub fn floating(mut self, floating: bool) -> Self {
        self.floating = floating;
        self
    }

    /// For a floating bar: once the scroll stops half way, finishes showing or hiding it.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// let _b: SliverAppBar = SliverAppBar::new().floating(true).snap(true);
    /// ```
    pub fn snap(mut self, snap: bool) -> Self {
        self.snap = snap;
        self
    }

    /// Whether the bar is at the top of the screen and takes the status bar's room above
    /// its toolbar; `true` unless told.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// let _b: SliverAppBar = SliverAppBar::new().primary(false);
    /// ```
    pub fn primary(mut self, primary: bool) -> Self {
        self.primary = primary;
        self
    }

    /// Draws the bar lifted even with nothing under it.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// let _b: SliverAppBar = SliverAppBar::new().force_elevated(true);
    /// ```
    pub fn force_elevated(mut self, force: bool) -> Self {
        self.force_elevated = force;
        self
    }

    /// Centres the toolbar title; unset, the theme's, else the platform's convention.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// let _b: SliverAppBar = SliverAppBar::new().center_title(true);
    /// ```
    pub fn center_title(mut self, centered: bool) -> Self {
        self.center_title = Some(centered);
        self
    }

    /// The bar's surface at rest.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// # use frus_core::Color;
    /// let _b: SliverAppBar = SliverAppBar::new().background(Color::rgb(0.2, 0.2, 0.3));
    /// ```
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// The bar's surface once content scrolls under it.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// # use frus_core::Color;
    /// let _b: SliverAppBar = SliverAppBar::new().scrolled_under_background(Color::rgb(0.3, 0.3, 0.4));
    /// ```
    pub fn scrolled_under_background(mut self, color: Color) -> Self {
        self.scrolled_under_background = Some(color);
        self
    }

    /// The colour of the title, the leading and the actions.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// # use frus_core::Color;
    /// let _b: SliverAppBar = SliverAppBar::new().foreground(Color::WHITE);
    /// ```
    pub fn foreground(mut self, color: Color) -> Self {
        self.foreground = Some(color);
        self
    }

    /// The bar's elevation at rest.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// let _b: SliverAppBar = SliverAppBar::new().elevation(1.0);
    /// ```
    pub fn elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation.max(0.0));
        self
    }

    /// The bar's elevation once content scrolls under it; unset, the theme's, else 3.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// let _b: SliverAppBar = SliverAppBar::new().scrolled_under_elevation(4.0);
    /// ```
    pub fn scrolled_under_elevation(mut self, elevation: f32) -> Self {
        self.scrolled_under_elevation = Some(elevation.max(0.0));
        self
    }

    /// The colour of the shadow a lifted bar casts; unset, the theme's, else none.
    ///
    /// ```
    /// # use frus_widgets::SliverAppBar;
    /// # use frus_core::Color;
    /// let _b: SliverAppBar = SliverAppBar::new().shadow_color(Color::BLACK);
    /// ```
    pub fn shadow_color(mut self, color: Color) -> Self {
        self.shadow_color = Some(color);
        self
    }

    /// The toolbar's height, as the theme has it.
    fn toolbar_of(toolbar: Option<f32>, theme: &Theme) -> f32 {
        toolbar
            .or(theme.widgets.app_bar.height)
            .unwrap_or(crate::appbar::APP_BAR_HEIGHT)
    }

    /// `(min, max)`: the toolbar under the status bar, and the expanded extent.
    fn extents_of(
        toolbar: Option<f32>,
        collapsed: Option<f32>,
        expanded: Option<f32>,
        primary: bool,
        theme: &Theme,
    ) -> (f32, f32) {
        let top = if primary {
            crate::MediaQuery::of().padding.top
        } else {
            0.0
        };
        let toolbar = Self::toolbar_of(toolbar, theme);
        let min = collapsed.unwrap_or(toolbar + top).max(toolbar + top);
        let max = expanded.unwrap_or(toolbar + top).max(min);
        (min, max)
    }
}

/// Everything a frame's bar is made from, shared by the frames.
struct BarParts<Msg> {
    title: Option<Rc<dyn Widget<Msg>>>,
    leading: Option<Rc<dyn Widget<Msg>>>,
    actions: Vec<Rc<dyn Widget<Msg>>>,
    space: Space<Msg>,
    toolbar_height: Option<f32>,
    pinned: bool,
    floating: bool,
    primary: bool,
    force_elevated: bool,
    center_title: Option<bool>,
    background: Option<Color>,
    scrolled_under_background: Option<Color>,
    foreground: Option<Color>,
    elevation: Option<f32>,
    scrolled_under_elevation: Option<f32>,
    shadow_color: Option<Color>,
    automatically_imply_leading: bool,
}

impl<Msg: Clone + 'static> BarParts<Msg> {
    /// The bar as it is at `state`.
    fn build(&self, state: SliverHeaderState) -> Box<dyn Widget<Msg>> {
        let parts = BarFrame {
            title: self.title.clone(),
            leading: self.leading.clone(),
            actions: self.actions.clone(),
            space: match &self.space {
                Space::None => Space::None,
                Space::Bar(bar) => Space::Bar(FlexibleSpaceBar {
                    title: bar.title.clone(),
                    background: bar.background.clone(),
                    center_title: bar.center_title,
                    expanded_title_scale: bar.expanded_title_scale,
                    title_padding: bar.title_padding,
                    title_style: bar.title_style,
                }),
                Space::Custom(w) => Space::Custom(w.clone()),
                Space::Built(b) => Space::Built(b.clone()),
            },
            state,
            toolbar_height: self.toolbar_height,
            pinned: self.pinned,
            floating: self.floating,
            primary: self.primary,
            force_elevated: self.force_elevated,
            center_title: self.center_title,
            background: self.background,
            scrolled_under_background: self.scrolled_under_background,
            foreground: self.foreground,
            elevation: self.elevation,
            scrolled_under_elevation: self.scrolled_under_elevation,
            shadow_color: self.shadow_color,
            automatically_imply_leading: self.automatically_imply_leading,
        };
        Box::new(crate::themebuilder::ThemeBuilder::boxed(move |theme| {
            parts.compose(theme)
        }))
    }
}

/// One frame's bar, composed once the theme is known.
struct BarFrame<Msg> {
    title: Option<Rc<dyn Widget<Msg>>>,
    leading: Option<Rc<dyn Widget<Msg>>>,
    actions: Vec<Rc<dyn Widget<Msg>>>,
    space: Space<Msg>,
    state: SliverHeaderState,
    toolbar_height: Option<f32>,
    pinned: bool,
    floating: bool,
    primary: bool,
    force_elevated: bool,
    center_title: Option<bool>,
    background: Option<Color>,
    scrolled_under_background: Option<Color>,
    foreground: Option<Color>,
    elevation: Option<f32>,
    scrolled_under_elevation: Option<f32>,
    shadow_color: Option<Color>,
    automatically_imply_leading: bool,
}

/// What a bar's surface looks like at `state`: its colour and its elevation. Pure, so the
/// rule is tested on its own.
pub(crate) fn bar_surface(
    theme: &Theme,
    background: Option<Color>,
    scrolled_under_background: Option<Color>,
    elevation: Option<f32>,
    scrolled_under_elevation: Option<f32>,
    scrolled_under: bool,
) -> (Color, f32) {
    let t = theme.widgets.app_bar;
    let rest = background.or(t.background).unwrap_or(theme.scheme.surface);
    if scrolled_under {
        let color = scrolled_under_background
            .or(t.scrolled_under_background)
            .unwrap_or(theme.scheme.surface_container);
        let lift = scrolled_under_elevation
            .or(t.scrolled_under_elevation)
            .unwrap_or(3.0);
        (color, lift)
    } else {
        (rest, elevation.or(t.elevation).unwrap_or(0.0))
    }
}

/// How opaque the toolbar of a bar that is not pinned is: it fades out over the last
/// toolbar's height of its travel.
pub(crate) fn toolbar_opacity(
    pinned: bool,
    state: SliverHeaderState,
    top: f32,
    toolbar_height: f32,
) -> f32 {
    if pinned || toolbar_height <= 0.0 {
        return 1.0;
    }
    let visible = state.max_extent - state.shrink_offset - top;
    (visible / toolbar_height).clamp(0.0, 1.0)
}

impl<Msg: Clone + 'static> BarFrame<Msg> {
    fn compose(self, theme: &Theme) -> Box<dyn Widget<Msg>> {
        let state = self.state;
        let top = if self.primary {
            crate::MediaQuery::of().padding.top
        } else {
            0.0
        };
        let toolbar_h = SliverAppBar::<Msg>::toolbar_of(self.toolbar_height, theme);
        let scrolled_under = self.force_elevated
            || state.overlaps_content
            || (self.pinned && state.shrink_offset > state.max_extent - state.min_extent);
        let (surface, lift) = bar_surface(
            theme,
            self.background,
            self.scrolled_under_background,
            self.elevation,
            self.scrolled_under_elevation,
            scrolled_under,
        );
        let foreground = self
            .foreground
            .or(theme.widgets.app_bar.foreground)
            .unwrap_or(theme.scheme.on_surface);
        let width = state.cross_extent;
        let extent = state.extent;
        let rtl = theme.direction.is_rtl();

        let mut stack = crate::Stack::new().width(width).height(extent);

        // Behind the toolbar: the flexible space, at the bar's current extent.
        let mut flexible_title: Option<Box<dyn Widget<Msg>>> = None;
        match self.space {
            Space::None => {}
            Space::Custom(widget) => {
                stack = stack.layer(crate::Positioned::fill(shared(&widget)));
            }
            Space::Built(build) => {
                stack = stack.layer(crate::Positioned::fill(Shared {
                    inner: Rc::from(build(state)),
                }));
            }
            Space::Bar(bar) => {
                let t = state.progress();
                let delta = state.max_extent - state.min_extent;
                if let Some(background) = &bar.background {
                    // Fades out over the last toolbar's height of the collapse, and moves
                    // up at half the scroll's pace.
                    let fade_start = if delta > 0.0 {
                        (1.0 - toolbar_h / delta).max(0.0)
                    } else {
                        0.0
                    };
                    let opacity = if t <= fade_start {
                        1.0
                    } else if fade_start >= 1.0 {
                        0.0
                    } else {
                        1.0 - (t - fade_start) / (1.0 - fade_start)
                    };
                    let shift = -(delta / 2.0) * t;
                    let back = crate::Container::new()
                        .height(state.max_extent)
                        .opacity(opacity.clamp(0.0, 1.0))
                        .child(shared(background));
                    stack = stack.layer(
                        crate::Positioned::new(back)
                            .top(shift)
                            .left(0.0)
                            .right(0.0)
                            .height(state.max_extent),
                    );
                }
                if let Some(title) = &bar.title {
                    let has_leading = self.leading.is_some()
                        || (self.automatically_imply_leading && {
                            let shell = crate::ScaffoldInfo::of();
                            shell.has_drawer() || shell.back::<Msg>().is_some()
                        });
                    let centered = bar
                        .center_title
                        .or(self.center_title)
                        .or(theme.widgets.app_bar.center_title)
                        .unwrap_or_else(|| {
                            crate::appbar::platform_centers_title(
                                theme.platform,
                                self.actions.len(),
                            )
                        });
                    let (start, bottom) = bar.title_padding.unwrap_or((
                        if centered {
                            0.0
                        } else if has_leading {
                            72.0
                        } else {
                            16.0
                        },
                        16.0,
                    ));
                    let scale = bar.expanded_title_scale + (1.0 - bar.expanded_title_scale) * t;
                    let mut style = bar
                        .title_style
                        .or(theme.widgets.app_bar.title_style)
                        .unwrap_or(theme.text.title_large);
                    style.color = Some(foreground);
                    let words = crate::DefaultTextStyle {
                        soft_wrap: Some(false),
                        overflow: Some(frus_core::TextOverflow::Ellipsis),
                        ..crate::DefaultTextStyle::from_text_style(style)
                    };
                    let dressed = crate::Themed::tweak(
                        move |t| t.widgets.text = t.widgets.text.merge(words),
                        shared(title),
                    );
                    // The room it has at toolbar size, so that scaled up it still ends
                    // inside the bar.
                    let room = ((width - start - 16.0) / scale).max(0.0);
                    let pivot = match (centered, rtl) {
                        (true, _) => Alignment::BOTTOM_CENTER,
                        (false, false) => Alignment::BOTTOM_LEFT,
                        (false, true) => Alignment::BOTTOM_RIGHT,
                    };
                    let scaled = crate::Transform::scale_from(scale, pivot).child(dressed);
                    let placed: Box<dyn Widget<Msg>> = if centered {
                        let column = crate::Container::new()
                            .width(width)
                            .alignment(Alignment::BOTTOM_CENTER)
                            .child(scaled);
                        Box::new(crate::Positioned::new(column).left(0.0).bottom(bottom))
                    } else {
                        let boxed = crate::Container::new()
                            .width(room)
                            .alignment(if rtl {
                                Alignment::BOTTOM_RIGHT
                            } else {
                                Alignment::BOTTOM_LEFT
                            })
                            .child(scaled);
                        Box::new(crate::Positioned::new(boxed).start(start).bottom(bottom))
                    };
                    flexible_title = Some(placed);
                }
            }
        }

        // The toolbar, at the top of the bar's box.
        let mut bar = crate::AppBar::new()
            .width(width)
            .height(toolbar_h)
            .primary(self.primary)
            .force_material_transparency(true)
            .automatically_imply_leading(self.automatically_imply_leading)
            .foreground(foreground)
            .toolbar_opacity(toolbar_opacity(self.pinned, state, top, toolbar_h));
        if let Some(title) = &self.title {
            bar = bar.title(shared(title));
        }
        if let Some(leading) = &self.leading {
            bar = bar.leading(shared(leading));
        }
        for action in &self.actions {
            bar = bar.action(shared(action));
        }
        if let Some(centered) = self.center_title {
            bar = bar.center_title(centered);
        }
        // A bar that is not pinned scrolls its toolbar away; a floating one too, so the
        // toolbar sits at the bottom of what is left of it when it is shorter than its
        // toolbar, which is the child's own position.
        let _ = self.floating;
        stack = stack.layer(crate::Positioned::new(bar).top(0.0).left(0.0).right(0.0));
        if let Some(title) = flexible_title {
            stack = stack.layer(title);
        }

        let mut chrome = crate::Container::new()
            .width(width)
            .height(extent)
            .color(surface)
            .clip();
        if let Some(shadow) = self
            .shadow_color
            .or(theme.widgets.app_bar.shadow_color)
            .filter(|shadow| lift > 0.0 && shadow.a > 0.0)
        {
            chrome = chrome.shadow(0.0, lift * 0.25, lift, shadow);
        }
        Box::new(chrome.child(stack))
    }
}

impl<Msg: Clone + 'static> From<SliverAppBar<Msg>> for Sliver<Msg> {
    fn from(bar: SliverAppBar<Msg>) -> Self {
        let (toolbar, collapsed, expanded, primary) = (
            bar.toolbar_height,
            bar.collapsed_height,
            bar.expanded_height,
            bar.primary,
        );
        let (pinned, floating, snap) = (bar.pinned, bar.floating, bar.snap && bar.floating);
        let parts = BarParts {
            title: bar.title,
            leading: bar.leading,
            actions: bar.actions,
            space: bar.space,
            toolbar_height: bar.toolbar_height,
            pinned,
            floating,
            primary,
            force_elevated: bar.force_elevated,
            center_title: bar.center_title,
            background: bar.background,
            scrolled_under_background: bar.scrolled_under_background,
            foreground: bar.foreground,
            elevation: bar.elevation,
            scrolled_under_elevation: bar.scrolled_under_elevation,
            shadow_color: bar.shadow_color,
            automatically_imply_leading: bar.automatically_imply_leading,
        };
        Sliver {
            kind: SliverKind::Header(HeaderSpec {
                extents: Box::new(move |theme| {
                    SliverAppBar::<Msg>::extents_of(toolbar, collapsed, expanded, primary, theme)
                }),
                pinned,
                floating,
                snap,
                build: Box::new(move |state| parts.build(state)),
            }),
        }
    }
}

// ---------------------------------------------------------------------------------------
// The geometry
// ---------------------------------------------------------------------------------------

/// What a piece is told when it is laid out, along the scroll.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SliverConstraints {
    /// How far into this piece the view has scrolled.
    pub(crate) scroll_offset: f32,
    /// How much of the viewport's start the pieces before this one paint over.
    pub(crate) overlap: f32,
    /// How much of the viewport is left from where this piece starts.
    pub(crate) remaining_paint_extent: f32,
    /// The viewport's extent along the scroll.
    pub(crate) viewport_main_extent: f32,
    /// The extent across the scroll.
    pub(crate) cross_axis_extent: f32,
    /// How far the pieces before this one scroll, together.
    pub(crate) preceding_scroll_extent: f32,
    /// How far before `scroll_offset` lazy children are still built.
    pub(crate) cache_before: f32,
    /// How far from `scroll_offset` lazy children are built: what is left of the viewport,
    /// and of the cache extent after it.
    pub(crate) remaining_cache_extent: f32,
}

/// What a piece answers once laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SliverGeometry {
    /// How far it scrolls.
    pub(crate) scroll_extent: f32,
    /// How much of the viewport it paints.
    pub(crate) paint_extent: f32,
    /// Where it paints, relative to where it is laid out.
    pub(crate) paint_origin: f32,
    /// How much room it takes from the pieces after it.
    pub(crate) layout_extent: f32,
}

/// How much of `[from, to)`, in the piece's scroll coordinates, is in the viewport.
pub(crate) fn paint_offset(c: &SliverConstraints, from: f32, to: f32) -> f32 {
    let a = c.scroll_offset;
    let b = c.scroll_offset + c.remaining_paint_extent.max(0.0);
    let clamp = |v: f32| v.max(a).min(b);
    (clamp(to) - clamp(from))
        .max(0.0)
        .min(c.remaining_paint_extent.max(0.0))
}

/// A piece that is one run of `extent`, painted as far as it shows.
pub(crate) fn box_geometry(c: &SliverConstraints, extent: f32) -> SliverGeometry {
    let paint = paint_offset(c, 0.0, extent);
    SliverGeometry {
        scroll_extent: extent,
        paint_extent: paint,
        paint_origin: 0.0,
        layout_extent: paint,
    }
}

/// A header's layout: its geometry, the extent its child is given, where the child sits
/// relative to where the header paints, and how far it is collapsed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct HeaderLayout {
    pub(crate) geometry: SliverGeometry,
    pub(crate) child_extent: f32,
    pub(crate) child_position: f32,
    pub(crate) shrink_offset: f32,
    pub(crate) overlaps_content: bool,
}

/// Lays out a header between `min` and `max`. `effective` is, for a floating header, how
/// far it is scrolled away (see [`floating_effective`]); the others ignore it.
pub(crate) fn header_layout(
    c: &SliverConstraints,
    min: f32,
    max: f32,
    mode: HeaderMode,
    effective: f32,
) -> HeaderLayout {
    let so = c.scroll_offset;
    let rem = c.remaining_paint_extent.max(0.0);
    let collapse = |offset: f32| {
        let shrink = offset.min(max).max(0.0);
        (shrink, (max - shrink).max(min))
    };
    match mode {
        HeaderMode::Scrolling => {
            let (shrink, child) = collapse(so);
            let paint = max - so;
            let painted = paint.clamp(0.0, rem);
            HeaderLayout {
                geometry: SliverGeometry {
                    scroll_extent: max,
                    paint_extent: painted,
                    paint_origin: c.overlap.min(0.0),
                    layout_extent: painted,
                },
                child_extent: child,
                child_position: (paint - child).min(0.0),
                shrink_offset: shrink,
                overlaps_content: false,
            }
        }
        HeaderMode::Pinned => {
            let (shrink, child) = collapse(so);
            let room = (rem - c.overlap).max(0.0);
            let layout = (max - so).clamp(0.0, room);
            HeaderLayout {
                geometry: SliverGeometry {
                    scroll_extent: max,
                    paint_extent: child.min(room),
                    paint_origin: c.overlap,
                    layout_extent: layout,
                },
                child_extent: child,
                child_position: 0.0,
                shrink_offset: shrink,
                overlaps_content: c.overlap > 0.0,
            }
        }
        HeaderMode::Floating => {
            let (shrink, child) = collapse(effective);
            let paint = max - effective;
            HeaderLayout {
                geometry: SliverGeometry {
                    scroll_extent: max,
                    paint_extent: paint.clamp(0.0, rem),
                    paint_origin: c.overlap.min(0.0),
                    layout_extent: (max - so).clamp(0.0, rem),
                },
                child_extent: child,
                child_position: (paint - child).min(0.0),
                shrink_offset: shrink,
                overlaps_content: effective < so,
            }
        }
        HeaderMode::FloatingPinned => {
            let (shrink, child) = collapse(effective);
            let least = if rem > min { min } else { rem };
            let paint = (max - effective).max(least).min(rem);
            HeaderLayout {
                geometry: SliverGeometry {
                    scroll_extent: max,
                    paint_extent: paint,
                    paint_origin: c.overlap.min(0.0),
                    layout_extent: (max - so).clamp(0.0, paint),
                },
                child_extent: child,
                child_position: 0.0,
                shrink_offset: shrink,
                overlaps_content: effective < so,
            }
        }
    }
}

/// What a floating header keeps from one frame to the next: the scroll offset it was last
/// laid out at, how far it was scrolled away then, which way the scroll last went, and a
/// snap under way.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FloatState {
    pub(crate) last_offset: Option<f32>,
    pub(crate) effective: f32,
    /// `true` when the last movement brought the header back (scrolled towards the start).
    pub(crate) showing: bool,
    /// Whether the view was moving at the last frame.
    pub(crate) moving: bool,
    /// `(from, to, start time)` of a snap under way.
    pub(crate) snap: Option<(f32, f32, f32)>,
}

/// How far a floating header of extent `max` is scrolled away at scroll offset `so`, given
/// what it was at the last frame: scrolling back brings it back by as much, scrolling on
/// takes it away by as much, and it is never further away than the content is scrolled.
pub(crate) fn floating_effective(last: Option<f32>, effective: f32, so: f32, max: f32) -> f32 {
    match last {
        Some(last) if so < last || effective < max => {
            let delta = last - so;
            (effective.min(max) - delta).clamp(0.0, so.max(0.0))
        }
        _ => so,
    }
}

/// Moves a floating header's state to scroll offset `so`: its effective offset, the
/// direction, and a snap that starts when the view comes to rest half way (when `snap`),
/// runs `SNAP_DURATION`, and is dropped as soon as the view moves again.
pub(crate) fn advance_float(
    state: &mut FloatState,
    so: f32,
    max: f32,
    snap: bool,
    at_rest: bool,
    now: f32,
) -> bool {
    if let Some(last) = state.last_offset {
        if (so - last).abs() > 1e-3 {
            state.showing = so < last;
        }
    }
    let mut animating = false;
    if !at_rest {
        state.snap = None;
        state.effective = floating_effective(state.last_offset, state.effective, so, max);
    } else if let Some((from, to, start)) = state.snap {
        let t = ((now - start) / SNAP_DURATION).clamp(0.0, 1.0);
        let eased = frus_core::Curve::ease_out().transform(t);
        state.effective = (from + (to - from) * eased).clamp(0.0, so.max(0.0));
        if t >= 1.0 {
            state.snap = None;
        } else {
            animating = true;
        }
    } else {
        state.effective = floating_effective(state.last_offset, state.effective, so, max);
        if snap && state.moving && state.effective > 0.0 && state.effective < max.min(so) {
            let to = if state.showing { 0.0 } else { max };
            state.snap = Some((state.effective, to, now));
            animating = true;
        }
    }
    state.moving = !at_rest;
    state.last_offset = Some(so);
    animating
}

/// The rest of the viewport, for a [`SliverFillRemaining`].
pub(crate) fn fill_extent(c: &SliverConstraints, child: Option<f32>, has_scroll_body: bool) -> f32 {
    let room = (c.viewport_main_extent - c.preceding_scroll_extent).max(0.0);
    if has_scroll_body {
        room
    } else {
        room.max(child.unwrap_or(0.0))
    }
}

/// The constraints a padded piece is laid out under, inside `before` and `after` along the
/// scroll and `cross` across it.
pub(crate) fn padded_constraints(
    c: &SliverConstraints,
    before: f32,
    cross: f32,
) -> SliverConstraints {
    let before_paint = paint_offset(c, 0.0, before);
    SliverConstraints {
        scroll_offset: (c.scroll_offset - before).max(0.0),
        overlap: (c.overlap - before_paint).max(0.0),
        remaining_paint_extent: (c.remaining_paint_extent - before_paint).max(0.0),
        viewport_main_extent: c.viewport_main_extent,
        cross_axis_extent: (c.cross_axis_extent - cross).max(0.0),
        preceding_scroll_extent: c.preceding_scroll_extent + before,
        cache_before: c.cache_before.min((c.scroll_offset - before).max(0.0)),
        remaining_cache_extent: (c.remaining_cache_extent - (before - c.scroll_offset).max(0.0))
            .max(0.0),
    }
}

/// A padded piece's geometry, from its child's.
pub(crate) fn padded_geometry(
    c: &SliverConstraints,
    before: f32,
    after: f32,
    child: SliverGeometry,
) -> SliverGeometry {
    let before_paint = paint_offset(c, 0.0, before);
    let after_paint = paint_offset(
        c,
        before + child.scroll_extent,
        before + child.scroll_extent + after,
    );
    let paint = (before_paint + child.paint_extent.max(child.layout_extent + after_paint))
        .min(c.remaining_paint_extent.max(0.0));
    SliverGeometry {
        scroll_extent: before + child.scroll_extent + after,
        paint_extent: paint,
        paint_origin: child.paint_origin,
        layout_extent: (before_paint + child.layout_extent + after_paint).min(paint),
    }
}

/// A grid's shape across `cross`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GridMetrics {
    pub(crate) columns: usize,
    pub(crate) tile_cross: f32,
    pub(crate) tile_main: f32,
    pub(crate) row_stride: f32,
    pub(crate) column_stride: f32,
}

impl GridMetrics {
    /// How far `count` tiles scroll.
    pub(crate) fn extent(&self, count: usize, row_gap: f32) -> f32 {
        let rows = count.div_ceil(self.columns.max(1));
        if rows == 0 {
            0.0
        } else {
            rows as f32 * self.row_stride - row_gap
        }
    }
}

pub(crate) fn grid_metrics<Msg>(grid: &SliverGrid<Msg>, cross: f32) -> GridMetrics {
    let columns = match grid.columns {
        GridColumns::Count(n) => n.max(1),
        GridColumns::MaxExtent(max) => ((cross / (max + grid.column_gap)).ceil().max(1.0)) as usize,
    };
    let tile_cross =
        ((cross - grid.column_gap * columns.saturating_sub(1) as f32) / columns as f32).max(0.0);
    let tile_main = grid.tile_height.unwrap_or(match grid.aspect {
        Some(ratio) if ratio > 0.0 => tile_cross / ratio,
        _ => tile_cross,
    });
    GridMetrics {
        columns,
        tile_cross,
        tile_main,
        row_stride: tile_main + grid.row_gap,
        column_stride: tile_cross + grid.column_gap,
    }
}

/// The items of a run of `count` items `extent` long that the window `[lo, hi)` reaches.
pub(crate) fn fixed_window(count: usize, extent: f32, lo: f32, hi: f32) -> std::ops::Range<usize> {
    if extent <= 0.0 || count == 0 || hi <= lo {
        return 0..0;
    }
    let first = (lo.max(0.0) / extent).floor() as usize;
    let last = ((hi / extent).ceil().max(0.0) as usize).min(count);
    first.min(last)..last
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(so: f32, rem: f32) -> SliverConstraints {
        SliverConstraints {
            scroll_offset: so,
            remaining_paint_extent: rem,
            viewport_main_extent: 600.0,
            cross_axis_extent: 400.0,
            remaining_cache_extent: rem + 250.0,
            ..Default::default()
        }
    }

    #[test]
    fn a_box_paints_what_shows_of_it() {
        let g = box_geometry(&at(0.0, 600.0), 200.0);
        assert_eq!(
            (g.scroll_extent, g.paint_extent, g.layout_extent),
            (200.0, 200.0, 200.0)
        );
        let g = box_geometry(&at(150.0, 600.0), 200.0);
        assert_eq!(g.paint_extent, 50.0);
        let g = box_geometry(&at(0.0, 30.0), 200.0);
        assert_eq!(g.paint_extent, 30.0, "no more than the viewport has left");
        let g = box_geometry(&at(250.0, 600.0), 200.0);
        assert_eq!(g.paint_extent, 0.0, "scrolled past");
    }

    /// A header that scrolls: it shrinks to its smallest extent, then goes out of the top,
    /// its bottom on the bottom of what it paints.
    #[test]
    fn a_scrolling_header_shrinks_then_leaves() {
        let h = header_layout(&at(0.0, 600.0), 64.0, 200.0, HeaderMode::Scrolling, 0.0);
        assert_eq!(
            (h.child_extent, h.geometry.paint_extent, h.child_position),
            (200.0, 200.0, 0.0)
        );
        let h = header_layout(&at(100.0, 600.0), 64.0, 200.0, HeaderMode::Scrolling, 0.0);
        assert_eq!(
            (h.child_extent, h.geometry.paint_extent, h.shrink_offset),
            (100.0, 100.0, 100.0)
        );
        let h = header_layout(&at(170.0, 600.0), 64.0, 200.0, HeaderMode::Scrolling, 0.0);
        assert_eq!(h.child_extent, 64.0, "never below its smallest extent");
        assert_eq!(h.geometry.paint_extent, 30.0);
        assert_eq!(
            h.child_position, -34.0,
            "its bottom on the bottom of what it paints"
        );
        let h = header_layout(&at(300.0, 600.0), 64.0, 200.0, HeaderMode::Scrolling, 0.0);
        assert_eq!(h.geometry.paint_extent, 0.0);
    }

    /// A pinned header collapses to its smallest extent and stays, painting more than it
    /// takes: what follows goes under it.
    #[test]
    fn a_pinned_header_stays_at_its_smallest_extent() {
        let h = header_layout(&at(100.0, 600.0), 64.0, 200.0, HeaderMode::Pinned, 0.0);
        assert_eq!(
            (
                h.child_extent,
                h.geometry.paint_extent,
                h.geometry.layout_extent
            ),
            (100.0, 100.0, 100.0)
        );
        let h = header_layout(&at(500.0, 600.0), 64.0, 200.0, HeaderMode::Pinned, 0.0);
        assert_eq!(h.child_extent, 64.0);
        assert_eq!(h.geometry.paint_extent, 64.0, "still painted");
        assert_eq!(h.geometry.layout_extent, 0.0, "but takes no room");
        assert_eq!(h.geometry.scroll_extent, 200.0);
        // A second pinned header is laid out below what the first paints over.
        let mut c = at(0.0, 600.0);
        c.overlap = 64.0;
        let h = header_layout(&c, 40.0, 40.0, HeaderMode::Pinned, 0.0);
        assert_eq!(h.geometry.paint_origin, 64.0);
        assert!(h.overlaps_content);
    }

    /// A floating header comes back by as much as the content is scrolled back, wherever
    /// the content is.
    #[test]
    fn a_floating_header_comes_back_with_the_first_scroll_back() {
        // Scrolled far down: fully away.
        let e = floating_effective(None, 0.0, 1000.0, 120.0);
        assert_eq!(e, 1000.0);
        let e = floating_effective(Some(1000.0), e, 1100.0, 120.0);
        assert_eq!(e, 1100.0, "scrolling on keeps it away");
        // 30 back up: 30 of it shows.
        let e = floating_effective(Some(1100.0), e, 1070.0, 120.0);
        assert_eq!(e, 90.0);
        let h = header_layout(&at(1070.0, 600.0), 56.0, 120.0, HeaderMode::Floating, e);
        assert_eq!(h.geometry.paint_extent, 30.0);
        assert_eq!(
            h.geometry.layout_extent, 0.0,
            "it takes no room: it floats over"
        );
        assert!(h.overlaps_content);
        // Another 200 back: all of it, and no more.
        let e = floating_effective(Some(1070.0), e, 870.0, 120.0);
        assert_eq!(e, 0.0);
        let h = header_layout(&at(870.0, 600.0), 56.0, 120.0, HeaderMode::Floating, e);
        assert_eq!(h.geometry.paint_extent, 120.0);
        // Down again by 50: it goes by as much.
        let e = floating_effective(Some(870.0), e, 920.0, 120.0);
        assert_eq!(e, 50.0);
    }

    /// Floating and pinned: it comes back as a floating one does, and never goes below its
    /// smallest extent.
    #[test]
    fn a_floating_pinned_header_keeps_its_smallest_extent() {
        let h = header_layout(
            &at(1000.0, 600.0),
            56.0,
            200.0,
            HeaderMode::FloatingPinned,
            1000.0,
        );
        assert_eq!(h.geometry.paint_extent, 56.0);
        assert_eq!(h.child_extent, 56.0);
        let h = header_layout(
            &at(1000.0, 600.0),
            56.0,
            200.0,
            HeaderMode::FloatingPinned,
            100.0,
        );
        assert_eq!(h.geometry.paint_extent, 100.0);
        assert_eq!(h.child_extent, 100.0);
    }

    /// Snap: the view stops with the header half shown; it finishes the way the scroll
    /// was going, over the snap's duration, and a new scroll cancels it.
    #[test]
    fn a_snapping_header_finishes_the_way_the_scroll_went() {
        let mut s = FloatState::default();
        advance_float(&mut s, 1000.0, 120.0, true, false, 0.0);
        advance_float(&mut s, 960.0, 120.0, true, false, 0.1);
        assert_eq!(s.effective, 80.0, "40 of it shows");
        assert!(s.showing);
        // The view comes to rest: a snap towards showing starts.
        assert!(advance_float(&mut s, 960.0, 120.0, true, true, 0.2));
        assert!(s.snap.is_some());
        advance_float(&mut s, 960.0, 120.0, true, true, 0.3);
        assert!(s.effective < 80.0 && s.effective > 0.0, "{}", s.effective);
        let done = advance_float(&mut s, 960.0, 120.0, true, true, 0.2 + SNAP_DURATION);
        assert!(!done);
        assert_eq!(s.effective, 0.0, "all of it shows");

        // Going down and stopping half way hides it.
        let mut s = FloatState::default();
        advance_float(&mut s, 500.0, 120.0, true, false, 0.0);
        advance_float(&mut s, 440.0, 120.0, true, false, 0.1);
        advance_float(&mut s, 470.0, 120.0, true, false, 0.2);
        assert!(!s.showing);
        assert_eq!(s.effective, 90.0, "30 of it shows");
        advance_float(&mut s, 470.0, 120.0, true, true, 0.3);
        advance_float(&mut s, 470.0, 120.0, true, true, 0.3 + SNAP_DURATION);
        assert_eq!(s.effective, 120.0, "all of it hidden");

        // Without snap, it stays where the scroll left it.
        let mut s = FloatState::default();
        advance_float(&mut s, 1000.0, 120.0, false, false, 0.0);
        advance_float(&mut s, 960.0, 120.0, false, false, 0.1);
        assert!(!advance_float(&mut s, 960.0, 120.0, false, true, 0.2));
        assert_eq!(s.effective, 80.0);

        // A snap under way is dropped as soon as the view moves.
        let mut s = FloatState::default();
        advance_float(&mut s, 1000.0, 120.0, true, false, 0.0);
        advance_float(&mut s, 960.0, 120.0, true, false, 0.1);
        advance_float(&mut s, 960.0, 120.0, true, true, 0.2);
        advance_float(&mut s, 965.0, 120.0, true, false, 0.25);
        assert!(s.snap.is_none());
    }

    /// The rest of the viewport, or the child's length when it is longer and allowed to be.
    #[test]
    fn fill_remaining_takes_what_is_left() {
        let mut c = at(0.0, 400.0);
        c.preceding_scroll_extent = 200.0;
        assert_eq!(fill_extent(&c, Some(100.0), true), 400.0);
        assert_eq!(fill_extent(&c, Some(700.0), true), 400.0, "squeezed");
        assert_eq!(
            fill_extent(&c, Some(700.0), false),
            700.0,
            "scrolls instead"
        );
        c.preceding_scroll_extent = 900.0;
        assert_eq!(fill_extent(&c, None, true), 0.0, "nothing left");
    }

    /// Padding before a piece scrolls in ahead of it; the piece sees only what is past it.
    #[test]
    fn padding_shifts_the_piece_it_wraps() {
        let c = at(10.0, 600.0);
        let inner = padded_constraints(&c, 16.0, 32.0);
        assert_eq!(inner.scroll_offset, 0.0);
        assert_eq!(inner.remaining_paint_extent, 594.0);
        assert_eq!(inner.cross_axis_extent, 368.0);
        let child = box_geometry(&inner, 100.0);
        let g = padded_geometry(&c, 16.0, 16.0, child);
        assert_eq!(g.scroll_extent, 132.0);
        assert_eq!(g.paint_extent, 122.0);
    }

    /// Columns by count or by the widest a tile may be; tiles square unless told.
    #[test]
    fn grid_metrics_follow_the_width() {
        let grid = SliverGrid::<()>::extent(150.0, 10, |_| crate::Container::<()>::new());
        let m = grid_metrics(&grid, 500.0);
        assert_eq!((m.columns, m.tile_cross, m.tile_main), (4, 125.0, 125.0));
        let grid = SliverGrid::<()>::count(3, 10, |_| crate::Container::<()>::new())
            .gap(10.0)
            .aspect(2.0);
        let m = grid_metrics(&grid, 320.0);
        assert_eq!((m.columns, m.tile_cross, m.tile_main), (3, 100.0, 50.0));
        assert_eq!(m.extent(10, 10.0), 4.0 * 60.0 - 10.0);
    }

    #[test]
    fn the_window_is_the_items_it_reaches() {
        assert_eq!(fixed_window(100, 40.0, 0.0, 200.0), 0..5);
        assert_eq!(fixed_window(100, 40.0, 30.0, 230.0), 0..6);
        assert_eq!(fixed_window(3, 40.0, 0.0, 200.0), 0..3);
        assert_eq!(fixed_window(100, 40.0, 5000.0, 6000.0), 100..100);
    }
}

/// The view as a whole, through `build_ui`: what is built, where it is painted, what takes
/// a touch.
#[cfg(test)]
mod view_tests {
    use super::*;
    use crate::interaction::WidgetId;
    use crate::{build_ui, Container, Runtime, Ui};
    use frus_core::{Point, Primitive, Size};
    use std::cell::RefCell;

    const RED: Color = Color::rgb(1.0, 0.0, 0.0);
    const BLUE: Color = Color::rgb(0.0, 0.0, 1.0);
    const GREEN: Color = Color::rgb(0.0, 1.0, 0.0);

    fn frame(view: &CustomScrollView<u32>, runtime: &Runtime, theme: &Theme) -> Ui<u32> {
        build_ui(view, Size::new(400.0, 400.0), runtime, theme)
    }

    /// A view the size of the window the tests build it in.
    fn v() -> CustomScrollView<u32> {
        CustomScrollView::new().width(400.0).height(400.0)
    }

    fn painted(ui: &Ui<u32>, color: Color) -> Vec<Rect> {
        fn walk(list: &[Primitive], color: Color, out: &mut Vec<Rect>) {
            for p in list {
                match p {
                    Primitive::Rect { rect, color: c, .. } if *c == color => out.push(*rect),
                    Primitive::Layer { primitives, .. } => walk(primitives, color, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(ui.scene().primitives(), color, &mut out);
        out
    }

    fn at(offset: f32) -> Runtime {
        let mut runtime = Runtime::default();
        runtime.scroll.insert(WidgetId::ROOT, (0.0, offset));
        runtime
    }

    /// **Lazy**: of ten thousand rows, only those in the viewport and its cache extent are
    /// built — at the top, and in the middle of the list.
    #[test]
    fn a_list_of_ten_thousand_builds_only_what_it_shows() {
        let built = Rc::new(RefCell::new(Vec::new()));
        let log = built.clone();
        let view = v().sliver(SliverList::new(10_000, 40.0, move |i| {
            log.borrow_mut().push(i);
            Container::<u32>::new().color(BLUE)
        }));
        let _ = frame(&view, &Runtime::default(), &Theme::default());
        // 400 of viewport and 250 of cache after it: rows 0 to 16.
        assert_eq!(*built.borrow(), (0..17).collect::<Vec<_>>());

        built.borrow_mut().clear();
        let ui = frame(&view, &at(200_000.0), &Theme::default());
        let rows = built.borrow().clone();
        // 250 before 200 000 and 650 after it: rows 4 993 to 5 016.
        assert_eq!(rows.first(), Some(&4993));
        assert_eq!(rows.last(), Some(&5016));
        assert_eq!(rows.len(), 24, "never the ten thousand");
        assert_eq!(ui.scroll_regions()[0].max_y, 400_000.0 - 400.0);
        // Row 5 000 is drawn at the top of the viewport.
        let top = painted(&ui, BLUE)
            .into_iter()
            .filter(|r| r.y >= 0.0)
            .map(|r| r.y)
            .fold(f32::INFINITY, f32::min);
        assert_eq!(top, 0.0);
    }

    /// A list of rows of any extent measures the ones it builds, and still builds only those.
    #[test]
    fn a_builder_list_measures_only_what_it_builds() {
        let built = Rc::new(RefCell::new(0usize));
        let count = built.clone();
        let view = v().sliver(SliverList::builder(10_000, move |i| {
            *count.borrow_mut() += 1;
            Container::<u32>::new()
                .height(if i % 2 == 0 { 20.0 } else { 60.0 })
                .color(BLUE)
        }));
        let runtime = Runtime::default();
        let ui = frame(&view, &runtime, &Theme::default());
        assert!(*built.borrow() <= 40, "{}", built.borrow());
        assert_eq!(
            ui.scroll_regions()[0].max_y,
            10_000.0 * 56.0 - 400.0,
            "nothing measured yet: the estimate"
        );
        // The second frame knows the mean of the rows it measured (20 and 60 in turn, so
        // about 40) and counts the rest at it.
        let ui = frame(&view, &runtime, &Theme::default());
        let max = ui.scroll_regions()[0].max_y;
        assert!((380_000.0..=400_000.0).contains(&max), "{max}");
    }

    /// A pinned header collapses to its smallest extent, stays, and is drawn over the rows
    /// going under it, which it takes the touches from.
    #[test]
    fn a_pinned_header_stays_over_the_rows() {
        let view = v()
            .sliver(
                SliverPersistentHeader::new(56.0, 200.0, |_| {
                    Container::<u32>::new().color(RED).on_click(1u32)
                })
                .pinned(true),
            )
            .sliver(SliverList::new(100, 50.0, |_| {
                Container::<u32>::new().color(BLUE).on_click(2u32)
            }));
        let ui = frame(&view, &Runtime::default(), &Theme::default());
        assert_eq!(painted(&ui, RED), vec![Rect::new(0.0, 0.0, 400.0, 200.0)]);
        assert_eq!(painted(&ui, BLUE)[0].y, 200.0, "the rows start under it");
        assert_eq!(ui.scroll_regions()[0].max_y, 200.0 + 5000.0 - 400.0);

        let ui = frame(&view, &at(100.0), &Theme::default());
        assert_eq!(painted(&ui, RED), vec![Rect::new(0.0, 0.0, 400.0, 100.0)]);
        assert_eq!(painted(&ui, BLUE)[0].y, 100.0);

        let ui = frame(&view, &at(1000.0), &Theme::default());
        assert_eq!(painted(&ui, RED), vec![Rect::new(0.0, 0.0, 400.0, 56.0)]);
        let last = ui.scene().primitives().iter().rev().find_map(|p| match p {
            Primitive::Rect { color, .. } if *color == RED || *color == BLUE => Some(*color),
            _ => None,
        });
        assert_eq!(last, Some(RED), "drawn over the rows");
        let hit = ui.hit(Point::new(200.0, 20.0)).expect("the header");
        assert_eq!(ui.msg_for(hit), Some(1));
        let hit = ui.hit(Point::new(200.0, 80.0)).expect("a row");
        assert_eq!(ui.msg_for(hit), Some(2));
    }

    /// A header that is not pinned scrolls away: shrunk to its smallest extent, then out
    /// of the top with its bottom on what it still paints.
    #[test]
    fn a_scrolling_header_leaves() {
        let view = v()
            .sliver(SliverPersistentHeader::new(56.0, 200.0, |_| {
                Container::<u32>::new().color(RED)
            }))
            .sliver(SliverList::new(100, 50.0, |_| {
                Container::<u32>::new().color(BLUE)
            }));
        let ui = frame(&view, &at(170.0), &Theme::default());
        assert_eq!(painted(&ui, RED), vec![Rect::new(0.0, -26.0, 400.0, 56.0)]);
        assert_eq!(painted(&ui, BLUE)[0].y, 30.0);
        let ui = frame(&view, &at(400.0), &Theme::default());
        assert!(painted(&ui, RED).is_empty(), "gone, and not built");
    }

    /// A floating header comes back with the first scroll back, over the rows.
    #[test]
    fn a_floating_header_comes_back_over_the_rows() {
        let view = v()
            .sliver(
                SliverPersistentHeader::new(56.0, 56.0, |_| Container::<u32>::new().color(RED))
                    .floating(true),
            )
            .sliver(SliverList::new(100, 50.0, |_| {
                Container::<u32>::new().color(BLUE)
            }));
        let mut runtime = Runtime::default();
        // A finger holds the view, which is moving.
        runtime.hold_scroll(WidgetId::ROOT);
        runtime.scroll.insert(WidgetId::ROOT, (0.0, 1000.0));
        let ui = frame(&view, &runtime, &Theme::default());
        assert!(painted(&ui, RED).is_empty(), "scrolled away");
        runtime.scroll.insert(WidgetId::ROOT, (0.0, 970.0));
        let ui = frame(&view, &runtime, &Theme::default());
        assert_eq!(painted(&ui, RED), vec![Rect::new(0.0, -26.0, 400.0, 56.0)]);
        runtime.scroll.insert(WidgetId::ROOT, (0.0, 900.0));
        let ui = frame(&view, &runtime, &Theme::default());
        assert_eq!(painted(&ui, RED), vec![Rect::new(0.0, 0.0, 400.0, 56.0)]);
    }

    /// Padding scrolls with the piece it wraps; a grid places its columns along the
    /// reading direction.
    #[test]
    fn a_padded_grid_follows_the_reading_direction() {
        let view = v().sliver(
            SliverPadding::new(
                SliverGrid::count(3, 7, |i| {
                    Container::<u32>::new().color(if i == 0 { RED } else { BLUE })
                })
                .gap(10.0),
            )
            .padding_each(20.0, 10.0, 0.0, 10.0),
        );
        let ui = frame(&view, &Runtime::default(), &Theme::default());
        // 380 across: three columns of 120 with two gaps of 10.
        assert_eq!(painted(&ui, RED), vec![Rect::new(10.0, 20.0, 120.0, 120.0)]);
        assert_eq!(painted(&ui, BLUE).len(), 6);
        // 20 of padding and three rows of 120 with two gaps of 10: exactly the viewport.
        assert_eq!(ui.scroll_regions()[0].max_y, 0.0);
        let rtl = Theme::default().rtl();
        let ui = frame(&view, &Runtime::default(), &rtl);
        assert_eq!(
            painted(&ui, RED),
            vec![Rect::new(270.0, 20.0, 120.0, 120.0)]
        );
    }

    /// The rest of the viewport goes to the last piece; with no scroll body, a longer
    /// child scrolls instead of being squeezed.
    #[test]
    fn fill_remaining_takes_the_rest_of_the_viewport() {
        let view = v()
            .sliver(SliverToBoxAdapter::new(
                Container::<u32>::new().height(150.0).color(BLUE),
            ))
            .sliver(SliverFillRemaining::new(
                Container::<u32>::new().color(GREEN),
            ));
        let ui = frame(&view, &Runtime::default(), &Theme::default());
        assert_eq!(
            painted(&ui, GREEN),
            vec![Rect::new(0.0, 150.0, 400.0, 250.0)]
        );
        assert_eq!(ui.scroll_regions()[0].max_y, 0.0);

        let view = v()
            .sliver(SliverToBoxAdapter::new(
                Container::<u32>::new().height(150.0).color(BLUE),
            ))
            .sliver(
                SliverFillRemaining::new(Container::<u32>::new().height(600.0).color(GREEN))
                    .has_scroll_body(false),
            );
        let ui = frame(&view, &Runtime::default(), &Theme::default());
        assert_eq!(ui.scroll_regions()[0].max_y, 350.0);
    }

    /// Across the screen: the same pieces, the scroll along x.
    #[test]
    fn a_horizontal_view_scrolls_across() {
        let view = v()
            .axis(Axis::Horizontal)
            .sliver(SliverToBoxAdapter::new(
                Container::<u32>::new().width(100.0).color(RED),
            ))
            .sliver(SliverList::new(50, 80.0, |_| {
                Container::<u32>::new().color(BLUE)
            }));
        let mut runtime = Runtime::default();
        runtime.scroll.insert(WidgetId::ROOT, (60.0, 0.0));
        let ui = frame(&view, &runtime, &Theme::default());
        assert_eq!(painted(&ui, RED), vec![Rect::new(-60.0, 0.0, 100.0, 400.0)]);
        assert_eq!(painted(&ui, BLUE)[0], Rect::new(40.0, 0.0, 80.0, 400.0));
        let area = ui.scroll_regions()[0];
        assert_eq!((area.max_x, area.max_y), (100.0 + 4000.0 - 400.0, 0.0));
    }

    /// The app bar's surface follows the theme, changes once content goes under it, and
    /// gives way to an override.
    #[test]
    fn the_app_bar_surface_is_themed_and_overridable() {
        let theme = Theme::default();
        let (rest, lift) = bar_surface(&theme, None, None, None, None, false);
        assert_eq!((rest, lift), (theme.scheme.surface, 0.0));
        let (under, lift) = bar_surface(&theme, None, None, None, None, true);
        assert_eq!((under, lift), (theme.scheme.surface_container, 3.0));
        let mut themed = theme.clone();
        themed.widgets.app_bar.scrolled_under_background = Some(GREEN);
        themed.widgets.app_bar.scrolled_under_elevation = Some(1.0);
        assert_eq!(
            bar_surface(&themed, None, None, None, None, true),
            (GREEN, 1.0)
        );
        assert_eq!(
            bar_surface(&themed, None, Some(RED), None, Some(6.0), true),
            (RED, 6.0)
        );
    }

    /// A pinned app bar: fully expanded at rest, its toolbar's height once collapsed, and
    /// painted in the scrolled-under colour then.
    #[test]
    fn a_pinned_app_bar_collapses_to_its_toolbar() {
        let view = v()
            .sliver(
                SliverAppBar::new()
                    .pinned(true)
                    .primary(false)
                    .expanded_height(200.0)
                    .background(RED)
                    .scrolled_under_background(GREEN)
                    .flexible_space(FlexibleSpaceBar::new().title(crate::Text::new("Title"))),
            )
            .sliver(SliverList::new(100, 50.0, |_| {
                Container::<u32>::new().color(BLUE)
            }));
        let ui = frame(&view, &Runtime::default(), &Theme::default());
        assert_eq!(painted(&ui, RED), vec![Rect::new(0.0, 0.0, 400.0, 200.0)]);
        let ui = frame(&view, &at(500.0), &Theme::default());
        let toolbar = crate::appbar::APP_BAR_HEIGHT;
        assert_eq!(
            painted(&ui, GREEN),
            vec![Rect::new(0.0, 0.0, 400.0, toolbar)]
        );
        assert!(painted(&ui, RED).is_empty());
    }
}
