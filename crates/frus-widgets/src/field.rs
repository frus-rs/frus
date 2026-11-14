// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`Field`] and [`InputGroup`]: the frame around a form control (milestone 810).
//!
//! A [`Field`] puts a label, a description, an error message and a required marker around
//! **any** control — a text field, an [`OtpInput`](crate::OtpInput), a dropdown, a group of
//! radio buttons — so that a form reads the same whatever it is made of. Its error is a
//! live region: a screen reader reads it out when it appears.
//!
//! An [`InputGroup`] joins a text field and the things attached to it — a `https://` before
//! it, a search icon, a button after it — into **one bordered box** that lights up as a
//! whole when the field has the focus.

use std::cell::Cell;
use std::rc::Rc;

use frus_core::{
    BorderRadius, BorderRadiusDirectional, Color, FontWeight, Rect, Role, Scene,
    SemanticsProperties, TextStyle,
};
use frus_layout::{Align, Dimension, Style};

use crate::barrier::ExcludeSemantics;
use crate::icons::IconData;
use crate::interaction::{Status, WidgetId};
use crate::lazy::Lazy;
use crate::semantics::Semantics;
use crate::textinput::TextField;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::widget::Widget;
use crate::{Container, Flex, Icon, Text};

/// Defaults for [`Field`]. Each field unset is the default named beside it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FieldTheme {
    /// The space between the label, the control and the lines under it. Unset, 6.
    pub gap: Option<f32>,
    /// The label's type. Unset, the type scale's `label_large`.
    pub label_style: Option<TextStyle>,
    /// The label's colour. Unset, the scheme's `on_surface`.
    pub label_color: Option<Color>,
    /// The description's type. Unset, the type scale's `body_small`.
    pub description_style: Option<TextStyle>,
    /// The description's colour. Unset, the scheme's `on_surface_variant`.
    pub description_color: Option<Color>,
    /// The error message's type. Unset, the type scale's `body_small`.
    pub error_style: Option<TextStyle>,
    /// The error message's colour. Unset, the scheme's `error`.
    pub error_color: Option<Color>,
    /// The required marker's colour. Unset, the error colour.
    pub required_color: Option<Color>,
    /// The required marker. Unset, an asterisk.
    pub required_marker: Option<char>,
}

struct FieldParts<Msg> {
    control: Box<dyn Widget<Msg>>,
    label: Option<String>,
    description: Option<String>,
    error: Option<String>,
    required: bool,
    gap: Option<f32>,
    label_style: Option<TextStyle>,
    label_color: Option<Color>,
    description_style: Option<TextStyle>,
    description_color: Option<Color>,
    error_style: Option<TextStyle>,
    error_color: Option<Color>,
    required_marker: Option<char>,
}

/// **A labelled form control**: a label above any control, a description and an error
/// message below it, and a marker on the label when the control must be filled in.
///
/// ```
/// use frus_widgets::{Field, TextField};
///
/// #[derive(Clone)]
/// enum Msg { Email(String) }
///
/// let email = String::from("ada@");
/// let _field: Field<Msg> = Field::new(TextField::new(&email).on_input(Msg::Email))
///     .label("Email")
///     .description("We never share it.")
///     .error(Some("Enter a whole address".to_string()))
///     .required(true);
/// ```
///
/// - The label is read as "Email, required" when the control is required; the marker
///   itself is drawn, not said.
/// - The **error** is a live region: when it appears or changes, a screen reader reads it
///   out where the reader is, which is rarely under the field. It is drawn in the error
///   colour under the description.
/// - Everything follows the reading direction, and every colour, type and gap is the
///   theme's ([`FieldTheme`]) unless the field says otherwise.
pub struct Field<Msg = crate::callback::Callback> {
    inner: Lazy<FieldParts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> Field<Msg> {
    /// A field around `control`.
    pub fn new(control: impl Widget<Msg> + 'static) -> Self {
        Self {
            inner: Lazy::new(
                FieldParts {
                    control: Box::new(control),
                    label: None,
                    description: None,
                    error: None,
                    required: false,
                    gap: None,
                    label_style: None,
                    label_color: None,
                    description_style: None,
                    description_color: None,
                    error_style: None,
                    error_color: None,
                    required_marker: None,
                },
                build_field::<Msg>,
            ),
        }
    }

    fn p(&mut self) -> &mut FieldParts<Msg> {
        self.inner.parts_mut()
    }

    /// The label above the control.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.p().label = Some(label.into());
        self
    }

    /// A line of guidance under the control.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.p().description = Some(description.into());
        self
    }

    /// What is wrong with the value, or `None` while nothing is. Drawn under the
    /// description in the error colour, and read out when it appears.
    #[must_use]
    pub fn error(mut self, error: Option<String>) -> Self {
        self.p().error = error.filter(|e| !e.is_empty());
        self
    }

    /// Marks the control as one that must be filled in.
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.p().required = required;
        self
    }

    /// The space between the parts, over the theme's.
    #[must_use]
    pub fn gap(mut self, gap: f32) -> Self {
        self.p().gap = Some(gap);
        self
    }

    /// The label's type and colour, over the theme's.
    #[must_use]
    pub fn label_style(mut self, style: TextStyle, color: Color) -> Self {
        let p = self.p();
        p.label_style = Some(style);
        p.label_color = Some(color);
        self
    }

    /// The description's type and colour, over the theme's.
    #[must_use]
    pub fn description_style(mut self, style: TextStyle, color: Color) -> Self {
        let p = self.p();
        p.description_style = Some(style);
        p.description_color = Some(color);
        self
    }

    /// The error message's type and colour, over the theme's.
    #[must_use]
    pub fn error_style(mut self, style: TextStyle, color: Color) -> Self {
        let p = self.p();
        p.error_style = Some(style);
        p.error_color = Some(color);
        self
    }

    /// The required marker, over the theme's asterisk.
    #[must_use]
    pub fn required_marker(mut self, marker: char) -> Self {
        self.p().required_marker = Some(marker);
        self
    }
}

crate::composite::composite_widget!(Field);

fn build_field<Msg: Clone + 'static>(p: FieldParts<Msg>) -> Box<dyn Widget<Msg>> {
    let p = Rc::new(std::cell::RefCell::new(Some(p)));
    Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
        let Some(p) = p.borrow_mut().take() else {
            return Box::new(Flex::column()) as Box<dyn Widget<Msg>>;
        };
        let t = theme.widgets.field;
        let scheme = &theme.scheme;
        let scale = crate::theme::type_scale(Some(theme));
        let error_color = p.error_color.or(t.error_color).unwrap_or(scheme.error);
        let mut column = Flex::column()
            .gap(p.gap.or(t.gap).unwrap_or(6.0))
            .align(Align::Stretch);
        if let Some(label) = &p.label {
            let style = p
                .label_style
                .or(t.label_style)
                .unwrap_or(scale.label_large)
                .color(p.label_color.or(t.label_color).unwrap_or(scheme.on_surface));
            let mut row = Flex::row()
                .gap(2.0)
                .align(Align::Start)
                .child(Text::styled(label.clone(), style));
            let spoken = if p.required {
                let marker = p.required_marker.or(t.required_marker).unwrap_or('*');
                let color = t.required_color.unwrap_or(error_color);
                row = row.child(Text::styled(
                    marker.to_string(),
                    style.color(color).weight(FontWeight::SemiBold),
                ));
                format!("{label}, required")
            } else {
                label.clone()
            };
            column = column.child(Semantics::new(
                SemanticsProperties::new(Role::Label).label(spoken),
                ExcludeSemantics::new(row),
            ));
        }
        column = column.child_boxed(p.control);
        if let Some(description) = &p.description {
            let style = p
                .description_style
                .or(t.description_style)
                .unwrap_or(scale.body_small)
                .color(
                    p.description_color
                        .or(t.description_color)
                        .unwrap_or(scheme.on_surface_variant),
                );
            column = column.child(Text::styled(description.clone(), style).wrap());
        }
        if let Some(error) = &p.error {
            let style = p
                .error_style
                .or(t.error_style)
                .unwrap_or(scale.body_small)
                .color(error_color);
            column = column.child(Semantics::new(
                SemanticsProperties::new(Role::Label)
                    .label(error.clone())
                    .live(true),
                ExcludeSemantics::new(Text::styled(error.clone(), style).wrap()),
            ));
        }
        Box::new(column) as Box<dyn Widget<Msg>>
    }))
}

// ---------------------------------------------------------------------------------------
// InputGroup
// ---------------------------------------------------------------------------------------

/// Defaults for [`InputGroup`]. Each field unset is the default named beside it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InputGroupTheme {
    /// The group's height. Unset, 48.
    pub height: Option<f32>,
    /// The group's corners. Unset, 8.
    pub radius: Option<f32>,
    /// The border at rest. Unset, 1.
    pub border_width: Option<f32>,
    /// The border while the field has the focus. Unset, 2.
    pub focused_border_width: Option<f32>,
    /// The border at rest. Unset, the scheme's `outline`.
    pub border_color: Option<Color>,
    /// The border while the field has the focus. Unset, the scheme's `primary`.
    pub focused_border_color: Option<Color>,
    /// The border while in error. Unset, the scheme's `error`.
    pub error_color: Option<Color>,
    /// The group's fill. Unset, none.
    pub fill: Option<Color>,
    /// A text addon's fill. Unset, the scheme's `surface_container_high`.
    pub addon_fill: Option<Color>,
    /// An addon's content: its words and its icon. Unset, the scheme's `on_surface_variant`.
    pub addon_color: Option<Color>,
    /// A text addon's type. Unset, the type scale's `body_large`.
    pub addon_text_style: Option<TextStyle>,
    /// The padding either side of an addon. Unset, 12.
    pub addon_padding: Option<f32>,
}

/// Something attached to an [`InputGroup`]'s field: words, an icon, or any widget — a
/// button, a dropdown.
pub enum Addon<Msg = crate::callback::Callback> {
    /// Words on a filled band, such as `https://` or `.com`.
    Text(String),
    /// An icon, such as a search glass.
    Icon(IconData),
    /// Any widget, such as a button.
    Widget(Box<dyn Widget<Msg>>),
}

impl<Msg> Addon<Msg> {
    /// Words on a filled band.
    pub fn text(words: impl Into<String>) -> Self {
        Addon::Text(words.into())
    }

    /// An icon.
    pub fn icon(icon: IconData) -> Self {
        Addon::Icon(icon)
    }

    /// Any widget — a button, most often.
    pub fn widget(widget: impl Widget<Msg> + 'static) -> Self {
        Addon::Widget(Box::new(widget))
    }
}

/// An input group's parts before its row is built: the field, the leading addons and the
/// trailing ones.
type Parts<Msg> = (TextField<Msg>, Vec<Addon<Msg>>, Vec<Addon<Msg>>);

#[derive(Clone, Copy, Debug)]
struct Look {
    height: f32,
    radius: f32,
    border_width: f32,
    focused_border_width: f32,
    border_color: Color,
    focused_border_color: Color,
    error_color: Color,
    fill: Color,
}

/// **A text field with things attached**: words, icons or buttons before and after it, in
/// one bordered box.
///
/// ```
/// use frus_widgets::{button, Addon, Icons, InputGroup, TextField};
///
/// #[derive(Clone)]
/// enum Msg { Site(String), Go }
///
/// let site = String::from("example");
/// let _group: InputGroup<Msg> = InputGroup::new(TextField::new(&site).on_input(Msg::Site))
///     .leading(Addon::text("https://"))
///     .trailing(Addon::text(".com"))
///     .trailing(Addon::widget(button("Go", Msg::Go)));
/// let _search: InputGroup<Msg> = InputGroup::new(TextField::new("").placeholder("Search"))
///     .leading(Addon::icon(Icons::SEARCH));
/// ```
///
/// The field loses its own border and the group draws one around everything, lit while
/// the field has the focus, red with [`error`](InputGroup::error). Leading addons come
/// first in the reading direction — on the left, or on the right in a right-to-left
/// interface. Wrap the group in a [`Field`] for a label and an error message.
pub struct InputGroup<Msg = crate::callback::Callback> {
    /// The parts, until the row is first asked for.
    parts: std::cell::RefCell<Option<Parts<Msg>>>,
    /// The row: the leading addons, the field, the trailing addons.
    row: std::cell::OnceCell<Vec<Box<dyn Widget<Msg>>>>,
    /// Where the field is among the row's children: after the leading addons.
    field_index: usize,
    error: bool,
    width: Dimension,
    sized: bool,
    height: Option<f32>,
    radius: Option<f32>,
    border_color: Option<Color>,
    focused_border_color: Option<Color>,
    fill: Option<Color>,
    /// Whether the field had the focus when the frame was built.
    focused: Cell<bool>,
}

impl<Msg: Clone + 'static> InputGroup<Msg> {
    /// A group around `field`, which loses its own border to the group's.
    pub fn new(field: TextField<Msg>) -> Self {
        Self {
            parts: std::cell::RefCell::new(Some((field.borderless(), Vec::new(), Vec::new()))),
            row: std::cell::OnceCell::new(),
            field_index: 0,
            error: false,
            width: Dimension::Auto,
            sized: false,
            height: None,
            radius: None,
            border_color: None,
            focused_border_color: None,
            fill: None,
            focused: Cell::new(false),
        }
    }

    /// Attaches `addon` before the field, after any already there.
    #[must_use]
    pub fn leading(mut self, addon: Addon<Msg>) -> Self {
        if let Some((_, leading, _)) = self.parts.get_mut().as_mut() {
            leading.push(addon);
            self.field_index = leading.len();
        }
        self
    }

    /// Attaches `addon` after the field, after any already there.
    #[must_use]
    pub fn trailing(mut self, addon: Addon<Msg>) -> Self {
        if let Some((_, _, trailing)) = self.parts.get_mut().as_mut() {
            trailing.push(addon);
        }
        self
    }

    /// Draws the border in the error colour.
    #[must_use]
    pub fn error(mut self, error: bool) -> Self {
        self.error = error;
        self
    }

    /// A fixed width. Unset, the room on offer.
    #[must_use]
    pub fn width(mut self, width: f32) -> Self {
        self.width = Dimension::Length(width);
        self.sized = true;
        self
    }

    /// The group's height, over the theme's.
    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// The corners, over the theme's.
    #[must_use]
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
        self
    }

    /// The border at rest and while focused, over the theme's.
    #[must_use]
    pub fn border_colors(mut self, rest: Color, focused: Color) -> Self {
        self.border_color = Some(rest);
        self.focused_border_color = Some(focused);
        self
    }

    /// The group's fill, over the theme's.
    #[must_use]
    pub fn fill(mut self, fill: Color) -> Self {
        self.fill = Some(fill);
        self
    }

    fn look(&self, theme: &Theme) -> Look {
        let t = theme.widgets.input_group;
        let s = &theme.scheme;
        Look {
            height: self.height.or(t.height).unwrap_or(48.0),
            radius: self.radius.or(t.radius).unwrap_or(8.0),
            border_width: t.border_width.unwrap_or(1.0),
            focused_border_width: t.focused_border_width.unwrap_or(2.0),
            border_color: self.border_color.or(t.border_color).unwrap_or(s.outline),
            focused_border_color: self
                .focused_border_color
                .or(t.focused_border_color)
                .unwrap_or(s.primary),
            error_color: t.error_color.unwrap_or(s.error),
            fill: self.fill.or(t.fill).unwrap_or(Color::TRANSPARENT),
        }
    }

    /// The field's identity: the group's child at its place in the row.
    fn field_id(&self, id: WidgetId) -> WidgetId {
        id.child(self.field_index)
    }
}

impl<Msg: Clone + 'static> InputGroup<Msg> {
    /// The row, laid out the first time it is asked for.
    fn row(&self) -> &[Box<dyn Widget<Msg>>] {
        self.row.get_or_init(|| {
            let Some((field, leading, trailing)) = self.parts.borrow_mut().take() else {
                return Vec::new();
            };
            let count = leading.len() + trailing.len();
            let mut row: Vec<Box<dyn Widget<Msg>>> = Vec::with_capacity(count + 1);
            for (i, addon) in leading.into_iter().enumerate() {
                row.push(addon_widget(addon, i == 0, false));
            }
            row.push(Box::new(crate::Expanded::new(field)));
            let last = trailing.len();
            for (i, addon) in trailing.into_iter().enumerate() {
                row.push(addon_widget(addon, false, i + 1 == last));
            }
            row
        })
    }
}

/// An addon as a widget of the row. `first` and `last` round its outer corners to fit the
/// group's.
fn addon_widget<Msg: Clone + 'static>(
    addon: Addon<Msg>,
    first: bool,
    last: bool,
) -> Box<dyn Widget<Msg>> {
    match addon {
        Addon::Text(words) => Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
            let t = theme.widgets.input_group;
            let s = &theme.scheme;
            let inner = (t.radius.unwrap_or(8.0) - t.border_width.unwrap_or(1.0)).max(0.0);
            let radius = BorderRadiusDirectional::horizontal(
                if first { inner } else { 0.0 },
                if last { inner } else { 0.0 },
            )
            .resolve(theme.direction);
            let style = t
                .addon_text_style
                .unwrap_or(crate::theme::type_scale(Some(theme)).body_large)
                .color(t.addon_color.unwrap_or(s.on_surface_variant));
            let pad = t.addon_padding.unwrap_or(12.0);
            Box::new(
                Container::new()
                    .color(t.addon_fill.unwrap_or(s.surface_container_high))
                    .radius(radius)
                    .padding_each(0.0, pad, 0.0, pad)
                    .alignment(frus_core::Alignment::CENTER)
                    .child(Text::styled(words.clone(), style).no_wrap()),
            ) as Box<dyn Widget<Msg>>
        })),
        Addon::Icon(icon) => Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
            let t = theme.widgets.input_group;
            let pad = t.addon_padding.unwrap_or(12.0);
            Box::new(
                Container::new()
                    .padding_each(0.0, pad, 0.0, pad)
                    .alignment(frus_core::Alignment::CENTER)
                    .child(
                        Icon::new(icon)
                            .size(20.0)
                            .color(t.addon_color.unwrap_or(theme.scheme.on_surface_variant)),
                    ),
            ) as Box<dyn Widget<Msg>>
        })),
        Addon::Widget(widget) => Box::new(
            Flex::row()
                .padding_each(0.0, 4.0, 0.0, 4.0)
                .align(Align::Center)
                .child_boxed(widget),
        ),
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for InputGroup<Msg> {
    /// Notes whether the field holds the focus: the border is the group's, and lights up
    /// for it.
    fn build_in(&self, id: WidgetId, runtime: &crate::runtime::Runtime, theme: &Theme) {
        self.focused
            .set(runtime.input.focused == Some(self.field_id(id)));
        self.build_themed(theme)
    }

    fn style(&self) -> Style {
        self.style_themed(&Theme::default())
    }

    fn style_themed(&self, theme: &Theme) -> Style {
        let look = self.look(theme);
        let inset = look.focused_border_width;
        Style {
            width: self.width,
            height: Dimension::Length(look.height),
            flex_direction: frus_layout::FlexDirection::Row,
            align: Align::Stretch,
            padding: frus_core::Insets::uniform(inset),
            ..Default::default()
        }
    }

    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        crate::widget::FillAxes {
            horizontal: !self.sized,
            vertical: false,
        }
    }

    fn soft_extent(&self) -> crate::widget::FillAxes {
        crate::widget::FillAxes {
            horizontal: !self.sized,
            vertical: false,
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.row()
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let look = self.look(theme);
        let o = status.opacity;
        let focused = self.focused.get();
        let (color, width) = if self.error {
            (
                look.error_color,
                if focused {
                    look.focused_border_width
                } else {
                    look.border_width
                },
            )
        } else if focused {
            (look.focused_border_color, look.focused_border_width)
        } else {
            (look.border_color, look.border_width)
        };
        // The border is drawn inside the padding the children sit within, so a thicker one
        // while focused moves nothing.
        let inset = look.focused_border_width - width;
        let rect = Rect::new(
            bounds.x + inset,
            bounds.y + inset,
            bounds.width - 2.0 * inset,
            bounds.height - 2.0 * inset,
        );
        scene.draw_rect(
            rect,
            look.fill.fade(o),
            BorderRadius::uniform(look.radius),
            width,
            color.fade(o),
        );
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "InputGroup"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Size};
    use frus_core::Primitive;

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Typed(String),
    }

    fn labels(ui: &crate::Ui<Msg>) -> Vec<SemanticsProperties> {
        ui.semantics().iter().map(|(_, _, s)| s.clone()).collect()
    }

    /// **The label says the field is required, and the error is a live region** — the two
    /// things a reader cannot see.
    #[test]
    fn a_field_says_required_and_its_error_is_live() {
        let field = Field::new(TextField::new("ada@").on_input(Msg::Typed))
            .label("Email")
            .description("We never share it.")
            .error(Some("Enter a whole address".into()))
            .required(true);
        let ui = build_ui(
            &field,
            Size::new(320.0, 300.0),
            &Runtime::default(),
            &Theme::default(),
        );
        let nodes = labels(&ui);
        assert!(
            nodes
                .iter()
                .any(|s| s.label.as_deref() == Some("Email, required") && !s.live),
            "{nodes:#?}"
        );
        let error = nodes
            .iter()
            .find(|s| s.label.as_deref() == Some("Enter a whole address"))
            .expect("the error is in the tree");
        assert!(error.live, "and read out when it appears");
        assert!(
            !nodes.iter().any(|s| s.label.as_deref() == Some("*")),
            "the marker is drawn, not said"
        );
        assert!(nodes.iter().any(|s| s.role == Role::TextInput));
        // The error is drawn in the error colour.
        let theme = Theme::default();
        assert!(ui.scene().primitives().iter().any(|p| matches!(
            p,
            Primitive::Text { text, color, .. } if text == "Enter a whole address" && *color == theme.scheme.error
        )));
    }

    /// No error, no error line, and nothing live.
    #[test]
    fn a_field_without_an_error_has_no_live_region() {
        let field = Field::new(TextField::<Msg>::new(""))
            .label("Name")
            .error(None);
        let ui = build_ui(
            &field,
            Size::new(320.0, 300.0),
            &Runtime::default(),
            &Theme::default(),
        );
        assert!(labels(&ui).iter().all(|s| !s.live));
    }

    fn group() -> InputGroup<Msg> {
        InputGroup::new(TextField::new("example").on_input(Msg::Typed))
            .leading(Addon::text("https://"))
            .trailing(Addon::text(".com"))
    }

    fn border(ui: &crate::Ui<Msg>) -> (Color, f32) {
        ui.scene()
            .primitives()
            .iter()
            .find_map(|p| match p {
                Primitive::Rect {
                    border_width,
                    border_color,
                    ..
                } if *border_width > 0.0 => Some((*border_color, *border_width)),
                _ => None,
            })
            .expect("the group's border")
    }

    /// **One border around everything**, lit while the field holds the focus, in the error
    /// colour when told — and the addons in reading order.
    #[test]
    fn the_group_border_follows_the_field_focus() {
        let theme = Theme::default();
        let g = group();
        let ui = build_ui(&g, Size::new(400.0, 100.0), &Runtime::default(), &theme);
        assert_eq!(border(&ui), (theme.scheme.outline, 1.0));
        let field = ui
            .semantics()
            .iter()
            .find(|(_, _, s)| s.role == Role::TextInput)
            .map(|(id, _, _)| *id)
            .expect("the field");
        let mut runtime = Runtime::default();
        runtime.input.focused = Some(field);
        let g = group();
        let ui = build_ui(&g, Size::new(400.0, 100.0), &runtime, &theme);
        assert_eq!(border(&ui), (theme.scheme.primary, 2.0), "lit by the focus");
        let g = group().error(true);
        let ui = build_ui(&g, Size::new(400.0, 100.0), &Runtime::default(), &theme);
        assert_eq!(border(&ui).0, theme.scheme.error);

        // The addons' words, left to right, and mirrored in RTL.
        let xs = |ui: &crate::Ui<Msg>| {
            let at = |words: &str| {
                ui.scene()
                    .primitives()
                    .iter()
                    .find_map(|p| match p {
                        Primitive::Text { text, position, .. } if text == words => Some(position.x),
                        _ => None,
                    })
                    .expect(words)
            };
            (at("https://"), at(".com"))
        };
        let g = group();
        let (lead, trail) = xs(&build_ui(
            &g,
            Size::new(400.0, 100.0),
            &Runtime::default(),
            &theme,
        ));
        assert!(lead < trail);
        let rtl = Theme::default().rtl();
        let g = group();
        let (lead, trail) = xs(&build_ui(
            &g,
            Size::new(400.0, 100.0),
            &Runtime::default(),
            &rtl,
        ));
        assert!(lead > trail, "mirrored: {lead} {trail}");
    }
}
