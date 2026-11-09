// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`SplitButton`]: a leading action and a trailing toggle that opens a menu of related
//! ones, as one control (milestone 786) — and [`ButtonSize`], the five sizes the
//! expressive buttons share.

use frus_core::{BorderRadius, BorderRadiusDirectional, ShapeBorder, TextStyle};

use crate::button::{Button, ButtonStyle, Variant};
use crate::icons::{IconData, Icons};
use crate::lazy::Lazy;
use crate::menu::{MenuItem, PopupMenuButton};
use crate::semantics::Semantics;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::widget::Widget;
use crate::widgetstate::{WidgetState, WidgetStateProperty};
use crate::{Flex, Icon};

/// The five sizes of the expressive buttons — a [`SplitButton`], a
/// [`ButtonGroup`](crate::ButtonGroup) — each with its own height, glyph, room and type.
///
/// One enum rather than four numbers, because each of them follows from **which** size it
/// is: an extra-large button with a small one's glyph is not a size, it is a mistake.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum ButtonSize {
    /// 32 tall, a 20 glyph, 12 either side, `label_large`.
    ExtraSmall,
    /// 40 tall, a 20 glyph, 16 either side, `label_large`. The default, and the size of
    /// every ordinary button.
    #[default]
    Small,
    /// 56 tall, a 24 glyph, 24 either side, `title_medium`.
    Medium,
    /// 96 tall, a 32 glyph, 48 either side, `headline_small`.
    Large,
    /// 136 tall, a 40 glyph, 64 either side, `headline_large`.
    ExtraLarge,
}

impl ButtonSize {
    /// The container's height.
    pub const fn height(self) -> f32 {
        match self {
            ButtonSize::ExtraSmall => 32.0,
            ButtonSize::Small => 40.0,
            ButtonSize::Medium => 56.0,
            ButtonSize::Large => 96.0,
            ButtonSize::ExtraLarge => 136.0,
        }
    }

    /// The glyph's side.
    pub const fn icon_size(self) -> f32 {
        match self {
            ButtonSize::ExtraSmall | ButtonSize::Small => 20.0,
            ButtonSize::Medium => 24.0,
            ButtonSize::Large => 32.0,
            ButtonSize::ExtraLarge => 40.0,
        }
    }

    /// The room between the container's edge and its content.
    pub const fn padding(self) -> f32 {
        match self {
            ButtonSize::ExtraSmall => 12.0,
            ButtonSize::Small => 16.0,
            ButtonSize::Medium => 24.0,
            ButtonSize::Large => 48.0,
            ButtonSize::ExtraLarge => 64.0,
        }
    }

    /// The corner of the **square** shape, and the one a round button takes while
    /// pressed — the shape morph of the expressive buttons.
    pub const fn square_radius(self) -> f32 {
        match self {
            ButtonSize::ExtraSmall | ButtonSize::Small => 12.0,
            ButtonSize::Medium => 16.0,
            ButtonSize::Large | ButtonSize::ExtraLarge => 28.0,
        }
    }

    /// The corner a pressed button morphs to.
    pub const fn pressed_radius(self) -> f32 {
        match self {
            ButtonSize::ExtraSmall | ButtonSize::Small => 8.0,
            ButtonSize::Medium => 12.0,
            ButtonSize::Large | ButtonSize::ExtraLarge => 16.0,
        }
    }

    /// The label's type in `theme`'s scale.
    pub fn label_style(self, theme: &Theme) -> TextStyle {
        match self {
            ButtonSize::ExtraSmall | ButtonSize::Small => theme.text.label_large,
            ButtonSize::Medium => theme.text.title_medium,
            ButtonSize::Large => theme.text.headline_small,
            ButtonSize::ExtraLarge => theme.text.headline_large,
        }
    }

    /// A split button's **inner** corners, the two either side of the seam.
    pub const fn split_inner_radius(self) -> f32 {
        match self {
            ButtonSize::ExtraSmall | ButtonSize::Small | ButtonSize::Medium => 4.0,
            ButtonSize::Large => 8.0,
            ButtonSize::ExtraLarge => 12.0,
        }
    }

    /// A split button's inner corners while that half is pressed.
    pub const fn split_pressed_inner_radius(self) -> f32 {
        match self {
            ButtonSize::ExtraSmall | ButtonSize::Small => 8.0,
            ButtonSize::Medium => 12.0,
            ButtonSize::Large | ButtonSize::ExtraLarge => 20.0,
        }
    }

    /// The width of a split button's trailing half.
    pub const fn split_trailing_width(self) -> f32 {
        match self {
            ButtonSize::ExtraSmall => 32.0,
            ButtonSize::Small => 48.0,
            ButtonSize::Medium => 56.0,
            ButtonSize::Large => 96.0,
            ButtonSize::ExtraLarge => 136.0,
        }
    }

    /// The chevron's side on a split button's trailing half.
    pub const fn split_trailing_icon_size(self) -> f32 {
        match self {
            ButtonSize::ExtraSmall | ButtonSize::Small => 22.0,
            ButtonSize::Medium => 26.0,
            ButtonSize::Large => 38.0,
            ButtonSize::ExtraLarge => 50.0,
        }
    }
}

/// The gap between a split button's two halves.
pub const SPLIT_BUTTON_GAP: f32 = 2.0;

/// Defaults for [`SplitButton`]. Each field unset is the size's own.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SplitButtonTheme {
    /// The size, unless the button names one.
    pub size: Option<ButtonSize>,
    /// The colours, unless the button names a variant.
    pub variant: Option<Variant>,
    /// The gap between the halves. Unset, [`SPLIT_BUTTON_GAP`].
    pub gap: Option<f32>,
    /// The two corners either side of the seam.
    pub inner_radius: Option<f32>,
    /// Those corners while their half is pressed.
    pub pressed_inner_radius: Option<f32>,
    /// The label's type.
    pub label_style: Option<TextStyle>,
}

struct Parts<Msg> {
    label: String,
    icon: Option<IconData>,
    on_press: Option<Msg>,
    open: bool,
    on_toggle: Option<Msg>,
    items: Vec<MenuItem<Msg>>,
    size: Option<ButtonSize>,
    variant: Option<Variant>,
    enabled: bool,
    gap: Option<f32>,
    inner_radius: Option<f32>,
    pressed_inner_radius: Option<f32>,
    label_style: Option<TextStyle>,
    trailing_icon: Option<IconData>,
    trailing_label: Option<String>,
}

/// **A leading action and a trailing menu toggle**, joined: the action most people want,
/// and the related ones a press away.
///
/// ```
/// # use frus_widgets::{ButtonSize, Icons, SplitButton, Variant};
/// # #[derive(Clone)] enum Msg { Send, ToggleMenu, Schedule, Draft }
/// let menu_open = false;
/// SplitButton::<Msg>::new("Send", Msg::Send)
///     .icon(Icons::SEND)
///     .size(ButtonSize::Medium)
///     .variant(Variant::Tonal)
///     .menu(menu_open, Msg::ToggleMenu)
///     .item("Schedule send", Msg::Schedule)
///     .item("Save draft", Msg::Draft);
/// ```
///
/// The two halves sit two pixels apart, round on the outside and nearly square at the
/// seam; a pressed half rounds its seam corners further, and the trailing half turns into
/// a pill while its menu is open, its chevron pointing up. Both are the theme's
/// [`Button`]s of the chosen [`Variant`] — filled, tonal, elevated or outlined.
///
/// The menu is controlled: `open` says whether it shows, and `on_toggle` is sent by the
/// trailing half and by a press outside the open menu. It traps the keyboard while open,
/// and Escape or the system's Back close it. A reader hears the trailing half as "More
/// options", expanded or collapsed.
pub struct SplitButton<Msg = crate::callback::Callback> {
    inner: Lazy<Parts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> SplitButton<Msg> {
    /// A split button whose leading half reads `label` and sends `on_press`.
    pub fn new(label: impl Into<String>, on_press: impl Into<Msg>) -> Self {
        Self {
            inner: Lazy::new(
                Parts {
                    label: label.into(),
                    icon: None,
                    on_press: Some(on_press.into()),
                    open: false,
                    on_toggle: None,
                    items: Vec::new(),
                    size: None,
                    variant: None,
                    enabled: true,
                    gap: None,
                    inner_radius: None,
                    pressed_inner_radius: None,
                    label_style: None,
                    trailing_icon: None,
                    trailing_label: None,
                },
                build::<Msg>,
            ),
        }
    }

    fn p(&mut self) -> &mut Parts<Msg> {
        self.inner.parts_mut()
    }

    /// A glyph before the leading label.
    #[must_use]
    pub fn icon(mut self, icon: IconData) -> Self {
        self.p().icon = Some(icon);
        self
    }

    /// Which of the five sizes. Unset, the theme's, then [`ButtonSize::Small`].
    #[must_use]
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.p().size = Some(size);
        self
    }

    /// The colours: [`Variant::Filled`] (the default), `Tonal`, `Elevated` or `Outlined`.
    #[must_use]
    pub fn variant(mut self, variant: Variant) -> Self {
        self.p().variant = Some(variant);
        self
    }

    /// Whether the menu shows, and what the trailing half — and a press outside the open
    /// menu — sends to change that.
    #[must_use]
    pub fn menu(mut self, open: bool, on_toggle: impl Into<Msg>) -> Self {
        let p = self.p();
        p.open = open;
        p.on_toggle = Some(on_toggle.into());
        self
    }

    /// A row of the menu.
    #[must_use]
    pub fn item(self, label: impl Into<String>, message: impl Into<Msg>) -> Self {
        self.entry(MenuItem::new(label, message.into()))
    }

    /// A row of the menu, said in full — a glyph, a tick, a shortcut, a divider.
    #[must_use]
    pub fn entry(mut self, item: MenuItem<Msg>) -> Self {
        self.p().items.push(item);
        self
    }

    /// Whether either half can be pressed.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.p().enabled = enabled;
        self
    }

    /// The gap between the halves. Two by default.
    #[must_use]
    pub fn gap(mut self, gap: f32) -> Self {
        self.p().gap = Some(gap);
        self
    }

    /// The corners either side of the seam, and those corners while pressed.
    #[must_use]
    pub fn inner_radius(mut self, radius: f32, pressed: f32) -> Self {
        let p = self.p();
        p.inner_radius = Some(radius);
        p.pressed_inner_radius = Some(pressed);
        self
    }

    /// The leading label's type. Unset, the size's.
    #[must_use]
    pub fn label_style(mut self, style: TextStyle) -> Self {
        self.p().label_style = Some(style);
        self
    }

    /// The trailing glyph, over the chevron.
    #[must_use]
    pub fn trailing_icon(mut self, icon: IconData) -> Self {
        self.p().trailing_icon = Some(icon);
        self
    }

    /// What a reader hears on the trailing half. Unset, "More options".
    #[must_use]
    pub fn trailing_label(mut self, label: impl Into<String>) -> Self {
        self.p().trailing_label = Some(label.into());
        self
    }
}

crate::composite::composite_widget!(SplitButton);

/// A half's shape: round outside, `inner` at the seam, `pressed` there while held.
/// `leading` says which half, in reading order; the corners follow the direction.
fn half_shape(theme: &Theme, leading: bool, outer: f32, inner: f32) -> ShapeBorder {
    let radii = if leading {
        BorderRadiusDirectional::horizontal(outer, inner)
    } else {
        BorderRadiusDirectional::horizontal(inner, outer)
    };
    ShapeBorder::rounded(radii.resolve(theme.direction))
}

fn build<Msg: Clone + 'static>(p: Parts<Msg>) -> Box<dyn Widget<Msg>> {
    Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
        let t = theme.widgets.split_button;
        let size = p.size.or(t.size).unwrap_or_default();
        let variant = p.variant.or(t.variant).unwrap_or_default();
        let outer = size.height() * 0.5;
        let inner = p
            .inner_radius
            .or(t.inner_radius)
            .unwrap_or(size.split_inner_radius());
        let pressed = p
            .pressed_inner_radius
            .or(t.pressed_inner_radius)
            .unwrap_or(size.split_pressed_inner_radius());
        let label_style = p
            .label_style
            .or(t.label_style)
            .unwrap_or(size.label_style(theme));
        let morph = |leading: bool| {
            WidgetStateProperty::new()
                .when(
                    WidgetState::Pressed,
                    half_shape(theme, leading, outer, pressed),
                )
                .otherwise(half_shape(theme, leading, outer, inner))
        };

        // The leading half: the action.
        let pad = size.padding();
        let leading_style = ButtonStyle {
            shape: Some(morph(true)),
            icon_size: Some(WidgetStateProperty::all(size.icon_size())),
            text_style: Some(WidgetStateProperty::all(label_style)),
            ..ButtonStyle::default()
        }
        .padding(WidgetStateProperty::all(directional_padding(
            theme,
            pad,
            (pad - 4.0).max(8.0),
        )));
        let mut leading = Button::new(p.label.clone())
            .variant(variant)
            .style(leading_style)
            .height(size.height())
            .enabled(p.enabled);
        if let Some(icon) = p.icon {
            leading = leading.icon(Icon::new(icon));
        }
        if let Some(message) = p.on_press.clone() {
            leading = leading.on_press(message);
        }

        // The trailing half: the toggle. Open, it is a pill all round.
        let trailing_shape = if p.open {
            WidgetStateProperty::all(ShapeBorder::rounded(BorderRadius::uniform(outer)))
        } else {
            morph(false)
        };
        let glyph = p.trailing_icon.unwrap_or(if p.open {
            Icons::KEYBOARD_ARROW_UP
        } else {
            Icons::KEYBOARD_ARROW_DOWN
        });
        let trailing_style = ButtonStyle {
            shape: Some(trailing_shape),
            icon_size: Some(WidgetStateProperty::all(size.split_trailing_icon_size())),
            padding: Some(WidgetStateProperty::all(frus_core::Insets::ZERO)),
            fixed_size: Some(WidgetStateProperty::all(frus_core::Size::new(
                size.split_trailing_width(),
                size.height(),
            ))),
            minimum_size: Some(WidgetStateProperty::all(frus_core::Size::new(
                size.split_trailing_width(),
                size.height(),
            ))),
            ..ButtonStyle::default()
        };
        let mut trailing = Button::with_child(Icon::new(glyph))
            .variant(variant)
            .style(trailing_style)
            .height(size.height())
            .enabled(p.enabled);
        if let Some(message) = p.on_toggle.clone() {
            trailing = trailing.on_press(message);
        }
        let name = p
            .trailing_label
            .clone()
            .unwrap_or_else(|| crate::localizations::of().more_options_label().to_string());
        let trailing = Semantics::new(
            frus_core::SemanticsProperties::new(frus_core::Role::Button)
                .label(name)
                .expanded(p.open),
            trailing,
        )
        .merging(true);

        let gap = p.gap.or(t.gap).unwrap_or(SPLIT_BUTTON_GAP);
        let row = Flex::row().gap(gap).align(frus_layout::Align::Center);
        let row = row.child(leading);
        match p.on_toggle {
            Some(toggle) => {
                let mut menu = PopupMenuButton::new(trailing, p.open, toggle);
                for item in p.items {
                    menu = menu.entry(item);
                }
                Box::new(row.child(menu)) as Box<dyn Widget<Msg>>
            }
            None => Box::new(row.child(trailing)),
        }
    }))
}

/// Room at the start and the end of the content, in reading order.
fn directional_padding(theme: &Theme, start: f32, end: f32) -> frus_core::Insets {
    if theme.direction == frus_core::TextDirection::Rtl {
        frus_core::Insets::new(0.0, start, 0.0, end)
    } else {
        frus_core::Insets::new(0.0, end, 0.0, start)
    }
}
