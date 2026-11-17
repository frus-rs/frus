// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`TypingIndicator`]: three dots that say someone is typing (milestone 815).
//!
//! A module of its own because it is not a control: it has no `enabled` flag to honour,
//! and the composer's beside it would otherwise put it under the disabled-control check.

use frus_core::{Color, Rect, Role, Scene, SemanticsProperties};
use frus_layout::{Dimension, Style};

use crate::interaction::Status;
use crate::theme::Theme;
use crate::widget::Widget;

/// **Three dots that rise and fall in turn**: someone is typing (milestone 815).
///
/// It animates while it is on the screen and costs nothing once it is gone. Its colour is the
/// caller's, then the theme's [`ChatTheme::meta_color`](crate::ChatTheme::meta_color), then
/// the muted ink. It reads to a screen reader as its [`label`](Self::label).
#[derive(Clone, Debug)]
pub struct TypingIndicator {
    color: Option<Color>,
    dot: f32,
    label: String,
}

impl Default for TypingIndicator {
    fn default() -> Self {
        Self::new()
    }
}

impl TypingIndicator {
    /// The dots, seven pixels across each.
    pub fn new() -> Self {
        Self {
            color: None,
            dot: 7.0,
            label: "Typing".into(),
        }
    }

    /// The dots' colour.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// How wide one dot is.
    #[must_use]
    pub fn dot_size(mut self, size: f32) -> Self {
        self.dot = size.max(1.0);
        self
    }

    /// What a screen reader says: who is typing.
    #[must_use]
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// How far dot `i` is raised, and how strong it is, at `time` seconds: each dot rises
    /// and falls over the first part of a 1.2-second cycle, a fifth of it after the one
    /// before.
    pub(super) fn pose(i: usize, time: f32) -> (f32, f32) {
        let phase = (time / 1.2 - i as f32 * 0.2).rem_euclid(1.0);
        // Up and down over the first 60 % of the cycle, then resting.
        let bump = if phase < 0.6 {
            (phase / 0.6 * std::f32::consts::PI).sin()
        } else {
            0.0
        };
        (bump, 0.4 + 0.6 * bump)
    }
}

impl<Msg> Widget<Msg> for TypingIndicator {
    fn style(&self) -> Style {
        Style {
            width: Dimension::Length(self.dot * 5.0),
            height: Dimension::Length(self.dot * 2.5),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn continuous(&self) -> bool {
        true
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let color = self
            .color
            .or(theme.widgets.chat.meta_color)
            .unwrap_or(theme.muted);
        let d = self.dot;
        let rest = bounds.y + bounds.height - d;
        for i in 0..3 {
            let (lift, strength) = Self::pose(i, status.time);
            let x = bounds.x + i as f32 * d * 2.0;
            let y = rest - lift * d * 0.6;
            scene.draw_rect(
                Rect::new(x, y, d, d),
                color.fade(strength * status.opacity),
                d / 2.0,
                0.0,
                Color::TRANSPARENT,
            );
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn semantics(&self) -> Option<SemanticsProperties> {
        Some(SemanticsProperties::new(Role::Label).label(self.label.clone()))
    }
}
