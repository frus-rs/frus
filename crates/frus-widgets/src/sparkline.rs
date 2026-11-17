// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`Sparkline`]: a small line chart with no axes, drawn in the room it is given — the
//! trend beside a figure on a dashboard card (milestone 811).

use frus_core::{Color, Path, Point, Rect, Role, Scene, SemanticsProperties};
use frus_layout::{Dimension, Style};

use crate::interaction::Status;
use crate::theme::Theme;
use crate::widget::Widget;

/// Default height of a sparkline, in logical pixels.
pub const SPARKLINE_HEIGHT: f32 = 32.0;

/// Theme defaults for [`Sparkline`] (`theme.widgets.sparkline`); each `None` falls through
/// to the default.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SparklineTheme {
    /// The line. Unset, `primary`.
    pub color: Option<Color>,
    /// The line's width. Unset, 2.
    pub stroke_width: Option<f32>,
    /// How strongly the area under the line is filled, in the line's colour; `0` for no
    /// fill. Unset, 0.12.
    pub fill_opacity: Option<f32>,
    /// The height. Unset, 32.
    pub height: Option<f32>,
}

/// A small line chart of `values`, scaled to fill its box: the lowest value at the bottom,
/// the highest at the top, the first at the start and the last at the end.
///
/// ```
/// use frus_widgets::Sparkline;
/// let _trend: Sparkline = Sparkline::new([3.0, 5.0, 4.0, 8.0, 7.0, 11.0]).height(40.0);
/// ```
///
/// It takes the width it is offered when it is given none. **Themed** through
/// [`SparklineTheme`] (`theme.widgets.sparkline`).
pub struct Sparkline<Msg = crate::callback::Callback> {
    values: Vec<f32>,
    color: Option<Color>,
    stroke_width: Option<f32>,
    fill_opacity: Option<f32>,
    width: Option<f32>,
    height: Option<f32>,
    label: Option<String>,
    _msg: std::marker::PhantomData<Msg>,
}

impl<Msg> Sparkline<Msg> {
    /// A sparkline of `values`, in order.
    pub fn new(values: impl IntoIterator<Item = f32>) -> Self {
        Self {
            values: values.into_iter().filter(|v| v.is_finite()).collect(),
            color: None,
            stroke_width: None,
            fill_opacity: None,
            width: None,
            height: None,
            label: None,
            _msg: std::marker::PhantomData,
        }
    }

    /// The line's colour, over the theme's.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// The line's width.
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = Some(width);
        self
    }

    /// How strongly the area under the line is filled; `0` for none.
    pub fn fill_opacity(mut self, opacity: f32) -> Self {
        self.fill_opacity = Some(opacity);
        self
    }

    /// A fixed width; unset, the width offered.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// The height.
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// What a screen reader hears; unset, the chart is decoration and is not read.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The points of the line inside `bounds`, inset by half the stroke so it is not cut.
    fn points(&self, bounds: Rect, inset: f32) -> Vec<Point> {
        let n = self.values.len();
        if n == 0 {
            return Vec::new();
        }
        let (lo, hi) = self
            .values
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(*v), hi.max(*v))
            });
        let span = hi - lo;
        let inner = Rect::new(
            bounds.x + inset,
            bounds.y + inset,
            (bounds.width - 2.0 * inset).max(0.0),
            (bounds.height - 2.0 * inset).max(0.0),
        );
        self.values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let fx = if n == 1 {
                    0.5
                } else {
                    i as f32 / (n - 1) as f32
                };
                // A flat series is drawn across the middle.
                let fy = if span > 0.0 { (v - lo) / span } else { 0.5 };
                Point::new(
                    inner.x + inner.width * fx,
                    inner.y + inner.height * (1.0 - fy),
                )
            })
            .collect()
    }
}

impl<Msg> Widget<Msg> for Sparkline<Msg> {
    fn style(&self) -> Style {
        self.style_themed(&Theme::default())
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        let t = theme.widgets.sparkline;
        Style {
            width: self.width.map_or(Dimension::Auto, Dimension::Length),
            height: Dimension::Length(self.height.or(t.height).unwrap_or(SPARKLINE_HEIGHT)),
            ..Default::default()
        }
    }

    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        if self.width.is_none() {
            crate::widget::FillAxes::WIDTH
        } else {
            crate::widget::FillAxes::NONE
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn semantics(&self) -> Option<SemanticsProperties> {
        self.label
            .as_ref()
            .map(|label| SemanticsProperties::new(Role::Image).label(label.clone()))
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let t = theme.widgets.sparkline;
        let color = self
            .color
            .or(t.color)
            .unwrap_or(theme.scheme.primary)
            .fade(status.opacity);
        let stroke = self.stroke_width.or(t.stroke_width).unwrap_or(2.0).max(0.0);
        let points = self.points(bounds, stroke * 0.5);
        let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) else {
            return;
        };
        let fill = self.fill_opacity.or(t.fill_opacity).unwrap_or(0.12);
        if fill > 0.0 && points.len() > 1 {
            let bottom = bounds.y + bounds.height;
            let mut area = Path::new().move_to(Point::new(first.x, bottom));
            for p in &points {
                area = area.line_to(*p);
            }
            area = area.line_to(Point::new(last.x, bottom)).close();
            scene.fill_path(&area, color.fade(fill));
        }
        if points.len() == 1 {
            scene.draw_rect(
                Rect::new(
                    first.x - stroke,
                    first.y - stroke,
                    stroke * 2.0,
                    stroke * 2.0,
                ),
                color,
                stroke,
                0.0,
                Color::TRANSPARENT,
            );
            return;
        }
        let mut line = Path::new().move_to(first);
        for p in &points[1..] {
            line = line.line_to(*p);
        }
        scene.stroke_path(&line, color, stroke);
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "Sparkline"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lowest_value_sits_at_the_bottom_and_the_highest_at_the_top() {
        let s = Sparkline::<()>::new([2.0, 10.0, 6.0]);
        let p = s.points(Rect::new(0.0, 0.0, 100.0, 40.0), 0.0);
        assert_eq!(p[0], Point::new(0.0, 40.0));
        assert_eq!(p[1], Point::new(50.0, 0.0));
        assert_eq!(p[2], Point::new(100.0, 20.0));
    }

    #[test]
    fn a_flat_series_runs_across_the_middle_and_nothing_is_not_drawn() {
        let flat = Sparkline::<()>::new([3.0, 3.0]);
        let p = flat.points(Rect::new(0.0, 0.0, 10.0, 10.0), 0.0);
        assert_eq!((p[0].y, p[1].y), (5.0, 5.0));
        let empty = Sparkline::<()>::new([f32::NAN]);
        let mut scene = Scene::new();
        Widget::<()>::paint(
            &empty,
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Status::default(),
            &Theme::light(),
            &mut scene,
        );
        assert!(scene.primitives().is_empty());
    }

    #[test]
    fn it_is_decoration_unless_labelled() {
        assert!(Widget::<()>::semantics(&Sparkline::<()>::new([1.0])).is_none());
        let named = Sparkline::<()>::new([1.0]).label("Visits, rising");
        assert_eq!(
            Widget::<()>::semantics(&named)
                .and_then(|s| s.label)
                .as_deref(),
            Some("Visits, rising")
        );
    }

    #[test]
    fn the_theme_sets_height_and_colour() {
        let mut theme = Theme::light();
        theme.widgets.sparkline.height = Some(50.0);
        let s = Sparkline::<()>::new([1.0, 2.0]);
        assert_eq!(
            Widget::<()>::style_themed(&s, &theme).height,
            Dimension::Length(50.0)
        );
        theme.widgets.sparkline.color = Some(Color::rgb(1.0, 0.0, 0.0));
        let mut scene = Scene::new();
        Widget::<()>::paint(
            &s.fill_opacity(0.0),
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Status::default(),
            &theme,
            &mut scene,
        );
        let stroked = scene.primitives().iter().find_map(|p| match p {
            frus_core::Primitive::Path {
                stroke: Some(stroke),
                ..
            } => Some(stroke.color),
            _ => None,
        });
        assert_eq!(stroked, Some(Color::rgb(1.0, 0.0, 0.0)));
    }
}
