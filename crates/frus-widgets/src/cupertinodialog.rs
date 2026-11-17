// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`CupertinoAlertDialog`] and [`CupertinoActionSheet`]: the Apple-style alert, which asks
//! a question in the middle of the screen, and the action sheet, which offers a choice from
//! the bottom of it (milestone 812).
//!
//! Both are **controlled**, like every overlay here: the application holds the open flag
//! and says what closes them.
//!
//! ```ignore
//! CupertinoAlertDialog::new(app.confirm_open)
//!     .title("Delete this photo?")
//!     .content("It will be gone from all your devices.")
//!     .action(CupertinoDialogAction::new("Cancel", Msg::Close).default_action())
//!     .action(CupertinoDialogAction::new("Delete", Msg::Delete).destructive())
//!     .body(screen)
//! ```

use frus_core::{Color, Insets, Rect, Scene, TextAlign, TextStyle};
use frus_layout::{Align, Dimension, FlexDirection, Justify, Style};

use crate::cupertinocolors::{CupertinoColors, CupertinoDynamicColor};
use crate::cupertinotheme::CupertinoTheme;
use crate::interaction::Status;
use crate::portal::{OverlayPresentation, Placement};
use crate::theme::Theme;
use crate::widget::Widget;

/// The scrim behind an Apple-style modal (`route.dart:53`).
pub const CUPERTINO_MODAL_BARRIER: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::from_argb_u32(0x33000000),
    Color::from_argb_u32(0x7A000000),
);

/// An alert's width (`dialog.dart:92`).
pub const CUPERTINO_ALERT_WIDTH: f32 = 270.0;
/// An alert's corners (`:85`).
const ALERT_RADIUS: f32 = 14.0;
/// The room round an alert's title and message (`:94`).
const ALERT_EDGE: f32 = 20.0;
/// An alert's least button height (`:95`).
const ALERT_BUTTON_MIN: f32 = 45.0;
/// The room inside an alert's button (`:2335`).
const ALERT_BUTTON_PADDING: f32 = 8.0;
/// How far an alert is held off the window's edges (`:447`).
const ALERT_INSET: Insets = Insets::new(24.0, 40.0, 24.0, 40.0);
/// How long an alert takes to come and go (`route.dart:1481`).
const ALERT_SECONDS: f32 = 0.25;
/// How much larger an alert is as it starts to appear (`route.dart:1549`).
const ALERT_SCALE_FROM: f32 = 1.3;
/// The blur under an alert and a sheet (`CupertinoPopupSurface.defaultBlurSigma`).
const POPUP_BLUR: f32 = 30.0;

/// The room round a sheet and between it and its cancel button (`dialog.dart:102`).
const SHEET_EDGE: f32 = 8.0;
/// A sheet's corners.
const SHEET_RADIUS: f32 = 12.0;
/// The room round a sheet's title and message (`:104`).
const SHEET_CONTENT_H: f32 = 16.0;
const SHEET_CONTENT_V: f32 = 13.5;
/// A sheet's least button height (`:111`) and the room inside one (`:107`, `:112`).
const SHEET_BUTTON_MIN: f32 = 57.17;
const SHEET_BUTTON_H: f32 = 10.0;
/// How long a sheet takes to slide in (`route.dart:59`).
const SHEET_SECONDS: f32 = 0.335;

/// An alert's background, over the blur (`dialog.dart:118`).
const ALERT_COLOR: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::from_argb_u32(0xCCF2F2F2),
    Color::from_argb_u32(0xCC2D2D2D),
);
/// An alert's pressed button (`:126`).
const ALERT_PRESSED: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::from_argb_u32(0xFFE1E1E1),
    Color::from_argb_u32(0xFF404040),
);
/// A sheet's pressed button (`:134`).
const SHEET_PRESSED: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::from_argb_u32(0xCAE0E0E0),
    Color::from_argb_u32(0xC1515151),
);
/// A sheet's cancel button, at rest and pressed (`:139`, `:143`).
const SHEET_CANCEL: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::from_argb_u32(0xFFFFFFFF),
    Color::from_argb_u32(0xFF2C2C2C),
);
const SHEET_CANCEL_PRESSED: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::from_argb_u32(0xFFECECEC),
    Color::from_argb_u32(0xFF494949),
);
/// A sheet's background, over the blur (`:153`).
const SHEET_COLOR: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::from_argb_u32(0xC8FCFCFC),
    Color::from_argb_u32(0xBE292929),
);
/// A sheet's title and message (`:160`).
const SHEET_CONTENT_TEXT: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::from_argb_u32(0x851D1D1D),
    Color::from_argb_u32(0x96F1F1F1),
);
/// A sheet's rules (`:169`).
const SHEET_DIVIDER: CupertinoDynamicColor = CupertinoDynamicColor::with_brightness(
    Color::from_argb_u32(0xD4C9C9C9),
    Color::from_argb_u32(0xD57D7D7D),
);

/// An alert's title: 17, semibold (`dialog.dart:36`).
const ALERT_TITLE: TextStyle = TextStyle::new(17.0)
    .weight(frus_core::FontWeight::SemiBold)
    .letter_spacing(-0.5);
/// An alert's message: 13 (`:46`).
const ALERT_CONTENT: TextStyle = TextStyle::new(13.0).letter_spacing(-0.2);
/// An alert's buttons: 17 — the reference's 16.8, rounded to the type scale (`:56`).
const ALERT_ACTION: TextStyle = TextStyle::new(17.0);
/// A sheet's buttons (`:65`) and its title and message (`:74`).
const SHEET_ACTION: TextStyle = TextStyle::new(17.0);
const SHEET_CONTENT: TextStyle = TextStyle::new(13.0);

/// One button of a [`CupertinoAlertDialog`] or a [`CupertinoActionSheet`]: a label and what
/// pressing it sends.
///
/// The **default** action — the one the alert expects — is set semibold; a **destructive**
/// one — that deletes, discards, cannot be undone — is red. An action with no message is
/// disabled and drawn at half strength.
#[derive(Clone, Debug, PartialEq)]
pub struct CupertinoDialogAction<Msg = crate::callback::Callback> {
    label: String,
    on_press: Option<Msg>,
    default: bool,
    destructive: bool,
    text_style: Option<TextStyle>,
}

/// An action sheet's button: the same thing as an alert's.
pub type CupertinoActionSheetAction<Msg = crate::callback::Callback> = CupertinoDialogAction<Msg>;

impl<Msg> CupertinoDialogAction<Msg> {
    /// An action labelled `label`, sending `message`.
    pub fn new(label: impl Into<String>, message: impl Into<Msg>) -> Self {
        Self {
            label: label.into(),
            on_press: Some(message.into()),
            default: false,
            destructive: false,
            text_style: None,
        }
    }

    /// A disabled action: shown, and inert.
    pub fn disabled(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            on_press: None,
            default: false,
            destructive: false,
            text_style: None,
        }
    }

    /// The action the alert expects, set semibold.
    #[must_use]
    pub fn default_action(mut self) -> Self {
        self.default = true;
        self
    }

    /// An action that destroys something, in red.
    #[must_use]
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    /// Its label's style, over the default one.
    #[must_use]
    pub fn text_style(mut self, style: TextStyle) -> Self {
        self.text_style = Some(style);
        self
    }

    /// The label's resolved style, from `base`.
    fn style(&self, base: TextStyle, theme: &Theme) -> TextStyle {
        let colour = if self.destructive {
            CupertinoColors::SYSTEM_RED.resolve(theme)
        } else {
            CupertinoTheme::of(theme).primary_color
        };
        let colour = if self.on_press.is_some() {
            colour
        } else {
            colour.with_alpha(colour.a * 0.5)
        };
        let mut style = base.color(colour);
        if self.default {
            style = style.weight(frus_core::FontWeight::SemiBold);
        }
        match self.text_style {
            Some(over) => style.merge(over),
            None => style,
        }
    }
}

/// A pressable row: its background at rest, another while held, and a centred label.
struct ActionButton<Msg> {
    children: Vec<Box<dyn Widget<Msg>>>,
    label: String,
    message: Option<Msg>,
    idle: Color,
    pressed: Color,
    min_height: f32,
    padding: Insets,
}

impl<Msg: Clone> Widget<Msg> for ActionButton<Msg> {
    fn style(&self) -> Style {
        Style {
            min_height: Dimension::Length(self.min_height),
            padding: self.padding,
            justify: Justify::Center,
            align: Align::Center,
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, bounds: Rect, status: Status, _theme: &Theme, scene: &mut Scene) {
        let o = status.opacity;
        if self.idle.a > 0.0 {
            scene.fill_rect(bounds, self.idle.fade(o));
        }
        if self.message.is_some() && status.press_progress > 0.0 {
            scene.fill_rect(bounds, self.pressed.fade(o * status.press_progress));
        }
    }

    fn on_click(&self) -> Option<Msg> {
        self.message.clone()
    }

    fn focusable(&self) -> bool {
        self.message.is_some()
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        let s =
            frus_core::SemanticsProperties::new(frus_core::Role::Button).label(self.label.clone());
        Some(if self.message.is_some() {
            s.clickable()
        } else {
            s.disabled(true)
        })
    }

    fn debug_name(&self) -> &'static str {
        "CupertinoDialogAction"
    }
}

/// An action as a button, in `style`.
fn action_button<Msg: Clone + 'static>(
    action: &CupertinoDialogAction<Msg>,
    style: TextStyle,
    idle: Color,
    pressed: Color,
    min_height: f32,
    padding: Insets,
) -> ActionButton<Msg> {
    let text = crate::Text::styled(action.label.clone(), style)
        .align(TextAlign::Center)
        .no_wrap()
        .ellipsis();
    ActionButton {
        children: vec![Box::new(crate::ExcludeSemantics::new(text))],
        label: action.label.clone(),
        message: action.on_press.clone(),
        idle,
        pressed,
        min_height,
        padding,
    }
}

/// A hairline rule, across or down.
fn rule<Msg: Clone + 'static>(colour: Color, across: bool) -> crate::Container<Msg> {
    let thickness = 1.0 / crate::MediaQuery::of().device_pixel_ratio.max(1.0);
    let rule = crate::Container::new().color(colour);
    if across {
        rule.height(thickness)
    } else {
        rule.width(thickness)
    }
}

/// A column that hugs its content down and fills the width across.
fn stack_down<Msg: Clone + 'static>() -> crate::Column<Msg> {
    crate::Column::new()
        .main_axis_size(crate::MainAxisSize::Min)
        .cross_axis_alignment(Align::Stretch)
}

/// The panel an overlay shows, with its box sized and held off the edges.
struct Panel<Msg> {
    children: Vec<Box<dyn Widget<Msg>>>,
    width: Option<f32>,
    margin: Insets,
    semantics_label: String,
}

impl<Msg: Clone> Widget<Msg> for Panel<Msg> {
    fn style(&self) -> Style {
        Style {
            flex_direction: FlexDirection::Column,
            width: self.width.map_or(Dimension::Auto, Dimension::Length),
            margin: self.margin,
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        Some(
            frus_core::SemanticsProperties::new(frus_core::Role::Dialog)
                .label(self.semantics_label.clone()),
        )
    }
}

/// **The Apple-style alert**: a title and a message centred in a 270-pixel panel over the
/// screen, its buttons along the bottom — side by side when there are two that fit, one under
/// another otherwise.
///
/// It grows down into place from a little larger as it fades in, and only fades on the way
/// out. Its background is translucent over a blur of the screen; the scrim behind it is
/// Apple's. A press outside it sends nothing unless [`on_dismiss`](Self::on_dismiss) says
/// what — an alert is answered with its buttons.
pub struct CupertinoAlertDialog<Msg = crate::callback::Callback> {
    open: bool,
    on_dismiss: Option<Msg>,
    title: Option<String>,
    content: Option<String>,
    title_widget: Option<Box<dyn Widget<Msg>>>,
    content_widget: Option<Box<dyn Widget<Msg>>>,
    actions: Vec<CupertinoDialogAction<Msg>>,
    background: Option<Color>,
    scrim: Option<Color>,
    panel: Option<Box<dyn Widget<Msg>>>,
    children: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: Clone + 'static> CupertinoAlertDialog<Msg> {
    /// An alert; `open` says whether it is showing.
    pub fn new(open: bool) -> Self {
        Self {
            open,
            on_dismiss: None,
            title: None,
            content: None,
            title_widget: None,
            content_widget: None,
            actions: Vec::new(),
            background: None,
            scrim: None,
            panel: None,
            children: Vec::new(),
        }
    }

    /// Its title, 17 pixels and semibold, centred.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Its title as any widget, in the title's style.
    #[must_use]
    pub fn title_widget(mut self, title: impl Widget<Msg> + 'static) -> Self {
        self.title_widget = Some(Box::new(title));
        self
    }

    /// Its message, 13 pixels, centred.
    #[must_use]
    pub fn content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    /// Its message as any widget, in the message's style.
    #[must_use]
    pub fn content_widget(mut self, content: impl Widget<Msg> + 'static) -> Self {
        self.content_widget = Some(Box::new(content));
        self
    }

    /// Adds a button.
    #[must_use]
    pub fn action(mut self, action: CupertinoDialogAction<Msg>) -> Self {
        self.actions.push(action);
        self
    }

    /// What a press on the scrim sends. Without it the scrim is inert.
    #[must_use]
    pub fn on_dismiss(mut self, message: impl Into<Msg>) -> Self {
        self.on_dismiss = Some(message.into());
        self
    }

    /// The panel's colour, in place of Apple's translucent grey.
    #[must_use]
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// The scrim's colour, in place of Apple's.
    #[must_use]
    pub fn scrim(mut self, color: Color) -> Self {
        self.scrim = Some(color);
        self
    }

    /// Sets the screen behind it and finishes the alert.
    #[must_use]
    pub fn body(mut self, body: impl Widget<Msg> + 'static) -> Self {
        let parts = AlertParts {
            title: self.title.take(),
            content: self.content.take(),
            title_widget: self.title_widget.take(),
            content_widget: self.content_widget.take(),
            actions: std::mem::take(&mut self.actions),
            background: self.background,
        };
        self.panel = Some(Box::new(Panel {
            children: vec![Box::new(crate::themebuilder::ThemeBuilder::boxed(
                move |theme| alert_panel(parts, theme),
            ))],
            width: Some(CUPERTINO_ALERT_WIDTH),
            margin: ALERT_INSET,
            semantics_label: crate::localizations::of().alert_dialog_label().to_string(),
        }));
        self.children = vec![Box::new(body)];
        self
    }
}

/// What an alert's panel is made of.
struct AlertParts<Msg> {
    title: Option<String>,
    content: Option<String>,
    title_widget: Option<Box<dyn Widget<Msg>>>,
    content_widget: Option<Box<dyn Widget<Msg>>>,
    actions: Vec<CupertinoDialogAction<Msg>>,
    background: Option<Color>,
}

/// Whether two buttons fit side by side in an alert: each label within half the width less
/// its padding (`dialog.dart:2105`).
fn side_by_side<Msg>(actions: &[CupertinoDialogAction<Msg>], theme: &Theme) -> bool {
    if actions.len() != 2 {
        return false;
    }
    let half = CUPERTINO_ALERT_WIDTH * 0.5 - 2.0 * ALERT_BUTTON_PADDING;
    actions.iter().all(|a| {
        let style = a.style(ALERT_ACTION, theme);
        frus_text::measure_style(&a.label, style).width <= half
    })
}

fn alert_panel<Msg: Clone + 'static>(p: AlertParts<Msg>, theme: &Theme) -> Box<dyn Widget<Msg>> {
    let label = CupertinoColors::LABEL.resolve(theme);
    let divider = CupertinoColors::SEPARATOR.resolve(theme);
    let background = p.background.unwrap_or_else(|| ALERT_COLOR.resolve(theme));
    let pressed = ALERT_PRESSED.resolve(theme);

    let has_title = p.title.is_some() || p.title_widget.is_some();
    let has_content = p.content.is_some() || p.content_widget.is_some();
    let mut section = stack_down::<Msg>();
    let texted = |style: TextStyle, child: Box<dyn Widget<Msg>>| {
        crate::Themed::tweak(
            move |t: &mut Theme| {
                t.widgets.text.style = style;
                t.widgets.text.align = Some(TextAlign::Center);
            },
            child,
        )
    };
    if has_title {
        let widget: Box<dyn Widget<Msg>> = match (p.title_widget, p.title) {
            (Some(w), _) => w,
            (None, Some(t)) => Box::new(crate::Text::new(t).align(TextAlign::Center).heading()),
            (None, None) => unreachable!(),
        };
        let bottom = if has_content { 1.0 } else { ALERT_EDGE };
        section = section.child(crate::Padding::new(
            Insets::new(ALERT_EDGE, ALERT_EDGE, bottom, ALERT_EDGE),
            texted(ALERT_TITLE.color(label), widget),
        ));
    }
    if has_content {
        let widget: Box<dyn Widget<Msg>> = match (p.content_widget, p.content) {
            (Some(w), _) => w,
            (None, Some(t)) => Box::new(crate::Text::new(t).align(TextAlign::Center)),
            (None, None) => unreachable!(),
        };
        let top = if has_title { 1.0 } else { ALERT_EDGE };
        section = section.child(crate::Padding::new(
            Insets::new(top, ALERT_EDGE, ALERT_EDGE, ALERT_EDGE),
            texted(ALERT_CONTENT.color(label), widget),
        ));
    }

    let mut column = stack_down::<Msg>();
    if has_title || has_content {
        column = column.child(section);
    }
    if !p.actions.is_empty() {
        if has_title || has_content {
            column = column.child(rule(divider, true));
        }
        let padding = Insets::new(
            ALERT_BUTTON_PADDING,
            ALERT_BUTTON_PADDING,
            ALERT_BUTTON_PADDING,
            ALERT_BUTTON_PADDING,
        );
        if side_by_side(&p.actions, theme) {
            let mut row = crate::Row::new().cross_axis_alignment(Align::Stretch);
            for (i, action) in p.actions.iter().enumerate() {
                if i > 0 {
                    row = row.child(rule(divider, false));
                }
                row = row.child(crate::Expanded::new(action_button(
                    action,
                    action.style(ALERT_ACTION, theme),
                    Color::TRANSPARENT,
                    pressed,
                    ALERT_BUTTON_MIN,
                    padding,
                )));
            }
            column = column.child(row);
        } else {
            for (i, action) in p.actions.iter().enumerate() {
                if i > 0 {
                    column = column.child(rule(divider, true));
                }
                column = column.child(action_button(
                    action,
                    action.style(ALERT_ACTION, theme),
                    Color::TRANSPARENT,
                    pressed,
                    ALERT_BUTTON_MIN,
                    padding,
                ));
            }
        }
    }

    // The alert is a raised surface: the colours under it resolve at the elevated level.
    Box::new(
        crate::ClipRRect::new(ALERT_RADIUS).child(
            crate::BackdropFilter::blur(POPUP_BLUR)
                .enabled(background.a < 1.0)
                .child(crate::Container::new().color(background).child(column)),
        ),
    )
}

impl<Msg: Clone> Widget<Msg> for CupertinoAlertDialog<Msg> {
    fn style(&self) -> Style {
        Style {
            height: Dimension::Percent(1.0),
            flex_direction: FlexDirection::Column,
            ..Default::default()
        }
    }

    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        crate::widget::FillAxes::WIDTH
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn overlay(&self) -> Option<(&dyn Widget<Msg>, Placement)> {
        self.panel.as_ref().map(|p| (p.as_ref(), Placement::Center))
    }

    fn overlay_dismiss(&self) -> Option<Msg> {
        self.on_dismiss.clone()
    }

    fn overlay_scrim(&self, theme: &Theme) -> Option<Color> {
        Some(
            self.scrim
                .unwrap_or_else(|| CUPERTINO_MODAL_BARRIER.resolve(theme)),
        )
    }

    /// Down from 1.3 times its size as it fades in; faded only on the way out
    /// (`route.dart:1531`).
    fn overlay_presentation(&self) -> Option<OverlayPresentation> {
        Some(if self.open {
            OverlayPresentation::fade_and_scale(ALERT_SCALE_FROM)
        } else {
            OverlayPresentation::fade()
        })
    }

    fn anim_target(&self) -> Option<f32> {
        Some(if self.open { 1.0 } else { 0.0 })
    }

    fn anim_duration(&self) -> f32 {
        ALERT_SECONDS
    }

    /// A quick start and a long settle, as the spring the reference runs it on.
    fn anim_curve(&self) -> frus_core::Curve {
        frus_core::Curve::ease_out()
    }

    fn debug_name(&self) -> &'static str {
        "CupertinoAlertDialog"
    }
}

/// **The Apple-style action sheet**: a set of choices that slides up from the bottom of the
/// screen — an optional title and message over the actions in one rounded panel, and a cancel
/// button apart under it.
///
/// ```ignore
/// CupertinoActionSheet::new(app.share_open)
///     .title("Share")
///     .action(CupertinoActionSheetAction::new("Copy link", Msg::Copy))
///     .action(CupertinoActionSheetAction::new("Delete", Msg::Delete).destructive())
///     .cancel(CupertinoActionSheetAction::new("Cancel", Msg::Close))
///     .on_dismiss(Msg::Close)
///     .body(screen)
/// ```
///
/// It is the screen's width less 8 pixels either side — as wide as the screen is tall, in
/// landscape — and keeps clear of the system's bars at the bottom.
pub struct CupertinoActionSheet<Msg = crate::callback::Callback> {
    open: bool,
    on_dismiss: Option<Msg>,
    title: Option<String>,
    message: Option<String>,
    actions: Vec<CupertinoActionSheetAction<Msg>>,
    cancel: Option<CupertinoActionSheetAction<Msg>>,
    scrim: Option<Color>,
    panel: Option<Box<dyn Widget<Msg>>>,
    children: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: Clone + 'static> CupertinoActionSheet<Msg> {
    /// A sheet; `open` says whether it is showing.
    pub fn new(open: bool) -> Self {
        Self {
            open,
            on_dismiss: None,
            title: None,
            message: None,
            actions: Vec::new(),
            cancel: None,
            scrim: None,
            panel: None,
            children: Vec::new(),
        }
    }

    /// Its title, over the message.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Its message.
    #[must_use]
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Adds a choice.
    #[must_use]
    pub fn action(mut self, action: CupertinoActionSheetAction<Msg>) -> Self {
        self.actions.push(action);
        self
    }

    /// The cancel button, apart under the choices.
    #[must_use]
    pub fn cancel(mut self, action: CupertinoActionSheetAction<Msg>) -> Self {
        self.cancel = Some(action);
        self
    }

    /// What a press on the scrim sends — usually what the cancel button sends.
    #[must_use]
    pub fn on_dismiss(mut self, message: impl Into<Msg>) -> Self {
        self.on_dismiss = Some(message.into());
        self
    }

    /// The scrim's colour, in place of Apple's.
    #[must_use]
    pub fn scrim(mut self, color: Color) -> Self {
        self.scrim = Some(color);
        self
    }

    /// Sets the screen behind it and finishes the sheet.
    #[must_use]
    pub fn body(mut self, body: impl Widget<Msg> + 'static) -> Self {
        let parts = SheetParts {
            title: self.title.take(),
            message: self.message.take(),
            actions: std::mem::take(&mut self.actions),
            cancel: self.cancel.take(),
        };
        self.panel = Some(Box::new(Panel {
            children: vec![Box::new(crate::themebuilder::ThemeBuilder::boxed(
                move |theme| sheet_panel(parts, theme),
            ))],
            width: None,
            margin: Insets::ZERO,
            semantics_label: crate::localizations::of().alert_dialog_label().to_string(),
        }));
        self.children = vec![Box::new(body)];
        self
    }
}

/// What a sheet's panel is made of.
struct SheetParts<Msg> {
    title: Option<String>,
    message: Option<String>,
    actions: Vec<CupertinoActionSheetAction<Msg>>,
    cancel: Option<CupertinoActionSheetAction<Msg>>,
}

fn sheet_panel<Msg: Clone + 'static>(p: SheetParts<Msg>, theme: &Theme) -> Box<dyn Widget<Msg>> {
    let media = crate::MediaQuery::of();
    let background = SHEET_COLOR.resolve(theme);
    let divider = SHEET_DIVIDER.resolve(theme);
    let pressed = SHEET_PRESSED.resolve(theme);
    let text = SHEET_CONTENT_TEXT.resolve(theme);
    let button_v = 1.8 + SHEET_ACTION.size.unwrap_or(17.0) * 0.4;
    let button_padding = Insets::new(button_v, SHEET_BUTTON_H, button_v, SHEET_BUTTON_H);

    // The title and the message: the one that stands alone, or the title over a message,
    // is semibold (`dialog.dart:1178`).
    let mut main = stack_down::<Msg>();
    let has_content = p.title.is_some() || p.message.is_some();
    if has_content {
        let mut section = stack_down::<Msg>();
        let bold = SHEET_CONTENT
            .color(text)
            .weight(frus_core::FontWeight::SemiBold);
        let plain = SHEET_CONTENT.color(text);
        let has_message = p.message.is_some();
        if let Some(title) = &p.title {
            let style = if has_message { bold } else { plain };
            section = section.child(crate::Padding::new(
                Insets::new(
                    SHEET_CONTENT_V,
                    SHEET_CONTENT_H,
                    if has_message { 0.0 } else { SHEET_CONTENT_V },
                    SHEET_CONTENT_H,
                ),
                crate::Text::styled(title.clone(), style)
                    .align(TextAlign::Center)
                    .heading(),
            ));
        }
        if let Some(message) = &p.message {
            let style = if p.title.is_some() { plain } else { bold };
            let top = if p.title.is_some() {
                4.0
            } else {
                SHEET_CONTENT_V
            };
            section = section.child(crate::Padding::new(
                Insets::new(top, SHEET_CONTENT_H, SHEET_CONTENT_V, SHEET_CONTENT_H),
                crate::Text::styled(message.clone(), style).align(TextAlign::Center),
            ));
        }
        main = main.child(section);
    }
    for (i, action) in p.actions.iter().enumerate() {
        if i > 0 || has_content {
            main = main.child(rule(divider, true));
        }
        main = main.child(action_button(
            action,
            action.style(SHEET_ACTION, theme),
            Color::TRANSPARENT,
            pressed,
            SHEET_BUTTON_MIN,
            button_padding,
        ));
    }

    let mut column = stack_down::<Msg>().child(
        crate::ClipRRect::new(SHEET_RADIUS).child(
            crate::BackdropFilter::blur(POPUP_BLUR)
                .child(crate::Container::new().color(background).child(main)),
        ),
    );
    if let Some(cancel) = &p.cancel {
        let gap = if has_content || !p.actions.is_empty() {
            SHEET_EDGE
        } else {
            0.0
        };
        column = column.child(crate::Padding::new(
            Insets::new(gap, 0.0, 0.0, 0.0),
            crate::ClipRRect::new(SHEET_RADIUS).child(action_button(
                cancel,
                cancel.style(SHEET_ACTION, theme),
                SHEET_CANCEL.resolve(theme),
                SHEET_CANCEL_PRESSED.resolve(theme),
                SHEET_BUTTON_MIN,
                button_padding,
            )),
        ));
    }

    // As wide as the screen in portrait and as it is tall in landscape, less the edges; clear
    // of the system's bars at the bottom, 8 pixels at least (`dialog.dart:1331`).
    let bottom = media.padding.bottom.max(SHEET_EDGE);
    let sheet: Box<dyn Widget<Msg>> = match media.orientation() {
        crate::Orientation::Portrait => Box::new(column),
        crate::Orientation::Landscape => Box::new(crate::Center::new(
            crate::SizedBox::new(column).width((media.size.height - 2.0 * SHEET_EDGE).max(0.0)),
        )),
    };
    Box::new(crate::Padding::new(
        Insets::new(0.0, SHEET_EDGE, bottom, SHEET_EDGE),
        sheet,
    ))
}

impl<Msg: Clone> Widget<Msg> for CupertinoActionSheet<Msg> {
    fn style(&self) -> Style {
        Style {
            height: Dimension::Percent(1.0),
            flex_direction: FlexDirection::Column,
            ..Default::default()
        }
    }

    fn fill_axes(&self, _theme: &Theme) -> crate::widget::FillAxes {
        crate::widget::FillAxes::WIDTH
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.children
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn overlay(&self) -> Option<(&dyn Widget<Msg>, Placement)> {
        self.panel.as_ref().map(|p| (p.as_ref(), Placement::Bottom))
    }

    fn overlay_dismiss(&self) -> Option<Msg> {
        self.on_dismiss.clone()
    }

    fn overlay_scrim(&self, theme: &Theme) -> Option<Color> {
        Some(
            self.scrim
                .unwrap_or_else(|| CUPERTINO_MODAL_BARRIER.resolve(theme)),
        )
    }

    fn anim_target(&self) -> Option<f32> {
        Some(if self.open { 1.0 } else { 0.0 })
    }

    fn anim_duration(&self) -> f32 {
        SHEET_SECONDS
    }

    fn debug_name(&self) -> &'static str {
        "CupertinoActionSheet"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cupertinobutton::AllPrimitives;
    use crate::{build_ui, Container, MediaQuery, Runtime, Ui, WidgetId};
    use frus_core::{Point, Primitive, Size};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Cancel,
        Delete,
        Other,
    }

    const SCREEN: Size = Size::new(400.0, 700.0);

    fn screen() -> Container<Msg> {
        Container::new().width(400.0).height(700.0)
    }

    fn lay(root: &dyn Widget<Msg>, theme: &Theme, rt: &Runtime) -> Ui<Msg> {
        let _surface = MediaQuery::new(SCREEN).install();
        build_ui(root, SCREEN, rt, theme)
    }

    fn alert(open: bool) -> CupertinoAlertDialog<Msg> {
        CupertinoAlertDialog::new(open)
            .title("Delete?")
            .content("This cannot be undone.")
            .action(CupertinoDialogAction::new("Cancel", Msg::Cancel).default_action())
            .action(CupertinoDialogAction::new("Delete", Msg::Delete).destructive())
    }

    fn buttons(ui: &Ui<Msg>) -> Vec<(WidgetId, Rect, String)> {
        ui.semantics()
            .iter()
            .filter(|(_, _, s)| s.role == frus_core::Role::Button)
            .map(|(id, r, s)| (*id, *r, s.label.clone().unwrap_or_default()))
            .collect()
    }

    fn text_colour(ui: &Ui<Msg>, label: &str) -> (Color, frus_core::FontWeight) {
        ui.scene()
            .all()
            .into_iter()
            .find_map(|p| match p {
                Primitive::Text {
                    text,
                    color,
                    weight,
                    ..
                } if text == label => Some((*color, *weight)),
                _ => None,
            })
            .expect("drawn")
    }

    /// **Closed, nothing shows; open, a 270-pixel panel in the middle of the screen**, over
    /// Apple's scrim, announced as an alert.
    #[test]
    fn open_it_is_a_270_pixel_panel_in_the_middle() {
        let theme = Theme::light();
        let rt = Runtime::default();
        let closed = alert(false).body(screen());
        assert!(buttons(&lay(&closed, &theme, &rt)).is_empty());
        let open = alert(true).body(screen());
        let ui = lay(&open, &theme, &rt);
        let (_, panel, node) = ui
            .semantics()
            .iter()
            .find(|(_, _, s)| s.role == frus_core::Role::Dialog)
            .cloned()
            .expect("announced as a dialog");
        assert_eq!(node.label.as_deref(), Some("Alert"));
        assert_eq!(panel.width, CUPERTINO_ALERT_WIDTH);
        assert!((panel.x + panel.width * 0.5 - 200.0).abs() < 0.5);
        assert!((panel.y + panel.height * 0.5 - 350.0).abs() < 0.5);
        let scrim = ui.scene().all().into_iter().any(|p| {
            matches!(p, Primitive::Rect { color, rect, .. }
                if *color == CUPERTINO_MODAL_BARRIER.color && rect.width == 400.0)
        });
        assert!(scrim, "Apple's scrim behind it");
    }

    /// **Two buttons that fit sit side by side, each half the width; three are stacked.**
    #[test]
    fn two_fit_side_by_side_three_are_stacked() {
        let theme = Theme::light();
        let rt = Runtime::default();
        let ui = lay(&alert(true).body(screen()), &theme, &rt);
        let b = buttons(&ui);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].1.y, b[1].1.y, "side by side");
        assert!(b[0].1.height >= ALERT_BUTTON_MIN);
        // 270 less a one-pixel rule splits into 135 and 134 once snapped to pixels.
        assert!((b[0].1.width - b[1].1.width).abs() <= 1.0);
        let three = alert(true)
            .action(CupertinoDialogAction::new("Other", Msg::Other))
            .body(screen());
        let b = buttons(&lay(&three, &theme, &rt));
        assert_eq!(b.len(), 3);
        assert!(
            b[1].1.y > b[0].1.y && b[2].1.y > b[1].1.y,
            "one under another"
        );
        assert!((b[0].1.width - CUPERTINO_ALERT_WIDTH).abs() < 0.5);
        // A label too long for half the width stacks two as well.
        let long = CupertinoAlertDialog::new(true)
            .action(CupertinoDialogAction::new(
                "Keep a copy on this device",
                Msg::Other,
            ))
            .action(CupertinoDialogAction::new("Delete", Msg::Delete))
            .body(screen());
        let b = buttons(&lay(&long, &theme, &rt));
        assert!(b[1].1.y > b[0].1.y);
    }

    /// **The default action is semibold, the destructive one red, and each sends its
    /// message.**
    #[test]
    fn the_default_is_bold_and_the_destructive_red() {
        let theme = Theme::light();
        let rt = Runtime::default();
        let ui = lay(&alert(true).body(screen()), &theme, &rt);
        let (cancel, weight) = text_colour(&ui, "Cancel");
        assert_eq!(cancel, CupertinoTheme::of(&theme).primary_color);
        assert_eq!(weight, frus_core::FontWeight::SemiBold);
        let (delete, weight) = text_colour(&ui, "Delete");
        assert_eq!(delete, CupertinoColors::SYSTEM_RED.resolve(&theme));
        assert_eq!(weight, frus_core::FontWeight::Regular);
        let b = buttons(&ui);
        assert_eq!(ui.msg_for(b[0].0), Some(Msg::Cancel));
        assert_eq!(ui.msg_for(b[1].0), Some(Msg::Delete));
        // In the dark, the red is the dark red.
        let dark = Theme::dark();
        let ui = lay(&alert(true).body(screen()), &dark, &rt);
        assert_eq!(
            text_colour(&ui, "Delete").0,
            CupertinoColors::SYSTEM_RED.dark_color
        );
    }

    /// **It comes in faded and from 1.3 times its size, and goes out faded only.**
    #[test]
    fn it_grows_down_as_it_fades_in() {
        let open = alert(true).body(screen());
        assert_eq!(
            Widget::<Msg>::overlay_presentation(&open),
            Some(OverlayPresentation::fade_and_scale(1.3))
        );
        let closing = alert(false).body(screen());
        assert_eq!(
            Widget::<Msg>::overlay_presentation(&closing),
            Some(OverlayPresentation::fade())
        );
        assert_eq!(Widget::<Msg>::anim_duration(&open), 0.25);
        let (opacity, scale) = OverlayPresentation::fade_and_scale(1.3).at(0.5);
        assert!((opacity - 0.5).abs() < 1e-6 && (scale - 1.15).abs() < 1e-6);

        // Half way in, the panel is one layer at half opacity, scaled about its centre.
        let theme = Theme::light();
        let mut rt = Runtime::default();
        rt.set_value(WidgetId::ROOT, 0.5);
        let ui = lay(&open, &theme, &rt);
        let (opacity, transform) = ui
            .scene()
            .all()
            .into_iter()
            .find_map(|p| match p {
                Primitive::Layer {
                    opacity, transform, ..
                } if transform.is_some() => Some((*opacity, *transform)),
                _ => None,
            })
            .expect("the entering panel is a transformed layer");
        assert!((opacity - 0.5).abs() < 1e-6);
        let centre = transform.unwrap().affine.apply(Point::new(200.0, 350.0));
        assert!((centre.x - 200.0).abs() < 0.5 && (centre.y - 350.0).abs() < 0.5);
        // Settled, no layer is left over the panel.
        let ui = lay(&open, &theme, &Runtime::default());
        assert!(!ui
            .scene()
            .all()
            .into_iter()
            .any(|p| matches!(p, Primitive::Layer { transform, .. } if transform.is_some())));
    }

    /// **The sheet slides up from the bottom, 8 pixels in from the edges, its cancel button
    /// apart under its choices**, and a press on the scrim sends what it was told.
    #[test]
    fn the_sheet_sits_at_the_bottom_with_its_cancel_apart() {
        let theme = Theme::light();
        let rt = Runtime::default();
        let sheet = CupertinoActionSheet::new(true)
            .title("Share")
            .message("Choose where to send it")
            .action(CupertinoActionSheetAction::new("Copy", Msg::Other))
            .action(CupertinoActionSheetAction::new("Delete", Msg::Delete).destructive())
            .cancel(CupertinoActionSheetAction::new("Cancel", Msg::Cancel))
            .on_dismiss(Msg::Cancel)
            .body(screen());
        assert_eq!(Widget::<Msg>::overlay_dismiss(&sheet), Some(Msg::Cancel));
        let ui = lay(&sheet, &theme, &rt);
        let b = buttons(&ui);
        assert_eq!(b.len(), 3);
        let cancel = b.iter().find(|(_, _, l)| l == "Cancel").unwrap().1;
        let delete = b.iter().find(|(_, _, l)| l == "Delete").unwrap().1;
        assert_eq!(cancel.x, 8.0);
        assert_eq!(cancel.width, 400.0 - 16.0);
        assert!((cancel.y + cancel.height - (700.0 - 8.0)).abs() < 0.5);
        assert!((cancel.y - (delete.y + delete.height) - 8.0).abs() < 0.5);
        assert!(cancel.height >= SHEET_BUTTON_MIN - 0.01);
        assert_eq!(
            text_colour(&ui, "Delete").0,
            CupertinoColors::SYSTEM_RED.resolve(&theme)
        );
        // The title over a message is semibold, in the sheet's grey.
        let (grey, weight) = text_colour(&ui, "Share");
        assert_eq!(weight, frus_core::FontWeight::SemiBold);
        assert_eq!(grey, SHEET_CONTENT_TEXT.resolve(&theme));
        // The cancel button has its own opaque background.
        assert!(ui.scene().all().into_iter().any(|p| matches!(p,
            Primitive::Rect { color, rect, .. }
                if *color == SHEET_CANCEL.resolve(&theme) && *rect == cancel)));
    }

    /// **Right to left, an alert's two buttons swap sides.**
    #[test]
    fn right_to_left_the_buttons_swap() {
        let ui = lay(
            &alert(true).body(screen()),
            &Theme::light().rtl(),
            &Runtime::default(),
        );
        let b = buttons(&ui);
        let cancel = b.iter().find(|(_, _, l)| l == "Cancel").unwrap().1;
        let delete = b.iter().find(|(_, _, l)| l == "Delete").unwrap().1;
        assert!(cancel.x > delete.x);
    }
}
