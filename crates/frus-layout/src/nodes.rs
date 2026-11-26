// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The node store under a [`crate::Layout`], and what it keeps from one frame to the next.
//!
//! The flexbox and grid algorithms are taffy's; the tree they run on is this one. Owning the
//! tree is what lets a layout **carry its measurements across frames** (milestone 853): each
//! node keeps taffy's per-node cache (the sizes it answered, under which constraints, and its
//! last placement), and a node of a new tree that is *the same subtree* as a node of the last
//! one — the same style, the same content, the same children, all the way down — starts with
//! that cache filled. Asked the same question, it answers without being laid out again, and a
//! box whose content changed is laid out along the path from it to the root, with its
//! untouched siblings answering from what they kept.

use std::collections::HashMap;

use taffy::{
    compute_block_layout, compute_cached_layout, compute_flexbox_layout, compute_grid_layout,
    compute_hidden_layout, compute_leaf_layout, compute_root_layout, round_layout, AvailableSpace,
    Cache, CacheTree, Display, Layout as Placed, LayoutBlockContainer, LayoutFlexboxContainer,
    LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, NodeId, RoundTree, RunMode,
    Size, Style, TraversePartialTree, TraverseTree,
};

/// One node: its style, its children, and what taffy computed for it.
pub(crate) struct Node<T> {
    pub(crate) style: Style,
    pub(crate) children: Vec<NodeId>,
    parent: Option<NodeId>,
    pub(crate) data: Option<T>,
    /// Whether a measurement answers for this leaf's size.
    measured: bool,
    /// What a measured leaf's answer depends on, beyond its style (milestone 853). `None` for
    /// a measurement nothing describes: such a node, and every node above it, is laid out
    /// afresh each time.
    content: Option<u64>,
    /// Where the node stands in the application's tree: how a node of the next frame finds
    /// the node it may be the same as. Only a hint; sameness is checked.
    identity: Option<u64>,
    cache: Cache,
    unrounded: Placed,
    placed: Placed,
    /// The node of the last frame this one is the same subtree as.
    same_as: Option<usize>,
    /// Its placement answer still comes from the last frame: answering with it has to put
    /// the last frame's boxes back below it too.
    placed_from_memo: bool,
}

impl<T> Node<T> {
    fn new(style: Style, data: Option<T>) -> Self {
        Self {
            style,
            children: Vec::new(),
            parent: None,
            data,
            measured: false,
            content: Some(0),
            identity: None,
            cache: Cache::new(),
            unrounded: Placed::with_order(0),
            placed: Placed::with_order(0),
            same_as: None,
            placed_from_memo: false,
        }
    }
}

/// What a computed layout leaves for the next frame's layout of the same root: every node's
/// style, content, children and cache, and where it was placed. Opaque; made by
/// [`crate::Layout::into_memo`] and handed back with [`crate::Layout::reuse`].
#[derive(Default)]
pub struct Memo {
    nodes: Vec<MemoNode>,
    by_identity: HashMap<u64, usize>,
    /// What the measurements of the last frame were taken under, said by the caller: a memo
    /// taken under another is not used.
    context: u64,
}

impl Memo {
    /// How many nodes it remembers.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether it remembers nothing.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

struct MemoNode {
    style: Style,
    children: Vec<usize>,
    measured: bool,
    content: Option<u64>,
    cache: Cache,
    unrounded: Placed,
}

/// The node store, and the measurement of the leaves that are measured.
pub(crate) struct Tree<T> {
    pub(crate) nodes: Vec<Node<T>>,
    memo: Option<Memo>,
    /// Whether the nodes have been matched against the memo yet: once, before the first
    /// computation.
    matched: bool,
    /// How many nodes were laid out (not answered from a cache) by the computations so far.
    pub(crate) computed: u32,
    /// How many nodes started with the last frame's cache.
    pub(crate) reused: u32,
}

impl<T> Tree<T> {
    pub(crate) fn new() -> Self {
        Self {
            nodes: Vec::new(),
            memo: None,
            matched: false,
            computed: 0,
            reused: 0,
        }
    }

    pub(crate) fn add(&mut self, style: Style, data: Option<T>) -> NodeId {
        self.nodes.push(Node::new(style, data));
        NodeId::from(self.nodes.len() - 1)
    }

    pub(crate) fn add_measured(
        &mut self,
        style: Style,
        data: Option<T>,
        content: Option<u64>,
    ) -> NodeId {
        let node = self.add(style, data);
        let n = &mut self.nodes[usize::from(node)];
        n.measured = true;
        n.content = content;
        node
    }

    pub(crate) fn add_container(&mut self, style: Style, children: &[NodeId]) -> NodeId {
        let node = self.add(style, None);
        for &child in children {
            self.nodes[usize::from(child)].parent = Some(node);
        }
        self.nodes[usize::from(node)].children = children.to_vec();
        node
    }

    pub(crate) fn style(&self, node: NodeId) -> &Style {
        &self.nodes[usize::from(node)].style
    }

    /// Replaces a node's style, and forgets what it and every node above it computed, as
    /// taffy's own tree does when a style changes.
    pub(crate) fn set_style(&mut self, node: NodeId, style: Style) {
        self.nodes[usize::from(node)].style = style;
        let mut at = Some(node);
        while let Some(node) = at {
            let n = &mut self.nodes[usize::from(node)];
            n.cache.clear();
            n.same_as = None;
            n.placed_from_memo = false;
            at = n.parent;
        }
    }

    pub(crate) fn set_identity(&mut self, node: NodeId, identity: u64) {
        self.nodes[usize::from(node)].identity = Some(identity);
    }

    pub(crate) fn reuse(&mut self, memo: Memo) {
        self.memo = Some(memo);
    }

    pub(crate) fn child_at(&self, node: NodeId, index: usize) -> NodeId {
        self.nodes[usize::from(node)].children[index]
    }

    pub(crate) fn count(&self, node: NodeId) -> usize {
        self.nodes[usize::from(node)].children.len()
    }

    pub(crate) fn data(&self, node: NodeId) -> Option<&T> {
        self.nodes[usize::from(node)].data.as_ref()
    }

    pub(crate) fn placed(&self, node: NodeId) -> &Placed {
        &self.nodes[usize::from(node)].placed
    }

    pub(crate) fn unrounded(&self, node: NodeId) -> &Placed {
        &self.nodes[usize::from(node)].unrounded
    }

    /// Lays the tree out from `root`, measured leaves answering through `measure`, then rounds
    /// every box to whole pixels — what taffy's own tree does.
    pub(crate) fn compute<M>(
        &mut self,
        root: NodeId,
        available: Size<AvailableSpace>,
        measure: M,
        context: u64,
    ) where
        M: FnMut(Size<Option<f32>>, Size<AvailableSpace>, NodeId) -> Size<f32>,
    {
        if !self.matched {
            self.matched = true;
            self.match_memo(root, context);
        }
        let mut view = View {
            tree: self,
            measure,
        };
        compute_root_layout(&mut view, root, available);
        round_layout(&mut view, root);
    }

    /// Finds, for each node, the node of the last frame it is the same subtree as, and starts
    /// it with that node's cache. Children first, so that a node can only be the same as one
    /// whose children are the same as its own.
    fn match_memo(&mut self, root: NodeId, context: u64) {
        let Some(memo) = self.memo.take() else {
            return;
        };
        if memo.context == context && !memo.nodes.is_empty() {
            self.match_node(root, &memo);
        }
        self.memo = Some(memo);
    }

    fn match_node(&mut self, node: NodeId, memo: &Memo) -> Option<usize> {
        let index = usize::from(node);
        let count = self.nodes[index].children.len();
        let mut children_same = true;
        let mut matched = Vec::with_capacity(count);
        for i in 0..count {
            let child = self.nodes[index].children[i];
            match self.match_node(child, memo) {
                Some(old) => matched.push(old),
                None => children_same = false,
            }
        }
        let n = &self.nodes[index];
        let content = n.content?;
        if !children_same {
            return None;
        }
        let old_index = *memo.by_identity.get(&n.identity?)?;
        let old = &memo.nodes[old_index];
        let same = old.content == Some(content)
            && old.measured == n.measured
            && old.children == matched
            && old.style == n.style;
        if !same {
            return None;
        }
        let n = &mut self.nodes[index];
        n.cache = old.cache.clone();
        n.same_as = Some(old_index);
        n.placed_from_memo = true;
        self.reused += 1;
        Some(old_index)
    }

    /// Puts back, below `node`, where the last frame placed each box: `node` answered from the
    /// last frame's cache, which placed everything below it the same way.
    fn restore_below(&mut self, node: NodeId) {
        let Some(memo) = self.memo.as_ref() else {
            return;
        };
        let mut stack: Vec<NodeId> = self.nodes[usize::from(node)].children.clone();
        while let Some(at) = stack.pop() {
            let n = &mut self.nodes[usize::from(at)];
            if let Some(old) = n.same_as {
                n.unrounded = memo.nodes[old].unrounded;
            }
            stack.extend(n.children.iter().copied());
        }
    }

    /// What this layout leaves for the next frame's.
    pub(crate) fn into_memo(self, root: NodeId, context: u64) -> Memo {
        // Only what can be reached from the root: a node built and left out (a measurement's
        // own tree is not one of these, but a style replaced on the way is) is not a box.
        let mut order = Vec::with_capacity(self.nodes.len());
        let mut renumber = vec![usize::MAX; self.nodes.len()];
        let mut stack = vec![root];
        while let Some(at) = stack.pop() {
            let index = usize::from(at);
            renumber[index] = order.len();
            order.push(index);
            stack.extend(self.nodes[index].children.iter().rev().copied());
        }
        let mut slots: Vec<Option<Node<T>>> = self.nodes.into_iter().map(Some).collect();
        let mut nodes = Vec::with_capacity(order.len());
        let mut by_identity = HashMap::with_capacity(order.len());
        for (new_index, &index) in order.iter().enumerate() {
            let n = slots[index].take().expect("each node once");
            if let Some(identity) = n.identity {
                by_identity.insert(identity, new_index);
            }
            nodes.push(MemoNode {
                style: n.style,
                children: n
                    .children
                    .iter()
                    .map(|&c| renumber[usize::from(c)])
                    .collect(),
                measured: n.measured,
                content: n.content,
                cache: n.cache,
                unrounded: n.unrounded,
            });
        }
        Memo {
            nodes,
            by_identity,
            context,
        }
    }
}

/// The tree as taffy's algorithms see it during one computation.
struct View<'t, T, M> {
    tree: &'t mut Tree<T>,
    measure: M,
}

/// The children of a node, as taffy walks them.
pub(crate) struct Children<'a>(std::iter::Copied<std::slice::Iter<'a, NodeId>>);

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        self.0.next()
    }
}

impl<T, M> TraversePartialTree for View<'_, T, M> {
    type ChildIter<'a>
        = Children<'a>
    where
        Self: 'a;

    fn child_ids(&self, node: NodeId) -> Self::ChildIter<'_> {
        Children(self.tree.nodes[usize::from(node)].children.iter().copied())
    }

    fn child_count(&self, node: NodeId) -> usize {
        self.tree.nodes[usize::from(node)].children.len()
    }

    fn get_child_id(&self, node: NodeId, index: usize) -> NodeId {
        self.tree.nodes[usize::from(node)].children[index]
    }
}

impl<T, M> TraverseTree for View<'_, T, M> {}

impl<T, M> LayoutPartialTree for View<'_, T, M>
where
    M: FnMut(Size<Option<f32>>, Size<AvailableSpace>, NodeId) -> Size<f32>,
{
    type CoreContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_core_container_style(&self, node: NodeId) -> Self::CoreContainerStyle<'_> {
        &self.tree.nodes[usize::from(node)].style
    }

    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Placed) {
        self.tree.nodes[usize::from(node)].unrounded = *layout;
    }

    fn compute_child_layout(&mut self, node: NodeId, inputs: LayoutInput) -> LayoutOutput {
        // Taffy's own dispatch, kept as it is in taffy's tree.
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            return compute_hidden_layout(self, node);
        }
        // A placement answered from the last frame puts the last frame's boxes back below it:
        // the algorithms that would have placed them are not run.
        let from_memo = inputs.run_mode == RunMode::PerformLayout
            && self.tree.nodes[usize::from(node)].placed_from_memo
            && self.tree.nodes[usize::from(node)]
                .cache
                .get(
                    inputs.known_dimensions,
                    inputs.available_space,
                    inputs.run_mode,
                )
                .is_some();
        if from_memo {
            self.tree.restore_below(node);
        }
        compute_cached_layout(self, node, inputs, |view, node, inputs| {
            view.tree.computed += 1;
            let index = usize::from(node);
            let display = view.tree.nodes[index].style.display;
            let has_children = !view.tree.nodes[index].children.is_empty();
            match (display, has_children) {
                (Display::None, _) => compute_hidden_layout(view, node),
                (Display::Block, true) => compute_block_layout(view, node, inputs),
                (Display::Flex, true) => compute_flexbox_layout(view, node, inputs),
                (Display::Grid, true) => compute_grid_layout(view, node, inputs),
                (_, false) => {
                    let measured = view.tree.nodes[index].measured;
                    let style = &view.tree.nodes[index].style;
                    let measure = &mut view.measure;
                    compute_leaf_layout(inputs, style, |known, available| {
                        if measured {
                            measure(known, available, node)
                        } else {
                            Size::ZERO
                        }
                    })
                }
            }
        })
    }
}

impl<T, M> CacheTree for View<'_, T, M> {
    fn cache_get(
        &self,
        node: NodeId,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        run_mode: RunMode,
    ) -> Option<LayoutOutput> {
        self.tree.nodes[usize::from(node)]
            .cache
            .get(known_dimensions, available_space, run_mode)
    }

    fn cache_store(
        &mut self,
        node: NodeId,
        known_dimensions: Size<Option<f32>>,
        available_space: Size<AvailableSpace>,
        run_mode: RunMode,
        layout_output: LayoutOutput,
    ) {
        let n = &mut self.tree.nodes[usize::from(node)];
        if run_mode == RunMode::PerformLayout {
            // Placed in this frame: what is below it was placed in this frame too.
            n.placed_from_memo = false;
        }
        n.cache
            .store(known_dimensions, available_space, run_mode, layout_output);
    }

    fn cache_clear(&mut self, node: NodeId) {
        let n = &mut self.tree.nodes[usize::from(node)];
        n.cache.clear();
        n.placed_from_memo = false;
    }
}

impl<T, M> LayoutFlexboxContainer for View<'_, T, M>
where
    M: FnMut(Size<Option<f32>>, Size<AvailableSpace>, NodeId) -> Size<f32>,
{
    type FlexboxContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node: NodeId) -> Self::FlexboxContainerStyle<'_> {
        &self.tree.nodes[usize::from(node)].style
    }

    fn get_flexbox_child_style(&self, child: NodeId) -> Self::FlexboxItemStyle<'_> {
        &self.tree.nodes[usize::from(child)].style
    }
}

impl<T, M> LayoutGridContainer for View<'_, T, M>
where
    M: FnMut(Size<Option<f32>>, Size<AvailableSpace>, NodeId) -> Size<f32>,
{
    type GridContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_grid_container_style(&self, node: NodeId) -> Self::GridContainerStyle<'_> {
        &self.tree.nodes[usize::from(node)].style
    }

    fn get_grid_child_style(&self, child: NodeId) -> Self::GridItemStyle<'_> {
        &self.tree.nodes[usize::from(child)].style
    }
}

impl<T, M> LayoutBlockContainer for View<'_, T, M>
where
    M: FnMut(Size<Option<f32>>, Size<AvailableSpace>, NodeId) -> Size<f32>,
{
    type BlockContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_block_container_style(&self, node: NodeId) -> Self::BlockContainerStyle<'_> {
        &self.tree.nodes[usize::from(node)].style
    }

    fn get_block_child_style(&self, child: NodeId) -> Self::BlockItemStyle<'_> {
        &self.tree.nodes[usize::from(child)].style
    }
}

impl<T, M> RoundTree for View<'_, T, M> {
    fn get_unrounded_layout(&self, node: NodeId) -> &Placed {
        &self.tree.nodes[usize::from(node)].unrounded
    }

    fn set_final_layout(&mut self, node: NodeId, layout: &Placed) {
        self.tree.nodes[usize::from(node)].placed = *layout;
    }
}
