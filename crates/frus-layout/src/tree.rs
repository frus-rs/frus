// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Building the layout tree and computing absolute rectangles.

use std::collections::HashMap;

use frus_core::{Rect, Size};
use taffy::AvailableSpace;

use crate::nodes::{Memo, Tree};

use crate::style::Style;

/// An opaque identifier for a layout node.
pub type NodeId = taffy::NodeId;

/// A **constrained measurement** for a leaf whose size depends on the space
/// offered — wrapping text, custom-painted content. It receives the maximum
/// width and height (`None` = unconstrained) and returns the content's size. Taffy
/// calls it during layout, including for **intrinsic** sizes (min-content gives a
/// width of `Some(0.0)`, max-content gives `None`).
/// The lifetime is what lets a measurement **borrow**: a text's closure owns everything
/// it needs and is `'static`, but a [`crate::Layout`] built for a widget tree can hold a
/// closure that borrows the tree, which is how a box whose content is built from the
/// space offered gets measured at all.
pub type MeasureFn<'a> = Box<dyn Fn(Option<f32>, Option<f32>) -> Size + 'a>;

/// A node's style, kept to be put back ([`Layout::style_snapshot`]).
pub struct StyleSnapshot(taffy::Style);

/// The edge a box's content ran past.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// Past the left edge.
    Left,
    /// Past the right edge.
    Right,
    /// Past the top edge.
    Top,
    /// Past the bottom edge.
    Bottom,
}

/// A box whose children do not fit inside it, and by how much.
///
/// The reference reports this to the console and paints a striped band across the
/// offending edge. Here a child simply draws outside its parent and nothing says so,
/// which is how a task row's delete button came to be laid out past the window — drawn
/// nowhere, hittable nowhere, and undiagnosed across three milestones (327, 333, 334).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overflowing {
    /// The **parent's** absolute box: the one that turned out to be too small.
    pub rect: Rect,
    /// The edge the content ran past.
    pub side: Side,
    /// By how many logical pixels.
    pub amount: f32,
}

/// A layout tree. `T` is user data attached to the nodes — a colour, say — handed
/// back with each computed rectangle.
pub struct Layout<'a, T> {
    tree: Tree<T>,
    /// Constrained measurements, one per "measured" leaf.
    measures: HashMap<NodeId, MeasureFn<'a>>,
    /// What the measurements are taken under, said by the caller (see [`Layout::reuse`]).
    context: u64,
}

/// **The rule a self-measuring widget has to keep**, checked where every measurement
/// passes through — in debug builds only, the cost in release being a rule nobody can
/// see broken.
///
/// **A measurement is a whole number of pixels.** The boxes this engine hands out are
/// rounded, so a widget that measures itself at `146.4` and says so is given `146`, and
/// `146 < 146.4` — at paint time, where it is measured again, it no longer fits and
/// wraps, clips or overflows. That is milestone 289, and it presented as a line of text
/// that vanished rather than as arithmetic. The cure is [`frus_core::fits`]; this is what
/// says so when a new measurer rounds to nearest instead, which is what anyone writing
/// one does by reflex.
///
/// **Two things it deliberately does not check.**
///
/// A measurement *larger than the space it was offered* is not a violation here. This
/// framework lets content overflow and reports it ([`crate::Overflowing`], milestones
/// 327–334) rather than crushing it, so a measurer answering with its natural size is
/// doing its job; a box too small for it is a fact about the layout, not about the
/// measurement.
///
/// And a measurement that came back **exactly** at its bound is exempt from the rule
/// above: it was clamped there, the bound is the caller's number rather than the
/// measurer's, and rounding it up would make a box a fraction wider than it was allowed
/// — which is a different bug. Clamp after rounding up, never before.
#[inline]
fn debug_assert_measurement(measured: f32, bound: Option<f32>, axis: &str) {
    #[cfg(debug_assertions)]
    {
        const EPSILON: f32 = 1e-3;
        if bound.is_some_and(|bound| (measured - bound).abs() <= EPSILON) {
            return;
        }
        debug_assert!(
            (measured - measured.ceil()).abs() <= EPSILON,
            "a measured {axis} of {measured} is not a whole number of pixels, so the box \
             it is rounded to is one it no longer fits in — go through `frus_core::fits` \
             (milestone 289, issue #54)"
        );
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (measured, bound, axis);
    }
}

impl<'a, T> Layout<'a, T> {
    /// Creates an empty tree.
    pub fn new() -> Self {
        Self {
            tree: Tree::new(),
            measures: HashMap::new(),
            context: 0,
        }
    }

    /// Adds a leaf — a node with no children — carrying associated data.
    pub fn leaf(&mut self, style: Style, data: T) -> NodeId {
        self.tree.add(style.to_taffy(), Some(data))
    }

    /// Adds a **measured** leaf: its size comes from `measure`, given the space
    /// offered, rather than from a dimension fixed in the style.
    ///
    /// Nothing says what its answer depends on, so it is measured afresh in every layout,
    /// and so is every box above it: [`Layout::measured_leaf_keyed`] says it.
    pub fn measured_leaf(&mut self, style: Style, data: T, measure: MeasureFn<'a>) -> NodeId {
        self.measured_leaf_keyed(style, data, None, measure)
    }

    /// A measured leaf whose answer depends on its style, the space offered and `key` alone:
    /// two leaves with the same key and style measure alike. A leaf of a layout made with
    /// [`Layout::reuse`] that is the same as one of the last layout keeps that one's answers
    /// (milestone 853). `None` is [`Layout::measured_leaf`].
    pub fn measured_leaf_keyed(
        &mut self,
        style: Style,
        data: T,
        key: Option<u64>,
        measure: MeasureFn<'a>,
    ) -> NodeId {
        let node = self.tree.add_measured(style.to_taffy(), Some(data), key);
        self.measures.insert(node, measure);
        node
    }

    /// Says where a node stands in the caller's own tree: a stable identity, the same from
    /// one frame to the next, that [`Layout::reuse`] looks the last layout's node up by.
    ///
    /// It only says where to look. A node is the same as the last layout's only when its
    /// style, its content and its children are the same as that node's, all the way down.
    pub fn set_identity(&mut self, node: NodeId, identity: u64) {
        self.tree.set_identity(node, identity);
    }

    /// Hands this layout what the last layout of the same root left ([`Layout::into_memo`]),
    /// before it is computed (milestone 853).
    ///
    /// A node that is the same subtree as one of the last layout — the same style, the same
    /// content, the same children, all the way down — starts with the answers that one gave:
    /// asked the same question, it answers without being laid out again, and if it is placed
    /// the way it was, everything below it is placed the way it was too. A change deep in a
    /// tree lays out the path from it to the root, and each box on that path asks its other
    /// children what they already know. The rectangles are the ones a layout from scratch
    /// gives.
    ///
    /// `context` is what the measurements depend on that no node says — the reader's font
    /// size, say: a memo made under another context is not used.
    pub fn reuse(&mut self, memo: Memo, context: u64) {
        self.context = context;
        self.tree.reuse(memo);
    }

    /// What this layout leaves for the next layout of the same root, after it is computed.
    pub fn into_memo(self, root: NodeId) -> Memo {
        self.tree.into_memo(root, self.context)
    }

    /// How many nodes the computations so far laid out, rather than answering from a cache —
    /// what [`Layout::reuse`] saves.
    pub fn laid_out(&self) -> u32 {
        self.tree.computed
    }

    /// A node's style as it is now, to put back with [`Layout::restore_style`]: a tree laid
    /// out more than once under different constraints starts each time from the style it was
    /// built with, since each `compute*` may change its root's.
    pub fn style_snapshot(&self, node: NodeId) -> StyleSnapshot {
        StyleSnapshot(self.tree.style(node).clone())
    }

    /// Puts back a style taken with [`Layout::style_snapshot`]. What the node's children
    /// measured is kept; what the node and the nodes above it computed is forgotten only if
    /// the style differs.
    pub fn restore_style(&mut self, node: NodeId, snapshot: &StyleSnapshot) {
        if *self.tree.style(node) != snapshot.0 {
            self.tree.set_style(node, snapshot.0.clone());
        }
    }

    /// How many nodes started with the last layout's answers.
    pub fn reused(&self) -> u32 {
        self.tree.reused
    }

    /// Adds `dy` to a node's **top margin**, after it has been built.
    ///
    /// Baseline alignment needs it: a row can only know how far to push a child down
    /// once it has measured every child, and by then the children are nodes rather
    /// than styles. Nothing else should reach for this — a style is otherwise settled
    /// when the node is made.
    pub fn add_margin_top(&mut self, node: NodeId, dy: f32) {
        if dy <= 0.0 {
            return;
        }
        let mut style = self.tree.style(node).clone();
        let base = match style.margin.top {
            taffy::LengthPercentageAuto::Length(v) => v,
            // A percentage or `auto` margin is resolved against the parent, which is
            // not a number we have here; the shift is added as if it were zero, which
            // is the only reading that cannot make the row worse than it was.
            _ => 0.0,
        };
        style.margin.top = taffy::LengthPercentageAuto::Length(base + dy);
        self.tree.set_style(node, style);
    }

    /// Forgets the size a node was given on one axis, so that it is sized by the layout —
    /// what a default gives way to when the room on offer replaces it.
    pub fn clear_size(&mut self, node: NodeId, horizontal: bool) {
        let mut style = self.tree.style(node).clone();
        if horizontal {
            style.size.width = taffy::Dimension::Auto;
        } else {
            style.size.height = taffy::Dimension::Auto;
        }
        self.tree.set_style(node, style);
    }

    /// Makes a node **fill the space its parent leaves it**, after it has been built:
    /// growing along the parent's main axis, stretching across it, or both.
    ///
    /// This is what `MainAxisSize::Max` needs and cannot express on its own. A row that
    /// fills its line is asking about the *parent's* axis, not its own — grow if the
    /// parent runs the same way, stretch if it runs across — and a widget does not know
    /// what it was put inside. The parent does, so it says so here.
    pub fn fill_parent(&mut self, node: NodeId, horizontal: bool, grow: bool) {
        let mut style = self.tree.style(node).clone();
        let size = if horizontal {
            style.size.width
        } else {
            style.size.height
        };
        // A box that was given a size has already answered the question, and growing it
        // past that number would be answering a different one.
        if !matches!(size, taffy::Dimension::Auto) {
            return;
        }
        if grow {
            // From the content rather than from zero: filling is about taking the room
            // that is going spare, not about splitting the line into equal shares.
            if style.flex_grow != 0.0 {
                return;
            }
            style.flex_grow = 1.0;
        } else {
            if style.align_self.is_some() {
                return;
            }
            style.align_self = Some(taffy::AlignItems::Stretch);
        }
        self.tree.set_style(node, style);
    }

    /// Lets a node **give way** along its parent's main axis, after it has been built.
    ///
    /// The default is not to: the reference hands an inflexible child of a row or a
    /// column an *unbounded* main axis, so it is never squeezed and a line that does not
    /// fit overflows and says so. Flexbox's default is the opposite — every item absorbs
    /// its share of a deficit — and a framework that quietly crushes a button rather than
    /// reporting a layout that does not fit is a framework whose bugs are invisible.
    ///
    /// The exception is a box with **one** child, which is not dividing a line: it is
    /// handing its own constraints down, and the reference bounds a lone child by them.
    /// Only a node that has not asked for something else is touched. Growing is not
    /// asking for something else: a lone child that fills its parent is still bounded by
    /// it, and one that both grows and refuses to give way is a box that can only get
    /// bigger than the thing holding it.
    pub fn allow_shrink(&mut self, node: NodeId) {
        let mut style = self.tree.style(node).clone();
        // **The floor, not just the willingness.** A flex item's automatic minimum is its
        // **min-content** size, so `flex_shrink` on its own buys nothing: the item agrees
        // to shrink and then refuses to go below the widest thing inside it. A lone child
        // holding one over-wide row came out at that row's width, the box it was given
        // came out at the same, and every sibling stretched to match — a whole screen
        // laid out to the width of its widest line and clipped by the window. Found in a
        // real application whose product grid, search field and banner were all cut off
        // at the same edge.
        //
        // Zero is what makes the box the child's bound rather than a suggestion, which is
        // the reference's rule: a lone child is handed its parent's constraints, and it
        // cannot come back bigger than them.
        let mut changed = false;
        if style.flex_shrink == 0.0 {
            style.flex_shrink = 1.0;
            changed = true;
        }
        // **The width, and only the width.** A box is bounded by the width it was given;
        // a height is what scrolling is for, and squeezing one squashes text nobody
        // asked to squash. Tried both ways round: bounding the height too shortened a
        // pane in a golden until its background no longer covered its own content.
        //
        // **Only a minimum nobody asked for.** `Auto` is the automatic one — the item's
        // own min-content — and that is the one to drop. A box that named a floor, or a
        // tight one, keeps it: the point is to stop a child inventing a width, not to
        // overrule one it was handed.
        if matches!(style.min_size.width, taffy::Dimension::Auto) {
            style.min_size.width = taffy::style_helpers::length(0.0_f32);
            changed = true;
        }
        if changed {
            self.tree.set_style(node, style);
        }
    }

    /// Gives a node the **shape of a tile**: a height derived from the width it ends up
    /// with by `ratio` (`width / height`, taffy's convention and the reference's).
    ///
    /// A grid's tiles are all the same shape in the reference, and that shape is the
    /// *container's* say rather than each tile's — a tile cannot know how wide its
    /// column came out — which is why it is imposed here instead of asked of the child.
    ///
    /// Only the ratio is set, and that is enough **in a grid**: a track's width is
    /// definite, so the height follows from it. It would not be enough across a flex,
    /// where a box is merely stretched by the cross-axis alignment and there is no
    /// number to derive the other axis from (which is why [`crate::Style::aspect_ratio`]
    /// is paired with a percentage width there).
    ///
    /// A child that has already chosen a size of its own keeps it.
    pub fn set_tile_shape(&mut self, node: NodeId, ratio: f32) {
        let mut style = self.tree.style(node).clone();
        if !matches!(style.size.width, taffy::Dimension::Auto)
            || !matches!(style.size.height, taffy::Dimension::Auto)
        {
            return;
        }
        style.aspect_ratio = Some(ratio);
        self.tree.set_style(node, style);
    }

    /// Sets a node's **minimum width**, after it has been built.
    ///
    /// It is what stops a leaf being squeezed along a row, and only a row can say so: the
    /// same number across a column would be a floor on the *cross* axis, where the box is
    /// handed a width rather than asked for one.
    pub fn set_min_width(&mut self, node: NodeId, width: f32) {
        let mut style = self.tree.style(node).clone();
        if !matches!(style.min_size.width, taffy::Dimension::Auto) {
            return;
        }
        style.min_size.width = taffy::Dimension::Length(width);
        self.tree.set_style(node, style);
    }

    /// Lifts a node's **least size on one axis**, after it has been built: the floor of a
    /// child whose size along its parent's line is exactly its share of what is left.
    ///
    /// Only the parent can say so, since only the parent knows which axis is its line;
    /// across the line the child's own floor stands.
    pub fn clear_min(&mut self, node: NodeId, horizontal: bool) {
        let mut style = self.tree.style(node).clone();
        let floor = if horizontal {
            &mut style.min_size.width
        } else {
            &mut style.min_size.height
        };
        *floor = taffy::style_helpers::length(0.0_f32);
        self.tree.set_style(node, style);
    }

    /// Makes the **root** of a layout fill one axis of whatever room it is computed in.
    ///
    /// A root has no parent to grow into, so the flex machinery `fill_parent` uses has
    /// nothing to work with. A percentage does: it resolves against the available space
    /// when that space is definite, and falls back to hugging the content when it is not
    /// — which is the right answer in both cases. A row at the top of a frame fills the
    /// window; the same row measured for its natural size still shrink-wraps.
    pub fn fill_root(&mut self, node: NodeId, horizontal: bool) {
        let mut style = self.tree.style(node).clone();
        let axis = if horizontal {
            &mut style.size.width
        } else {
            &mut style.size.height
        };
        if !matches!(axis, taffy::Dimension::Auto) {
            return;
        }
        *axis = taffy::Dimension::Percent(1.0);
        self.tree.set_style(node, style);
    }

    /// Adds a container — a node with children — with no associated data.
    pub fn container(&mut self, style: Style, children: &[NodeId]) -> NodeId {
        // An **overlapping** container puts every child in its one cell ([`Style::overlap`]).
        // A placement belongs to the child's taffy style, and a child's style is its own,
        // so it is added here: the only point where a parent and its children meet.
        if style.overlap {
            use taffy::style_helpers::{line, span};
            for &child in children {
                let mut placed = self.tree.style(child).clone();
                placed.grid_row = taffy::Line {
                    start: line(1),
                    end: span(1),
                };
                placed.grid_column = taffy::Line {
                    start: line(1),
                    end: span(1),
                };
                self.tree.set_style(child, placed);
            }
        }
        self.tree.add_container(style.to_taffy(), children)
    }

    /// Computes the layout from `root`, within the given available space.
    pub fn compute(&mut self, root: NodeId, available: Size) {
        self.compute_in(
            root,
            taffy::Size {
                width: AvailableSpace::Definite(available.width),
                height: AvailableSpace::Definite(available.height),
            },
        );
    }

    /// Computes the layout of content that is **handed a box**: both axes are
    /// definite, and an unset (`Auto`) root dimension takes the box's size instead
    /// of hugging the content.
    ///
    /// The distinction from [`Layout::compute`] is who decides the size. A widget
    /// laid out in the ordinary way is asked how big it wants to be; a page of a
    /// paged view is *told*, and a page that hugged its content would leave the rest
    /// of the panel empty and unclickable.
    pub fn compute_filled(&mut self, root: NodeId, width: f32, height: f32) {
        {
            let current = self.tree.style(root);
            let mut style = current.clone();
            let mut changed = false;
            if matches!(style.size.width, taffy::Dimension::Auto) {
                style.size.width = taffy::Dimension::Length(width);
                changed = true;
            }
            if matches!(style.size.height, taffy::Dimension::Auto) {
                style.size.height = taffy::Dimension::Length(height);
                changed = true;
            }
            if changed {
                self.tree.set_style(root, style);
            }
        }
        self.compute(root, Size::new(width, height));
    }

    /// Computes the layout of content **forced** into a box on the axes that are not
    /// free: the node's own width or height is overruled rather than merely bounded.
    ///
    /// The difference from [`Layout::compute_filled`] is who wins when the content has
    /// already chosen. Filling defers to it — an explicit size is a size the caller
    /// asked for — while this overrules it, which is what a stack layer pinned to two
    /// opposite edges means: the edges say how wide it is, and a width of its own is not
    /// a second opinion but a contradiction.
    pub fn compute_tight(
        &mut self,
        root: NodeId,
        width: f32,
        height: f32,
        free_x: bool,
        free_y: bool,
    ) {
        {
            let current = self.tree.style(root);
            let mut style = current.clone();
            if !free_x {
                style.size.width = taffy::Dimension::Length(width);
            }
            if !free_y {
                style.size.height = taffy::Dimension::Length(height);
            }
            self.tree.set_style(root, style);
        }
        self.compute_scroll(root, width, height, free_x, free_y);
    }

    /// Computes the layout of scrollable content: each axis is either constrained
    /// to the viewport or **free**, in which case the content takes its natural
    /// size, according to `free_x` and `free_y`.
    pub fn compute_scroll(
        &mut self,
        root: NodeId,
        width: f32,
        height: f32,
        free_x: bool,
        free_y: bool,
    ) {
        // **Fill the constrained axis**: the content of a **single-axis** scrollable takes
        // the viewport's size on the **cross** (constrained) axis, instead of hugging its
        // content. Without this a `flex(1)` or `Percent` child on that axis would collapse
        // for want of a definite basis, and the app would have to insert a "filler"
        // container. We only touch the axis when it is **constrained while the other is
        // free** — true single-axis scrolling, neither definite layout (both constrained)
        // nor 2D scrolling (both free) — and **only** when the root dimension is `Auto`,
        // since an explicit size choice is respected.
        let fill_w = !free_x && free_y;
        let fill_h = !free_y && free_x;
        self.compute_each(root, width, height, [free_x, free_y], [fill_w, fill_h]);
    }

    /// Computes the layout with each axis said separately: **free** (the content takes its
    /// natural size), or bounded by `width` / `height` and then either **filled** (an unset
    /// root dimension takes the bound) or not (the bound is only the most it may take).
    ///
    /// The other `compute*` are fixed choices of these: [`Layout::compute_filled`] fills both
    /// bounds, [`Layout::compute_scroll`] fills a lone bound, and [`Layout::compute`] fills
    /// none. A box that hands its child the room on one axis and only allows it on the other
    /// needs the choice per axis.
    ///
    /// A bound that is not filled is still **the most** the root may take: a root with no
    /// size of its own that came out past it — a container whose paragraph was measured on
    /// one line — is laid out again at the bound, where the paragraph wraps. A root with a
    /// size of its own keeps it, and spills.
    pub fn compute_axes(
        &mut self,
        root: NodeId,
        width: f32,
        height: f32,
        free: [bool; 2],
        fill: [bool; 2],
    ) {
        self.compute_each(root, width, height, free, fill);
        let size = self.size_of(root);
        let past =
            |free: bool, fill: bool, came: f32, bound: f32| !free && !fill && came > bound + 0.5;
        let over = [
            past(free[0], fill[0], size.width, width),
            past(free[1], fill[1], size.height, height),
        ];
        if over[0] || over[1] {
            let fill = [fill[0] || over[0], fill[1] || over[1]];
            self.compute_each(root, width, height, free, fill);
        }
    }

    /// [`Layout::compute_axes`] without the second look at a bound that was not filled.
    fn compute_each(
        &mut self,
        root: NodeId,
        width: f32,
        height: f32,
        free: [bool; 2],
        fill: [bool; 2],
    ) {
        let [free_x, free_y] = free;
        let fill_w = fill[0] && !free_x;
        let fill_h = fill[1] && !free_y;
        if fill_w || fill_h {
            {
                let current = self.tree.style(root);
                let mut style = current.clone();
                let mut changed = false;
                if fill_w && matches!(style.size.width, taffy::Dimension::Auto) {
                    style.size.width = taffy::Dimension::Length(width);
                    changed = true;
                }
                if fill_h && matches!(style.size.height, taffy::Dimension::Auto) {
                    style.size.height = taffy::Dimension::Length(height);
                    changed = true;
                }
                if changed {
                    self.tree.set_style(root, style);
                }
            }
        }

        let axis = |free: bool, size: f32| {
            if free {
                AvailableSpace::MaxContent
            } else {
                AvailableSpace::Definite(size)
            }
        };
        self.compute_in(
            root,
            taffy::Size {
                width: axis(free_x, width),
                height: axis(free_y, height),
            },
        );
    }

    /// The shared computation: taffy, with measured leaves' **constrained
    /// measurements** routed to their closures.
    fn compute_in(&mut self, root: NodeId, available: taffy::Size<AvailableSpace>) {
        let measures = &self.measures;
        self.tree.compute(
            root,
            available,
            |known: taffy::Size<Option<f32>>, space: taffy::Size<AvailableSpace>, node| {
                let Some(measure) = measures.get(&node) else {
                    return taffy::Size::ZERO;
                };
                // The constraint resolved per axis: the known dimension if taffy
                // has already settled it, otherwise from the space offered
                // (min-content = as narrow as possible, 0; max-content = free).
                let bound = |known: Option<f32>, space: AvailableSpace| {
                    known.or(match space {
                        AvailableSpace::Definite(v) => Some(v),
                        AvailableSpace::MinContent => Some(0.0),
                        AvailableSpace::MaxContent => None,
                    })
                };
                let (bw, bh) = (
                    bound(known.width, space.width),
                    bound(known.height, space.height),
                );
                let size = measure(bw, bh);
                debug_assert_measurement(size.width, bw, "width");
                debug_assert_measurement(size.height, bh, "height");
                taffy::Size {
                    width: size.width,
                    height: size.height,
                }
            },
            self.context,
        );
    }

    /// The size a node came out at, after a `compute*`.
    ///
    /// [`Layout::absolute_rects`] answers the same question for a whole subtree and
    /// allocates one entry per node to do it; this is for the one caller that wants a
    /// single box back — a measurement asking how big the thing it just laid out is.
    pub fn size_of(&self, node: NodeId) -> Size {
        let l = self.tree.placed(node);
        Size::new(l.size.width, l.size.height)
    }

    /// Walks the tree in prefix order and returns, for each node, its rectangle in
    /// **absolute** coordinates along with any associated data.
    ///
    /// taffy expresses positions relative to the parent; here we accumulate the
    /// offsets to get absolute coordinates that can be rendered directly.
    pub fn absolute_rects(&self, root: NodeId) -> Vec<(Rect, Option<&T>)> {
        let mut out = Vec::new();
        self.collect(root, 0.0, 0.0, &mut out);
        out
    }

    /// Every box in the tree whose children spill out of it, with the edge and the
    /// amount — the measurement behind [`Overflowing`].
    ///
    /// Only within **one** taffy tree, which is exactly the useful scope: a scrollable, a
    /// stack, a page view, a fitter and an overflow box are all laid out as leaves here
    /// and their content computed separately, so content larger than its viewport — the
    /// one overflow that is deliberate — never reaches this walk.
    pub fn overflows(&self, root: NodeId) -> Vec<Overflowing> {
        let mut out = Vec::new();
        self.collect_overflow(root, 0.0, 0.0, &mut out);
        out
    }

    fn collect_overflow(
        &self,
        node: NodeId,
        offset_x: f32,
        offset_y: f32,
        out: &mut Vec<Overflowing>,
    ) {
        // The **unrounded** layout, which is the only one the question can be asked of.
        // taffy rounds each node's edges to whole pixels independently, so a box 169.6
        // tall whose child sits at 21.6 and is 148 tall becomes a box of 169 with a child
        // at 22: one pixel of overflow that exists nowhere but in the rounding, reported
        // against a widget nobody wrote wrong. Four of this framework's own goldens wore a
        // band for it the day the band was painted.
        let layout = self.tree.unrounded(node);
        let x = offset_x + layout.location.x;
        let y = offset_y + layout.location.y;

        let child_count = self.tree.count(node);
        if child_count > 0 {
            // The content box, in the node's own coordinates. Taffy places children
            // relative to the border box, so these bounds are directly comparable.
            let left = layout.border.left + layout.padding.left;
            let top = layout.border.top + layout.padding.top;
            let right = layout.size.width - layout.border.right - layout.padding.right;
            let bottom = layout.size.height - layout.border.bottom - layout.padding.bottom;

            let (mut over_l, mut over_t, mut over_r, mut over_b) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for i in 0..child_count {
                let child = self.tree.child_at(node, i);
                let c = self.tree.unrounded(child);
                over_l = over_l.max(left - c.location.x);
                over_t = over_t.max(top - c.location.y);
                over_r = over_r.max(c.location.x + c.size.width - right);
                over_b = over_b.max(c.location.y + c.size.height - bottom);
            }

            // Sub-pixel slack. Rounding a fractional text width against a fractional box
            // produces overflows of a few hundredths that no one can see and nobody
            // should be told about; the reference has the same tolerance for the same
            // reason.
            const TOLERANCE: f32 = 0.5;
            let rect = Rect::new(x, y, layout.size.width, layout.size.height);
            for (amount, side) in [
                (over_l, Side::Left),
                (over_t, Side::Top),
                (over_r, Side::Right),
                (over_b, Side::Bottom),
            ] {
                if amount > TOLERANCE {
                    out.push(Overflowing { rect, side, amount });
                }
            }
        }

        for i in 0..child_count {
            let child = self.tree.child_at(node, i);
            self.collect_overflow(child, x, y, out);
        }
    }

    fn collect<'s>(
        &'s self,
        node: NodeId,
        offset_x: f32,
        offset_y: f32,
        out: &mut Vec<(Rect, Option<&'s T>)>,
    ) {
        let layout = self.tree.placed(node);
        let x = offset_x + layout.location.x;
        let y = offset_y + layout.location.y;

        let rect = Rect::new(x, y, layout.size.width, layout.size.height);
        out.push((rect, self.tree.data(node)));

        let child_count = self.tree.count(node);
        for i in 0..child_count {
            let child = self.tree.child_at(node, i);
            self.collect(child, x, y, out);
        }
    }
}

impl<T> Default for Layout<'_, T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {

    /// A row that does not fit says so, on the side it ran past and by how much.
    #[test]
    fn a_row_too_small_for_its_children_reports_it() {
        let mut layout: Layout<'_, ()> = Layout::new();
        let a = layout.leaf(
            Style {
                width: Dimension::Length(80.0),
                flex_shrink: 0.0,
                ..Default::default()
            },
            (),
        );
        let b = layout.leaf(
            Style {
                width: Dimension::Length(80.0),
                flex_shrink: 0.0,
                ..Default::default()
            },
            (),
        );
        let row = layout.container(
            Style {
                width: Dimension::Length(100.0),
                height: Dimension::Length(20.0),
                ..Default::default()
            },
            &[a, b],
        );
        layout.compute(row, Size::new(200.0, 200.0));
        let over = layout.overflows(row);
        assert_eq!(over.len(), 1, "one edge, once: {over:?}");
        assert_eq!(over[0].side, Side::Right);
        assert_eq!(over[0].amount, 60.0);
        assert_eq!(over[0].rect.width, 100.0, "the box named is the parent's");
    }

    /// Padding is part of the box the children have to fit inside.
    #[test]
    fn padding_counts_as_room_taken() {
        let mut layout: Layout<'_, ()> = Layout::new();
        let child = layout.leaf(
            Style {
                width: Dimension::Length(100.0),
                flex_shrink: 0.0,
                ..Default::default()
            },
            (),
        );
        let boxed = layout.container(
            Style {
                width: Dimension::Length(100.0),
                height: Dimension::Length(20.0),
                padding: frus_core::Insets::uniform(10.0),
                ..Default::default()
            },
            &[child],
        );
        layout.compute(boxed, Size::new(200.0, 200.0));
        let over = layout.overflows(boxed);
        assert_eq!(over.len(), 1);
        assert_eq!(over[0].amount, 20.0, "10 px of padding on either side");
    }

    /// And a row that fits says nothing — including when the arithmetic lands a hair off,
    /// which fractional text widths do constantly.
    #[test]
    fn a_row_that_fits_is_silent() {
        let mut layout: Layout<'_, ()> = Layout::new();
        let a = layout.leaf(
            Style {
                width: Dimension::Length(50.2),
                flex_shrink: 0.0,
                ..Default::default()
            },
            (),
        );
        let b = layout.leaf(
            Style {
                width: Dimension::Length(50.1),
                flex_shrink: 0.0,
                ..Default::default()
            },
            (),
        );
        let row = layout.container(
            Style {
                width: Dimension::Length(100.0),
                height: Dimension::Length(20.0),
                ..Default::default()
            },
            &[a, b],
        );
        layout.compute(row, Size::new(200.0, 200.0));
        assert!(layout.overflows(row).is_empty(), "0.3 px is nobody's bug");
    }
    use super::*;
    use crate::{Align, Dimension, FlexDirection, Justify, Style};
    use frus_core::Insets;

    #[test]
    fn flex_row_computes_absolute_positions() {
        let mut layout: Layout<'_, ()> = Layout::new();

        // Child A: fixed width 120. Child B: flex_grow 1, filling the rest.
        let a = layout.leaf(
            Style {
                width: Dimension::Length(120.0),
                ..Default::default()
            },
            (),
        );
        let b = layout.leaf(
            Style {
                flex_grow: 1.0,
                ..Default::default()
            },
            (),
        );
        let root = layout.container(
            Style {
                width: Dimension::Length(400.0),
                height: Dimension::Length(100.0),
                flex_direction: FlexDirection::Row,
                padding: Insets::uniform(10.0),
                gap: 8.0,
                ..Default::default()
            },
            &[a, b],
        );

        layout.compute(root, Size::new(400.0, 100.0));
        let rects = layout.absolute_rects(root);

        // Prefix walk: [root, a, b].
        let (root_rect, _) = rects[0];
        let (a_rect, _) = rects[1];
        let (b_rect, _) = rects[2];

        assert_eq!(root_rect, Rect::new(0.0, 0.0, 400.0, 100.0));

        // The content area: 10 of padding on every side.
        // A: x = 10, width 120, stretched in height (align stretch) => 80, y = 10.
        assert_eq!(a_rect, Rect::new(10.0, 10.0, 120.0, 80.0));

        // B: after A plus the gap => x = 10 + 120 + 8 = 138.
        // Width = content(380) - A(120) - gap(8) = 252.
        assert_eq!(b_rect, Rect::new(138.0, 10.0, 252.0, 80.0));
    }

    #[test]
    fn flex_wrap_moves_overflowing_child_to_next_line() {
        let mut layout: Layout<'_, ()> = Layout::new();

        // Three 80px children in a 200px container: 80+80 fit on the first line, and
        // the third (240 > 200) wraps to the next.
        let kids: Vec<_> = (0..3)
            .map(|_| {
                layout.leaf(
                    Style {
                        width: Dimension::Length(80.0),
                        height: Dimension::Length(40.0),
                        ..Default::default()
                    },
                    (),
                )
            })
            .collect();
        let root = layout.container(
            Style {
                width: Dimension::Length(200.0),
                height: Dimension::Length(200.0),
                flex_direction: FlexDirection::Row,
                align: Align::Start,
                flex_wrap: true,
                ..Default::default()
            },
            &kids,
        );

        layout.compute(root, Size::new(200.0, 200.0));
        let rects = layout.absolute_rects(root);
        // Prefix walk: [root, k0, k1, k2].
        let (k0, _) = rects[1];
        let (k1, _) = rects[2];
        let (k2, _) = rects[3];

        // k0 and k1 on the first line; k2 comes back to the left, lower down.
        assert_eq!(k0.y, k1.y);
        assert_eq!(k2.x, k0.x);
        assert!(k2.y >= k0.y + 40.0, "k2.y = {}, expected >= 40", k2.y);
    }

    #[test]
    fn measured_leaf_wraps_to_the_offered_width() {
        // Simulates a 250px text that wraps: given a width W, it occupies min(W, 250)
        // across and grows in height when it wraps.
        let measure: crate::MeasureFn<'_> = Box::new(|w, _| {
            let w = w.unwrap_or(250.0).min(250.0);
            let lines = (250.0 / w).ceil();
            Size::new(w, lines * 20.0)
        });
        let mut layout: Layout<'_, ()> = Layout::new();
        let text = layout.measured_leaf(Style::default(), (), measure);
        let root = layout.container(
            Style {
                width: Dimension::Length(100.0),
                flex_direction: FlexDirection::Column,
                align: Align::Start,
                ..Default::default()
            },
            &[text],
        );
        layout.compute(root, Size::new(100.0, 600.0));
        let rects = layout.absolute_rects(root);
        let (text_rect, _) = rects[1];
        assert!(
            text_rect.width <= 100.0,
            "wrapped to the offered width: {text_rect:?}"
        );
        assert!(
            text_rect.height >= 60.0,
            "3 lines expected (250/100 → 3 × 20): {text_rect:?}"
        );
    }

    /// The same measured leaf, but **centred** on the cross axis instead of
    /// stretched. Centring means the item is sized to fit rather than filled, and the
    /// height must still be the one that goes with the width it ends up at.
    #[test]
    fn a_centred_measured_leaf_reports_the_height_of_the_width_it_got() {
        let measure: crate::MeasureFn<'_> = Box::new(|w, _| {
            let w = w.unwrap_or(250.0).min(250.0);
            let lines = (250.0 / w).ceil();
            Size::new(w, lines * 20.0)
        });
        let mut layout: Layout<'_, ()> = Layout::new();
        let text = layout.measured_leaf(Style::default(), (), measure);
        let root = layout.container(
            Style {
                width: Dimension::Length(100.0),
                flex_direction: FlexDirection::Column,
                align: Align::Center,
                ..Default::default()
            },
            &[text],
        );
        layout.compute(root, Size::new(100.0, 600.0));
        let rects = layout.absolute_rects(root);
        let (text_rect, _) = rects[1];
        assert!(
            text_rect.width <= 100.0,
            "wrapped to the offered width: {text_rect:?}"
        );
        assert!(
            text_rect.height >= 60.0,
            "3 lines expected (250/100 -> 3 x 20): {text_rect:?}"
        );
    }

    #[test]
    fn justify_center_centers_child() {
        let mut layout: Layout<'_, ()> = Layout::new();
        let child = layout.leaf(
            Style {
                width: Dimension::Length(100.0),
                height: Dimension::Length(40.0),
                align: Align::Start,
                ..Default::default()
            },
            (),
        );
        let root = layout.container(
            Style {
                width: Dimension::Length(400.0),
                height: Dimension::Length(100.0),
                flex_direction: FlexDirection::Row,
                justify: Justify::Center,
                ..Default::default()
            },
            &[child],
        );
        layout.compute(root, Size::new(400.0, 100.0));
        let rects = layout.absolute_rects(root);

        // A child centred on the main axis: x = (400 - 100) / 2 = 150.
        assert_eq!(rects[1].0, Rect::new(150.0, 0.0, 100.0, 40.0));
    }

    /// **A measurer that rounds to nearest is caught.** This is the whole point of the
    /// rule: the failure it prevents is a clipped glyph three crates away, and the thing
    /// that reintroduces it is one `round()` written by reflex where a `ceil()` belonged.
    ///
    /// Debug builds only, which is where tests run.
    #[test]
    #[should_panic(expected = "not a whole number of pixels")]
    fn a_measurement_that_is_not_whole_is_caught() {
        let mut layout: Layout<'_, ()> = Layout::new();
        let node = layout.measured_leaf(
            Style::default(),
            (),
            // 146.4, the number from milestone 289, reported as it came.
            Box::new(|_, _| Size::new(146.4, 20.0)),
        );
        layout.compute(node, Size::new(400.0, 400.0));
    }

    /// And one that goes through the helper is not. The pair is the test: an assertion
    /// that fired on everything would be turned off within a week.
    #[test]
    fn a_measurement_put_through_the_helper_passes() {
        let mut layout: Layout<'_, ()> = Layout::new();
        let node = layout.measured_leaf(
            Style::default(),
            (),
            Box::new(|_, _| Size::new(frus_core::fits(146.4), frus_core::fits(20.0))),
        );
        layout.compute(node, Size::new(400.0, 400.0));
        let size = layout.size_of(node);
        assert_eq!(size.width, 147.0, "rounded up, so the box still fits it");
    }

    /// **A measurement clamped to its constraint is exempt.** The bound is the caller's
    /// number, not the measurer's: rounding it up would hand back a box a fraction wider
    /// than the space offered, which is a different bug. Clamp after rounding up, never
    /// before — and this is what says the rule knows the difference.
    #[test]
    fn a_measurement_clamped_to_a_fractional_bound_is_not_a_violation() {
        let mut layout: Layout<'_, ()> = Layout::new();
        let node = layout.measured_leaf(
            Style::default(),
            (),
            Box::new(|w, _| Size::new(frus_core::fits(500.0).min(w.unwrap_or(0.0)), 20.0)),
        );
        // A fractional offer, which is what an intermediate flex computation hands down.
        layout.compute(node, Size::new(146.4, 400.0));
    }

    /// **A measurement larger than the space it was offered is not a violation either.**
    /// This framework lets content overflow and reports it (`Overflowing`) rather than
    /// crushing it, so a measurer answering with its natural size is doing its job. Said
    /// as a test because the obvious assertion to add here is the one that would break
    /// every screen with a line too long for its column.
    #[test]
    fn a_measurement_bigger_than_the_offer_is_reported_not_forbidden() {
        let mut layout: Layout<'_, ()> = Layout::new();
        let node = layout.measured_leaf(
            Style::default(),
            (),
            Box::new(|_, _| Size::new(500.0, 20.0)),
        );
        layout.compute(node, Size::new(100.0, 100.0));
        assert_eq!(layout.size_of(node).width, 500.0);
    }

    /// **Overlapping children share one cell**: the box is the largest of them on each
    /// axis — not their sum, as a line would make it — and a smaller one is centred in it.
    /// Handed a bigger box, the cell fills it and centres both in that.
    #[test]
    fn overlapping_children_share_one_cell_the_size_of_the_largest() {
        let place = |width: Dimension| {
            let mut layout: Layout<'_, u8> = Layout::new();
            let wide = layout.leaf(
                Style {
                    width: Dimension::Length(80.0),
                    height: Dimension::Length(20.0),
                    ..Default::default()
                },
                1,
            );
            let tall = layout.leaf(
                Style {
                    width: Dimension::Length(30.0),
                    height: Dimension::Length(50.0),
                    ..Default::default()
                },
                2,
            );
            let cell = layout.container(
                Style {
                    width,
                    overlap: true,
                    align: crate::style::Align::Center,
                    ..Default::default()
                },
                &[wide, tall],
            );
            layout.compute(cell, Size::new(400.0, 400.0));
            let rects = layout.absolute_rects(cell);
            let at = |tag: u8| {
                let r = rects.iter().find(|(_, d)| *d == Some(&tag)).unwrap().0;
                (r.x, r.y)
            };
            (layout.size_of(cell), at(1), at(2))
        };

        let (size, wide, tall) = place(Dimension::Auto);
        assert_eq!(
            (size.width, size.height),
            (80.0, 50.0),
            "the largest, per axis"
        );
        assert_eq!(wide, (0.0, 15.0), "the wide one centred down the cell");
        assert_eq!(tall, (25.0, 0.0), "the tall one centred across it");

        let (size, wide, tall) = place(Dimension::Length(200.0));
        assert_eq!(size.width, 200.0);
        assert_eq!(wide.0, 60.0, "a box handed over is filled, and centred in");
        assert_eq!(tall.0, 85.0);
    }

    /// A list of `rows` rows, each a row holding a measured label (a paragraph that wraps
    /// at the width it is given) and a fixed box; `wide` makes one label's text longer. Every
    /// node has an identity, as a widget tree gives it.
    fn list(rows: usize, wide: Option<usize>, memo: Option<Memo>) -> (Vec<Rect>, Memo, u32, u32) {
        let mut layout: Layout<'_, ()> = Layout::new();
        layout.reuse(memo.unwrap_or_default(), 7);
        let mut children = Vec::new();
        for i in 0..rows {
            let words: f32 = if wide == Some(i) { 30.0 } else { 10.0 };
            let label = layout.measured_leaf_keyed(
                Style {
                    flex_grow: 1.0,
                    ..Default::default()
                },
                (),
                Some(words.to_bits() as u64),
                Box::new(move |w, _| {
                    // Ten pixels a word, wrapped at the width offered, twenty a line.
                    let width = w.unwrap_or(f32::INFINITY);
                    let per_line = (width / 10.0).floor().max(1.0);
                    let lines = (words / per_line).ceil();
                    Size::new((words * 10.0).min(width), lines * 20.0)
                }),
            );
            layout.set_identity(label, 1000 + i as u64);
            let mark = layout.leaf(
                Style {
                    width: Dimension::Length(24.0),
                    height: Dimension::Length(24.0),
                    ..Default::default()
                },
                (),
            );
            layout.set_identity(mark, 2000 + i as u64);
            let row = layout.container(
                Style {
                    flex_direction: FlexDirection::Row,
                    ..Default::default()
                },
                &[label, mark],
            );
            layout.set_identity(row, 3000 + i as u64);
            children.push(row);
        }
        let column = layout.container(
            Style {
                flex_direction: FlexDirection::Column,
                ..Default::default()
            },
            &children,
        );
        layout.set_identity(column, 1);
        layout.compute_scroll(column, 200.0, 0.0, false, true);
        let rects = layout
            .absolute_rects(column)
            .into_iter()
            .map(|(r, _)| r)
            .collect();
        let (laid_out, reused) = (layout.laid_out(), layout.reused());
        (rects, layout.into_memo(column), laid_out, reused)
    }

    /// **A layout handed the last one's memo gives the rectangles a layout from scratch
    /// gives, and lays out only what changed** (milestone 853): one row of fifty grows a
    /// line, and the other forty-nine answer from what they measured a frame ago.
    #[test]
    fn a_memo_lays_out_the_changed_path_alone_and_changes_no_rectangle() {
        let (first, memo, fresh, reused) = list(50, None, None);
        assert_eq!(reused, 0, "nothing to start from");
        let (same, memo, again, reused) = list(50, None, Some(memo));
        assert_eq!(same, first, "the same tree, the same boxes");
        assert_eq!(again, 0, "and nothing laid out: the root answered");
        assert_eq!(reused, 151, "every node was the same");

        let (scratch, _, _, _) = list(50, Some(20), None);
        let (grown, _, laid_out, _) = list(50, Some(20), Some(memo));
        assert_eq!(grown, scratch, "the rectangles of a layout from scratch");
        assert_ne!(grown, first, "and the row did grow");
        assert!(
            laid_out * 10 < fresh,
            "the path to the changed row, not the list: {laid_out} of {fresh}"
        );
    }

    /// A memo taken under another context (the reader's font size, the surface) is not used.
    #[test]
    fn a_memo_from_another_context_is_ignored() {
        let (_, memo, _, _) = list(5, None, None);
        let mut layout: Layout<'_, ()> = Layout::new();
        layout.reuse(memo, 8);
        let leaf = layout.leaf(Style::default(), ());
        layout.set_identity(leaf, 1);
        layout.compute(leaf, Size::new(10.0, 10.0));
        assert_eq!(layout.reused(), 0);
    }

    /// **A measured leaf nothing describes** is measured afresh, and so is everything above
    /// it; its siblings still answer from the memo.
    #[test]
    fn an_unkeyed_measurement_is_measured_again() {
        let build = |memo: Option<Memo>| {
            let mut layout: Layout<'_, ()> = Layout::new();
            layout.reuse(memo.unwrap_or_default(), 0);
            let opaque =
                layout.measured_leaf(Style::default(), (), Box::new(|_, _| Size::new(30.0, 10.0)));
            layout.set_identity(opaque, 1);
            let plain = layout.leaf(
                Style {
                    width: Dimension::Length(10.0),
                    height: Dimension::Length(10.0),
                    ..Default::default()
                },
                (),
            );
            layout.set_identity(plain, 2);
            let row = layout.container(Style::default(), &[opaque, plain]);
            layout.set_identity(row, 3);
            layout.compute(row, Size::new(100.0, 100.0));
            let rects: Vec<Rect> = layout
                .absolute_rects(row)
                .into_iter()
                .map(|(r, _)| r)
                .collect();
            let reused = layout.reused();
            (rects, layout.into_memo(row), reused)
        };
        let (first, memo, _) = build(None);
        let (second, _, reused) = build(Some(memo));
        assert_eq!(first, second);
        assert_eq!(reused, 1, "the plain box alone");
    }

    /// A style changed after the first computation — a second pass at a bound, as
    /// `compute_axes` makes — forgets what the node and the nodes above it kept, the memo's
    /// answers included.
    #[test]
    fn a_style_changed_between_two_passes_is_laid_out_again() {
        let (_, memo, _, _) = list(3, None, None);
        let mut layout: Layout<'_, ()> = Layout::new();
        layout.reuse(memo, 7);
        let leaf = layout.leaf(Style::default(), ());
        layout.set_identity(leaf, 1);
        layout.compute(leaf, Size::new(10.0, 10.0));
        layout.fill_root(leaf, true);
        layout.compute(leaf, Size::new(10.0, 10.0));
        assert_eq!(layout.size_of(leaf).width, 10.0);
    }
}
