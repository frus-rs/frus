//! The building blocks every page is made of: text styles, cards, list groups, fields,
//! buttons, chips, avatars, banners and the controls the design draws its own way.

use frus::{
    Align, Border, BorderRadius, BorderSide, BoxDecoration, BoxShadow, Callback, Colors, Column,
    Container, Expanded, FontWeight, GestureDetector, Justify, KeyboardType, MainAxisSize, Row,
    SizedBox, Text, TextField, TextFieldStyle, TextStyle, Widget,
};

use crate::common::{
    colors::{black, gray, green_tint, grey, light_gray, primary_color, surface, white},
    icons::{icon, Glyph},
};

/// A boxed widget, as the pages pass them around.
pub(crate) type W = Box<dyn Widget>;

/// A radius large enough to make a pill or a circle of any box.
pub(crate) const FULL: f32 = 999.0;

// ---------------------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------------------

/// Text of `size` on lines `line_height` tall, as the design gives them (`14/20`).
pub(crate) fn text(
    content: impl Into<String>,
    size: f32,
    line_height: f32,
    color: frus::Color,
    weight: FontWeight,
) -> Text {
    Text::new(content)
        .size(size)
        .height(line_height / size)
        .color(color)
        .weight(weight)
}

/// Text with letter spacing, for the large titles.
pub(crate) fn tracked(
    content: impl Into<String>,
    size: f32,
    line_height: f32,
    tracking: f32,
    color: frus::Color,
    weight: FontWeight,
) -> Text {
    Text::styled(
        content,
        TextStyle::new(size)
            .height(line_height / size)
            .letter_spacing(tracking)
            .color(color)
            .weight(weight),
    )
}

/// A page's title: `24/32`, bold, tightened.
pub(crate) fn page_title(title: impl Into<String>) -> Text {
    tracked(title, 24.0, 32.0, -0.6, black(), FontWeight::Bold)
}

/// Secondary text: `14/20` grey.
pub(crate) fn muted(content: impl Into<String>) -> Text {
    text(content, 14.0, 20.0, grey(), FontWeight::Regular)
}

/// Small secondary text: `12/16` grey.
pub(crate) fn caption(content: impl Into<String>) -> Text {
    text(content, 12.0, 16.0, grey(), FontWeight::Regular)
}

/// Body text: `14/20` black.
pub(crate) fn body(content: impl Into<String>) -> Text {
    text(content, 14.0, 20.0, black(), FontWeight::Regular)
}

/// A section's heading: `14/20` semibold.
pub(crate) fn section_label(content: impl Into<String>) -> Text {
    text(content, 14.0, 20.0, black(), FontWeight::SemiBold)
}

/// A larger section heading: `18/28` semibold.
pub(crate) fn section_heading(content: impl Into<String>) -> Text {
    text(content, 18.0, 28.0, black(), FontWeight::SemiBold)
}

/// A page's title and the line under it.
pub(crate) fn title_block(title: &str, subtitle: &str, gap: f32) -> W {
    Box::new(
        Column::new()
            .spacing(gap)
            .cross_axis_alignment(Align::Start)
            .child(page_title(title))
            .child(muted(subtitle)),
    )
}

// ---------------------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------------------

/// A column of `children`, `gap` apart, stretched to the width.
pub(crate) fn stack(gap: f32, children: Vec<W>) -> Column {
    let mut column = Column::new()
        .spacing(gap)
        .cross_axis_alignment(Align::Stretch);
    for child in children {
        column = column.child(child);
    }
    column
}

/// A row of `children`, `gap` apart, centred on the cross axis.
pub(crate) fn line(gap: f32, children: Vec<W>) -> Row {
    let mut row = Row::new().spacing(gap).cross_axis_alignment(Align::Center);
    for child in children {
        row = row.child(child);
    }
    row
}

/// A row that hugs its children.
pub(crate) fn hug(gap: f32, children: Vec<W>) -> Row {
    line(gap, children).main_axis_size(MainAxisSize::Min)
}

/// Empty room.
pub(crate) fn gap(height: f32) -> W {
    Box::new(SizedBox::empty().height(height))
}

/// Takes what is left of a row.
pub(crate) fn fill(child: impl Widget + 'static) -> W {
    Box::new(Expanded::new(child))
}

/// A tappable area.
pub(crate) fn tap(child: impl Widget + 'static, action: Callback) -> W {
    Box::new(GestureDetector::new(child).on_tap(action))
}

// ---------------------------------------------------------------------------------------
// Boxes
// ---------------------------------------------------------------------------------------

/// A white box with the hairline border, `radius` round.
pub(crate) fn outlined(radius: f32) -> BoxDecoration {
    BoxDecoration::filled(surface())
        .border(Border::new(1.0, gray()))
        .radius(BorderRadius::uniform(radius))
}

/// A filled box with no border.
pub(crate) fn filled(color: frus::Color, radius: f32) -> BoxDecoration {
    BoxDecoration::filled(color).radius(BorderRadius::uniform(radius))
}

/// The design's card: white, a hairline border, 12 round, 16 inside (17 with the border).
pub(crate) fn card(child: impl Widget + 'static) -> Container {
    Container::new()
        .decoration(outlined(12.0))
        .padding(17.0)
        .child(child)
}

/// A card with a thick coloured left edge, as the project cards have.
///
/// The edge is a strip clipped to the card's rounded corners: a border whose sides differ
/// is drawn with square corners.
pub(crate) fn accent_card(
    accent: frus::Color,
    padding: (f32, f32, f32, f32),
    child: impl Widget + 'static,
) -> Container {
    // The strip, then the content: a row stretched to the content's height.
    Container::new().decoration(outlined(12.0)).clip().child(
        Row::new()
            .cross_axis_alignment(Align::Stretch)
            .child(Container::new().width(4.0).color(accent))
            .child(Expanded::new(
                Container::new()
                    .padding_each(padding.0, padding.1, padding.2, padding.3 - 4.0)
                    .child(child),
            )),
    )
}

/// A card holding rows split by hairlines: the settings lists, the member lists.
pub(crate) fn group(rows: Vec<W>) -> Container {
    group_with(12.0, rows)
}

/// [`group`] with another radius.
pub(crate) fn group_with(radius: f32, rows: Vec<W>) -> Container {
    let count = rows.len();
    let mut column = Column::new().cross_axis_alignment(Align::Stretch);
    for (i, row) in rows.into_iter().enumerate() {
        let last = i + 1 == count;
        let cell = Container::new().child(row);
        let cell = if last {
            cell
        } else {
            cell.box_border(bottom_border())
        };
        column = column.child(cell);
    }
    Container::new()
        .decoration(outlined(radius))
        .padding(1.0)
        .clip()
        .child(column)
}

/// A hairline under a box.
pub(crate) fn bottom_border() -> Border {
    Border {
        bottom: BorderSide::new(gray(), 1.0),
        ..Border::NONE
    }
}

/// A hairline over a box.
pub(crate) fn top_border() -> Border {
    Border {
        top: BorderSide::new(gray(), 1.0),
        ..Border::NONE
    }
}

/// A hairline across the page.
pub(crate) fn divider() -> W {
    Box::new(Container::new().height(1.0).color(gray()))
}

/// A small round dot.
pub(crate) fn dot(color: frus::Color, size: f32) -> W {
    Box::new(
        Container::new()
            .width(size)
            .height(size)
            .decoration(filled(color, FULL)),
    )
}

// ---------------------------------------------------------------------------------------
// Avatars, chips, badges
// ---------------------------------------------------------------------------------------

/// A round avatar with the member's letter: green for the signed-in member, grey otherwise.
pub(crate) fn avatar(letter: &str, size: f32, font: f32, you: bool) -> W {
    let (fill, ink) = if you {
        (green_tint(), primary_color())
    } else {
        (light_gray(), grey())
    };
    avatar_colored(letter, size, font, fill, ink, FontWeight::Medium)
}

/// A round avatar in the given colours.
pub(crate) fn avatar_colored(
    letter: &str,
    size: f32,
    font: f32,
    fill: frus::Color,
    ink: frus::Color,
    weight: FontWeight,
) -> W {
    Box::new(
        Container::new()
            .width(size)
            .height(size)
            .decoration(filled(fill, FULL))
            .alignment(frus::Alignment::CENTER)
            .child(text(letter, font, font * 1.4, ink, weight)),
    )
}

/// A pill of text.
pub(crate) fn pill(
    label: impl Into<String>,
    fill: frus::Color,
    ink: frus::Color,
    pad_x: f32,
    pad_y: f32,
    font: f32,
) -> Container {
    Container::new()
        .decoration(filled(fill, FULL))
        .padding_each(pad_y, pad_x, pad_y, pad_x)
        .child(text(label, font, font * 4.0 / 3.0, ink, FontWeight::Medium))
}

/// The small grey tag of a label or a status: `12/16`, 8 by 4 inside.
pub(crate) fn tag(label: impl Into<String>) -> Container {
    pill(label, light_gray(), grey(), 8.0, 4.0, 12.0)
}

/// [`tag`] in green.
pub(crate) fn green_tag(label: impl Into<String>) -> Container {
    pill(label, green_tint(), primary_color(), 8.0, 4.0, 12.0)
}

/// A presence dot and its label.
pub(crate) fn presence(presence: crate::models::member::Presence, label_color: frus::Color) -> W {
    Box::new(hug(
        6.0,
        vec![
            dot(presence.color(), 8.0),
            Box::new(caption(presence.label()).color(label_color)),
        ],
    ))
}

/// A green banner: "All changes synced".
pub(crate) fn banner(lead: W, message: &str, radius: f32, pad_x: f32, pad_y: f32) -> W {
    banner_sized(lead, message, 12.0, radius, pad_x, pad_y)
}

/// [`banner`] with `14/20` text.
pub(crate) fn banner_large(lead: W, message: &str, pad_x: f32, pad_y: f32) -> W {
    banner_sized(lead, message, 14.0, 8.0, pad_x, pad_y)
}

fn banner_sized(lead: W, message: &str, font: f32, radius: f32, pad_x: f32, pad_y: f32) -> W {
    Box::new(
        Container::new()
            .decoration(filled(green_tint(), radius))
            .padding_each(pad_y, pad_x, pad_y, pad_x)
            .child(line(
                8.0,
                vec![
                    lead,
                    fill(text(
                        message,
                        font,
                        font + if font > 12.0 { 6.0 } else { 4.0 },
                        primary_color(),
                        FontWeight::Regular,
                    )),
                ],
            )),
    )
}

// ---------------------------------------------------------------------------------------
// Navigation pieces
// ---------------------------------------------------------------------------------------

/// "‹ Projects": goes back.
pub(crate) fn back_link(label: &str, chevron: f32, gap: f32, action: Callback) -> W {
    tap(
        hug(
            gap,
            vec![
                Box::new(icon(Glyph::ChevronLeft, chevron, grey())),
                Box::new(muted(label)),
            ],
        ),
        action,
    )
}

/// A title over a subtitle.
pub(crate) fn two_lines(title: Text, subtitle: Text, gap: f32) -> W {
    Box::new(
        Column::new()
            .spacing(gap)
            .cross_axis_alignment(Align::Stretch)
            .child(title)
            .child(subtitle),
    )
}

// ---------------------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------------------

/// The green button, as wide as it is given.
pub(crate) fn primary_button(label: &str, height: f32, radius: f32, action: Callback) -> W {
    primary_button_with(None, label, height, radius, action)
}

/// The green button with an icon before its label.
pub(crate) fn primary_button_with(
    glyph: Option<Glyph>,
    label: &str,
    height: f32,
    radius: f32,
    action: Callback,
) -> W {
    let mut content = vec![];
    if let Some(glyph) = glyph {
        content.push(Box::new(icon(glyph, 16.0, white())) as W);
    }
    content.push(Box::new(text(
        label,
        14.0,
        20.0,
        white(),
        FontWeight::SemiBold,
    )));
    tap(
        Container::new()
            .height(height)
            .decoration(filled(primary_color(), radius))
            .pressed_color(Colors::BLACK.with_alpha(0.08))
            .alignment(frus::Alignment::CENTER)
            .child(hug(8.0, content)),
        action,
    )
}

/// A white button with the hairline border.
pub(crate) fn outline_button(label: W, height: f32, radius: f32, action: Callback) -> W {
    tap(
        Container::new()
            .height(height)
            .decoration(outlined(radius))
            .alignment(frus::Alignment::CENTER)
            .child(label),
        action,
    )
}

/// A white button with a red border and red label: signing out.
pub(crate) fn danger_button(label: &str, height: f32, radius: f32, action: Callback) -> W {
    tap(
        Container::new()
            .height(height)
            .decoration(
                filled(surface(), radius).border(Border::new(1.0, crate::common::colors::red())),
            )
            .alignment(frus::Alignment::CENTER)
            .child(text(
                label,
                14.0,
                20.0,
                crate::common::colors::red(),
                FontWeight::Medium,
            )),
        action,
    )
}

/// A green button that hugs its label: "+ Task", "+ Invite".
pub(crate) fn small_primary(
    glyph: Glyph,
    label: &str,
    height: f32,
    pad_x: f32,
    action: Callback,
) -> W {
    tap(
        Container::new()
            .height(height)
            .padding_each(0.0, pad_x, 0.0, pad_x)
            .decoration(filled(primary_color(), FULL))
            .alignment(frus::Alignment::CENTER)
            .child(hug(
                8.0,
                vec![
                    Box::new(icon(glyph, 16.0, white())),
                    Box::new(text(label, 14.0, 20.0, white(), FontWeight::SemiBold)),
                ],
            )),
        action,
    )
}

/// Centred text that acts: "Cancel".
pub(crate) fn text_button(label: &str, color: frus::Color, action: Callback) -> W {
    tap(
        Container::new()
            .height(40.0)
            .alignment(frus::Alignment::CENTER)
            .child(text(label, 14.0, 20.0, color, FontWeight::Medium)),
        action,
    )
}

/// The "Cancel" under a form's button: goes back.
pub(crate) fn cancel(action: Callback) -> W {
    text_button("Cancel", grey(), action)
}

/// A square button holding an icon.
pub(crate) fn icon_button(
    glyph: Glyph,
    size: f32,
    box_size: f32,
    color: frus::Color,
    action: Callback,
) -> W {
    tap(
        Container::new()
            .width(box_size)
            .height(box_size)
            .alignment(frus::Alignment::CENTER)
            .child(icon(glyph, size, color)),
        action,
    )
}

// ---------------------------------------------------------------------------------------
// Fields
// ---------------------------------------------------------------------------------------

/// The style of the text inside the design's fields.
pub(crate) fn field_style() -> TextFieldStyle {
    TextFieldStyle {
        text_color: Some(black()),
        cursor_color: Some(primary_color()),
        handle_color: Some(primary_color()),
        icon_size: Some(16.0),
        padding_x: Some(0.0),
        ..Default::default()
    }
}

/// A field's options.
pub(crate) struct Field {
    pub value: String,
    pub placeholder: String,
    pub on_input: std::rc::Rc<dyn Fn(String) -> Callback>,
    pub height: f32,
    pub radius: f32,
    pub font: f32,
    pub lead: Option<Glyph>,
    pub trailing: Option<W>,
    pub keyboard: KeyboardType,
    pub obscure: bool,
    pub lines: Option<u16>,
    pub on_submit: Option<Callback>,
}

impl Field {
    pub(crate) fn new(
        value: &str,
        placeholder: &str,
        on_input: impl Fn(String) -> Callback + 'static,
    ) -> Self {
        Field {
            value: value.to_string(),
            placeholder: placeholder.to_string(),
            on_input: std::rc::Rc::new(on_input),
            height: 48.0,
            radius: 12.0,
            font: 14.0,
            lead: None,
            trailing: None,
            keyboard: KeyboardType::Text,
            obscure: false,
            lines: None,
            on_submit: None,
        }
    }

    pub(crate) fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub(crate) fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    pub(crate) fn font(mut self, font: f32) -> Self {
        self.font = font;
        self
    }

    pub(crate) fn lead(mut self, glyph: Glyph) -> Self {
        self.lead = Some(glyph);
        self
    }

    pub(crate) fn trailing(mut self, trailing: W) -> Self {
        self.trailing = Some(trailing);
        self
    }

    pub(crate) fn keyboard(mut self, keyboard: KeyboardType) -> Self {
        self.keyboard = keyboard;
        self
    }

    pub(crate) fn obscure(mut self, obscure: bool) -> Self {
        self.obscure = obscure;
        self
    }

    /// A text area, `lines` tall.
    pub(crate) fn lines(mut self, lines: u16) -> Self {
        self.lines = Some(lines);
        self
    }

    pub(crate) fn on_submit(mut self, action: Callback) -> Self {
        self.on_submit = Some(action);
        self
    }

    /// The field: a bordered box with the text inside it.
    pub(crate) fn build(self) -> W {
        let mut input = TextField::new(self.value.as_str())
            .placeholder(self.placeholder.as_str())
            .on_input({
                let on_input = self.on_input.clone();
                move |value: String| on_input(value)
            })
            .borderless()
            .keyboard_type(self.keyboard)
            .obscure(self.obscure)
            .size(self.font)
            .style(field_style());
        if let Some(lines) = self.lines {
            input = input.multiline().min_lines(lines).max_lines(lines);
        }
        if let Some(action) = self.on_submit {
            input = input.on_submit(action);
        }

        let mut row = Row::new().cross_axis_alignment(if self.lines.is_some() {
            Align::Start
        } else {
            Align::Center
        });
        let pad_left = if let Some(glyph) = self.lead {
            row = row
                .child(icon(glyph, 16.0, grey()))
                .child(SizedBox::empty().width(12.0));
            12.0
        } else {
            16.0
        };
        row = row.child(Expanded::new(input));
        if let Some(trailing) = self.trailing {
            row = row.child(SizedBox::empty().width(8.0)).child(trailing);
        }

        let (pad_y, justify) = if self.lines.is_some() {
            (12.0, Justify::Start)
        } else {
            (0.0, Justify::Center)
        };
        Box::new(
            Container::new()
                .height(self.height)
                .decoration(outlined(self.radius))
                .padding_each(pad_y, 16.0, pad_y, pad_left)
                .child(
                    Column::new()
                        .main_axis_alignment(justify)
                        .cross_axis_alignment(Align::Stretch)
                        .child(row),
                ),
        )
    }
}

/// A field's label above it.
pub(crate) fn labelled(label: &str, field: W) -> W {
    Box::new(stack(8.0, vec![Box::new(section_label(label)), field]))
}

// ---------------------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------------------

/// A row of choices on a grey track, the chosen one on a white thumb.
pub(crate) fn segmented(
    options: &[&str],
    selected: usize,
    font: f32,
    on_select: impl Fn(usize) -> Callback,
) -> W {
    let options = options.iter().map(|o| (None, *o)).collect::<Vec<_>>();
    segmented_with(&options, selected, font, 24.0, on_select)
}

/// [`segmented`] whose choices may carry an icon, `height` tall.
pub(crate) fn segmented_with(
    options: &[(Option<Glyph>, &str)],
    selected: usize,
    font: f32,
    height: f32,
    on_select: impl Fn(usize) -> Callback,
) -> W {
    let mut row = Row::new().spacing(4.0).cross_axis_alignment(Align::Stretch);
    for (i, (glyph, option)) in options.iter().enumerate() {
        let chosen = i == selected;
        let ink = if chosen { black() } else { grey() };
        let mut content = vec![];
        if let Some(glyph) = glyph {
            content.push(Box::new(icon(*glyph, 16.0, ink)) as W);
        }
        content.push(Box::new(text(
            *option,
            font,
            font + 6.0,
            ink,
            if chosen {
                FontWeight::Medium
            } else {
                FontWeight::Regular
            },
        )));
        let cell = Container::new()
            .height(height)
            .alignment(frus::Alignment::CENTER)
            .decoration(if chosen {
                filled(surface(), 6.0).shadow(BoxShadow::new(
                    0.0,
                    1.0,
                    2.0,
                    Colors::BLACK.with_alpha(0.08),
                ))
            } else {
                filled(Colors::TRANSPARENT, 6.0)
            })
            .child(hug(8.0, content));
        row = row.child(Expanded::new(
            GestureDetector::new(cell).on_tap(on_select(i)),
        ));
    }
    Box::new(
        Container::new()
            .decoration(filled(light_gray(), 8.0))
            .padding(4.0)
            .child(row),
    )
}

/// Filter chips: the chosen one green, the others grey.
pub(crate) fn chips(
    options: &[String],
    selected: usize,
    on_select: impl Fn(usize) -> Callback,
) -> W {
    let mut row = Row::new().spacing(8.0).cross_axis_alignment(Align::Center);
    for (i, option) in options.iter().enumerate() {
        let chosen = i == selected;
        let chip = Container::new()
            .decoration(filled(
                if chosen { green_tint() } else { light_gray() },
                FULL,
            ))
            .padding_each(6.0, 12.0, 6.0, 12.0)
            .child(text(
                option.clone(),
                14.0,
                20.0,
                if chosen { primary_color() } else { grey() },
                FontWeight::Medium,
            ));
        row = row.child(GestureDetector::new(chip).on_tap(on_select(i)));
    }
    Box::new(
        frus::SingleChildScrollView::new()
            .axis(frus::Axis::Horizontal)
            .child(row),
    )
}

/// The underlined tabs at the top of a project.
pub(crate) fn underline_tabs(
    options: &[&str],
    selected: usize,
    on_select: impl Fn(usize) -> Callback,
) -> W {
    let mut row = Row::new().spacing(20.0).cross_axis_alignment(Align::End);
    for (i, option) in options.iter().enumerate() {
        let chosen = i == selected;
        let label = Container::new()
            .padding_each(0.0, 0.0, 10.0, 0.0)
            .box_border(Border {
                bottom: BorderSide::new(
                    if chosen {
                        primary_color()
                    } else {
                        Colors::TRANSPARENT
                    },
                    2.0,
                ),
                ..Border::NONE
            })
            .child(text(
                *option,
                14.0,
                20.0,
                if chosen { primary_color() } else { grey() },
                if chosen {
                    FontWeight::Medium
                } else {
                    FontWeight::Regular
                },
            ));
        row = row.child(GestureDetector::new(label).on_tap(on_select(i)));
    }
    Box::new(
        Container::new()
            .width_fraction(1.0)
            .box_border(bottom_border())
            .child(
                frus::SingleChildScrollView::new()
                    .axis(frus::Axis::Horizontal)
                    .child(row),
            ),
    )
}

/// The design's switch: a green track when on.
pub(crate) fn toggle(on_: bool, action: Callback) -> W {
    let knob =
        Container::new()
            .width(16.0)
            .height(16.0)
            .decoration(filled(surface(), FULL).shadow(BoxShadow::new(
                0.0,
                1.0,
                2.0,
                Colors::BLACK.with_alpha(0.15),
            )));
    let track = Container::new()
        .width(32.0)
        .height(18.0)
        .padding(1.0)
        .decoration(filled(if on_ { primary_color() } else { gray() }, FULL))
        .animated_alignment(
            if on_ {
                frus::Alignment::CENTER_RIGHT
            } else {
                frus::Alignment::CENTER_LEFT
            },
            0.15,
            frus::Curve::Decelerate,
        )
        .child(knob);
    tap(track, action)
}

/// The design's checkbox: 16 square, green and ticked when checked.
pub(crate) fn checkbox(checked: bool) -> W {
    let decoration = if checked {
        filled(primary_color(), 4.0)
    } else {
        outlined(4.0).shadow(BoxShadow::new(
            0.0,
            1.0,
            2.0,
            Colors::BLACK.with_alpha(0.05),
        ))
    };
    let mut boxed = Container::new()
        .width(16.0)
        .height(16.0)
        .decoration(decoration)
        .alignment(frus::Alignment::CENTER);
    if checked {
        boxed = boxed.child(icon(Glyph::Check, 12.0, white()));
    }
    Box::new(boxed)
}
