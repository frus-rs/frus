// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The box of a wrapper that **nests** its child and takes the child's box as its own.
//!
//! A builder that composes its child late ([`crate::ThemeBuilder`]), a semantics or an
//! overlay holder around an anchor: each is a node of its own in the layout, with the
//! child under it, and each wants to sit exactly where the child would have sat. So it
//! takes the child's style. Taken whole, though, the **box properties** come out twice:
//! the wrapper insets by the padding and the child insets by it again inside; the
//! wrapper leaves the margin around itself and the child leaves it again around itself.
//! An 8-pixel padding under a builder came out at 16 (milestone 817).
//!
//! The split is the one a box already makes. **Around** the box — size, flex, margin —
//! is the wrapper's, since the wrapper is what its parent places. **Inside** the box —
//! padding, and how the children are laid — is the child's, since the child is what
//! holds them. So the wrapper takes [`nested_box`] of the child's style, and holds the
//! child through [`Unmargined`], which is the child in every respect but its margin.

use std::rc::Rc;

use frus_core::Insets;
use frus_layout::Style;

use crate::widget::Widget;

/// The style of a wrapper that nests a child with style `child`: the child's own, less
/// the padding, which the child applies inside itself.
///
/// The margin stays: the wrapper is what its parent places, so the space around it is
/// the wrapper's to leave. Pair it with [`Unmargined`] around the child, or the margin is
/// left twice.
pub(crate) fn nested_box(child: Style) -> Style {
    Style {
        padding: Insets::ZERO,
        ..child
    }
}

/// The child of a wrapper that took its box ([`nested_box`]): the child in every
/// respect, its margin aside — the wrapper has already left it.
///
/// The child is shared with the wrapper, which still needs the child's own style, margin
/// included, to take its box from: see [`Unmargined::split`].
pub(crate) struct Unmargined<Msg> {
    inner: Rc<dyn Widget<Msg>>,
}

impl<Msg: 'static> Unmargined<Msg> {
    /// Splits `child` into what a wrapper that takes its box keeps: the child itself, to
    /// ask for its style, and the node to hand out as its child.
    pub(crate) fn split(
        child: Box<dyn Widget<Msg>>,
    ) -> (Rc<dyn Widget<Msg>>, Box<dyn Widget<Msg>>) {
        let child: Rc<dyn Widget<Msg>> = Rc::from(child);
        let node = Box::new(Self {
            inner: child.clone(),
        });
        (child, node)
    }
}

impl<Msg> Unmargined<Msg> {
    /// The margin, and only the margin, is the wrapper's.
    fn restyle(&self, base: Style) -> Style {
        Style {
            margin: Insets::ZERO,
            ..base
        }
    }
}

crate::transparent::forward_transparent!(Unmargined {
    /// Forwarded: the child's identity is the child's.
    fn key(&self) -> Option<u64> {
        self.inner.key()
    }
    fn positioned(&self) -> Option<crate::positioned::Positioning> {
        self.inner.positioned()
    }
    fn theme_override(
        &self,
        inherited: &crate::theme::Theme,
    ) -> Option<Box<crate::theme::Theme>> {
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
