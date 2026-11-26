// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The **shape** of the accessibility tree (milestone 848).
//!
//! The walk collects the frame's nodes in reading order, as it always has. Each node now also
//! carries how many of the nodes after it are its **descendants**: a list's items follow the
//! list and are counted in its span, a card's words and controls in the card's. A pre-order
//! list with a span per node is a tree, and it stays one under everything the walk does to the
//! list: a subtree cut off at its start (a barrier) takes its spans with it, a subtree cached
//! and replayed elsewhere keeps spans that only count inside it, and a subtree merged into one
//! node leaves a node with no span.
//!
//! A span is set when a container **closes**, once the walk has left its subtree and the
//! number is known; nothing has to be patched while the subtree is still being walked.

use frus_core::{Rect, Role, SemanticsProperties, SemanticsScroll};

use crate::interaction::WidgetId;

/// One node: its widget, its box on screen and its annotation.
pub(crate) type Entry = (WidgetId, Rect, SemanticsProperties);

/// The frame's nodes in reading order, each with the number of nodes after it that are its
/// descendants.
#[derive(Clone, Debug, Default)]
pub(crate) struct SemanticsBuf {
    pub(crate) nodes: Vec<Entry>,
    pub(crate) spans: Vec<u32>,
}

impl SemanticsBuf {
    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Adds a node with no children (yet).
    pub(crate) fn push(&mut self, entry: Entry) {
        self.nodes.push(entry);
        self.spans.push(0);
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.nodes.truncate(len);
        self.spans.truncate(len);
    }

    /// A copy of the nodes from `from` on: a cached subtree's.
    pub(crate) fn tail(&self, from: usize) -> SemanticsBuf {
        SemanticsBuf {
            nodes: self.nodes[from..].to_vec(),
            spans: self.spans[from..].to_vec(),
        }
    }

    /// A copy of the nodes `from..to`: whole subtrees, so their spans stay whole.
    pub(crate) fn range(&self, from: usize, to: usize) -> SemanticsBuf {
        SemanticsBuf {
            nodes: self.nodes[from..to].to_vec(),
            spans: self.spans[from..to].to_vec(),
        }
    }

    /// Takes the nodes from `from` on out.
    pub(crate) fn split_off(&mut self, from: usize) -> SemanticsBuf {
        SemanticsBuf {
            nodes: self.nodes.split_off(from),
            spans: self.spans.split_off(from),
        }
    }

    /// Adds `other`'s nodes at the end, their shape kept.
    pub(crate) fn append(&mut self, mut other: SemanticsBuf) {
        self.nodes.append(&mut other.nodes);
        self.spans.append(&mut other.spans);
    }

    /// Puts a node at `at`, holding the `span` nodes after it.
    pub(crate) fn insert(&mut self, at: usize, entry: Entry, span: u32) {
        self.nodes.insert(at, entry);
        self.spans.insert(at, span);
    }

    /// The boxes of the nodes from `from` on, to move them.
    pub(crate) fn rects_from(&mut self, from: usize) -> impl Iterator<Item = &mut Rect> {
        self.nodes[from..].iter_mut().map(|(_, rect, _)| rect)
    }

    /// The end of the subtree starting at `at`.
    pub(crate) fn subtree_end(&self, at: usize) -> usize {
        at + 1 + self.spans[at] as usize
    }

    /// The indices of the **top-level** nodes from `from` on: the roots of the subtrees the
    /// walk added there, in order.
    pub(crate) fn roots_from(&self, from: usize) -> Vec<usize> {
        let mut roots = Vec::new();
        let mut at = from;
        while at < self.len() {
            roots.push(at);
            at = self.subtree_end(at);
        }
        roots
    }

    /// Closes the container at `at`: every node after it is its descendant. Numbers the
    /// items it numbers that say nothing of their place, and drops it when it holds nothing
    /// and says nothing — an empty card is not something a reader should stop on.
    pub(crate) fn close(&mut self, at: usize) {
        let span = self.len() - at - 1;
        let (_, _, props) = &self.nodes[at];
        let silent = props.label.is_none()
            && !props.clickable
            && props.custom_actions.is_empty()
            && props.value.is_none()
            && matches!(
                props.role,
                Role::Group
                    | Role::ScrollView
                    | Role::ListItem
                    | Role::Toolbar
                    | Role::Navigation
                    | Role::TabList
            );
        if span == 0 && silent {
            self.truncate(at);
            return;
        }
        self.spans[at] = span as u32;
        self.number_items(at);
    }

    /// Lifts the children of every **unnamed group** out of it and drops the group: inside
    /// something pressed as a whole, a card's words are the press's name, as they were before
    /// cards were groups, rather than a silent group beside an unnamed button.
    pub(crate) fn unwrap_silent_groups(&mut self) {
        let keep: Vec<bool> = self
            .nodes
            .iter()
            .zip(&self.spans)
            .map(|((_, _, props), span)| {
                !(*span > 0
                    && props.role == Role::Group
                    && props.label.is_none()
                    && !props.clickable
                    && props.custom_actions.is_empty())
            })
            .collect();
        if keep.iter().all(|k| *k) {
            return;
        }
        // How many kept nodes come before each index: a kept node's new span is the number
        // of kept nodes inside its old one.
        let mut before = Vec::with_capacity(keep.len() + 1);
        before.push(0u32);
        for k in &keep {
            before.push(before.last().copied().unwrap_or(0) + u32::from(*k));
        }
        let mut nodes = Vec::with_capacity(self.len());
        let mut spans = Vec::with_capacity(self.len());
        for (i, entry) in std::mem::take(&mut self.nodes).into_iter().enumerate() {
            if keep[i] {
                let end = i + 1 + self.spans[i] as usize;
                spans.push(before[end] - before[i + 1]);
                nodes.push(entry);
            }
        }
        self.nodes = nodes;
        self.spans = spans;
    }

    /// Gives the items of the container at `at` their place, where they have none.
    pub(crate) fn number_items(&mut self, at: usize) {
        let Some(item) = self.nodes[at].2.role.item_role() else {
            return;
        };
        let end = self.subtree_end(at);
        let mut items = Vec::new();
        let mut child = at + 1;
        while child < end {
            if self.nodes[child].2.role == item {
                items.push(child);
            }
            child = self.subtree_end(child);
        }
        let count = items.len();
        for (n, index) in items.into_iter().enumerate() {
            let props = &mut self.nodes[index].2;
            if props.position.is_none() {
                props.position = Some((n + 1, count));
            }
        }
    }

    /// Makes the nodes the walk added from `from` on one item of a list: the item's own node
    /// when it is a single list item already, given its place where it states none, and a list
    /// item node holding them otherwise (`id`, at `rect`).
    pub(crate) fn wrap_item(
        &mut self,
        from: usize,
        id: WidgetId,
        rect: Rect,
        position: usize,
        count: usize,
    ) {
        if from >= self.len() {
            return;
        }
        if self.subtree_end(from) == self.len() && self.nodes[from].2.role == Role::ListItem {
            let props = &mut self.nodes[from].2;
            if props.position.is_none() {
                props.position = Some((position, count));
            }
            return;
        }
        let span = (self.len() - from) as u32;
        let props = SemanticsProperties::new(Role::ListItem).position(position, count);
        self.insert(from, (id, rect, props), span);
    }

    /// Records where a scrolling area rests: on its own node when the walk just added one
    /// for it (a list), or on a new scroll view node at `viewport` when the area moves at all.
    pub(crate) fn note_scroll(&mut self, id: WidgetId, viewport: Rect, scroll: SemanticsScroll) {
        if let Some((last, _, props)) = self.nodes.last_mut() {
            if *last == id && props.role.is_container() {
                props.scroll = Some(scroll);
                return;
            }
        }
        let moves = scroll.max.0 > 0.5 || scroll.max.1 > 0.5;
        if moves && viewport.width > 0.0 && viewport.height > 0.0 {
            self.push((
                id,
                viewport,
                SemanticsProperties::new(Role::ScrollView).scroll(scroll),
            ));
        }
    }
}

/// The parent of every node of a pre-order list whose node `i` has `spans[i]` descendants:
/// `None` for a node at the top.
///
/// ```ignore
/// // A list holding two items, each holding one checkbox, then a button.
/// assert_eq!(
///     parents_of(&[4, 1, 0, 1, 0, 0]),
///     [None, Some(0), Some(1), Some(0), Some(3), None]
/// );
/// ```
pub(crate) fn parents_of(spans: &[u32]) -> Vec<Option<usize>> {
    let mut parents = Vec::with_capacity(spans.len());
    // The open ancestors, each with the end of its subtree.
    let mut open: Vec<(usize, usize)> = Vec::new();
    for (i, span) in spans.iter().enumerate() {
        while open.last().is_some_and(|&(_, end)| i >= end) {
            open.pop();
        }
        parents.push(open.last().map(|&(at, _)| at));
        if *span > 0 {
            open.push((i, i + 1 + *span as usize));
        }
    }
    parents
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(raw: u64, role: Role) -> Entry {
        (
            WidgetId::from_u64(raw),
            Rect::new(0.0, 0.0, 10.0, 10.0),
            SemanticsProperties::new(role),
        )
    }

    #[test]
    fn parents_follow_the_spans() {
        assert_eq!(
            parents_of(&[4, 1, 0, 1, 0, 0]),
            [None, Some(0), Some(1), Some(0), Some(3), None]
        );
        assert_eq!(parents_of(&[0, 0]), [None, None]);
        assert!(parents_of(&[]).is_empty());
    }

    /// A list closes over its items and numbers them; an empty card goes.
    #[test]
    fn a_container_closes_over_what_follows_it() {
        let mut buf = SemanticsBuf::default();
        buf.push(node(1, Role::List));
        buf.push(node(2, Role::ListItem));
        buf.push(node(3, Role::CheckBox));
        buf.close(1);
        buf.push(node(4, Role::ListItem));
        buf.close(0);
        assert_eq!(buf.spans, [3, 1, 0, 0]);
        assert_eq!(buf.nodes[1].2.position, Some((1, 2)));
        assert_eq!(buf.nodes[3].2.position, Some((2, 2)));
        assert_eq!(buf.roots_from(0), [0]);
        assert_eq!(buf.roots_from(1), [1, 3]);

        let mut empty = SemanticsBuf::default();
        empty.push(node(1, Role::Group));
        empty.close(0);
        assert_eq!(empty.len(), 0, "an empty, silent group is not kept");
    }

    /// An item of a list holding two nodes gets a list item around them; one that is a list
    /// item already is only numbered.
    #[test]
    fn an_item_is_wrapped_once() {
        let mut buf = SemanticsBuf::default();
        buf.push(node(2, Role::CheckBox));
        buf.push(node(3, Role::Label));
        buf.wrap_item(
            0,
            WidgetId::from_u64(9),
            Rect::new(0.0, 0.0, 5.0, 5.0),
            4,
            10,
        );
        assert_eq!(buf.spans, [2, 0, 0]);
        assert_eq!(buf.nodes[0].2.role, Role::ListItem);
        assert_eq!(buf.nodes[0].2.position, Some((4, 10)));

        let mut single = SemanticsBuf::default();
        single.push(node(2, Role::ListItem));
        single.wrap_item(
            0,
            WidgetId::from_u64(9),
            Rect::new(0.0, 0.0, 5.0, 5.0),
            1,
            3,
        );
        assert_eq!(single.len(), 1);
        assert_eq!(single.nodes[0].2.position, Some((1, 3)));
    }
}
