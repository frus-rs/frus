// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`ButtonGroup`]: a row of toggle buttons that choose one or several things, standard
//! (apart) or connected (joined at the seams) — milestone 786.

use std::rc::Rc;

use frus_core::{BorderRadius, BorderRadiusDirectional, Color, ShapeBorder, TextStyle};

use crate::button::{Button, ButtonStyle, Variant};
use crate::icons::IconData;
use crate::lazy::Lazy;
use crate::semantics::Semantics;
use crate::splitbutton::ButtonSize;
use crate::theme::Theme;
use crate::themebuilder::ThemeBuilder;
use crate::widget::Widget;
use crate::widgetstate::{StateFilter, WidgetState, WidgetStateProperty};
use crate::{Flex, Icon};

/// How many of a group's buttons may be on at once.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum GroupSelection {
    /// One at most — a choice among options, read as radio buttons.
    #[default]
    Single,
    /// Any number — a set of independent switches, read as toggle buttons.
    Multiple,
}

/// The gap between a **connected** group's buttons.
pub const CONNECTED_GROUP_GAP: f32 = 2.0;

/// The gap between a **standard** group's buttons, by size.
pub const fn standard_group_gap(size: ButtonSize) -> f32 {
    match size {
        ButtonSize::ExtraSmall => 18.0,
        ButtonSize::Small => 12.0,
        _ => 8.0,
    }
}

/// The corners at a **connected** group's inner seams, by size.
pub const fn connected_inner_radius(size: ButtonSize) -> f32 {
    match size {
        ButtonSize::ExtraSmall => 4.0,
        ButtonSize::Small | ButtonSize::Medium => 8.0,
        ButtonSize::Large => 16.0,
        ButtonSize::ExtraLarge => 20.0,
    }
}

/// Defaults for [`ButtonGroup`]. Each field unset is the size's and the scheme's own.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ButtonGroupTheme {
    /// The size, unless the group names one.
    pub size: Option<ButtonSize>,
    /// The gap between a standard group's buttons.
    pub gap: Option<f32>,
    /// The gap between a connected group's buttons.
    pub connected_gap: Option<f32>,
    /// The corners at a connected group's inner seams.
    pub inner_radius: Option<f32>,
    /// An unselected button's surface. Unset, `secondary_container`.
    pub background: Option<Color>,
    /// An unselected button's content. Unset, `on_secondary_container`.
    pub foreground: Option<Color>,
    /// A selected button's surface. Unset, `secondary`.
    pub selected_background: Option<Color>,
    /// A selected button's content. Unset, `on_secondary`.
    pub selected_foreground: Option<Color>,
    /// The labels' type. Unset, the size's.
    pub label_style: Option<TextStyle>,
}

struct Entry {
    label: String,
    icon: Option<IconData>,
    /// Whether the label is drawn, or only said.
    shows_label: bool,
    enabled: bool,
}

type OnSelect<Msg> = Rc<dyn Fn(usize) -> Msg>;

struct Parts<Msg> {
    entries: Vec<Entry>,
    selected: Vec<usize>,
    selection: GroupSelection,
    on_select: OnSelect<Msg>,
    connected: bool,
    size: Option<ButtonSize>,
    gap: Option<f32>,
    inner_radius: Option<f32>,
    background: Option<Color>,
    foreground: Option<Color>,
    selected_background: Option<Color>,
    selected_foreground: Option<Color>,
    label_style: Option<TextStyle>,
    expand: bool,
}

/// **A row of toggle buttons** choosing one thing, or several.
///
/// ```
/// # use frus_widgets::{ButtonGroup, ButtonSize, Icons};
/// # #[derive(Clone)] enum Msg { View(usize), Style(usize) }
/// // One of three, joined at the seams.
/// ButtonGroup::single(Some(1), Msg::View)
///     .connected(true)
///     .button("Day")
///     .button("Week")
///     .button("Month");
/// // Any of three, apart.
/// ButtonGroup::multiple([0, 2], Msg::Style)
///     .size(ButtonSize::Medium)
///     .icon_button(Icons::FORMAT_BOLD, "Bold")
///     .icon_button(Icons::FORMAT_ITALIC, "Italic")
///     .icon_button(Icons::FORMAT_UNDERLINED, "Underline");
/// ```
///
/// The group is **controlled**: it shows the selection it is given and sends `on_select(i)`
/// when button `i` is pressed. What a press means — selecting it, toggling it, clearing a
/// single choice when it is pressed again — is the application's.
///
/// - A **standard** group keeps its buttons apart. Each is a pill, and a selected one
///   squares its corners; a pressed one rounds them less, the shape morph of the
///   expressive buttons.
/// - A **connected** group joins them two pixels apart: round at the two ends, nearly
///   square at the seams, and a selected button becomes a pill all round.
///
/// A reader hears a single-choice group as radio buttons and a multiple one as toggles,
/// each with its state.
pub struct ButtonGroup<Msg = crate::callback::Callback> {
    inner: Lazy<Parts<Msg>, Msg>,
}

impl<Msg: Clone + 'static> ButtonGroup<Msg> {
    fn with<R: crate::callback::IntoMsg<Msg>>(
        selection: GroupSelection,
        selected: Vec<usize>,
        on_select: impl Fn(usize) -> R + 'static,
    ) -> Self {
        let on_select = crate::callback::handler1(on_select);
        Self {
            inner: Lazy::new(
                Parts {
                    entries: Vec::new(),
                    selected,
                    selection,
                    on_select: Rc::new(on_select),
                    connected: false,
                    size: None,
                    gap: None,
                    inner_radius: None,
                    background: None,
                    foreground: None,
                    selected_background: None,
                    selected_foreground: None,
                    label_style: None,
                    expand: false,
                },
                build::<Msg>,
            ),
        }
    }

    /// A group choosing **one** button at most: `selected` is the chosen one, and
    /// `on_select(i)` is sent when button `i` is pressed.
    pub fn single<R: crate::callback::IntoMsg<Msg>>(
        selected: Option<usize>,
        on_select: impl Fn(usize) -> R + 'static,
    ) -> Self {
        Self::with(
            GroupSelection::Single,
            selected.into_iter().collect(),
            on_select,
        )
    }

    /// A group of **independent** toggles: `selected` lists the ones that are on, and
    /// `on_toggle(i)` is sent when button `i` is pressed.
    pub fn multiple<R: crate::callback::IntoMsg<Msg>>(
        selected: impl IntoIterator<Item = usize>,
        on_toggle: impl Fn(usize) -> R + 'static,
    ) -> Self {
        Self::with(
            GroupSelection::Multiple,
            selected.into_iter().collect(),
            on_toggle,
        )
    }

    fn p(&mut self) -> &mut Parts<Msg> {
        self.inner.parts_mut()
    }

    /// A button reading `label`.
    #[must_use]
    pub fn button(mut self, label: impl Into<String>) -> Self {
        self.p().entries.push(Entry {
            label: label.into(),
            icon: None,
            shows_label: true,
            enabled: true,
        });
        self
    }

    /// A button showing `icon`. `label` is what a reader hears; it is not drawn.
    #[must_use]
    pub fn icon_button(mut self, icon: IconData, label: impl Into<String>) -> Self {
        self.p().entries.push(Entry {
            label: label.into(),
            icon: Some(icon),
            shows_label: false,
            enabled: true,
        });
        self
    }

    /// A button showing `icon` before `label`.
    #[must_use]
    pub fn labelled_icon_button(mut self, icon: IconData, label: impl Into<String>) -> Self {
        self.p().entries.push(Entry {
            label: label.into(),
            icon: Some(icon),
            shows_label: true,
            enabled: true,
        });
        self
    }

    /// Whether the last button added can be pressed.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        if let Some(entry) = self.p().entries.last_mut() {
            entry.enabled = enabled;
        }
        self
    }

    /// **Connected** — joined at the seams — rather than standard.
    #[must_use]
    pub fn connected(mut self, connected: bool) -> Self {
        self.p().connected = connected;
        self
    }

    /// Which of the five sizes. Unset, the theme's, then [`ButtonSize::Small`].
    #[must_use]
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.p().size = Some(size);
        self
    }

    /// The gap between the buttons, over the size's.
    #[must_use]
    pub fn gap(mut self, gap: f32) -> Self {
        self.p().gap = Some(gap);
        self
    }

    /// A connected group's corners at the seams, over the size's.
    #[must_use]
    pub fn inner_radius(mut self, radius: f32) -> Self {
        self.p().inner_radius = Some(radius);
        self
    }

    /// An unselected button's surface and content.
    #[must_use]
    pub fn colors(mut self, background: Color, foreground: Color) -> Self {
        let p = self.p();
        p.background = Some(background);
        p.foreground = Some(foreground);
        self
    }

    /// A selected button's surface and content.
    #[must_use]
    pub fn selected_colors(mut self, background: Color, foreground: Color) -> Self {
        let p = self.p();
        p.selected_background = Some(background);
        p.selected_foreground = Some(foreground);
        self
    }

    /// The labels' type, over the size's.
    #[must_use]
    pub fn label_style(mut self, style: TextStyle) -> Self {
        self.p().label_style = Some(style);
        self
    }

    /// Share the width the group is given among the buttons, rather than each taking
    /// its content's width. Off by default.
    #[must_use]
    pub fn expand(mut self, expand: bool) -> Self {
        self.p().expand = expand;
        self
    }
}

crate::composite::composite_widget!(ButtonGroup);

/// Only while enabled: a disabled button keeps the shared disabled look.
fn enabled_only<T>(value: T) -> WidgetStateProperty<T> {
    WidgetStateProperty::new().when(
        StateFilter::Not(Box::new(WidgetState::Disabled.into())),
        value,
    )
}

/// The shape of button `index` of `count`: what it is at rest and what it morphs to
/// while pressed.
fn shape_of(
    theme: &Theme,
    connected: bool,
    selected: bool,
    index: usize,
    count: usize,
    size: ButtonSize,
    inner: f32,
) -> WidgetStateProperty<ShapeBorder> {
    let full = size.height() * 0.5;
    let rounded = |r: BorderRadiusDirectional| ShapeBorder::rounded(r.resolve(theme.direction));
    if connected {
        if selected {
            return WidgetStateProperty::all(ShapeBorder::rounded(BorderRadius::uniform(full)));
        }
        let start = if index == 0 { full } else { inner };
        let end = if index + 1 == count { full } else { inner };
        let pressed_inner = (inner * 0.5).max(2.0);
        let pressed_start = if index == 0 { full } else { pressed_inner };
        let pressed_end = if index + 1 == count {
            full
        } else {
            pressed_inner
        };
        return WidgetStateProperty::new()
            .when(
                WidgetState::Pressed,
                rounded(BorderRadiusDirectional::horizontal(
                    pressed_start,
                    pressed_end,
                )),
            )
            .otherwise(rounded(BorderRadiusDirectional::horizontal(start, end)));
    }
    let rest = if selected { size.square_radius() } else { full };
    WidgetStateProperty::new()
        .when(
            WidgetState::Pressed,
            ShapeBorder::rounded(BorderRadius::uniform(size.pressed_radius())),
        )
        .otherwise(ShapeBorder::rounded(BorderRadius::uniform(rest)))
}

fn build<Msg: Clone + 'static>(p: Parts<Msg>) -> Box<dyn Widget<Msg>> {
    Box::new(ThemeBuilder::boxed(move |theme: &Theme| {
        let t = theme.widgets.button_group;
        let size = p.size.or(t.size).unwrap_or_default();
        let gap = p.gap.unwrap_or(if p.connected {
            t.connected_gap.unwrap_or(CONNECTED_GROUP_GAP)
        } else {
            t.gap.unwrap_or(standard_group_gap(size))
        });
        let inner = p
            .inner_radius
            .or(t.inner_radius)
            .unwrap_or(connected_inner_radius(size));
        let label_style = p
            .label_style
            .or(t.label_style)
            .unwrap_or(size.label_style(theme));
        let scheme = &theme.scheme;
        let colors = |selected: bool| {
            if selected {
                (
                    p.selected_background
                        .or(t.selected_background)
                        .unwrap_or(scheme.secondary),
                    p.selected_foreground
                        .or(t.selected_foreground)
                        .unwrap_or(scheme.on_secondary),
                )
            } else {
                (
                    p.background
                        .or(t.background)
                        .unwrap_or(scheme.secondary_container),
                    p.foreground
                        .or(t.foreground)
                        .unwrap_or(scheme.on_secondary_container),
                )
            }
        };
        let count = p.entries.len();
        let mut row = Flex::row().gap(gap).align(frus_layout::Align::Center);
        for (index, entry) in p.entries.iter().enumerate() {
            let selected = p.selected.contains(&index);
            let (background, foreground) = colors(selected);
            let icon_only = entry.icon.is_some() && !entry.shows_label;
            let words = entry.label.clone();
            let style = ButtonStyle {
                shape: Some(shape_of(
                    theme,
                    p.connected,
                    selected,
                    index,
                    count,
                    size,
                    inner,
                )),
                background_color: Some(enabled_only(background)),
                foreground_color: Some(enabled_only(foreground)),
                icon_color: Some(enabled_only(foreground)),
                icon_size: Some(WidgetStateProperty::all(size.icon_size())),
                text_style: Some(WidgetStateProperty::all(label_style)),
                padding: Some(WidgetStateProperty::all(frus_core::Insets::new(
                    0.0,
                    size.padding(),
                    0.0,
                    size.padding(),
                ))),
                ..ButtonStyle::default()
            };
            let mut button = if icon_only {
                let glyph = entry.icon.expect("an icon button has a glyph");
                Button::with_child(Icon::new(glyph)).style(ButtonStyle {
                    padding: Some(WidgetStateProperty::all(frus_core::Insets::new(
                        0.0,
                        (size.height() - size.icon_size()) * 0.5,
                        0.0,
                        (size.height() - size.icon_size()) * 0.5,
                    ))),
                    ..style
                })
            } else {
                let mut button = Button::new(words.clone()).style(style);
                if let Some(glyph) = entry.icon {
                    button = button.icon(Icon::new(glyph));
                }
                button
            };
            button = button
                .variant(Variant::Tonal)
                .height(size.height())
                .enabled(entry.enabled)
                .on_press((p.on_select)(index));
            let role = match p.selection {
                GroupSelection::Single => frus_core::Role::RadioButton,
                GroupSelection::Multiple => frus_core::Role::Button,
            };
            // A button with words already says them; a glyph alone is named here.
            let props = frus_core::SemanticsProperties::new(role).toggled(selected);
            let props = if icon_only { props.label(words) } else { props };
            let said = Semantics::new(props, button).merging(true);
            row = if p.expand {
                row.child(crate::Expanded::new(said))
            } else {
                row.child(said)
            };
        }
        Box::new(row) as Box<dyn Widget<Msg>>
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_ui, Runtime, Size};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Pick(usize),
    }

    fn semantics_of(ui: &crate::Ui<Msg>) -> Vec<frus_core::SemanticsProperties> {
        ui.semantics().iter().map(|(_, _, s)| s.clone()).collect()
    }

    /// **A single choice reads as radio buttons, a multiple one as toggles**, each with its
    /// state, and a press sends the index.
    #[test]
    fn the_selection_is_said_and_a_press_sends_the_index() {
        let theme = Theme::default();
        let single = ButtonGroup::single(Some(1), Msg::Pick)
            .connected(true)
            .button("Day")
            .button("Week")
            .button("Month");
        let ui = build_ui(
            &single,
            Size::new(400.0, 100.0),
            &Runtime::default(),
            &theme,
        );
        let nodes = semantics_of(&ui);
        let radios: Vec<_> = nodes
            .iter()
            .filter(|s| s.role == frus_core::Role::RadioButton)
            .collect();
        assert_eq!(radios.len(), 3, "{nodes:#?}");
        assert_eq!(radios[1].label.as_deref(), Some("Week"));
        assert_eq!(radios[1].toggled, frus_core::Toggled::True);
        assert_eq!(radios[0].toggled, frus_core::Toggled::False);

        let multiple = ButtonGroup::multiple([0, 2], Msg::Pick)
            .button("B")
            .button("I")
            .button("U");
        let ui = build_ui(
            &multiple,
            Size::new(400.0, 100.0),
            &Runtime::default(),
            &theme,
        );
        let on: Vec<_> = semantics_of(&ui)
            .into_iter()
            .filter(|s| s.role == frus_core::Role::Button)
            .map(|s| s.toggled == frus_core::Toggled::True)
            .collect();
        assert_eq!(on, vec![true, false, true]);
    }

    /// **A connected group is round at its ends and nearly square at its seams**; the
    /// selected button is a pill; a pressed one tightens its seams.
    #[test]
    fn connected_corners_follow_position_and_state() {
        let theme = Theme::default();
        let size = ButtonSize::Small;
        let resolve = |prop: WidgetStateProperty<ShapeBorder>, pressed: bool| {
            let states = if pressed {
                crate::WidgetStates::from(WidgetState::Pressed)
            } else {
                crate::WidgetStates::default()
            };
            prop.resolve(states)
                .copied()
                .and_then(|s| s.as_rounded(frus_core::Rect::new(0.0, 0.0, 80.0, 40.0)))
                .map(|(_, r)| r)
                .unwrap()
        };
        let first = resolve(shape_of(&theme, true, false, 0, 3, size, 8.0), false);
        assert_eq!((first.top_left, first.top_right), (20.0, 8.0));
        let middle = resolve(shape_of(&theme, true, false, 1, 3, size, 8.0), false);
        assert_eq!((middle.top_left, middle.top_right), (8.0, 8.0));
        let last = resolve(shape_of(&theme, true, false, 2, 3, size, 8.0), false);
        assert_eq!((last.top_left, last.top_right), (8.0, 20.0));
        let pressed = resolve(shape_of(&theme, true, false, 1, 3, size, 8.0), true);
        assert_eq!(pressed.top_left, 4.0, "a pressed seam tightens");
        let chosen = resolve(shape_of(&theme, true, true, 1, 3, size, 8.0), false);
        assert_eq!(
            chosen,
            BorderRadius::uniform(20.0),
            "a selected one is a pill"
        );

        // RTL: the first button's round side is on the right.
        let rtl = Theme::default().rtl();
        let first = resolve(shape_of(&rtl, true, false, 0, 3, size, 8.0), false);
        assert_eq!((first.top_left, first.top_right), (8.0, 20.0));

        // Standard: a pill, square when selected, tighter when pressed.
        let plain = resolve(shape_of(&theme, false, false, 0, 3, size, 8.0), false);
        assert_eq!(plain, BorderRadius::uniform(20.0));
        let square = resolve(shape_of(&theme, false, true, 0, 3, size, 8.0), false);
        assert_eq!(square, BorderRadius::uniform(12.0));
        let held = resolve(shape_of(&theme, false, false, 0, 3, size, 8.0), true);
        assert_eq!(held, BorderRadius::uniform(8.0));
    }
}
