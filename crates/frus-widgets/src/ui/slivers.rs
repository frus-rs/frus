// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The walk of a [`CustomScrollView`] (milestone 780).
//!
//! Two passes over the pieces. The first asks each how far it scrolls, which is what the
//! offset is kept inside of; the second lays each out at the offset, in order, the way the
//! sliver protocol has it — how far into it the view has scrolled, how much of the viewport
//! is left, how much the pieces before it paint over — and builds only the children that
//! fall in the viewport or its cache extent. Then the children are walked, and the headers'
//! paint and touch targets raised over what scrolls under them.

use super::*;
use crate::scroll::Axis;
use crate::sliver::{
    box_geometry, fill_extent, fixed_window, grid_metrics, header_layout, padded_constraints,
    padded_geometry, paint_offset, CustomScrollView, ListItems, Sliver, SliverConstraints,
    SliverGeometry, SliverHeaderState, SliverKind, SliverList,
};

/// A child the layout placed, waiting to be walked.
struct Placed<'a, Msg> {
    widget: &'a dyn Widget<Msg>,
    id: WidgetId,
    rects: Vec<Rect>,
    /// Along the scroll, from the viewport's start.
    main: f32,
    /// Across it, from the viewport's (physical) start.
    cross: f32,
    /// Built on demand by a list or a grid: walked in its own row scope.
    row: bool,
    /// A header: painted, and touched, over what comes after it.
    raised: bool,
}

/// What every piece of one view shares while it is laid out.
struct Frame {
    across: bool,
    main_extent: f32,
    rtl: bool,
    at_rest: bool,
}

/// The rows a list built this frame, for what it keeps alive out of view.
type Walked<'a, Msg> = Vec<(WidgetId, &'a SliverList<Msg>, Vec<crate::list::KeptRow>)>;

/// The extent along the scroll of a box.
fn main_of(across: bool, rect: Option<&Rect>) -> f32 {
    rect.map_or(0.0, |r| if across { r.width } else { r.height })
}

/// A box as long as it needs along the scroll and handed the whole cross extent.
fn natural(across: bool, cross: f32, main: f32) -> Constraints {
    if across {
        Constraints::axes(main, cross, [true, false], [false, true])
    } else {
        Constraints::axes(cross, main, [false, true], [true, false])
    }
}

/// A box handed exactly `main` along the scroll and `cross` across it.
fn exactly(across: bool, main: f32, cross: f32) -> Constraints {
    Constraints::filled(if across {
        Size::new(main, cross)
    } else {
        Size::new(cross, main)
    })
}

/// Whether `[from, to)` of a piece reaches the part of it that is built.
fn in_cache(c: &SliverConstraints, from: f32, to: f32) -> bool {
    to > c.scroll_offset - c.cache_before && from < c.scroll_offset + c.remaining_cache_extent
}

impl<'a, Msg: Clone + 'static> Builder<'a, Msg> {
    pub(super) fn walk_slivers(
        &mut self,
        widget: &'a dyn Widget<Msg>,
        view: &'a CustomScrollView<Msg>,
        id: WidgetId,
        draw_rect: Rect,
        clip: Rect,
    ) {
        let viewport = draw_rect;
        let content_clip = clip.intersect(viewport);
        let across = view.axis == Axis::Horizontal;
        let (main_extent, cross_extent) = if across {
            (viewport.width, viewport.height)
        } else {
            (viewport.height, viewport.width)
        };

        // How far it all scrolls, which the offset is kept inside of.
        let mut total = 0.0;
        for (i, sliver) in view.slivers.iter().enumerate() {
            total += self.sliver_extent(
                sliver,
                id.child(i),
                across,
                cross_extent,
                main_extent,
                total,
            );
        }
        let max = (total - main_extent).max(0.0);
        let bounds = if across { (max, 0.0) } else { (0.0, max) };
        let offset = self
            .runtime
            .scroll_offset_within(id, bounds)
            .map_or(0.0, |(x, y)| if across { x } else { y });
        self.scrollables.push(Scrollable {
            id,
            viewport,
            max_x: if across { max } else { 0.0 },
            max_y: if across { 0.0 } else { max },
            axis: if across {
                Axis::Horizontal
            } else {
                Axis::Vertical
            },
            physics: region_physics(widget, &self.theme),
            keyboard_dismiss: region_keyboard_dismiss(widget),
            overscroll: crate::ScrollConfiguration::of().overscroll_indicator(&self.theme),
            fling: crate::ScrollConfiguration::of()
                .velocity_tracker(&self.theme)
                .strategy(),
            refresh: self.refresh_host,
            page: None,
            reverse_x: false,
            reverse_y: false,
            host: self.scroll_host,
            keep_visible: None,
        });

        let stretch_mark = (self.scene.primitives().len(), self.xform_base());
        let outer = self.scroll_host.replace(id);
        let frame = Frame {
            across,
            main_extent,
            rtl: self.rtl(),
            at_rest: self.runtime.scroll_at_rest(id),
        };
        let cache = view.cache_extent;

        // The layout proper, piece after piece. A view pulled past its start (a bounce)
        // starts its first piece that far down.
        let mut placed: Vec<Placed<'a, Msg>> = Vec::new();
        let mut walked: Walked<'a, Msg> = Vec::new();
        let mut layout_offset = (-offset).max(0.0);
        let mut remaining = offset;
        let mut max_paint = layout_offset;
        let mut preceding = 0.0;
        for (i, sliver) in view.slivers.iter().enumerate() {
            let so = remaining.max(0.0);
            let c = SliverConstraints {
                scroll_offset: so,
                overlap: max_paint - layout_offset,
                remaining_paint_extent: (main_extent - layout_offset).max(0.0),
                viewport_main_extent: main_extent,
                cross_axis_extent: cross_extent,
                preceding_scroll_extent: preceding,
                cache_before: cache.min(so),
                remaining_cache_extent: (main_extent + cache - layout_offset).max(0.0),
            };
            let first = placed.len();
            let geometry = self.lay_sliver(
                sliver,
                id.child(i),
                &c,
                0.0,
                &frame,
                &mut placed,
                &mut walked,
            );
            let base = layout_offset + geometry.paint_origin;
            for child in &mut placed[first..] {
                child.main += base;
            }
            max_paint = (base + geometry.paint_extent).max(max_paint);
            remaining -= geometry.scroll_extent;
            preceding += geometry.scroll_extent;
            layout_offset += geometry.layout_extent;
        }

        // The children, in order; the headers then raised over what came after them, the
        // last first, so that an earlier header ends up over a later one.
        let runtime: &'a Runtime = self.runtime;
        let mut raised = Vec::new();
        for child in placed {
            let (x, y) = if across {
                (viewport.x + child.main, viewport.y + child.cross)
            } else {
                (viewport.x + child.cross, viewport.y + child.main)
            };
            let start = child.raised.then(|| self.barrier_base());
            let mut index = 0;
            {
                let _row = child.row.then(|| runtime.states.enter_row(child.id, false));
                self.walk(
                    child.widget,
                    child.id,
                    (x, y),
                    content_clip,
                    &child.rects,
                    &mut index,
                );
            }
            if let Some(start) = start {
                raised.push((start, self.barrier_base()));
            }
        }
        for (start, end) in raised.iter().rev() {
            self.raise_segment(start, end);
        }
        for (list_id, list, rows) in &walked {
            if let ListItems::Built(build) = &list.items {
                let description = crate::list::VirtualList {
                    count: list.count,
                    item_extent: list.item_extent.unwrap_or(list.estimate),
                    axis: view.axis,
                    build: &**build,
                    variable: list.item_extent.is_none(),
                    keep_alive: false,
                    find_child_index: None,
                };
                self.keep_rows_alive(*list_id, &description, rows);
            }
        }

        self.scroll_host = outer;
        self.add_overscroll_stretch(id, viewport, content_clip, stretch_mark);
        self.scene.set_clip(clip);
        self.add_overscroll_glow(id, viewport);
        if max > 0.0 {
            self.add_scrollbar(
                id,
                viewport,
                Bar {
                    vertical: !across,
                    offset,
                    max,
                    reverse: false,
                    asked: widget.scrollbars(),
                    visible: widget.thumb_visibility(),
                },
            );
        }
    }

    /// How far `sliver` scrolls, `preceding` being how far the pieces before it do.
    fn sliver_extent(
        &mut self,
        sliver: &'a Sliver<Msg>,
        sid: WidgetId,
        across: bool,
        cross: f32,
        main_extent: f32,
        preceding: f32,
    ) -> f32 {
        match &sliver.kind {
            SliverKind::Box(child) => {
                let cid = sid.child(0);
                build_deferred_at(child.as_ref(), cid, &self.theme, self.runtime);
                let rects =
                    self.cached_rects(cid, child.as_ref(), natural(across, cross, main_extent));
                main_of(across, rects.first())
            }
            SliverKind::List(list) => match list.item_extent {
                Some(extent) => list.count as f32 * extent,
                None => match self.runtime.item_extents.borrow().get(&sid) {
                    Some(known) => known.total_for(list.count, list.estimate),
                    None => list.count as f32 * list.estimate,
                },
            },
            SliverKind::Grid(grid) => grid_metrics(grid, cross).extent(grid.count, grid.row_gap),
            SliverKind::Padding(pad, inner) => {
                let (before, after, cross_pad) = if across {
                    (pad.left, pad.right, pad.top + pad.bottom)
                } else {
                    (pad.top, pad.bottom, pad.left + pad.right)
                };
                before
                    + after
                    + self.sliver_extent(
                        inner,
                        sid.child(0),
                        across,
                        (cross - cross_pad).max(0.0),
                        main_extent,
                        preceding + before,
                    )
            }
            SliverKind::Fill(fill) => {
                let natural_extent = match &fill.child {
                    Some(child) => {
                        let cid = sid.child(0);
                        build_deferred_at(child.as_ref(), cid, &self.theme, self.runtime);
                        (!fill.has_scroll_body).then(|| {
                            let rects = self.cached_rects(
                                cid,
                                child.as_ref(),
                                natural(across, cross, main_extent),
                            );
                            main_of(across, rects.first())
                        })
                    }
                    None => None,
                };
                let c = SliverConstraints {
                    viewport_main_extent: main_extent,
                    preceding_scroll_extent: preceding,
                    ..Default::default()
                };
                fill_extent(&c, natural_extent, fill.has_scroll_body)
            }
            SliverKind::Header(header) => (header.extents)(&self.theme).1,
        }
    }

    /// Lays `sliver` out under `c`, its children placed in `out` relative to where it
    /// paints along the scroll, and at `cross_start` across it.
    #[allow(clippy::too_many_arguments)]
    fn lay_sliver(
        &mut self,
        sliver: &'a Sliver<Msg>,
        sid: WidgetId,
        c: &SliverConstraints,
        cross_start: f32,
        f: &Frame,
        out: &mut Vec<Placed<'a, Msg>>,
        walked: &mut Walked<'a, Msg>,
    ) -> SliverGeometry {
        let across = f.across;
        let so = c.scroll_offset;
        let cross = c.cross_axis_extent;
        let runtime: &'a Runtime = self.runtime;
        match &sliver.kind {
            SliverKind::Box(child) => {
                let cid = sid.child(0);
                let rects =
                    self.cached_rects(cid, child.as_ref(), natural(across, cross, f.main_extent));
                let extent = main_of(across, rects.first());
                if in_cache(c, 0.0, extent) {
                    out.push(Placed {
                        widget: child.as_ref(),
                        id: cid,
                        rects,
                        main: -so,
                        cross: cross_start,
                        row: false,
                        raised: false,
                    });
                }
                box_geometry(c, extent)
            }
            SliverKind::List(list) => {
                let lo = so - c.cache_before;
                let hi = so + c.remaining_cache_extent;
                let mut rows = Vec::new();
                let geometry = match list.item_extent {
                    Some(extent) => {
                        for i in fixed_window(list.count, extent, lo, hi) {
                            let (item, row, rects) =
                                self.list_item(list, sid, i, exactly(across, extent, cross));
                            if let Some(row) = row {
                                rows.push(crate::list::KeptRow {
                                    row,
                                    key: item.key(),
                                    index: i,
                                });
                            }
                            out.push(Placed {
                                widget: item,
                                id: row.unwrap_or_else(|| child_id(sid, i, item)),
                                rects,
                                main: i as f32 * extent - so,
                                cross: cross_start,
                                row: row.is_some(),
                                raised: false,
                            });
                        }
                        box_geometry(c, list.count as f32 * extent)
                    }
                    None => {
                        let mut known = runtime
                            .item_extents
                            .borrow_mut()
                            .remove(&sid)
                            .unwrap_or_default();
                        known.fit(list.count);
                        let guess = known.estimate(list.estimate);
                        let (mut i, mut start) = known.first_at(lo.max(0.0), guess);
                        let mut changed = false;
                        while i < list.count && start < hi {
                            let (item, row, rects) =
                                self.list_item(list, sid, i, natural(across, cross, f.main_extent));
                            let length = main_of(across, rects.first());
                            changed |= known.measure(i, length);
                            if let Some(row) = row {
                                rows.push(crate::list::KeptRow {
                                    row,
                                    key: item.key(),
                                    index: i,
                                });
                            }
                            out.push(Placed {
                                widget: item,
                                id: row.unwrap_or_else(|| child_id(sid, i, item)),
                                rects,
                                main: start - so,
                                cross: cross_start,
                                row: row.is_some(),
                                raised: false,
                            });
                            start += length;
                            i += 1;
                        }
                        let total = known.total(known.estimate(list.estimate));
                        runtime.item_extents.borrow_mut().insert(sid, known);
                        if changed {
                            // Measured something new: the extent the offset was kept inside
                            // of was a guess, and the next frame has the measurement.
                            self.wants_animation = true;
                        }
                        box_geometry(c, total)
                    }
                };
                if matches!(list.items, ListItems::Built(_)) {
                    walked.push((sid, list, rows));
                }
                geometry
            }
            SliverKind::Grid(grid) => {
                let m = grid_metrics(grid, cross);
                let rows = grid.count.div_ceil(m.columns);
                let lo = so - c.cache_before;
                let hi = so + c.remaining_cache_extent;
                for r in fixed_window(rows, m.row_stride, lo, hi) {
                    for column in 0..m.columns {
                        let index = r * m.columns + column;
                        if index >= grid.count {
                            break;
                        }
                        let item = (grid.build)(index);
                        let cell = child_id(sid, index, item.as_ref());
                        let rects = {
                            let _row = runtime.states.enter_row(cell, true);
                            build_deferred_at(item.as_ref(), cell, &self.theme, runtime);
                            self.cached_rects(
                                cell,
                                item.as_ref(),
                                exactly(across, m.tile_main, m.tile_cross),
                            )
                        };
                        let along = column as f32 * m.column_stride;
                        // Down the screen the columns follow the reading direction.
                        let at = if !across && f.rtl {
                            cross - along - m.tile_cross
                        } else {
                            along
                        };
                        out.push(Placed {
                            widget: self.keep(item),
                            id: cell,
                            rects,
                            main: r as f32 * m.row_stride - so,
                            cross: cross_start + at,
                            row: true,
                            raised: false,
                        });
                    }
                }
                box_geometry(c, m.extent(grid.count, grid.row_gap))
            }
            SliverKind::Padding(pad, inner) => {
                let (before, after, cross_before, cross_after) = if across {
                    (pad.left, pad.right, pad.top, pad.bottom)
                } else {
                    (pad.top, pad.bottom, pad.left, pad.right)
                };
                let inner_c = padded_constraints(c, before, cross_before + cross_after);
                let first = out.len();
                let child = self.lay_sliver(
                    inner,
                    sid.child(0),
                    &inner_c,
                    cross_start + cross_before,
                    f,
                    out,
                    walked,
                );
                let before_paint = paint_offset(c, 0.0, before);
                for placed in &mut out[first..] {
                    placed.main += before_paint;
                }
                padded_geometry(c, before, after, child)
            }
            SliverKind::Fill(fill) => {
                let cid = sid.child(0);
                let natural_extent = match (&fill.child, fill.has_scroll_body) {
                    (Some(child), false) => {
                        let rects = self.cached_rects(
                            cid,
                            child.as_ref(),
                            natural(across, cross, f.main_extent),
                        );
                        Some(main_of(across, rects.first()))
                    }
                    _ => None,
                };
                let extent = fill_extent(c, natural_extent, fill.has_scroll_body);
                if let Some(child) = &fill.child {
                    if in_cache(c, 0.0, extent) {
                        let rects =
                            self.cached_rects(cid, child.as_ref(), exactly(across, extent, cross));
                        out.push(Placed {
                            widget: child.as_ref(),
                            id: cid,
                            rects,
                            main: -so,
                            cross: cross_start,
                            row: false,
                            raised: false,
                        });
                    }
                }
                box_geometry(c, extent)
            }
            SliverKind::Header(header) => {
                let (min, max) = (header.extents)(&self.theme);
                let mode = header.mode();
                let effective = if mode.floats() {
                    let mut floats = runtime.floating_headers.borrow_mut();
                    let state = floats.entry(sid).or_default();
                    if crate::sliver::advance_float(
                        state,
                        so,
                        max,
                        header.snap,
                        f.at_rest,
                        runtime.time,
                    ) {
                        self.wants_animation = true;
                    }
                    state.effective
                } else {
                    0.0
                };
                let laid = header_layout(c, min, max, mode, effective);
                if laid.geometry.paint_extent > 0.0 {
                    let state = SliverHeaderState {
                        shrink_offset: laid.shrink_offset,
                        extent: laid.child_extent,
                        min_extent: min,
                        max_extent: max,
                        overlaps_content: laid.overlaps_content,
                        cross_extent: cross,
                    };
                    let built = (header.build)(state);
                    let cid = sid.child(0);
                    build_deferred_at(built.as_ref(), cid, &self.theme, runtime);
                    let rects = self.cached_rects(
                        cid,
                        built.as_ref(),
                        exactly(across, laid.child_extent, cross),
                    );
                    out.push(Placed {
                        widget: self.keep(built),
                        id: cid,
                        rects,
                        main: laid.child_position,
                        cross: cross_start,
                        row: false,
                        raised: true,
                    });
                }
                laid.geometry
            }
        }
    }

    /// Item `i` of `list`, built (or borrowed, for a list of given children) and laid out
    /// under `constraints`; the row identity when it was built on demand.
    fn list_item(
        &mut self,
        list: &'a SliverList<Msg>,
        sid: WidgetId,
        i: usize,
        constraints: Constraints,
    ) -> (&'a dyn Widget<Msg>, Option<WidgetId>, Vec<Rect>) {
        let runtime: &'a Runtime = self.runtime;
        match &list.items {
            ListItems::Built(build) => {
                let item = build(i);
                let row = child_id(sid, i, item.as_ref());
                let rects = {
                    let _row = runtime.states.enter_row(row, true);
                    build_deferred_at(item.as_ref(), row, &self.theme, runtime);
                    self.cached_rects(row, item.as_ref(), constraints)
                };
                (self.keep(item), Some(row), rects)
            }
            ListItems::Owned(children) => {
                let item = children[i].as_ref();
                let id = child_id(sid, i, item);
                build_deferred_at(item, id, &self.theme, runtime);
                let rects = self.cached_rects(id, item, constraints);
                (item, None, rects)
            }
        }
    }
}
