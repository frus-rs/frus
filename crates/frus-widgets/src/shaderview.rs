// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`AnimatedShader`]: a box painted by an application's fragment shader (milestone 792),
//! its `time` running while it plays — and nothing drawn at all while it does not.

use std::marker::PhantomData;

use frus_core::{Rect, Scene, ShaderPaint};
use frus_layout::{Dimension, Style};

use crate::interaction::Status;
use crate::theme::Theme;
use crate::widget::{FillAxes, Widget};

/// **A box painted by a custom shader**, whose `Fragment::time` runs on the frame clock
/// while it [plays](AnimatedShader::playing).
///
/// While it plays the frames keep coming, one per display refresh (or at the decoration
/// rate when it is [`decorative`](AnimatedShader::decorative)); once it stops, its last
/// time stays on the screen and it asks for no frame at all, so a still screen costs
/// nothing. Unsized, it takes the room its parent offers.
///
/// ```
/// use frus_core::Shader;
/// use frus_widgets::AnimatedShader;
///
/// let waves = Shader::from_wgsl(
///     "fn shade(f: Fragment) -> vec4<f32> {
///          let v = 0.5 + 0.5 * sin(f.uv.x * 20.0 + f.time * 3.0);
///          return vec4<f32>(v, 0.3, 1.0 - v, 1.0);
///      }",
/// )
/// .unwrap_or_else(Shader::placeholder);
/// let banner: AnimatedShader = AnimatedShader::new(waves.paint()).size(320.0, 120.0);
/// ```
pub struct AnimatedShader<Msg = crate::callback::Callback> {
    paint: ShaderPaint,
    width: Option<f32>,
    height: Option<f32>,
    playing: bool,
    decorative: bool,
    /// Subtracted from the clock, so the animation can start at nought.
    epoch: f32,
    _msg: PhantomData<fn() -> Msg>,
}

impl<Msg> AnimatedShader<Msg> {
    /// A box painted by `paint`, playing.
    pub fn new(paint: ShaderPaint) -> Self {
        Self {
            paint,
            width: None,
            height: None,
            playing: true,
            decorative: false,
            epoch: 0.0,
            _msg: PhantomData,
        }
    }

    /// Its size in logical pixels. Unset, it fills the room it is given.
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = Some(width);
        self.height = Some(height);
        self
    }

    /// Whether the clock runs. Stopped, the shader is drawn at the paint's own
    /// [`time`](ShaderPaint::time) and no frame is asked for.
    pub fn playing(mut self, playing: bool) -> Self {
        self.playing = playing;
        self
    }

    /// The animation only decorates (a shimmer, a glow): drawn at the application's rate
    /// for decoration rather than the display's while nothing else moves, which spares
    /// the battery. Leave it off for anything that follows input.
    pub fn decorative(mut self, decorative: bool) -> Self {
        self.decorative = decorative;
        self
    }

    /// The clock reading, in seconds since the application started, that `time` counts
    /// from: give it the moment the animation began to have `time` start at nought.
    pub fn started_at(mut self, seconds: f32) -> Self {
        self.epoch = seconds;
        self
    }
}

impl<Msg> Widget<Msg> for AnimatedShader<Msg> {
    fn style(&self) -> Style {
        Style {
            width: self.width.map_or(Dimension::Auto, Dimension::Length),
            height: self.height.map_or(Dimension::Auto, Dimension::Length),
            ..Default::default()
        }
    }

    fn fill_axes(&self, _theme: &Theme) -> FillAxes {
        FillAxes {
            horizontal: self.width.is_none(),
            vertical: self.height.is_none(),
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn continuous(&self) -> bool {
        self.playing
    }

    fn decorative(&self) -> bool {
        self.decorative
    }

    fn paint(&self, bounds: Rect, status: Status, _theme: &Theme, scene: &mut Scene) {
        let paint = if self.playing {
            self.paint.clone().time(status.time - self.epoch)
        } else {
            self.paint.clone()
        };
        if status.opacity < 1.0 {
            // Faded in with the widget, as a group.
            scene.layer(status.opacity, |s| s.shader_rect(bounds, &paint));
        } else {
            scene.shader_rect(bounds, &paint);
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frus_core::{Primitive, Shader};

    fn paint() -> ShaderPaint {
        Shader::from_wgsl("fn shade(f: Fragment) -> vec4<f32> { return vec4<f32>(f.time); }")
            .unwrap()
            .paint()
    }

    /// Playing, it asks for frames and paints the clock; stopped, it asks for none and
    /// paints the paint's own time.
    #[test]
    fn it_animates_only_while_playing() {
        let status = Status {
            time: 3.5,
            ..Status::default()
        };
        let playing: AnimatedShader<()> = AnimatedShader::new(paint()).started_at(1.0);
        assert!(playing.continuous());
        let mut scene = Scene::new();
        playing.paint(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            status,
            &Theme::default(),
            &mut scene,
        );
        let Primitive::Layer { filter, .. } = &scene.primitives()[0] else {
            panic!("a layer");
        };
        assert_eq!(filter.shader.as_ref().unwrap().seconds(), 2.5);

        let stopped: AnimatedShader<()> = AnimatedShader::new(paint().time(0.75)).playing(false);
        assert!(!stopped.continuous());
        let mut scene = Scene::new();
        stopped.paint(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            status,
            &Theme::default(),
            &mut scene,
        );
        let Primitive::Layer { filter, .. } = &scene.primitives()[0] else {
            panic!("a layer");
        };
        assert_eq!(filter.shader.as_ref().unwrap().seconds(), 0.75);
    }

    #[test]
    fn unsized_it_fills() {
        let free: AnimatedShader<()> = AnimatedShader::new(paint());
        assert_eq!(free.fill_axes(&Theme::default()), FillAxes::BOTH);
        let sized: AnimatedShader<()> = AnimatedShader::new(paint()).size(4.0, 4.0);
        assert_eq!(sized.fill_axes(&Theme::default()), FillAxes::NONE);
    }
}
