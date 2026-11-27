// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The **repaint boundary** cache: holds, between frames, the painted output
//! (primitives + interaction maps) of a subtree, and **gives it back as it was** for
//! as long as nothing it was painted from has moved.
//!
//! It is the *paint* counterpart of the relayout cache (`relayout.rs`, milestones
//! 55 and 853): where the layout cache reuses the **rectangles** when the *style and
//! structure* are stable, this one reuses the **primitives** and what can be touched
//! when the *configuration*, the *state read at paint time* (hover, focus, animated
//! value, opacity, cursor) and the **geometry** are stable.
//!
//! Two kinds of subtree are kept:
//!
//! - **Every component's** (milestone 856). A component's
//!   [`built mark`](crate::Widget::built_mark) moves whenever anything in its subtree is
//!   built; while it stands, the subtree is the same objects with the same configuration.
//!   A component whose state changed is painted again, its ancestors are walked down to it,
//!   and every component beside it gives back what it painted the frame before.
//! - **An explicit [`crate::Widget::repaint_boundary`]** (milestone 88), which is not a
//!   component: it stands until the next build of anything, since a build may have given it
//!   a new configuration (the generation).
//!
//! ## Correctness
//! What the configuration does not say is checked every frame before an entry is used
//! (`ui.rs`, `Builder::walk_boundary`):
//! - the **fingerprint**: each descendant's interaction state (`Status`, the animated
//!   values the walk reads, the ink), the subtree's rectangles relative to its own box, the
//!   clip, and the scopes the walk carries into it (focus, selection area, scroll host...);
//! - the **theme** in force, the **surface** (`MediaQuery`), the reader's text scale;
//! - **where it is**: at the same place, or moved by whole physical pixels while it stays
//!   well inside its clip, in which case everything is moved with it.
//!
//! A subtree is only kept when it is **plain** (every node takes the walk's ordinary branch:
//! no scroll area, list, stack, overlay or continuous animation inside), and only what it
//! registers in the registries the replay knows how to give back: a subtree that opened an
//! overlay, a hover card, a scrollbar, a hero, a refresh area... is painted every frame.
//!
//! ## Memory
//! A component nested in another is kept in both, so the kept output is bounded: past four
//! times what the frame itself painted and registered, the largest entries are dropped and
//! not taken again until their subtree changes.
//!
//! The cache does not know the application's `Msg` type (the `Runtime` is
//! generic-agnostic): the data is stored **erased** behind a `Box<dyn Any>`, and
//! `ui.rs` downcasts it back to its concrete type (a single `Msg` per app → the
//! `downcast` always succeeds).

use std::any::Any;
use std::collections::{HashMap, HashSet};

use crate::interaction::WidgetId;

/// What an entry was taken under: what has to be unchanged for its subtree to have the
/// configuration it was painted from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lock {
    /// An explicit repaint boundary: stands until the next build of anything.
    Generation(u64),
    /// A component's subtree: stands while its built mark does (milestone 856).
    Built(u64),
}

/// One entry.
pub(crate) struct Slot {
    pub(crate) lock: Lock,
    /// The fingerprint the output was captured under.
    pub(crate) fingerprint: u64,
    /// The number of rectangles the subtree consumes (to advance the walk index on a
    /// *hit*).
    pub(crate) rect_count: usize,
    /// The erased painted data; `None` when the subtree was walked and not kept.
    pub(crate) data: Option<Box<dyn Any>>,
    /// How many primitives and registrations the data holds.
    pub(crate) size: usize,
    /// Dropped for room: not taken again under this lock.
    pub(crate) oversize: bool,
}

/// How many times what the frame painted and registered the kept output may hold.
const BUDGET_FACTOR: usize = 4;
/// What may always be kept, however small the frame.
const BUDGET_FLOOR: usize = 4096;

/// The paint cache, retained in the [`crate::Runtime`] from one frame to the next.
#[derive(Default)]
pub struct PaintCache {
    entries: HashMap<WidgetId, Slot>,
    /// Boundaries touched during the current frame (to evict the vanished ones).
    touched: HashSet<WidgetId>,
    /// The current generation: bumped on every rebuild of the `view`.
    generation: u64,
    /// What the entries hold, in primitives and registrations.
    held: usize,
    hits: u32,
    misses: u32,
    last_hits: u32,
    last_misses: u32,
    /// Each boundary met this frame: where it is drawn and how many times it has been
    /// painted (not replayed) so far — what a repaint rainbow colours (milestone 788).
    boundaries: Vec<(frus_core::Rect, u32)>,
    last_boundaries: Vec<(frus_core::Rect, u32)>,
    /// How many times each boundary has been painted rather than replayed.
    repaints: HashMap<WidgetId, u32>,
}

impl PaintCache {
    /// Invalidates the explicit repaint boundaries **logically**: something was built, so
    /// their configuration may have changed. Entries from the old generation will never be
    /// *hits* again. A component's entry does not depend on it: its own mark says whether
    /// its subtree was built.
    pub fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// The current generation.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// The entry of boundary `key`, if there is one. Marks the boundary as touched, so it
    /// survives the end of the frame.
    pub(crate) fn slot(&mut self, key: WidgetId) -> Option<&Slot> {
        self.touched.insert(key);
        self.entries.get(&key)
    }

    /// Records (or replaces) a boundary's entry.
    pub(crate) fn put(&mut self, key: WidgetId, slot: Slot) {
        self.touched.insert(key);
        self.held += slot.size;
        if let Some(old) = self.entries.insert(key, slot) {
            self.held -= old.size;
        }
    }

    /// A diagnostic counter: one boundary reused this frame, drawn at `rect`.
    pub(crate) fn note_hit(&mut self, key: WidgetId, rect: frus_core::Rect) {
        self.hits += 1;
        let painted = self.repaints.get(&key).copied().unwrap_or(0);
        self.boundaries.push((rect, painted));
    }

    /// A diagnostic counter: one boundary repainted this frame (a miss, or
    /// non-cacheable), drawn at `rect`.
    pub(crate) fn note_miss(&mut self, key: WidgetId, rect: frus_core::Rect) {
        self.misses += 1;
        let painted = self.repaints.entry(key).or_insert(0);
        *painted += 1;
        self.boundaries.push((rect, *painted));
    }

    /// To be called at the end of a frame that painted and registered `frame` things:
    /// forgets untouched boundaries (vanished widgets), drops the largest entries while the
    /// cache holds more than its budget, and freezes the frame's diagnostic counters.
    pub(crate) fn end_frame(&mut self, frame: usize) {
        let touched = std::mem::take(&mut self.touched);
        self.entries.retain(|id, _| touched.contains(id));
        self.repaints.retain(|id, _| touched.contains(id));
        self.held = self.entries.values().map(|slot| slot.size).sum();
        let budget = frame.saturating_mul(BUDGET_FACTOR).max(BUDGET_FLOOR);
        if self.held > budget {
            // The largest first: a component nested in others is held by each of them, and
            // the outermost hold the most. What is left is the finer grain, which is what a
            // change in one place needs.
            let mut sizes: Vec<(usize, WidgetId)> = self
                .entries
                .iter()
                .filter(|(_, slot)| slot.data.is_some())
                .map(|(id, slot)| (slot.size, *id))
                .collect();
            sizes.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.as_u64().cmp(&b.1.as_u64())));
            for (size, id) in sizes {
                if self.held <= budget {
                    break;
                }
                if let Some(slot) = self.entries.get_mut(&id) {
                    slot.data = None;
                    slot.size = 0;
                    slot.oversize = true;
                    self.held -= size;
                }
            }
        }
        self.last_boundaries = std::mem::take(&mut self.boundaries);
        self.last_hits = self.hits;
        self.last_misses = self.misses;
        self.hits = 0;
        self.misses = 0;
    }

    /// Reuses and repaints of the last completed frame (diagnostic).
    pub fn last_frame_stats(&self) -> (u32, u32) {
        (self.last_hits, self.last_misses)
    }

    /// How many primitives and registrations the cache holds (diagnostic, milestone 856).
    pub fn held(&self) -> usize {
        self.held
    }

    /// The repaint boundaries of the last completed frame: where each was drawn, in the
    /// frame's logical coordinates, and how many times it has been painted rather than
    /// replayed. A boundary whose count did not move was reused as it was. What a repaint
    /// rainbow colours (milestone 788).
    ///
    /// ```
    /// use frus_widgets::Runtime;
    /// let runtime = Runtime::default();
    /// assert!(runtime.paint_cache.borrow().last_frame_boundaries().is_empty());
    /// ```
    pub fn last_frame_boundaries(&self) -> &[(frus_core::Rect, u32)] {
        &self.last_boundaries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: WidgetId = WidgetId::ROOT;

    fn slot(lock: Lock, size: usize) -> Slot {
        Slot {
            lock,
            fingerprint: 42,
            rect_count: 3,
            data: Some(Box::new(7u32)),
            size,
            oversize: false,
        }
    }

    #[test]
    fn an_entry_keeps_what_it_was_taken_under() {
        let mut c = PaintCache::default();
        c.put(A, slot(Lock::Generation(c.generation()), 1));
        let s = c.slot(A).expect("kept");
        assert_eq!(
            (s.lock, s.fingerprint, s.rect_count),
            (Lock::Generation(0), 42, 3)
        );
        assert_eq!(*s.data.as_ref().unwrap().downcast_ref::<u32>().unwrap(), 7);
        c.bump_generation();
        let now = Lock::Generation(c.generation());
        assert_ne!(c.slot(A).unwrap().lock, now);
    }

    #[test]
    fn end_frame_evicts_untouched_boundaries() {
        let mut c = PaintCache::default();
        c.put(A, slot(Lock::Built(1), 1));
        c.put(A.child(0), slot(Lock::Built(2), 1));
        c.end_frame(0);
        // A frame where only A is seen.
        c.slot(A);
        c.end_frame(0);
        assert!(c.entries.contains_key(&A));
        assert!(
            !c.entries.contains_key(&A.child(0)),
            "vanished boundary evicted"
        );
        assert_eq!(c.held(), 1);
    }

    /// Past its budget the cache drops its largest entries, and remembers not to take them
    /// again under the same lock.
    #[test]
    fn the_largest_entries_go_first_when_the_cache_is_full() {
        let mut c = PaintCache::default();
        c.put(A, slot(Lock::Built(1), 5000));
        c.put(A.child(0), slot(Lock::Built(2), 300));
        c.put(A.child(1), slot(Lock::Built(3), 300));
        c.end_frame(1000);
        assert_eq!(c.held(), 600, "under 4 × 1000");
        let big = c.slot(A).expect("still there");
        assert!(big.data.is_none() && big.oversize);
        assert!(c.slot(A.child(0)).unwrap().data.is_some());
    }

    #[test]
    fn frame_stats_freeze_at_end_frame() {
        let mut c = PaintCache::default();
        let r = frus_core::Rect::new(1.0, 2.0, 3.0, 4.0);
        c.note_hit(A, r);
        c.note_hit(A.child(1), r);
        c.note_miss(A.child(2), r);
        c.end_frame(0);
        assert_eq!(c.last_frame_stats(), (2, 1));
        // Each boundary, with how many times it was painted: the reused ones never were.
        assert_eq!(c.last_frame_boundaries(), &[(r, 0), (r, 0), (r, 1)]);
        // The counters start again from zero for the next frame.
        c.end_frame(0);
        assert_eq!(c.last_frame_stats(), (0, 0));
    }
}
