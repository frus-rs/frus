// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **How many times each component was built** (milestone 788): what DevTools shows beside
//! each widget, and per frame, to find the part of a screen that builds more than it should.
//!
//! Debug builds only: in a release, [`note`] does nothing and the counts are empty.

use crate::interaction::WidgetId;

#[cfg(debug_assertions)]
mod counting {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    /// Builds per component, by its identity.
    pub(super) static COUNTS: Mutex<Option<HashMap<u64, u32>>> = Mutex::new(None);
    /// Builds since the shell last took them: the frame's.
    pub(super) static FRAME: AtomicU32 = AtomicU32::new(0);
    /// Past this many components counted, the counts start over rather than grow forever.
    const MOST: usize = 100_000;

    pub(super) fn note(id: u64) {
        FRAME.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut counts) = COUNTS.lock() {
            let counts = counts.get_or_insert_with(HashMap::new);
            if counts.len() >= MOST {
                counts.clear();
            }
            *counts.entry(id).or_insert(0) += 1;
        }
    }
}

/// A component was built: counted, in a debug build.
#[inline]
pub(crate) fn note(id: WidgetId) {
    #[cfg(debug_assertions)]
    counting::note(id.as_u64());
    #[cfg(not(debug_assertions))]
    let _ = id;
}

/// How many times the component `id` has been built since the application started; `0`
/// for a widget that is not a component, and always in a release build.
///
/// ```
/// use frus_widgets::{rebuild_count, WidgetId};
/// assert_eq!(rebuild_count(WidgetId::from_u64(1)), 0);
/// ```
pub fn rebuild_count(id: WidgetId) -> u32 {
    #[cfg(debug_assertions)]
    {
        counting::COUNTS
            .lock()
            .ok()
            .and_then(|counts| counts.as_ref()?.get(&id.as_u64()).copied())
            .unwrap_or(0)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = id;
        0
    }
}

/// Every component's build count, the most built first, at most `limit` of them.
///
/// ```
/// let counts = frus_widgets::rebuild_counts(10);
/// assert!(counts.len() <= 10);
/// ```
pub fn rebuild_counts(limit: usize) -> Vec<(WidgetId, u32)> {
    #[cfg(debug_assertions)]
    {
        let mut all: Vec<(WidgetId, u32)> = counting::COUNTS
            .lock()
            .ok()
            .and_then(|counts| {
                counts.as_ref().map(|c| {
                    c.iter()
                        .map(|(id, n)| (WidgetId::from_u64(*id), *n))
                        .collect()
                })
            })
            .unwrap_or_default();
        all.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.as_u64().cmp(&b.0.as_u64())));
        all.truncate(limit);
        all
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = limit;
        Vec::new()
    }
}

/// The components built since the last call: one frame's, when the shell takes them once a
/// frame. Always `0` in a release build.
///
/// ```
/// let _ = frus_widgets::take_frame_rebuilds();
/// assert_eq!(frus_widgets::take_frame_rebuilds(), 0);
/// ```
pub fn take_frame_rebuilds() -> u32 {
    #[cfg(debug_assertions)]
    {
        counting::FRAME.swap(0, std::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(debug_assertions))]
    {
        0
    }
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;
    use crate::{build_deferred, component, Runtime, Text, Theme, Widget};

    /// A component is counted each time it is built, under its identity.
    #[test]
    fn builds_are_counted() {
        let theme = Theme::default();
        let runtime = Runtime::default();
        let make = || component(|_cx| Box::new(Text::new("x")) as Box<dyn Widget>);
        let before = rebuild_count(WidgetId::ROOT);
        for _ in 0..3 {
            // A new tree each time, as a rebuild of its parent makes: its component builds.
            let tree = make();
            build_deferred(&tree, &theme, &runtime);
        }
        // Other tests build components at the root too, on other threads: at least these.
        assert!(rebuild_count(WidgetId::ROOT) >= before + 3);
        assert!(rebuild_counts(usize::MAX)
            .iter()
            .any(|(id, _)| *id == WidgetId::ROOT));
    }
}
