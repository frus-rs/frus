// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`CupertinoSwitch`]: the Apple-style on/off switch — a 51 × 31 track, a white 28-pixel
//! thumb that stretches while it is held, green when on (milestone 812). And
//! [`Switch::adaptive`], which is this switch under an Apple platform's theme and the other
//! one everywhere else.

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use frus_core::{BorderRadius, Color, Curve, Rect, Scene};
use frus_layout::{Dimension, Style};

use crate::cupertinocolors::CupertinoColors;
use crate::cupertinotheme::CupertinoTheme;
use crate::interaction::Status;
use crate::switch::Switch;
use crate::theme::Theme;
use crate::widget::Widget;

/// The track (`cupertino/switch.dart:30`).
const TRACK_W: f32 = 51.0;
const TRACK_H: f32 = 31.0;
/// The thumb's radius (`:29`).
const THUMB_R: f32 = 14.0;
/// How much wider a held thumb grows (`:33`).
const THUMB_EXTENSION: f32 = 7.0;
/// The switch's box: the track and a little room round it (`:32`).
const BOX_W: f32 = 59.0;
const BOX_H: f32 = 39.0;
/// A disabled switch, all of it (`:28`).
const DISABLED_OPACITY: f32 = 0.5;
/// How long a flip takes (`:502`).
const FLIP_SECONDS: f32 = 0.2;
/// The focus ring round the track: 1.75 pixels outside it, 3.5 wide (`:1292`).
const FOCUS_GROW: f32 = 1.75;
const FOCUS_WIDTH: f32 = 3.5;

/// The reference's `Curves.ease`.
const EASE: Curve = Curve::Cubic {
    x1: 0.25,
    y1: 0.1,
    x2: 0.25,
    y2: 1.0,
};

/// **An Apple-style switch**, controlled: it shows what it is told and says what a tap or a
/// drag asks for.
///
/// ```
/// use frus_widgets::CupertinoSwitch;
///
/// #[derive(Clone)]
/// enum Msg { Wifi(bool) }
/// let _wifi: CupertinoSwitch<Msg> = CupertinoSwitch::new(true).on_toggle(Msg::Wifi);
/// ```
///
/// On, its track is the system green — or the Apple-style theme's primary colour when the
/// theme applies itself everywhere — and off, the secondary system fill. The thumb is white
/// with a soft shadow. Without [`on_toggle`](Self::on_toggle), or
/// [`enabled(false)`](Self::enabled), it is drawn at half opacity and does nothing.
pub struct CupertinoSwitch<Msg = crate::callback::Callback> {
    on: bool,
    enabled: bool,
    active_track_color: Option<Color>,
    inactive_track_color: Option<Color>,
    thumb_color: Option<Color>,
    inactive_thumb_color: Option<Color>,
    focus_color: Option<Color>,
    apply_theme: Option<bool>,
    on_toggle: Option<Rc<dyn Fn(bool) -> Msg>>,
}

impl<Msg> CupertinoSwitch<Msg> {
    /// A switch, on or off.
    pub fn new(on: bool) -> Self {
        Self {
            on,
            enabled: true,
            active_track_color: None,
            inactive_track_color: None,
            thumb_color: None,
            inactive_thumb_color: None,
            focus_color: None,
            apply_theme: None,
            on_toggle: None,
        }
    }

    /// What a flip sends, made from the state it asks for.
    pub fn on_toggle<R: crate::callback::IntoMsg<Msg>>(
        mut self,
        on_toggle: impl Fn(bool) -> R + 'static,
    ) -> Self {
        self.on_toggle = Some(Rc::new(crate::callback::handler1(on_toggle)));
        self
    }

    /// Whether it can be flipped. Disabled, it is drawn at half opacity, out of the tab order
    /// and announced as unavailable — and it still shows which way it is set.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The track's colour when on. The system green unless the theme applies itself.
    pub fn active_track_color(mut self, color: Color) -> Self {
        self.active_track_color = Some(color);
        self
    }

    /// The track's colour when off. The secondary system fill by default.
    pub fn inactive_track_color(mut self, color: Color) -> Self {
        self.inactive_track_color = Some(color);
        self
    }

    /// The thumb's colour (white by default), on — and off, unless
    /// [`inactive_thumb_color`](Self::inactive_thumb_color) says otherwise.
    pub fn thumb_color(mut self, color: Color) -> Self {
        self.thumb_color = Some(color);
        self
    }

    /// The thumb's colour when off.
    pub fn inactive_thumb_color(mut self, color: Color) -> Self {
        self.inactive_thumb_color = Some(color);
        self
    }

    /// The ring round the track while the keyboard is on it.
    pub fn focus_color(mut self, color: Color) -> Self {
        self.focus_color = Some(color);
        self
    }

    /// Whether the track takes the Apple-style theme's primary colour when on, in place of
    /// the system green. Unset, the theme's `apply_theme_to_all` decides.
    pub fn apply_theme(mut self, apply: bool) -> Self {
        self.apply_theme = Some(apply);
        self
    }

    fn live(&self) -> bool {
        self.enabled && self.on_toggle.is_some()
    }

    fn active_track(&self, theme: &Theme) -> Color {
        self.active_track_color.unwrap_or_else(|| {
            let c = CupertinoTheme::of(theme);
            if self.apply_theme.unwrap_or(c.apply_theme_to_all) {
                c.primary_color
            } else {
                CupertinoColors::SYSTEM_GREEN.resolve(theme)
            }
        })
    }

    fn inactive_track(&self, theme: &Theme) -> Color {
        self.inactive_track_color
            .unwrap_or_else(|| CupertinoColors::SECONDARY_SYSTEM_FILL.resolve(theme))
    }

    /// Where the flip has got to, curved: `Curves.ease` on the way on, the same flipped on
    /// the way off (`:505`); where a finger put it, as it put it.
    fn place(&self, status: &Status) -> f32 {
        let t = status.value.clamp(0.0, 1.0);
        if status.value_placed || t <= 0.0 || t >= 1.0 {
            t
        } else if self.on {
            EASE.transform(t)
        } else {
            1.0 - EASE.transform(1.0 - t)
        }
    }
}

/// From `a` to `b`, exactly either at the ends.
fn travel(a: Color, b: Color, t: f32) -> Color {
    if t <= 0.0 {
        a
    } else if t >= 1.0 {
        b
    } else {
        a.lerp(b, t)
    }
}

/// The track's and the thumb's boxes for a switch painted in `bounds`, the thumb `place` of
/// the way along (from the start edge), `stretch` of the way held.
fn geometry(bounds: Rect, place: f32, stretch: f32, rtl: bool) -> (Rect, Rect) {
    let track = Rect::new(
        bounds.x + (bounds.width - TRACK_W) * 0.5,
        bounds.y + (bounds.height - TRACK_H) * 0.5,
        TRACK_W,
        TRACK_H,
    );
    let visual = if rtl { 1.0 - place } else { place };
    let extension = THUMB_EXTENSION * stretch.clamp(0.0, 1.0);
    let inner = TRACK_W - TRACK_H;
    let cx = track.x + TRACK_H * 0.5 + extension * 0.5 + visual * (inner - extension);
    let cy = track.y + TRACK_H * 0.5;
    let (w, h) = (THUMB_R * 2.0 + extension, THUMB_R * 2.0);
    (track, Rect::new(cx - w * 0.5, cy - h * 0.5, w, h))
}

impl<Msg> Widget<Msg> for CupertinoSwitch<Msg> {
    fn style(&self) -> Style {
        Style {
            width: Dimension::Length(BOX_W),
            height: Dimension::Length(BOX_H),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        let o = status.opacity * if self.enabled { 1.0 } else { DISABLED_OPACITY };
        let place = self.place(&status);
        let stretch = if self.live() {
            status.press_progress
        } else {
            0.0
        };
        let rtl = theme.direction == frus_core::TextDirection::Rtl;
        let (track, thumb) = geometry(bounds, place, stretch, rtl);

        // The track's colour follows the thumb's place (`:1119`).
        let colour = travel(self.inactive_track(theme), self.active_track(theme), place);
        scene.draw_rect(
            track,
            colour.fade(o),
            TRACK_H * 0.5,
            0.0,
            Color::TRANSPARENT,
        );
        if self.live() && status.focus_progress > 0.0 {
            let ring = self.focus_color.unwrap_or_else(|| {
                crate::cupertinobutton::focus_ring_color(self.active_track(theme))
            });
            let grown = Rect::new(
                track.x - FOCUS_GROW,
                track.y - FOCUS_GROW,
                track.width + 2.0 * FOCUS_GROW,
                track.height + 2.0 * FOCUS_GROW,
            );
            scene.draw_rect(
                grown,
                Color::TRANSPARENT,
                TRACK_H * 0.5 + FOCUS_GROW,
                FOCUS_WIDTH,
                ring.fade(o * status.focus_progress),
            );
        }

        // The thumb's two soft shadows, 3 pixels down (`:34`), then the thumb.
        for (alpha, blur) in [(0x26 as f32 / 255.0, 8.0_f32), (0x0F as f32 / 255.0, 1.0)] {
            let shape = Rect::new(thumb.x, thumb.y + 3.0, thumb.width, thumb.height);
            scene.shadow(
                Rect::new(
                    shape.x - blur,
                    shape.y - blur,
                    shape.width + 2.0 * blur,
                    shape.height + 2.0 * blur,
                ),
                Color::rgba(0.0, 0.0, 0.0, alpha * o),
                BorderRadius::uniform(THUMB_R + blur),
                blur,
            );
        }
        let on_thumb = self.thumb_color.unwrap_or(CupertinoColors::WHITE);
        let off_thumb = self
            .inactive_thumb_color
            .or(self.thumb_color)
            .unwrap_or(CupertinoColors::WHITE);
        scene.draw_rect(
            thumb,
            travel(off_thumb, on_thumb, place).fade(o),
            THUMB_R,
            0.0,
            Color::TRANSPARENT,
        );
    }

    fn on_click(&self) -> Option<Msg> {
        if !self.enabled {
            return None;
        }
        self.on_toggle.as_ref().map(|make| make(!self.on))
    }

    fn pan_axis(&self) -> Option<crate::PanAxis> {
        self.live().then_some(crate::PanAxis::Horizontal)
    }

    /// The thumb follows the finger along the track's inner length, the other way in
    /// right-to-left.
    fn pan_value(&self, event: crate::PanEvent, value: f32, rtl: bool) -> Option<f32> {
        match event {
            crate::PanEvent::Start { .. } => Some(value.clamp(0.0, 1.0)),
            crate::PanEvent::Update { delta, .. } => {
                let along = delta.x / (TRACK_W - TRACK_H);
                let along = if rtl { -along } else { along };
                Some((value + along).clamp(0.0, 1.0))
            }
            crate::PanEvent::End { .. } => None,
        }
    }

    /// Let go past half way, it flips; short of it, it goes back and says nothing.
    fn on_value_release(&self, value: f32) -> Option<Msg> {
        if !self.enabled || (value >= 0.5) == self.on {
            return None;
        }
        self.on_toggle.as_ref().map(|make| make(!self.on))
    }

    fn focusable(&self) -> bool {
        self.live()
    }

    fn semantics(&self) -> Option<frus_core::SemanticsProperties> {
        let semantics =
            frus_core::SemanticsProperties::new(frus_core::Role::Switch).toggled(self.on);
        Some(if self.live() {
            semantics.clickable()
        } else {
            semantics.disabled(true)
        })
    }

    fn anim_target(&self) -> Option<f32> {
        Some(if self.on { 1.0 } else { 0.0 })
    }

    fn anim_duration(&self) -> f32 {
        FLIP_SECONDS
    }

    /// Linear: the paint curves it, one way on and the other way off.
    fn anim_curve(&self) -> Curve {
        Curve::Linear
    }

    fn debug_name(&self) -> &'static str {
        "CupertinoSwitch"
    }
}

/// What an [`AdaptiveSwitch`] was told, until it knows its platform.
struct AdaptiveParts<Msg> {
    on: bool,
    enabled: bool,
    active_track_color: Option<Color>,
    inactive_track_color: Option<Color>,
    thumb_color: Option<Color>,
    on_toggle: Option<Rc<dyn Fn(bool) -> Msg>>,
}

/// **A switch that looks like its platform's**: the Apple-style [`CupertinoSwitch`] under a
/// theme whose platform is iOS or macOS, the [`Switch`] everywhere else. Made by
/// [`Switch::adaptive`].
pub struct AdaptiveSwitch<Msg = crate::callback::Callback> {
    parts: RefCell<Option<AdaptiveParts<Msg>>>,
    built: OnceCell<Vec<Box<dyn Widget<Msg>>>>,
}

impl<Msg> Switch<Msg> {
    /// **A switch in its platform's style**: the Apple-style one under a theme whose platform
    /// is iOS or macOS, this one everywhere else. Only what both share can be set on it.
    pub fn adaptive(on: bool) -> AdaptiveSwitch<Msg> {
        AdaptiveSwitch {
            parts: RefCell::new(Some(AdaptiveParts {
                on,
                enabled: true,
                active_track_color: None,
                inactive_track_color: None,
                thumb_color: None,
                on_toggle: None,
            })),
            built: OnceCell::new(),
        }
    }
}

impl<Msg> AdaptiveSwitch<Msg> {
    fn with(self, f: impl FnOnce(&mut AdaptiveParts<Msg>)) -> Self {
        if let Some(parts) = self.parts.borrow_mut().as_mut() {
            f(parts);
        }
        self
    }

    /// What a flip sends, made from the state it asks for.
    pub fn on_toggle<R: crate::callback::IntoMsg<Msg>>(
        self,
        on_toggle: impl Fn(bool) -> R + 'static,
    ) -> Self {
        let f: Rc<dyn Fn(bool) -> Msg> = Rc::new(crate::callback::handler1(on_toggle));
        self.with(|p| p.on_toggle = Some(f))
    }

    /// Whether it can be flipped.
    pub fn enabled(self, enabled: bool) -> Self {
        self.with(|p| p.enabled = enabled)
    }

    /// The track's colour when on.
    pub fn active_track_color(self, color: Color) -> Self {
        self.with(|p| p.active_track_color = Some(color))
    }

    /// The track's colour when off.
    pub fn inactive_track_color(self, color: Color) -> Self {
        self.with(|p| p.inactive_track_color = Some(color))
    }

    /// The thumb's colour.
    pub fn thumb_color(self, color: Color) -> Self {
        self.with(|p| p.thumb_color = Some(color))
    }
}

/// The switch an adaptive one stands for under `theme`.
fn adaptive_switch<Msg: 'static>(p: &AdaptiveParts<Msg>, theme: &Theme) -> Box<dyn Widget<Msg>> {
    if theme.platform.is_apple() {
        let mut s = CupertinoSwitch::new(p.on).enabled(p.enabled);
        s.on_toggle = p.on_toggle.clone();
        s.active_track_color = p.active_track_color;
        s.inactive_track_color = p.inactive_track_color;
        s.thumb_color = p.thumb_color;
        Box::new(s)
    } else {
        let mut s = Switch::new(p.on).enabled(p.enabled);
        if let Some(f) = p.on_toggle.clone() {
            s = s.on_toggle(move |v| f(v));
        }
        if let Some(c) = p.active_track_color {
            s = s.track_color(c);
        }
        if let Some(c) = p.inactive_track_color {
            s = s.inactive_track_color(c);
        }
        if let Some(c) = p.thumb_color {
            s = s.thumb_color(c);
        }
        Box::new(s)
    }
}

impl<Msg: Clone + 'static> Widget<Msg> for AdaptiveSwitch<Msg> {
    fn style(&self) -> Style {
        Style::default()
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        self.built.get_or_init(|| {
            let Some(parts) = self.parts.borrow_mut().take() else {
                return Vec::new();
            };
            vec![Box::new(crate::themebuilder::ThemeBuilder::boxed(
                move |theme| adaptive_switch(&parts, theme),
            ))]
        })
    }

    fn paint(&self, _bounds: Rect, _status: Status, _theme: &Theme, _scene: &mut Scene) {}

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "AdaptiveSwitch"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cupertinobutton::AllPrimitives;
    use crate::{build_ui, Runtime, Ui};
    use frus_core::{Point, Primitive, Size, TargetPlatform};

    #[derive(Clone, Debug, PartialEq)]
    enum Msg {
        Set(bool),
    }

    /// Every rectangle painted with its colour, the shadows (blurred) left out.
    fn rects(ui: &Ui<Msg>) -> Vec<(Rect, Color)> {
        ui.scene()
            .all()
            .into_iter()
            .filter_map(|p| match p {
                Primitive::Rect {
                    rect, color, blur, ..
                } if *blur == 0.0 => Some((*rect, *color)),
                _ => None,
            })
            .collect()
    }

    fn lay(root: &dyn Widget<Msg>, theme: &Theme) -> Ui<Msg> {
        build_ui(root, Size::new(BOX_W, BOX_H), &Runtime::default(), theme)
    }

    /// **The track and the thumb are Apple's size**: a 59 × 39 box, a 51 × 31 track in its
    /// middle and a 28-pixel thumb 1.5 pixels in from the track's end.
    #[test]
    fn the_track_and_the_thumb_are_apple_s_size() {
        let theme = Theme::light();
        let off = CupertinoSwitch::new(false).on_toggle(Msg::Set);
        let painted = rects(&lay(&off, &theme));
        let (track, thumb) = (painted[0].0, painted[1].0);
        assert_eq!(track, Rect::new(4.0, 4.0, 51.0, 31.0));
        assert_eq!((thumb.width, thumb.height), (28.0, 28.0));
        assert_eq!(thumb.x, track.x + 1.5);
        let on = CupertinoSwitch::new(true).on_toggle(Msg::Set);
        let thumb = rects(&lay(&on, &theme))[1].0;
        assert_eq!(thumb.x + thumb.width, track.x + track.width - 1.5);
    }

    /// **Right to left, on is at the left.**
    #[test]
    fn right_to_left_on_is_at_the_left() {
        let theme = Theme::light().rtl();
        let on = CupertinoSwitch::new(true).on_toggle(Msg::Set);
        let painted = rects(&lay(&on, &theme));
        let (track, thumb) = (painted[0].0, painted[1].0);
        assert_eq!(thumb.x, track.x + 1.5);
    }

    /// **On is the system green and off the secondary fill, in the light and the dark**; the
    /// theme's primary colour when the theme applies itself; the caller's over both.
    #[test]
    fn the_track_colours_are_apple_s() {
        for theme in [Theme::light(), Theme::dark()] {
            let on = rects(&lay(
                &CupertinoSwitch::new(true).on_toggle(Msg::Set),
                &theme,
            ))[0]
                .1;
            assert_eq!(on, CupertinoColors::SYSTEM_GREEN.resolve(&theme));
            let off = rects(&lay(
                &CupertinoSwitch::new(false).on_toggle(Msg::Set),
                &theme,
            ))[0]
                .1;
            assert_eq!(off, CupertinoColors::SECONDARY_SYSTEM_FILL.resolve(&theme));
        }
        let theme = Theme::light();
        let themed = CupertinoSwitch::new(true)
            .apply_theme(true)
            .on_toggle(Msg::Set);
        assert_eq!(
            rects(&lay(&themed, &theme))[0].1,
            CupertinoTheme::of(&theme).primary_color
        );
        let red = Color::rgb(1.0, 0.0, 0.0);
        let own = CupertinoSwitch::new(true)
            .active_track_color(red)
            .on_toggle(Msg::Set);
        assert_eq!(rects(&lay(&own, &theme))[0].1, red);
        // The thumb is white.
        assert_eq!(rects(&lay(&own, &theme))[1].1, CupertinoColors::WHITE);
    }

    /// **Held, the thumb stretches by seven pixels**, from the end it sits at.
    #[test]
    fn a_held_thumb_stretches() {
        let theme = Theme::light();
        let off = CupertinoSwitch::new(false).on_toggle(Msg::Set);
        let ui = lay(&off, &theme);
        let id = ui.hit(Point::new(30.0, 20.0)).expect("it is pressable");
        let mut rt = Runtime::default();
        rt.input.pressed = Some(id);
        rt.input.hovered = Some(id);
        let held = build_ui(&off, Size::new(BOX_W, BOX_H), &rt, &theme);
        let painted = rects(&held);
        let (track, thumb) = (painted[0].0, painted[1].0);
        assert_eq!(thumb.width, 35.0);
        assert_eq!(thumb.x, track.x + 1.5);
    }

    /// **The flip ends where it is going**: half way, an easing that has gone past half; at
    /// its end, exactly the end.
    #[test]
    fn the_flip_eases_to_its_end() {
        let s = CupertinoSwitch::<Msg>::new(true);
        let at = |v: f32| {
            s.place(&Status {
                value: v,
                ..Default::default()
            })
        };
        assert_eq!(at(1.0), 1.0);
        assert_eq!(at(0.0), 0.0);
        assert!(at(0.5) > 0.5);
        let back = CupertinoSwitch::<Msg>::new(false);
        let half = back.place(&Status {
            value: 0.5,
            ..Default::default()
        });
        assert!(half < 0.5, "the way off is the same curve flipped: {half}");
        assert_eq!(Widget::<Msg>::anim_duration(&s), 0.2);
    }

    /// **A tap flips it, a drag past half way flips it, and a disabled one does nothing.**
    #[test]
    fn taps_and_drags_flip_it() {
        let s = CupertinoSwitch::new(false).on_toggle(Msg::Set);
        assert_eq!(Widget::<Msg>::on_click(&s), Some(Msg::Set(true)));
        assert_eq!(
            Widget::<Msg>::pan_axis(&s),
            Some(crate::PanAxis::Horizontal)
        );
        let moved = Widget::<Msg>::pan_value(
            &s,
            crate::PanEvent::Update {
                delta: Point::new(15.0, 0.0),
                local: Point::new(0.0, 0.0),
            },
            0.0,
            false,
        );
        assert_eq!(moved, Some(0.75));
        let rtl = Widget::<Msg>::pan_value(
            &s,
            crate::PanEvent::Update {
                delta: Point::new(15.0, 0.0),
                local: Point::new(0.0, 0.0),
            },
            1.0,
            true,
        );
        assert_eq!(rtl, Some(0.25));
        assert_eq!(
            Widget::<Msg>::on_value_release(&s, 0.75),
            Some(Msg::Set(true))
        );
        assert_eq!(Widget::<Msg>::on_value_release(&s, 0.3), None);
        let off = CupertinoSwitch::new(true)
            .enabled(false)
            .on_toggle(Msg::Set);
        assert_eq!(Widget::<Msg>::on_click(&off), None);
        assert_eq!(Widget::<Msg>::pan_axis(&off), None);
        assert!(!Widget::<Msg>::focusable(&off));
    }

    /// **Disabled, it is drawn at half opacity and still says which way it is set.**
    #[test]
    fn a_disabled_switch_is_half_drawn_and_still_announced() {
        let theme = Theme::light();
        let off = CupertinoSwitch::<Msg>::new(true).enabled(false);
        let track = rects(&lay(&off, &theme))[0].1;
        let green = CupertinoColors::SYSTEM_GREEN.resolve(&theme);
        assert!((track.a - green.a * 0.5).abs() < 1e-6);
        let s = Widget::<Msg>::semantics(&off).unwrap();
        assert_eq!(s.role, frus_core::Role::Switch);
        assert_eq!(s.toggled, frus_core::Toggled::True);
        assert!(s.disabled);
        let live = CupertinoSwitch::new(false).on_toggle(Msg::Set);
        let s = Widget::<Msg>::semantics(&live).unwrap();
        assert_eq!(s.toggled, frus_core::Toggled::False);
        assert!(s.clickable);
    }

    /// **The adaptive switch is Apple's on Apple's platforms and the other one elsewhere**,
    /// with what was set on it passed to either.
    #[test]
    fn the_adaptive_switch_follows_the_platform() {
        let track_width = |platform: TargetPlatform| {
            let theme = Theme::light().with_platform(platform);
            let s: AdaptiveSwitch<Msg> = Switch::adaptive(true).on_toggle(Msg::Set);
            let ui = build_ui(&s, Size::new(200.0, 100.0), &Runtime::default(), &theme);
            let id = ui.hit(Point::new(20.0, 20.0)).expect("it is pressable");
            assert_eq!(ui.msg_for(id), Some(Msg::Set(false)));
            rects(&ui)[0].0.width
        };
        assert_eq!(track_width(TargetPlatform::Ios), TRACK_W);
        assert_eq!(track_width(TargetPlatform::MacOs), TRACK_W);
        assert_eq!(track_width(TargetPlatform::Android), 52.0);
        assert_eq!(track_width(TargetPlatform::Windows), 52.0);
    }
}
