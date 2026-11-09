// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `composite_widget!`: a builder that **is** the widget it assembles (milestone 786).
//!
//! The widgets of milestone 786 — the split button, the button group, the floating
//! action button menu, the toolbars, the side sheet, the full-screen dialog, the rich
//! tooltip and the contextual selection bar — are each put together from widgets the
//! framework already has, under the theme of the place they land in. They keep their
//! parts in a [`Lazy`](crate::lazy::Lazy) until the walk first asks, assemble them then,
//! and answer every question as what they assembled — the same arrangement `AppBar` and
//! `BottomNavigationBar` use, written once.

/// Implements `Widget<Msg>` for a `{ inner: Lazy<Parts<Msg>, Msg> }` builder by forwarding
/// every hook to the widget it assembles.
macro_rules! composite_widget {
    ($ty:ident) => {
        impl<Msg> $ty<Msg> {
            /// Transparent: it is what it assembles.
            fn restyle(&self, base: frus_layout::Style) -> frus_layout::Style {
                base
            }
        }

        $crate::transparent::forward_transparent!($ty {
            fn key(&self) -> Option<u64> {
                self.inner.key()
            }
            fn positioned(&self) -> Option<$crate::positioned::Positioning> {
                self.inner.positioned()
            }
            fn theme_override(
                &self,
                inherited: &$crate::theme::Theme,
            ) -> Option<Box<$crate::theme::Theme>> {
                self.inner.theme_override(inherited)
            }
            fn media_override(&self, inherited: $crate::MediaQuery) -> Option<$crate::MediaQuery> {
                self.inner.media_override(inherited)
            }
            fn scaffold_override(&self) -> Option<$crate::ScaffoldInfo> {
                self.inner.scaffold_override()
            }
            fn scroll_behavior_override(&self) -> Option<$crate::ScrollBehavior> {
                self.inner.scroll_behavior_override()
            }
            fn autofill_group(&self) -> bool {
                self.inner.autofill_group()
            }
            fn area_toolbar(
                &self,
                context: $crate::ToolbarContext,
            ) -> Option<Option<&dyn $crate::widget::Widget<Msg>>> {
                self.inner.area_toolbar(context)
            }
        });
    };
}

pub(crate) use composite_widget;

/// A **surface**: a filled, optionally elevated shape that holds a column or a row of
/// children and stops the presses nothing inside it claims — the panel of a toolbar, a
/// rich tooltip, a full-screen dialog. Every colour, corner and height is resolved by the
/// widget that builds it, from its own builders, its theme and the scheme.
pub(crate) struct Surface<Msg> {
    pub(crate) children: Vec<Box<dyn crate::widget::Widget<Msg>>>,
    pub(crate) style: frus_layout::Style,
    pub(crate) color: frus_core::Color,
    pub(crate) radius: frus_core::BorderRadius,
    pub(crate) elevation: f32,
    pub(crate) shadow: frus_core::Color,
    /// The axes it takes all the room offered on — a docked bar across the window.
    pub(crate) fill: crate::widget::FillAxes,
}

impl<Msg> Surface<Msg> {
    pub(crate) fn new(style: frus_layout::Style, color: frus_core::Color) -> Self {
        Self {
            children: Vec::new(),
            style,
            color,
            radius: frus_core::BorderRadius::ZERO,
            elevation: 0.0,
            shadow: frus_core::Color::TRANSPARENT,
            fill: crate::widget::FillAxes::NONE,
        }
    }

    pub(crate) fn child(mut self, child: impl crate::widget::Widget<Msg> + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub(crate) fn child_boxed(mut self, child: Box<dyn crate::widget::Widget<Msg>>) -> Self {
        self.children.push(child);
        self
    }

    pub(crate) fn radius(mut self, radius: frus_core::BorderRadius) -> Self {
        self.radius = radius;
        self
    }

    pub(crate) fn fill(mut self, fill: crate::widget::FillAxes) -> Self {
        self.fill = fill;
        self
    }

    pub(crate) fn elevation(mut self, elevation: f32, shadow: frus_core::Color) -> Self {
        self.elevation = elevation;
        self.shadow = shadow;
        self
    }
}

impl<Msg: Clone> crate::widget::Widget<Msg> for Surface<Msg> {
    fn style(&self) -> frus_layout::Style {
        self.style
    }

    fn children(&self) -> &[Box<dyn crate::widget::Widget<Msg>>] {
        &self.children
    }

    fn fill_axes(&self, _theme: &crate::theme::Theme) -> crate::widget::FillAxes {
        self.fill
    }

    fn paint(
        &self,
        bounds: frus_core::Rect,
        status: crate::interaction::Status,
        _theme: &crate::theme::Theme,
        scene: &mut frus_core::Scene,
    ) {
        let o = status.opacity;
        if self.elevation > 0.0 && self.shadow.a > 0.0 {
            frus_core::paint_elevation(
                scene,
                bounds,
                self.radius,
                self.elevation,
                self.shadow.fade(o),
            );
        }
        if self.color.a > 0.0 {
            scene.draw_shape(
                bounds,
                frus_core::ShapeBorder::rounded(self.radius),
                self.color.fade(o),
            );
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    /// A press on the surface and on nothing inside it stops here: a panel is not a hole
    /// onto the page under it.
    fn opaque(&self) -> bool {
        true
    }
}
