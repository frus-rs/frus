// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`OtpInput`]: a row of boxes for a one-time code (milestone 810).
//!
//! ## One field, drawn as boxes
//!
//! The boxes are **one** text field underneath, not one field per box. That is what makes
//! every way a code arrives work the same: typed a character at a time, pasted whole, or
//! filled by the platform from the SMS it came in — an autofill service fills *a field*
//! with *the code*, and a row of six single-character fields would take the first digit
//! and drop the rest. Typing moves to the next box because the caret moves to the next
//! character; backspace moves back because it deletes the character before the caret; a
//! paste fills every box because it is one insertion into one value.
//!
//! The field is the [`TextField`] every other form uses — the keyboard it asks for, the
//! autofill hint, the selection bar with *Paste* on it, the screen reader's editable text,
//! the web's editing element — and this widget only paints it differently and turns a tap
//! on a box into a caret.

use std::rc::Rc;

use frus_core::{
    Color, FontWeight, Point, Rect, ResolvedTextStyle, Scene, TextPosition, TextStyle,
};
use frus_layout::{Dimension, Style};

use crate::autofill::AutofillHint;
use crate::ime::{Capitalization, Ime, KeyboardType};
use crate::interaction::{Cursor, Status};
use crate::runtime::Edit;
use crate::textinput::TextField;
use crate::theme::Theme;
use crate::widget::Widget;
use crate::Key;

/// A box's width, unless the input or its theme says otherwise.
pub const OTP_BOX_WIDTH: f32 = 48.0;
/// A box's height.
pub const OTP_BOX_HEIGHT: f32 = 56.0;
/// The gap between two boxes of a group.
pub const OTP_GAP: f32 = 8.0;
/// The room between two groups, where the separator is drawn.
pub const OTP_GROUP_GAP: f32 = 24.0;

/// What an [`OtpInput`] accepts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OtpKind {
    /// Digits only, on a number keypad: the usual SMS code.
    #[default]
    Numeric,
    /// Letters and digits, on a keyboard that neither suggests nor corrects.
    Alphanumeric,
}

/// Defaults for [`OtpInput`]. Each field unset is the input's own default, from the scheme
/// and the type scale.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OtpInputTheme {
    /// A box's width. Unset, [`OTP_BOX_WIDTH`].
    pub box_width: Option<f32>,
    /// A box's height. Unset, [`OTP_BOX_HEIGHT`].
    pub box_height: Option<f32>,
    /// The gap between two boxes of a group. Unset, [`OTP_GAP`].
    pub gap: Option<f32>,
    /// The room between two groups. Unset, [`OTP_GROUP_GAP`].
    pub group_gap: Option<f32>,
    /// A box's corners. Unset, 8.
    pub radius: Option<f32>,
    /// A box's border at rest. Unset, 1.
    pub border_width: Option<f32>,
    /// The box the next character goes in, while the input has the focus. Unset, 2.
    pub focused_border_width: Option<f32>,
    /// A box's fill. Unset, none.
    pub fill: Option<Color>,
    /// A box's border at rest. Unset, the scheme's `outline`.
    pub border_color: Option<Color>,
    /// The border of the box the next character goes in. Unset, the scheme's `primary`.
    pub focused_border_color: Option<Color>,
    /// Every border while the code is in error. Unset, the scheme's `error`.
    pub error_color: Option<Color>,
    /// The characters' type. Unset, the type scale's `headline_small`.
    pub text_style: Option<TextStyle>,
    /// The characters' colour. Unset, the scheme's `on_surface`.
    pub text_color: Option<Color>,
    /// The separator's colour. Unset, the scheme's `on_surface_variant`.
    pub separator_color: Option<Color>,
}

type OnCode<Msg> = Rc<dyn Fn(String) -> Msg>;

/// **Boxes for a one-time code** — six for an SMS code by default, typed into one by one,
/// pasted whole, or filled by the platform from the message it arrived in.
///
/// ```
/// use frus_widgets::{OtpInput, OtpKind};
///
/// #[derive(Clone)]
/// enum Msg { Code(String), Verify(String) }
///
/// let code = String::from("12");
/// let _input: OtpInput<Msg> = OtpInput::new(&code, 6, Msg::Code)
///     .on_completed(Msg::Verify)
///     .groups([3, 3])
///     .kind(OtpKind::Numeric);
/// ```
///
/// - Typing moves to the next box, backspace to the one before; a tap on a box puts the
///   caret there. A paste — from the selection bar a long press opens, or the keyboard —
///   fills every box at once, keeping the characters the input accepts.
/// - The platform is told it is a **one-time code**: Android's autofill offers the code
///   from the SMS, a browser's `autocomplete="one-time-code"` does the same, and the number
///   keypad comes up on a phone.
/// - [`masked`](OtpInput::masked) shows a dot for each character; [`error`](OtpInput::error)
///   draws every box in the error colour; [`enabled(false)`](OtpInput::enabled) greys it
///   out and takes it out of the focus order.
/// - The input is **controlled**: it shows the code it is given and sends `on_changed`
///   with the new one. A code is written left to right whatever the reading direction, as
///   digits are.
/// - Wrap it in a [`Field`](crate::Field) for a label and an error message.
pub struct OtpInput<Msg = crate::callback::Callback> {
    field: TextField<Msg>,
    value: String,
    length: usize,
    kind: OtpKind,
    groups: Vec<usize>,
    separator: Option<char>,
    masked: bool,
    mask: char,
    error: bool,
    enabled: bool,
    box_width: Option<f32>,
    box_height: Option<f32>,
    gap: Option<f32>,
    group_gap: Option<f32>,
    radius: Option<f32>,
    fill: Option<Color>,
    border_color: Option<Color>,
    focused_border_color: Option<Color>,
    error_color: Option<Color>,
    text_style: Option<TextStyle>,
    text_color: Option<Color>,
    on_changed: OnCode<Msg>,
    on_completed: Option<OnCode<Msg>>,
    label: Option<String>,
    autofocus: bool,
    uppercase: bool,
}

impl<Msg: Clone + 'static> OtpInput<Msg> {
    /// An input of `length` boxes holding `value`, sending `on_changed` with the new code
    /// when it is typed, pasted or filled in.
    pub fn new<R: crate::callback::IntoMsg<Msg>>(
        value: &str,
        length: usize,
        on_changed: impl Fn(String) -> R + 'static,
    ) -> Self {
        let length = length.max(1);
        let mut input = Self {
            field: TextField::new(""),
            value: value.chars().take(length).collect(),
            length,
            kind: OtpKind::Numeric,
            groups: Vec::new(),
            separator: Some('-'),
            masked: false,
            mask: '\u{2022}',
            error: false,
            enabled: true,
            box_width: None,
            box_height: None,
            gap: None,
            group_gap: None,
            radius: None,
            fill: None,
            border_color: None,
            focused_border_color: None,
            error_color: None,
            text_style: None,
            text_color: None,
            on_changed: Rc::new(crate::callback::handler1(on_changed)),
            on_completed: None,
            label: None,
            autofocus: false,
            uppercase: true,
        };
        input.field = input.make_field();
        input
    }

    /// The field underneath, configured from everything said so far. Remade by every
    /// builder: the field is the input's way of being typed into, not something a caller
    /// reaches.
    fn make_field(&self) -> TextField<Msg> {
        let length = self.length;
        let changed = self.on_changed.clone();
        let completed = self.on_completed.clone();
        let mut field = TextField::new(self.value.clone())
            .max_length(length)
            .enabled(self.enabled)
            .autofocus(self.autofocus)
            .obscure(self.masked)
            .autofill([AutofillHint::OneTimeCode])
            .on_input(move |code: String| match &completed {
                // The edit that fills the last box is the moment a code is ready to check.
                Some(completed) if code.chars().count() == length => completed(code),
                _ => changed(code),
            });
        field = match self.kind {
            OtpKind::Numeric => field
                .input_filter(|c| c.is_ascii_digit().then_some(c))
                .keyboard_type(KeyboardType::Number),
            OtpKind::Alphanumeric => {
                let upper = self.uppercase;
                field
                    .input_filter(move |c| {
                        c.is_alphanumeric().then(|| {
                            if upper {
                                c.to_uppercase().next().unwrap_or(c)
                            } else {
                                c
                            }
                        })
                    })
                    .keyboard_type(KeyboardType::VisiblePassword)
                    .capitalization(if upper {
                        Capitalization::Characters
                    } else {
                        Capitalization::None
                    })
            }
        };
        if let Some(label) = &self.label {
            field = field.label(label.clone());
        }
        field
    }

    fn remade(mut self) -> Self {
        self.field = self.make_field();
        self
    }

    /// The message for the edit that **fills the last box**, carrying the whole code — sent
    /// instead of `on_changed` for that one edit, so keep the code there as well. The moment
    /// to check it.
    #[must_use]
    pub fn on_completed<R: crate::callback::IntoMsg<Msg>>(
        mut self,
        on_completed: impl Fn(String) -> R + 'static,
    ) -> Self {
        self.on_completed = Some(Rc::new(crate::callback::handler1(on_completed)));
        self.remade()
    }

    /// Digits only (the default), or letters and digits.
    #[must_use]
    pub fn kind(mut self, kind: OtpKind) -> Self {
        self.kind = kind;
        self.remade()
    }

    /// Whether an alphanumeric code is upper-cased as it is typed. On by default: codes
    /// are read out in capitals, and a reader typing in lower case still means them.
    #[must_use]
    pub fn uppercase(mut self, uppercase: bool) -> Self {
        self.uppercase = uppercase;
        self.remade()
    }

    /// Splits the boxes into groups with a separator between them: `[3, 3]` for a six-digit
    /// code read as two halves. Groups that do not add up to the length are ignored.
    #[must_use]
    pub fn groups(mut self, groups: impl IntoIterator<Item = usize>) -> Self {
        self.groups = groups.into_iter().filter(|&g| g > 0).collect();
        self
    }

    /// The character drawn between two groups; `None` for a gap alone. Unset, a hyphen.
    #[must_use]
    pub fn separator(mut self, separator: Option<char>) -> Self {
        self.separator = separator;
        self
    }

    /// Shows a dot for each character rather than the character, and keeps the code from
    /// the clipboard and the screen reader, as a password field does.
    #[must_use]
    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self.remade()
    }

    /// The character a masked box shows. Unset, a bullet.
    #[must_use]
    pub fn mask_character(mut self, mask: char) -> Self {
        self.mask = mask;
        self
    }

    /// Draws every box in the error colour: the code was refused.
    #[must_use]
    pub fn error(mut self, error: bool) -> Self {
        self.error = error;
        self
    }

    /// Whether the code can be typed. A disabled input is greyed out and cannot be focused.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self.remade()
    }

    /// Takes the focus when it appears, bringing the keyboard up with it.
    #[must_use]
    pub fn autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self.remade()
    }

    /// What a screen reader calls the input. Unset, it is read as an editable text with
    /// its code.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self.remade()
    }

    /// A box's size, over the theme's.
    #[must_use]
    pub fn box_size(mut self, width: f32, height: f32) -> Self {
        self.box_width = Some(width);
        self.box_height = Some(height);
        self
    }

    /// The gap between boxes, and the room between groups, over the theme's.
    #[must_use]
    pub fn gaps(mut self, gap: f32, group_gap: f32) -> Self {
        self.gap = Some(gap);
        self.group_gap = Some(group_gap);
        self
    }

    /// A box's corners, over the theme's.
    #[must_use]
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
        self
    }

    /// A box's fill, over the theme's.
    #[must_use]
    pub fn fill(mut self, fill: Color) -> Self {
        self.fill = Some(fill);
        self
    }

    /// The borders at rest, of the box being typed into, and in error, over the theme's.
    #[must_use]
    pub fn border_colors(mut self, rest: Color, focused: Color, error: Color) -> Self {
        self.border_color = Some(rest);
        self.focused_border_color = Some(focused);
        self.error_color = Some(error);
        self
    }

    /// The characters' type, over the theme's.
    #[must_use]
    pub fn text_style(mut self, style: TextStyle) -> Self {
        self.text_style = Some(style);
        self
    }

    /// The characters' colour, over the theme's.
    #[must_use]
    pub fn text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }
}

/// The geometry of the row of boxes, resolved against a theme.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Metrics {
    width: f32,
    height: f32,
    gap: f32,
    group_gap: f32,
}

impl<Msg> OtpInput<Msg> {
    fn metrics(&self, theme: Option<&Theme>) -> Metrics {
        let t = theme.map(|t| t.widgets.otp_input).unwrap_or_default();
        Metrics {
            width: self.box_width.or(t.box_width).unwrap_or(OTP_BOX_WIDTH),
            height: self.box_height.or(t.box_height).unwrap_or(OTP_BOX_HEIGHT),
            gap: self.gap.or(t.gap).unwrap_or(OTP_GAP),
            group_gap: self.group_gap.or(t.group_gap).unwrap_or(OTP_GROUP_GAP),
        }
    }

    /// The groups the boxes are split into: the ones given when they add up to the
    /// length, one group otherwise.
    fn grouping(&self) -> Vec<usize> {
        if !self.groups.is_empty() && self.groups.iter().sum::<usize>() == self.length {
            self.groups.clone()
        } else {
            vec![self.length]
        }
    }

    /// Each box's left edge from the row's, and the middle of each gap between two groups.
    fn layout(&self, m: Metrics) -> (Vec<f32>, Vec<f32>) {
        let mut lefts = Vec::with_capacity(self.length);
        let mut separators = Vec::new();
        let mut x = 0.0;
        for (g, size) in self.grouping().into_iter().enumerate() {
            if g > 0 {
                // The previous box's trailing gap is replaced by the group's room.
                x -= m.gap;
                separators.push(x + m.group_gap / 2.0);
                x += m.group_gap;
            }
            for _ in 0..size {
                lefts.push(x);
                x += m.width + m.gap;
            }
        }
        (lefts, separators)
    }

    fn total_width(&self, m: Metrics) -> f32 {
        let (lefts, _) = self.layout(m);
        lefts.last().map_or(0.0, |l| l + m.width)
    }

    fn sizing(&self, theme: Option<&Theme>) -> Style {
        let m = self.metrics(theme);
        Style {
            width: Dimension::Length(self.total_width(m)),
            height: Dimension::Length(m.height),
            ..Default::default()
        }
    }

    /// The box under `x`, from the row's left edge: the nearest one.
    fn box_at(&self, x: f32, m: Metrics) -> usize {
        let (lefts, _) = self.layout(m);
        lefts
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let da = (x - (*a + m.width / 2.0)).abs();
                let db = (x - (*b + m.width / 2.0)).abs();
                da.total_cmp(&db)
            })
            .map_or(0, |(i, _)| i)
    }

    fn char_style(&self, theme: &Theme) -> ResolvedTextStyle {
        self.text_style
            .or(theme.widgets.otp_input.text_style)
            .unwrap_or_else(|| {
                crate::theme::type_scale(Some(theme))
                    .headline_small
                    .weight(FontWeight::Medium)
            })
            .resolved()
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for OtpInput<Msg> {
    fn build_in(
        &self,
        id: crate::interaction::WidgetId,
        runtime: &crate::runtime::Runtime,
        theme: &Theme,
    ) {
        self.field.build_in(id, runtime, theme)
    }

    fn style(&self) -> Style {
        self.sizing(None)
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        self.sizing(Some(theme))
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let t = theme.widgets.otp_input;
        let scheme = &theme.scheme;
        let m = self.metrics(Some(theme));
        let (lefts, separators) = self.layout(m);
        let o = status.opacity;
        let radius = self.radius.or(t.radius).unwrap_or(8.0);
        let border_width = t.border_width.unwrap_or(1.0);
        let focused_width = t.focused_border_width.unwrap_or(2.0);
        let fill = self.fill.or(t.fill).unwrap_or(Color::TRANSPARENT);
        let (rest, focused, error, ink, separator_ink) = if self.enabled {
            (
                self.border_color
                    .or(t.border_color)
                    .unwrap_or(scheme.outline),
                self.focused_border_color
                    .or(t.focused_border_color)
                    .unwrap_or(scheme.primary),
                self.error_color.or(t.error_color).unwrap_or(scheme.error),
                self.text_color
                    .or(t.text_color)
                    .unwrap_or(scheme.on_surface),
                t.separator_color.unwrap_or(scheme.on_surface_variant),
            )
        } else {
            let content = crate::disabled::disabled_content(theme);
            let container = crate::disabled::disabled_container(theme);
            (container, container, container, content, content)
        };
        let chars: Vec<char> = self.value.chars().collect();
        let style = self.char_style(theme);
        // The box the next character goes in: the caret's, while the input has the focus.
        let active = status
            .focused
            .then(|| status.cursor.unwrap_or(chars.len()).min(self.length - 1));
        let selected = status.selection.filter(|(a, b)| a != b);
        for (i, left) in lefts.iter().enumerate() {
            let rect = Rect::new(bounds.x + left, bounds.y, m.width, m.height);
            let is_active = active == Some(i) && selected.is_none();
            let in_selection = selected.is_some_and(|(a, b)| i >= a && i < b);
            let (border, width) = if self.error {
                (
                    error,
                    if is_active {
                        focused_width
                    } else {
                        border_width
                    },
                )
            } else if is_active {
                (focused, focused_width)
            } else {
                (rest, border_width)
            };
            let fill = if in_selection {
                theme.widgets.text_selection.selection_color_in(theme)
            } else {
                fill
            };
            scene.draw_rect(rect, fill.fade(o), radius, width, border.fade(o));
            if let Some(&c) = chars.get(i) {
                let shown = if self.masked { self.mask } else { c };
                let glyph = shown.to_string();
                let size = frus_text::measure_resolved(&glyph, &style);
                scene.text(
                    Point::new(
                        rect.x + (rect.width - size.width) / 2.0,
                        rect.y + (rect.height - size.height) / 2.0,
                    ),
                    glyph,
                    &style,
                    ink.fade(o),
                );
            } else if is_active {
                // The caret, in the empty box being typed into.
                let caret_h = (style.size * 1.2).min(m.height - 8.0);
                let caret = Rect::new(
                    rect.x + rect.width / 2.0 - 1.0,
                    rect.y + (rect.height - caret_h) / 2.0,
                    2.0,
                    caret_h,
                );
                let caret_color = theme
                    .widgets
                    .text_field
                    .cursor_color
                    .or(theme.widgets.text_selection.cursor_color)
                    .unwrap_or(scheme.primary);
                scene.fill_rect(caret, caret_color.fade(o));
            }
        }
        if let Some(separator) = self.separator {
            let glyph = separator.to_string();
            let size = frus_text::measure_resolved(&glyph, &style);
            for x in separators {
                scene.text(
                    Point::new(
                        bounds.x + x - size.width / 2.0,
                        bounds.y + (m.height - size.height) / 2.0,
                    ),
                    glyph.clone(),
                    &style,
                    separator_ink.fade(o),
                );
            }
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    /// A tap on a box puts the caret there — or after the last character typed, when the
    /// box is beyond it: a code is filled from the start.
    fn cursor_at(
        &self,
        local_x: f32,
        _local_y: f32,
        _width: f32,
        _scroll_cursor: usize,
    ) -> Option<TextPosition> {
        if !self.enabled {
            return None;
        }
        let index = self.box_at(local_x, self.metrics(None));
        Some(TextPosition::new(index.min(self.value.chars().count())))
    }

    fn cursor_icon(
        &self,
        _local_x: f32,
        _local_y: f32,
        _width: f32,
        _height: f32,
    ) -> Option<Cursor> {
        self.enabled.then_some(Cursor::Text)
    }

    fn on_edit(&self, edit: &mut Edit, key: &Key) -> Option<Msg> {
        self.field.on_edit(edit, key)
    }

    fn caret_key(&self, width: f32, edit: &mut Edit, key: &Key) -> bool {
        self.field.caret_key(width, edit, key)
    }

    fn ime(&self) -> Ime {
        self.field.ime()
    }

    fn text_value(&self) -> Option<&str> {
        self.field.text_value()
    }

    fn takes_typing(&self) -> bool {
        self.field.takes_typing()
    }

    fn replace_value(&self, value: String) -> Option<Msg> {
        self.field.replace_value(value)
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        let mut props = self.field.semantics()?;
        if self.error && props.label.is_some() {
            // A refused code is part of what the reader is told about the field.
            props.label = props.label.map(|l| format!("{l}, invalid"));
        }
        Some(props)
    }

    fn selected_text(&self, edit: &Edit) -> Option<String> {
        self.field.selected_text(edit)
    }

    fn word_at(&self, index: usize) -> Option<(usize, usize)> {
        self.field.word_at(index)
    }

    /// No handles: a code is replaced or pasted whole, never selected a character at a
    /// time with a finger.
    fn selection_handles(
        &self,
        _width: f32,
        _edit: &Edit,
        _scroll_y: f32,
    ) -> Option<[crate::SelectionHandle; 2]> {
        None
    }

    fn selection_toolbar(&self, context: crate::ToolbarContext) -> Option<&dyn Widget<Msg>> {
        self.field.selection_toolbar(context)
    }

    /// The bar (*Paste*, *Select all*) sits over the box being typed into.
    fn selection_anchor(&self, _width: f32, edit: &Edit, _scroll_y: f32) -> Option<Rect> {
        let m = self.metrics(None);
        let (lefts, _) = self.layout(m);
        let index = edit.cursor.min(self.length - 1);
        let left = lefts.get(index).copied().unwrap_or(0.0);
        Some(Rect::new(left, 0.0, m.width, m.height))
    }

    fn autofill_hints(&self) -> &[AutofillHint] {
        self.field.autofill_hints()
    }

    fn autofocus(&self) -> bool {
        self.field.autofocus()
    }

    fn on_tap_outside(&self) -> Option<Msg> {
        self.field.on_tap_outside()
    }

    fn tap_outside(&self, theme: &Theme) -> crate::TapOutside {
        self.field.tap_outside(theme)
    }

    fn focusable(&self) -> bool {
        self.field.focusable()
    }

    fn draws_own_focus(&self) -> bool {
        true
    }

    fn debug_name(&self) -> &'static str {
        "OtpInput"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Size};
    use frus_core::Primitive;

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Code(String),
        Done(String),
    }

    fn input(code: &str) -> OtpInput<Msg> {
        OtpInput::new(code, 6, Msg::Code).on_completed(Msg::Done)
    }

    fn key(c: char) -> Key {
        Key::Text(c.to_string())
    }

    /// **Typing moves to the next box**: the caret is the box, and a character typed at the
    /// end of the code is the next one.
    #[test]
    fn typing_fills_the_next_box_and_the_last_completes() {
        let input = input("12");
        let mut edit = Edit {
            cursor: 2,
            ..Default::default()
        };
        assert_eq!(
            input.on_edit(&mut edit, &key('3')),
            Some(Msg::Code("123".into()))
        );
        assert_eq!(edit.cursor, 3, "the caret moved to the next box");
        // A letter is not a digit, and is not taken.
        let mut edit = Edit {
            cursor: 2,
            ..Default::default()
        };
        assert_eq!(input.on_edit(&mut edit, &key('x')), None);
        // The edit that fills the last box carries the code to `on_completed`.
        let full = OtpInput::new("12345", 6, Msg::Code).on_completed(Msg::Done);
        let mut edit = Edit {
            cursor: 5,
            ..Default::default()
        };
        assert_eq!(
            full.on_edit(&mut edit, &key('6')),
            Some(Msg::Done("123456".into()))
        );
    }

    /// **Backspace moves back**: it removes the character before the caret, and the box it
    /// was in is the one typed into next.
    #[test]
    fn backspace_empties_the_box_before() {
        let input = input("123");
        let mut edit = Edit {
            cursor: 3,
            ..Default::default()
        };
        assert_eq!(
            input.on_edit(&mut edit, &Key::Backspace),
            Some(Msg::Code("12".into()))
        );
        assert_eq!(edit.cursor, 2);
    }

    /// **A paste fills every box**: the characters the code takes, in order, as many as
    /// there are boxes — `123 456` read out of a message lands as `123456`.
    #[test]
    fn a_paste_fills_every_box() {
        let input = input("");
        let mut edit = Edit::default();
        assert_eq!(
            input.on_edit(&mut edit, &Key::Text("Your code: 123 456.".into())),
            Some(Msg::Done("123456".into()))
        );
        let input = OtpInput::new("", 4, Msg::Code)
            .kind(OtpKind::Alphanumeric)
            .on_completed(Msg::Done);
        let mut edit = Edit::default();
        assert_eq!(
            input.on_edit(&mut edit, &Key::Text("ab-9z7".into())),
            Some(Msg::Done("AB9Z".into())),
            "upper-cased, the dash dropped, cut at four"
        );
    }

    /// **The platform is told it is a one-time code**, and asked for the right keys.
    #[test]
    fn the_platform_hears_a_one_time_code() {
        let numeric = input("");
        assert_eq!(numeric.autofill_hints(), &[AutofillHint::OneTimeCode]);
        assert_eq!(numeric.ime().keyboard, KeyboardType::Number);
        assert_eq!(numeric.ime().web().inputmode, "numeric");
        let letters = input("").kind(OtpKind::Alphanumeric);
        assert_eq!(letters.ime().keyboard, KeyboardType::VisiblePassword);
        assert_eq!(AutofillHint::OneTimeCode.web(), "one-time-code");
        assert_eq!(AutofillHint::OneTimeCode.ios(), "oneTimeCode");
        assert_eq!(AutofillHint::OneTimeCode.android(), "smsOTPCode");
    }

    /// **A tap on a box is the caret there**, or after the last character when the box is
    /// past it.
    #[test]
    fn a_tap_on_a_box_places_the_caret() {
        let input = input("1234");
        let m = input.metrics(None);
        let middle = |i: usize| input.layout(m).0[i] + m.width / 2.0;
        assert_eq!(
            input.cursor_at(middle(1), 10.0, 0.0, 0).map(|p| p.offset),
            Some(1)
        );
        assert_eq!(
            input.cursor_at(middle(5), 10.0, 0.0, 0).map(|p| p.offset),
            Some(4),
            "past the code: after its last character"
        );
    }

    fn painted(input: &OtpInput<Msg>, theme: &Theme, status: Status) -> Vec<Primitive> {
        let mut scene = Scene::new();
        let m = input.metrics(Some(theme));
        input.paint(
            Rect::new(0.0, 0.0, input.total_width(m), m.height),
            status,
            theme,
            &mut scene,
        );
        scene.primitives().to_vec()
    }

    fn borders(primitives: &[Primitive]) -> Vec<(Color, f32)> {
        primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Rect {
                    border_width,
                    border_color,
                    ..
                } if *border_width > 0.0 => Some((*border_color, *border_width)),
                _ => None,
            })
            .collect()
    }

    fn texts(primitives: &[Primitive]) -> Vec<String> {
        primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The box being typed into is drawn in the focus colour, the rest at rest; an error
    /// draws them all in the error colour; a masked code shows dots; groups get their
    /// separator.
    #[test]
    fn the_states_are_drawn() {
        let theme = Theme::default();
        let focused = Status {
            focused: true,
            cursor: Some(2),
            opacity: 1.0,
            ..Default::default()
        };
        let p = painted(&input("12"), &theme, focused);
        let b = borders(&p);
        assert_eq!(b.len(), 6);
        assert_eq!(b[2], (theme.scheme.primary, 2.0), "the third box is next");
        assert_eq!(b[0], (theme.scheme.outline, 1.0));
        assert_eq!(texts(&p), ["1", "2"]);

        let p = painted(&input("12").error(true), &theme, focused);
        assert!(borders(&p).iter().all(|(c, _)| *c == theme.scheme.error));

        let p = painted(&input("12").masked(true), &theme, Status::default());
        assert_eq!(texts(&p), ["\u{2022}", "\u{2022}"]);

        let p = painted(&input("").groups([3, 3]), &theme, Status::default());
        assert_eq!(texts(&p), ["-"], "one separator between two groups");

        // A theme reaches the boxes.
        let mut themed = Theme::default();
        themed.widgets.otp_input.border_color = Some(Color::rgb(0.0, 1.0, 0.0));
        themed.widgets.otp_input.box_width = Some(40.0);
        let p = painted(&input(""), &themed, Status::default());
        assert!(borders(&p)
            .iter()
            .all(|(c, _)| *c == Color::rgb(0.0, 1.0, 0.0)));
        assert_eq!(
            Widget::<Msg>::style_themed(&input(""), &themed).width,
            Dimension::Length(6.0 * 40.0 + 5.0 * OTP_GAP)
        );
    }

    /// **Disabled is out of the focus order** and takes no typing.
    #[test]
    fn disabled_takes_no_focus_and_no_typing() {
        let input = input("1").enabled(false);
        assert!(!input.focusable());
        assert!(!input.takes_typing());
        let ui = build_ui(
            &input,
            Size::new(400.0, 100.0),
            &Runtime::default(),
            &Theme::default(),
        );
        let (_, _, props) = ui
            .semantics()
            .iter()
            .find(|(_, _, s)| s.role == frus_core::Role::TextInput)
            .expect("an editable text");
        assert!(props.disabled);
    }

    /// A masked code is a secret to the reader too.
    #[test]
    fn a_masked_code_is_obscured() {
        let input = input("12").masked(true).label("Code");
        let props = Widget::<Msg>::semantics(&input).expect("semantics");
        assert!(props.obscured);
        assert_eq!(props.label.as_deref(), Some("Code"));
        assert!(input.selected_text(&Edit::default()).is_none());
    }
}
