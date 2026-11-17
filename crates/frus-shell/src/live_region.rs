// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Live regions** (milestone 810): the words of a node marked
//! [`live`](frus_widgets::SemanticsProperties::live) are read out when they appear or change,
//! without the reader moving to them — a field's error message, shown after typing somewhere
//! the reader is not looking.
//!
//! What is decided here is only *which words to say this frame*; saying them is the
//! announcement the shell already speaks through each platform's bridge.

use std::collections::HashMap;

use frus_widgets::{Rect, SemanticsProperties, WidgetId};

/// What every live node said on the last frame, and whether a frame has been seen at all.
#[derive(Default)]
pub(crate) struct LiveRegions {
    said: Option<HashMap<WidgetId, String>>,
}

impl LiveRegions {
    /// The words to read out for this frame's `nodes`: those of every live node that has
    /// appeared or changed since the last frame, joined, or `None`.
    ///
    /// The window's **first** frame says nothing: what an interface opens with is read when
    /// the reader gets to it, and announcing it as well would read it twice.
    pub(crate) fn changes(
        &mut self,
        nodes: &[(WidgetId, Rect, SemanticsProperties)],
    ) -> Option<String> {
        let now: HashMap<WidgetId, String> = nodes
            .iter()
            .filter(|(_, _, props)| props.live)
            .filter_map(|(id, _, props)| {
                let words = words_of(props);
                (!words.is_empty()).then_some((*id, words))
            })
            .collect();
        let before = self.said.replace(now.clone())?;
        let mut fresh: Vec<(WidgetId, &String)> = now
            .iter()
            .filter(|(id, words)| before.get(id) != Some(*words))
            .map(|(id, words)| (*id, words))
            .collect();
        if fresh.is_empty() {
            return None;
        }
        // In the order the tree has them, which is the order they are read in.
        fresh.sort_by_key(|(id, _)| nodes.iter().position(|(n, ..)| n == id));
        Some(
            fresh
                .into_iter()
                .map(|(_, words)| words.as_str())
                .collect::<Vec<_>>()
                .join(". "),
        )
    }
}

/// A node's words: its label, then its value.
fn words_of(props: &SemanticsProperties) -> String {
    let mut words = props.label.clone().unwrap_or_default();
    if let Some(value) = props.readable_value() {
        if !words.is_empty() {
            words.push(' ');
        }
        words.push_str(&value);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use frus_widgets::Role;

    fn node(id: u64, label: &str, live: bool) -> (WidgetId, Rect, SemanticsProperties) {
        (
            WidgetId::from_u64(id),
            Rect::new(0.0, 0.0, 10.0, 10.0),
            SemanticsProperties::new(Role::Label)
                .label(label)
                .live(live),
        )
    }

    /// **An error that appears is read out once**: not on the first frame, then when it
    /// appears, not again while it stays, and again when its words change.
    #[test]
    fn a_live_node_is_read_when_it_appears_and_when_it_changes() {
        let mut live = LiveRegions::default();
        assert_eq!(live.changes(&[node(1, "Name", false)]), None);
        assert_eq!(
            live.changes(&[node(1, "Name", false), node(2, "Required", true)]),
            Some("Required".to_string())
        );
        assert_eq!(
            live.changes(&[node(1, "Name", false), node(2, "Required", true)]),
            None,
            "the same words are not read twice"
        );
        assert_eq!(
            live.changes(&[node(2, "Too short", true)]),
            Some("Too short".to_string())
        );
        assert_eq!(live.changes(&[]), None, "a region leaving says nothing");
        assert_eq!(
            live.changes(&[node(2, "Too short", true)]),
            Some("Too short".to_string()),
            "and coming back is appearing again"
        );
    }

    /// The first frame is what the interface opens with: read when the reader gets to it.
    #[test]
    fn the_first_frame_is_not_announced() {
        let mut live = LiveRegions::default();
        assert_eq!(live.changes(&[node(2, "Required", true)]), None);
    }

    /// A node that is not live never speaks, however its words change.
    #[test]
    fn a_plain_node_is_never_announced() {
        let mut live = LiveRegions::default();
        live.changes(&[]);
        assert_eq!(live.changes(&[node(1, "Hello", false)]), None);
        assert_eq!(live.changes(&[node(1, "Bye", false)]), None);
    }
}
