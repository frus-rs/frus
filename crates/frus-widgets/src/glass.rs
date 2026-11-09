// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`GlassSurface`]: a **frosted-glass panel** — what is behind it, blurred, under a tint
//! and a thin rim, in the panel's rounded shape.
//!
//! It is three existing pieces in one, so that the panel is one call and every part of it
//! answers to the theme: a rounded clip (the blur's region is the clip, and a backdrop with
//! no clip blurs the whole window), a [`BackdropFilter`](crate::BackdropFilter) (the renderer
//! copies what has been drawn so far, blurs it on the GPU and draws it back inside the
//! clip), and a container with the tint, the rim and the content.
//!
//! **Cost.** Each glass panel is one copy and one two-pass blur of the frame drawn so far.
//! Several panels that do not overlap can share one blur with
//! [`grouped`](GlassSurface::grouped) inside a [`BackdropGroup`](crate::BackdropGroup).
//!
//! **Colour.** The tint is blended over the blur in linear light, as every translucent
//! colour in frus is: a 60 % tint is visibly stronger than the same token blended in sRGB
//! would be. The defaults were chosen against the rendered pixel, not the token.

use frus_core::{BorderRadius, Color, Rect, Scene};
use frus_layout::{Dimension, Style};

use crate::interaction::Status;
use crate::theme::Theme;
use crate::widget::Widget;

/// The blur's spread unless told, in logical pixels.
pub const GLASS_BLUR_SIGMA: f32 = 24.0;

/// Theme defaults for [`GlassSurface`]; each `None` falls through to the default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GlassTheme {
    /// The tint over the blur. Unset, `surface` at 55 % (light) or 45 % (dark).
    pub tint: Option<Color>,
    /// The rim. Unset, white at 45 % in a light theme and 14 % in a dark one.
    pub border: Option<Color>,
    /// The rim's width. Unset, one.
    pub border_width: Option<f32>,
    /// The blur's spread. Unset, twenty-four.
    pub blur_sigma: Option<f32>,
    /// The corner radius. Unset, twenty-eight (the extra-large shape).
    pub radius: Option<f32>,
}

/// A **frosted-glass panel**: the content behind it blurred, tinted and rimmed, clipped to
/// a rounded rectangle.
///
/// ```
/// use frus_widgets::{GlassSurface, Text};
/// let _panel: GlassSurface<()> = GlassSurface::new(Text::new("Now playing")).padding(20.0);
/// ```
///
/// **Themed** through [`GlassTheme`] (`theme.widgets.glass`); every value has a builder
/// that outranks the theme.
pub struct GlassSurface<Msg = crate::callback::Callback> {
    child_cell: std::cell::RefCell<Option<Box<dyn Widget<Msg>>>>,
    tint: Option<Color>,
    border: Option<Color>,
    border_width: Option<f32>,
    sigma: Option<f32>,
    radius: Option<BorderRadius>,
    padding: f32,
    width: Dimension,
    height: Dimension,
    grouped: bool,
    enabled: bool,
    children: std::cell::OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg: Clone + 'static> GlassSurface<Msg> {
    /// A glass panel around `child`.
    pub fn new(child: impl Widget<Msg> + 'static) -> Self {
        Self {
            child_cell: std::cell::RefCell::new(Some(Box::new(child))),
            tint: None,
            border: None,
            border_width: None,
            sigma: None,
            radius: None,
            padding: 0.0,
            width: Dimension::Auto,
            height: Dimension::Auto,
            grouped: false,
            enabled: true,
            children: std::cell::OnceCell::new(),
        }
    }

    /// The tint laid over the blur.
    ///
    /// ```
    /// # use frus_widgets::{Color, GlassSurface, Text};
    /// let _g: GlassSurface<()> = GlassSurface::new(Text::new("A")).tint(Color::rgba(0.1, 0.2, 0.5, 0.3));
    /// ```
    pub fn tint(mut self, color: Color) -> Self {
        self.tint = Some(color);
        self
    }

    /// The rim's colour; transparent for none.
    ///
    /// ```
    /// # use frus_widgets::{Color, GlassSurface, Text};
    /// let _g: GlassSurface<()> = GlassSurface::new(Text::new("A")).border(Color::TRANSPARENT);
    /// ```
    pub fn border(mut self, color: Color) -> Self {
        self.border = Some(color);
        self
    }

    /// The rim's width.
    ///
    /// ```
    /// # use frus_widgets::{GlassSurface, Text};
    /// let _g: GlassSurface<()> = GlassSurface::new(Text::new("A")).border_width(2.0);
    /// ```
    pub fn border_width(mut self, width: f32) -> Self {
        self.border_width = Some(width.max(0.0));
        self
    }

    /// How far the blur spreads, in logical pixels; zero still frosts nothing but costs the
    /// copy, so turn the panel off with [`enabled`](Self::enabled) instead.
    ///
    /// ```
    /// # use frus_widgets::{GlassSurface, Text};
    /// let _g: GlassSurface<()> = GlassSurface::new(Text::new("A")).blur(12.0);
    /// ```
    pub fn blur(mut self, sigma: f32) -> Self {
        self.sigma = Some(sigma.max(0.0));
        self
    }

    /// The corner radius.
    ///
    /// ```
    /// # use frus_widgets::{GlassSurface, Text};
    /// let _g: GlassSurface<()> = GlassSurface::new(Text::new("A")).radius(16.0);
    /// ```
    pub fn radius(mut self, radius: impl Into<BorderRadius>) -> Self {
        self.radius = Some(radius.into());
        self
    }

    /// The room between the rim and the content.
    ///
    /// ```
    /// # use frus_widgets::{GlassSurface, Text};
    /// let _g: GlassSurface<()> = GlassSurface::new(Text::new("A")).padding(16.0);
    /// ```
    pub fn padding(mut self, padding: f32) -> Self {
        self.padding = padding.max(0.0);
        self
    }

    /// The panel's width; its content's unless told.
    ///
    /// ```
    /// # use frus_widgets::{GlassSurface, Text};
    /// let _g: GlassSurface<()> = GlassSurface::new(Text::new("A")).width(280.0);
    /// ```
    pub fn width(mut self, width: f32) -> Self {
        self.width = Dimension::Length(width);
        self
    }

    /// The panel's height; its content's unless told.
    ///
    /// ```
    /// # use frus_widgets::{GlassSurface, Text};
    /// let _g: GlassSurface<()> = GlassSurface::new(Text::new("A")).height(120.0);
    /// ```
    pub fn height(mut self, height: f32) -> Self {
        self.height = Dimension::Length(height);
        self
    }

    /// Shares its blur with the other grouped panels of the nearest
    /// [`BackdropGroup`](crate::BackdropGroup). Panels that overlap must not be grouped.
    ///
    /// ```
    /// # use frus_widgets::{GlassSurface, Text};
    /// let _g: GlassSurface<()> = GlassSurface::new(Text::new("A")).grouped();
    /// ```
    pub fn grouped(mut self) -> Self {
        self.grouped = true;
        self
    }

    /// Turns the blur off (and its cost with it): the tint and the rim stay, as a plain
    /// translucent panel.
    ///
    /// ```
    /// # use frus_widgets::{GlassSurface, Text};
    /// let _g: GlassSurface<()> = GlassSurface::new(Text::new("A")).enabled(false);
    /// ```
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn make(&self) -> Box<dyn Widget<Msg>> {
        // The child is moved into the tree once: `children` is only asked for after the
        // builder is finished, and its answer is kept.
        let (tint, border, border_width, sigma, radius) = (
            self.tint,
            self.border,
            self.border_width,
            self.sigma,
            self.radius,
        );
        let (padding, width, height, grouped, enabled) = (
            self.padding,
            self.width,
            self.height,
            self.grouped,
            self.enabled,
        );
        let content = self.child_cell.borrow_mut().take();
        Box::new(crate::themebuilder::ThemeBuilder::boxed(move |theme| {
            let r = glass_resolve(theme, tint, border, border_width, sigma, radius);
            let mut panel = crate::Container::new()
                .color(r.tint)
                .radius(r.radius)
                .border(r.border_width, r.border)
                .padding(padding);
            if let Dimension::Length(w) = width {
                panel = panel.width(w);
            }
            if let Dimension::Length(h) = height {
                panel = panel.height(h);
            }
            if let Some(content) = content {
                panel = panel.child(Boxed(content));
            }
            let mut blur = crate::BackdropFilter::blur(r.sigma).enabled(enabled);
            if grouped {
                blur = blur.grouped();
            }
            Box::new(crate::ClipRRect::rounded(r.radius).child(blur.child(panel)))
                as Box<dyn Widget<Msg>>
        }))
    }
}

/// A panel's look, resolved against the theme.
#[derive(Clone, Copy, Debug, PartialEq)]
struct GlassLook {
    tint: Color,
    border: Color,
    border_width: f32,
    sigma: f32,
    radius: BorderRadius,
}

fn glass_resolve(
    theme: &Theme,
    tint: Option<Color>,
    border: Option<Color>,
    border_width: Option<f32>,
    sigma: Option<f32>,
    radius: Option<BorderRadius>,
) -> GlassLook {
    let t = theme.widgets.glass;
    let dark = theme.scheme.brightness == crate::Brightness::Dark;
    GlassLook {
        tint: tint
            .or(t.tint)
            .unwrap_or(theme.scheme.surface.fade(if dark { 0.45 } else { 0.55 })),
        border: border
            .or(t.border)
            .unwrap_or(Color::WHITE.fade(if dark { 0.14 } else { 0.45 })),
        border_width: border_width.or(t.border_width).unwrap_or(1.0),
        sigma: sigma.or(t.blur_sigma).unwrap_or(GLASS_BLUR_SIGMA),
        radius: radius
            .or(t.radius.map(BorderRadius::uniform))
            .unwrap_or(BorderRadius::uniform(28.0)),
    }
}

/// A boxed widget as a widget, with the child's box.
struct Boxed<Msg>(Box<dyn Widget<Msg>>);

impl<Msg: 'static> Widget<Msg> for Boxed<Msg> {
    fn style(&self) -> Style {
        self.0.style()
    }
    fn style_themed(&self, theme: &Theme) -> Style {
        self.0.style_themed(theme)
    }
    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        std::slice::from_ref(&self.0)
    }
    fn paint(&self, _: Rect, _: Status, _: &Theme, _: &mut Scene) {}
    fn on_click(&self) -> Option<Msg> {
        None
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for GlassSurface<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.children.get_or_init(|| vec![self.make()])
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "GlassSurface"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Container, Runtime, Size, Text};
    use frus_core::Primitive;

    fn has_backdrop(prims: &[Primitive]) -> bool {
        prims.iter().any(|p| match p {
            Primitive::Layer {
                filter, primitives, ..
            } => filter.backdrop.is_some() || has_backdrop(primitives),
            _ => false,
        })
    }

    #[test]
    fn the_defaults_follow_the_theme_s_brightness() {
        let light = glass_resolve(&Theme::light(), None, None, None, None, None);
        let dark = glass_resolve(&Theme::dark(), None, None, None, None, None);
        assert_eq!(light.tint, Theme::light().scheme.surface.fade(0.55));
        assert_eq!(dark.tint, Theme::dark().scheme.surface.fade(0.45));
        assert_eq!(light.sigma, GLASS_BLUR_SIGMA);
        assert_eq!(light.radius, BorderRadius::uniform(28.0));
        let mut theme = Theme::light();
        theme.widgets.glass.tint = Some(Color::rgba(1.0, 0.0, 0.0, 0.5));
        theme.widgets.glass.blur_sigma = Some(8.0);
        let told = glass_resolve(&theme, None, Some(Color::BLACK), None, Some(4.0), None);
        assert_eq!(told.tint, Color::rgba(1.0, 0.0, 0.0, 0.5));
        assert_eq!((told.border, told.sigma), (Color::BLACK, 4.0));
    }

    #[test]
    fn a_panel_is_a_backdrop_blur_inside_its_clip() {
        let panel = Container::<()>::new()
            .color(Color::rgb(0.2, 0.4, 0.8))
            .child(
                GlassSurface::new(Text::new("Glass"))
                    .width(200.0)
                    .height(100.0)
                    .blur(10.0),
            );
        let ui = build_ui(
            &panel,
            Size::new(300.0, 200.0),
            &Runtime::default(),
            &Theme::light(),
        );
        assert!(has_backdrop(ui.scene().primitives()), "a backdrop layer");
        // Off, there is no backdrop at all.
        let off =
            Container::<()>::new().child(GlassSurface::new(Text::new("Glass")).enabled(false));
        let ui = build_ui(
            &off,
            Size::new(300.0, 200.0),
            &Runtime::default(),
            &Theme::light(),
        );
        assert!(!has_backdrop(ui.scene().primitives()));
    }
}
