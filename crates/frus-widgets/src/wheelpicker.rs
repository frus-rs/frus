// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`WheelPicker`]: one or more spinning wheels side by side behind one selection band —
//! a quantity, a time of day, a date (milestone 810).
//!
//! Each column is a [`ListWheel`](crate::ListWheel): the cylinder, the fling, the settling
//! on a row and the reader's "half past three, 15 of 60" are its. The picker adds what a
//! picker is made of besides: the band across the middle that says *this row is the
//! answer*, the words in each row, the chosen row drawn stronger than the rest, wheels that
//! go round, and a tick under the finger as each value passes the middle.

use std::rc::Rc;

use frus_core::{BorderRadius, Color, FontWeight, TextAlign, TextStyle};
use frus_layout::{Align, Justify};

use crate::lazy::Lazy;
use crate::stack::Stack;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::wheel::ListWheel;
use crate::widget::Widget;
use crate::{Container, Flex, Positioned, Text};

/// The height of one row, unless the picker or its theme says otherwise.
pub const WHEEL_ITEM_EXTENT: f32 = 40.0;

/// How many rows a picker shows, unless it is given a height: the chosen one and two either
/// side.
pub const WHEEL_VISIBLE_ROWS: f32 = 5.0;

/// Defaults for [`WheelPicker`]. Each field unset is the picker's own default, from the
/// scheme and the type scale.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelPickerTheme {
    /// The height of one row. Unset, [`WHEEL_ITEM_EXTENT`].
    pub item_extent: Option<f32>,
    /// The height of the whole picker. Unset, [`WHEEL_VISIBLE_ROWS`] rows.
    pub height: Option<f32>,
    /// The band across the middle row. Unset, the scheme's `surface_container_highest`.
    pub band_color: Option<Color>,
    /// The band's corners. Unset, 8.
    pub band_radius: Option<f32>,
    /// A row's type. Unset, the type scale's `title_medium`.
    pub text_style: Option<TextStyle>,
    /// A row's colour. Unset, the scheme's `on_surface_variant`.
    pub text_color: Option<Color>,
    /// The chosen row's colour. Unset, the scheme's `on_surface`.
    pub selected_text_color: Option<Color>,
    /// The chosen row's weight. Unset, semi-bold.
    pub selected_weight: Option<FontWeight>,
    /// The cylinder's diameter as a multiple of the picker's height. Unset, the wheel's.
    pub diameter_ratio: Option<f32>,
    /// A tick under the finger as each value passes the middle. Unset, on.
    pub haptic_ticks: Option<bool>,
}

type Label = Rc<dyn Fn(usize) -> String>;
type OnSelected<Msg> = Rc<dyn Fn(usize) -> Msg>;

/// One wheel of a [`WheelPicker`]: its values, the one chosen, and what to send when another
/// is.
///
/// ```
/// use frus_widgets::WheelColumn;
///
/// #[derive(Clone)]
/// enum Msg { Hour(usize) }
///
/// let _hours: WheelColumn<Msg> = WheelColumn::new(24, |h| format!("{h:02}"), Msg::Hour)
///     .selected(9)
///     .looping(true)
///     .semantic_label(|h| format!("{h} hours"));
/// ```
pub struct WheelColumn<Msg = crate::callback::Callback> {
    count: usize,
    label: Label,
    spoken: Option<Label>,
    on_selected: OnSelected<Msg>,
    selected: usize,
    looping: bool,
    width: Option<f32>,
    flex: f32,
    align: TextAlign,
}

impl<Msg: 'static> WheelColumn<Msg> {
    /// A wheel of `count` values, each written `label(i)`, sending `on_selected(i)` when
    /// value `i` comes to the middle.
    pub fn new<R: crate::callback::IntoMsg<Msg>>(
        count: usize,
        label: impl Fn(usize) -> String + 'static,
        on_selected: impl Fn(usize) -> R + 'static,
    ) -> Self {
        Self {
            count,
            label: Rc::new(label),
            spoken: None,
            on_selected: Rc::new(crate::callback::handler1(on_selected)),
            selected: 0,
            looping: false,
            width: None,
            flex: 1.0,
            align: TextAlign::Center,
        }
    }

    /// A wheel of the whole numbers `range`, written as they are, sending the **number**
    /// (not its index) when it comes to the middle.
    ///
    /// ```
    /// use frus_widgets::WheelColumn;
    ///
    /// #[derive(Clone)]
    /// enum Msg { Year(i32) }
    ///
    /// let _years: WheelColumn<Msg> = WheelColumn::numbers(1950..=2030, 1990, Msg::Year);
    /// ```
    pub fn numbers<R: crate::callback::IntoMsg<Msg>>(
        range: std::ops::RangeInclusive<i32>,
        selected: i32,
        on_selected: impl Fn(i32) -> R + 'static,
    ) -> Self {
        let start = *range.start();
        let count = (range.end() - start + 1).max(0) as usize;
        Self::new(
            count,
            move |i| (start + i as i32).to_string(),
            move |i| on_selected(start + i as i32),
        )
        .selected((selected - start).max(0) as usize)
    }

    /// The value in the middle.
    #[must_use]
    pub fn selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    /// The values go round: after the last comes the first (minutes, hours, months).
    #[must_use]
    pub fn looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    /// What a screen reader says for value `i`, when the written label is not enough —
    /// "9 hours" for a row that only reads "09". Unset, the label.
    #[must_use]
    pub fn semantic_label(mut self, label: impl Fn(usize) -> String + 'static) -> Self {
        self.spoken = Some(Rc::new(label));
        self
    }

    /// A fixed width, rather than a share of the picker's.
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// The share of the picker's width this column takes, against the others' (1 each by
    /// default).
    #[must_use]
    pub fn flex(mut self, flex: f32) -> Self {
        self.flex = flex.max(0.0);
        self
    }

    /// Where the label sits across the row. Unset, centred; a date's day column reads better
    /// against its month.
    #[must_use]
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }
}

struct Parts<Msg> {
    columns: Vec<WheelColumn<Msg>>,
    item_extent: Option<f32>,
    height: Option<f32>,
    width: Option<f32>,
    band_color: Option<Color>,
    band_radius: Option<f32>,
    text_style: Option<TextStyle>,
    text_color: Option<Color>,
    selected_text_color: Option<Color>,
    selected_weight: Option<FontWeight>,
    diameter_ratio: Option<f32>,
    haptic_ticks: Option<bool>,
    enabled: bool,
}

/// **Spinning wheels** for picking values — one column, or several side by side behind one
/// band, the way a time or a date is picked on a phone.
///
/// ```
/// use frus_widgets::{WheelColumn, WheelPicker};
///
/// #[derive(Clone)]
/// enum Msg { Hour(usize), Minute(usize) }
///
/// let (hour, minute) = (9, 30);
/// let _time: WheelPicker<Msg> = WheelPicker::new()
///     .column(WheelColumn::new(24, |h| format!("{h:02}"), Msg::Hour).selected(hour).looping(true))
///     .column(WheelColumn::new(60, |m| format!("{m:02}"), Msg::Minute).selected(minute).looping(true));
/// ```
///
/// - A spin **settles on a value**, never between two, and a fling carries on with the
///   scroll physics of the platform before it does.
/// - A **looping** column goes round: past the last value comes the first.
/// - A **tick** is felt as each value passes the middle, where the platform has haptics
///   ([`haptic_ticks`](WheelPicker::haptic_ticks), on by default; the theme's
///   [`enable_feedback`](crate::Theme::enable_feedback) turns every widget's off).
/// - Each column is **controlled**: it rests on the value it is given and sends its message
///   as another comes to the middle — while the wheel still turns, which is when a reader
///   would say it changed.
/// - A screen reader hears each column as an adjustable control saying its value.
///
/// Columns are laid out in the reading direction: in a right-to-left interface the first
/// column is on the right.
pub struct WheelPicker<Msg = crate::callback::Callback> {
    inner: Lazy<Parts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> Default for WheelPicker<Msg> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Msg: Clone + 'static> WheelPicker<Msg> {
    /// A picker with no columns yet.
    pub fn new() -> Self {
        Self {
            inner: Lazy::new(
                Parts {
                    columns: Vec::new(),
                    item_extent: None,
                    height: None,
                    width: None,
                    band_color: None,
                    band_radius: None,
                    text_style: None,
                    text_color: None,
                    selected_text_color: None,
                    selected_weight: None,
                    diameter_ratio: None,
                    haptic_ticks: None,
                    enabled: true,
                },
                build::<Msg>,
            ),
        }
    }

    fn p(&mut self) -> &mut Parts<Msg> {
        self.inner.parts_mut()
    }

    /// Adds a column.
    #[must_use]
    pub fn column(mut self, column: WheelColumn<Msg>) -> Self {
        self.p().columns.push(column);
        self
    }

    /// The height of one row, over the theme's and [`WHEEL_ITEM_EXTENT`].
    #[must_use]
    pub fn item_extent(mut self, extent: f32) -> Self {
        self.p().item_extent = Some(extent.max(1.0));
        self
    }

    /// The picker's height, over the theme's and [`WHEEL_VISIBLE_ROWS`] rows.
    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.p().height = Some(height);
        self
    }

    /// A fixed width. Unset, the room on offer.
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.p().width = Some(width);
        self
    }

    /// The band across the middle row, over the theme's.
    #[must_use]
    pub fn band_color(mut self, color: Color) -> Self {
        self.p().band_color = Some(color);
        self
    }

    /// The band's corners, over the theme's.
    #[must_use]
    pub fn band_radius(mut self, radius: f32) -> Self {
        self.p().band_radius = Some(radius);
        self
    }

    /// The rows' type, over the theme's.
    #[must_use]
    pub fn text_style(mut self, style: TextStyle) -> Self {
        self.p().text_style = Some(style);
        self
    }

    /// The rows' colour, and the chosen row's, over the theme's.
    #[must_use]
    pub fn text_colors(mut self, rest: Color, selected: Color) -> Self {
        let p = self.p();
        p.text_color = Some(rest);
        p.selected_text_color = Some(selected);
        self
    }

    /// The chosen row's weight, over the theme's.
    #[must_use]
    pub fn selected_weight(mut self, weight: FontWeight) -> Self {
        self.p().selected_weight = Some(weight);
        self
    }

    /// How curved the wheels are: the cylinder's diameter as a multiple of the height.
    /// Larger is flatter.
    #[must_use]
    pub fn diameter_ratio(mut self, ratio: f32) -> Self {
        self.p().diameter_ratio = Some(ratio);
        self
    }

    /// A tick under the finger as each value passes the middle. On unless the theme says
    /// otherwise.
    #[must_use]
    pub fn haptic_ticks(mut self, ticks: bool) -> Self {
        self.p().haptic_ticks = Some(ticks);
        self
    }

    /// Whether the wheels turn. A disabled picker is drawn faded and ignores the finger.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.p().enabled = enabled;
        self
    }
}

crate::composite::composite_widget!(WheelPicker);

/// Everything a column's rows are drawn with, resolved once.
#[derive(Clone, Copy)]
struct RowLook {
    extent: f32,
    style: TextStyle,
    color: Color,
    selected_color: Color,
    selected_weight: FontWeight,
}

fn build<Msg: Clone + 'static>(p: Parts<Msg>) -> Box<dyn Widget<Msg>> {
    Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
        let t = theme.widgets.wheel_picker;
        let scheme = &theme.scheme;
        let extent = p
            .item_extent
            .or(t.item_extent)
            .unwrap_or(WHEEL_ITEM_EXTENT);
        let height = p
            .height
            .or(t.height)
            .unwrap_or(extent * WHEEL_VISIBLE_ROWS);
        let look = RowLook {
            extent,
            style: p
                .text_style
                .or(t.text_style)
                .unwrap_or(crate::theme::type_scale(Some(theme)).title_medium),
            color: p
                .text_color
                .or(t.text_color)
                .unwrap_or(scheme.on_surface_variant),
            selected_color: p
                .selected_text_color
                .or(t.selected_text_color)
                .unwrap_or(scheme.on_surface),
            selected_weight: p
                .selected_weight
                .or(t.selected_weight)
                .unwrap_or(FontWeight::SemiBold),
        };
        let band = p
            .band_color
            .or(t.band_color)
            .unwrap_or(scheme.surface_container_highest);
        let radius = p.band_radius.or(t.band_radius).unwrap_or(8.0);
        let ticks = p.haptic_ticks.or(t.haptic_ticks).unwrap_or(true);
        let diameter = p.diameter_ratio.or(t.diameter_ratio);

        let mut row = Flex::row().height(height).align(Align::Center);
        for column in &p.columns {
            let wheel = wheel_of(column, look, height, diameter, ticks && p.enabled);
            let boxed: Box<dyn Widget<Msg>> = match column.width {
                Some(width) => Box::new(Container::new().width(width).height(height).child(wheel)),
                None => Box::new(crate::Expanded::new(wheel).flex(column.flex)),
            };
            row = row.child_boxed(boxed);
        }
        let band = Positioned::new(
            Container::new()
                .height(extent)
                .color(band)
                .radius(BorderRadius::uniform(radius)),
        )
        .left(0.0)
        .right(0.0)
        .top((height - extent) / 2.0)
        .height(extent);
        let mut stack = Stack::new().height(height).layer(band).layer(row);
        if let Some(width) = p.width {
            stack = stack.width(width);
        }
        let picker: Box<dyn Widget<Msg>> = if p.enabled {
            Box::new(stack)
        } else {
            Box::new(crate::IgnorePointer::new(crate::Opacity::new(
                crate::disabled::DISABLED_CONTENT_OPACITY,
                stack,
            )))
        };
        picker
    }))
}

/// One column's wheel: its rows written in the picker's type, the chosen one stronger.
fn wheel_of<Msg: Clone + 'static>(
    column: &WheelColumn<Msg>,
    look: RowLook,
    height: f32,
    diameter: Option<f32>,
    ticks: bool,
) -> ListWheel<Msg> {
    let label = column.label.clone();
    let chosen = column.selected.min(column.count.saturating_sub(1));
    let align = column.align;
    let justify = match align {
        TextAlign::Start | TextAlign::Left => Justify::Start,
        TextAlign::End | TextAlign::Right => Justify::End,
        _ => Justify::Center,
    };
    let mut wheel = ListWheel::new(column.count, look.extent, move |i| {
        let mut style = look.style.color(look.color);
        if i == chosen {
            style = style
                .color(look.selected_color)
                .weight(look.selected_weight);
        }
        Flex::row()
            .height(look.extent)
            .padding_each(0.0, 12.0, 0.0, 12.0)
            .align(Align::Center)
            .justify(justify)
            .child(Text::styled(label(i), style).no_wrap())
    })
    .height(height)
    .selected(chosen)
    .looping(column.looping)
    .haptic_ticks(ticks);
    let on_selected = column.on_selected.clone();
    wheel = wheel.on_selected(move |i| on_selected(i));
    let spoken = column.spoken.clone().unwrap_or_else(|| column.label.clone());
    wheel = wheel.label(move |i| spoken(i));
    if let Some(ratio) = diameter {
        wheel = wheel.diameter_ratio(ratio);
    }
    wheel
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Size};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Hour(usize),
        Minute(usize),
    }

    fn time(hour: usize, minute: usize) -> WheelPicker<Msg> {
        WheelPicker::new()
            .column(
                WheelColumn::new(24, |h| format!("{h:02}"), Msg::Hour)
                    .selected(hour)
                    .looping(true)
                    .semantic_label(|h| format!("{h} hours")),
            )
            .column(
                WheelColumn::new(60, |m| format!("{m:02}"), Msg::Minute)
                    .selected(minute)
                    .looping(true),
            )
    }

    fn sliders(ui: &crate::Ui<Msg>) -> Vec<frus_core::SemanticsProperties> {
        ui.semantics()
            .iter()
            .filter(|(_, _, s)| s.role == frus_core::Role::Slider)
            .map(|(_, _, s)| s.clone())
            .collect()
    }

    /// **Two columns, each a wheel that says its own value** — the spoken words where they
    /// were given, the written ones where not — and a looping wheel still speaks of its
    /// values `0..count`, never of the copy on show.
    #[test]
    fn each_column_is_a_wheel_saying_its_value() {
        let picker = time(9, 30);
        let ui = build_ui(
            &picker,
            Size::new(300.0, 400.0),
            &Runtime::default(),
            &Theme::default(),
        );
        let wheels = sliders(&ui);
        assert_eq!(wheels.len(), 2, "{wheels:#?}");
        assert_eq!(wheels[0].value.as_deref(), Some("9 hours"));
        assert_eq!(wheels[0].range, Some((0.0, 9.0, 23.0)));
        assert_eq!(wheels[1].value.as_deref(), Some("30"));
        // Side by side, and the first is on the start side.
        let rects: Vec<_> = ui
            .semantics()
            .iter()
            .filter(|(_, _, s)| s.role == frus_core::Role::Slider)
            .map(|(_, r, _)| *r)
            .collect();
        assert!(rects[0].x < rects[1].x, "{rects:?}");
        let rtl = build_ui(
            &time(9, 30),
            Size::new(300.0, 400.0),
            &Runtime::default(),
            &Theme::default().rtl(),
        );
        let rects: Vec<_> = rtl
            .semantics()
            .iter()
            .filter(|(_, _, s)| s.role == frus_core::Role::Slider)
            .map(|(_, r, _)| *r)
            .collect();
        assert!(
            rects[0].x > rects[1].x,
            "in RTL the first column is on the right: {rects:?}"
        );
    }

    /// **A looping wheel opens in the middle of its turns**, so it can be spun either way
    /// from the first frame, and reports the value — not the place — as it turns.
    #[test]
    fn a_looping_wheel_reports_values_not_places() {
        let picker = time(0, 59);
        let runtime = Runtime::default();
        let ui = build_ui(
            &picker,
            Size::new(300.0, 400.0),
            &runtime,
            &Theme::default(),
        );
        let areas = ui.scroll_regions();
        assert_eq!(areas.len(), 2);
        let minutes = areas[1].page.expect("a wheel is paged");
        assert!(minutes.count > 60 * 3, "many turns: {}", minutes.count);
        assert_eq!(minutes.requested % 60, 59);
        assert!(
            minutes.requested > 60 && minutes.requested < minutes.count - 60,
            "away from both ends: {}",
            minutes.requested
        );
        // The place after it is the first minute again.
        let widget = crate::ui::find_widget(&picker, areas[1].id).expect("the wheel");
        assert_eq!(
            widget.on_page_changed(minutes.requested + 1),
            Some(Msg::Minute(0))
        );
        assert!(widget.page_tick(), "a picker ticks by default");
    }

    /// The theme reaches every part a picker draws: the row height, the band.
    #[test]
    fn the_theme_sizes_and_colours_the_picker() {
        let mut theme = Theme::default();
        theme.widgets.wheel_picker.item_extent = Some(30.0);
        theme.widgets.wheel_picker.band_color = Some(Color::rgb(1.0, 0.0, 0.0));
        theme.widgets.wheel_picker.haptic_ticks = Some(false);
        let picker = time(1, 2);
        let ui = build_ui(&picker, Size::new(300.0, 400.0), &Runtime::default(), &theme);
        let area = ui.scroll_regions()[0];
        assert_eq!(area.page.expect("paged").extent, 30.0);
        assert_eq!(area.viewport.height, 150.0, "five rows of thirty");
        assert!(ui.scene().primitives().iter().any(|p| matches!(
            p,
            frus_core::Primitive::Rect { color, .. } if *color == Color::rgb(1.0, 0.0, 0.0)
        )));
        let widget = crate::ui::find_widget(&picker, area.id).expect("the wheel");
        assert!(!widget.page_tick(), "the theme turned the ticks off");
    }
}
