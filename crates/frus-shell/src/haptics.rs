// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Haptic feedback** (milestone 777): a short vibration that answers a gesture or marks an
//! outcome.
//!
//! ```no_run
//! // A toggle flipped, an item snapped into place:
//! let _ = frus_shell::haptics::selection();
//! // A payment went through:
//! let _ = frus_shell::haptics::success();
//! ```
//!
//! Each call returns at once; the result says whether the platform took it, and an
//! application usually ignores it: feedback is never worth an error message.
//!
//! What each platform does:
//!
//! - **Android**: the system's own touch feedback (`View.performHapticFeedback`), which needs
//!   no permission and is tuned per device. [`light`] is the key click, [`medium`] the keyboard
//!   tap, [`heavy`] the context click, [`selection`] the clock tick; [`success`] and [`error`]
//!   are the confirm and reject effects from Android 11, and [`warning`] the keyboard tap.
//!   When the system has no such effect, a short vibration of the vibrator stands in, which
//!   needs `android.permission.VIBRATE` — `frus build` and `frus run` declare it. **The
//!   person's setting is obeyed**: with touch feedback turned off in the system's settings,
//!   nothing vibrates, and the call still succeeds.
//! - **The web**: `navigator.vibrate`, for 10 to 30 milliseconds. Browsers without it (Safari,
//!   Firefox on the desktop) answer [`HapticsError::Unsupported`]; Chrome vibrates only once
//!   the person has touched the page, and refuses before
//!   ([`HapticsError::Failed`]).
//! - **Windows, macOS and Linux**: there is no haptic hardware to drive. Every call answers
//!   [`HapticsError::Unsupported`] and does nothing, so the same code runs everywhere.
//! - **iOS**: [`HapticsError::Unsupported`] for now.
//!
//! The widgets give the platform's feedback by themselves where the platform does: on Android,
//! a long press that selects text vibrates. [`Theme::enable_feedback`](frus_widgets::Theme)
//! turns that off for an application.

use std::fmt;
use std::time::Duration;

/// Why haptic feedback was not given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HapticsError {
    /// The platform has nothing to vibrate with: the desktops, iOS for now, and browsers
    /// without `navigator.vibrate`.
    Unsupported,
    /// The platform refused, or could not be reached: no vibrator, a missing
    /// `android.permission.VIBRATE`, a browser waiting for the person to touch the page.
    Failed(String),
}

impl fmt::Display for HapticsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HapticsError::Unsupported => f.write_str("haptic feedback is not supported here"),
            HapticsError::Failed(why) => write!(f, "haptic feedback failed: {why}"),
        }
    }
}

impl std::error::Error for HapticsError {}

/// One kind of feedback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Haptic {
    Light,
    Medium,
    Heavy,
    Selection,
    Success,
    Warning,
    Error,
    /// What a long press feels like: Android's own long-press effect.
    LongPress,
}

// `android.view.HapticFeedbackConstants`.
const LONG_PRESS: i32 = 0;
const VIRTUAL_KEY: i32 = 1;
const KEYBOARD_TAP: i32 = 3;
const CLOCK_TICK: i32 = 4;
const CONTEXT_CLICK: i32 = 6;
const CONFIRM: i32 = 16;
const REJECT: i32 = 17;

#[cfg_attr(not(any(android, test)), allow(dead_code))]
impl Haptic {
    /// The `HapticFeedbackConstants` value Android performs for this, on API level `sdk`.
    /// The confirm and reject effects arrived in Android 11 (API 30); before, the nearest
    /// older effects stand in rather than nothing.
    pub(crate) fn android_constant(self, sdk: i32) -> i32 {
        match self {
            Haptic::Light => VIRTUAL_KEY,
            Haptic::Medium => KEYBOARD_TAP,
            Haptic::Heavy => CONTEXT_CLICK,
            Haptic::Selection => CLOCK_TICK,
            Haptic::Success if sdk >= 30 => CONFIRM,
            Haptic::Success => CONTEXT_CLICK,
            Haptic::Warning => KEYBOARD_TAP,
            Haptic::Error if sdk >= 30 => REJECT,
            Haptic::Error => LONG_PRESS,
            Haptic::LongPress => LONG_PRESS,
        }
    }

    /// How long the vibration that stands in for it lasts, where the system's effect is not
    /// there — and how long the web vibrates for it.
    pub(crate) fn millis(self) -> u32 {
        match self {
            Haptic::Light | Haptic::Selection => 10,
            Haptic::Medium | Haptic::Success | Haptic::Warning => 20,
            Haptic::Heavy | Haptic::Error => 30,
            Haptic::LongPress => 50,
        }
    }

    /// The constant's name, for the log.
    #[cfg_attr(not(android), allow(dead_code))]
    pub(crate) fn android_name(constant: i32) -> &'static str {
        match constant {
            LONG_PRESS => "LONG_PRESS",
            VIRTUAL_KEY => "VIRTUAL_KEY",
            KEYBOARD_TAP => "KEYBOARD_TAP",
            CLOCK_TICK => "CLOCK_TICK",
            CONTEXT_CLICK => "CONTEXT_CLICK",
            CONFIRM => "CONFIRM",
            REJECT => "REJECT",
            _ => "?",
        }
    }
}

/// The feedback a long press that selects text gives on the system's `platform`, if any:
/// Android's long-press effect, a heavy impact on iOS, nothing on the desktops.
pub(crate) fn long_press_feedback(platform: frus_widgets::TargetPlatform) -> Option<Haptic> {
    use frus_widgets::TargetPlatform as P;
    match platform {
        P::Android | P::Fuchsia => Some(Haptic::LongPress),
        P::Ios => Some(Haptic::Heavy),
        P::Linux | P::MacOs | P::Windows => None,
    }
}

#[cfg(any(test, feature = "testing"))]
thread_local! {
    /// The feedback the shell gave on this thread, for its tests.
    static GIVEN: std::cell::RefCell<Vec<Haptic>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The system a test pretends to run on, on its own thread only: the process-wide
    /// override would reach the tests running beside it.
    static SYSTEM: std::cell::Cell<Option<frus_widgets::TargetPlatform>> =
        const { std::cell::Cell::new(None) };
}

/// Makes this thread's shell give the feedback of `platform`'s system, for a test.
#[cfg(test)]
#[inline(never)]
pub(crate) fn pretend_system(platform: Option<frus_widgets::TargetPlatform>) {
    SYSTEM.with(|system| system.set(platform));
}

/// The system whose feedback the widgets give: the one running, not the theme's, as the
/// leading toolkit does — feedback is how the device feels, not how the application looks.
#[inline(never)]
pub(crate) fn system_platform() -> frus_widgets::TargetPlatform {
    #[cfg(any(test, feature = "testing"))]
    if let Some(platform) = SYSTEM.with(|system| system.get()) {
        return platform;
    }
    frus_widgets::default_target_platform()
}

/// The feedback the shell gave on this thread since the last call, for its tests.
#[cfg(any(test, feature = "testing"))]
#[cfg_attr(not(test), allow(dead_code))]
#[inline(never)]
pub(crate) fn take_given() -> Vec<Haptic> {
    GIVEN.with(|given| std::mem::take(&mut *given.borrow_mut()))
}

/// The feedback a widget's gesture gives — a long press that selected text —, unless the
/// application's theme turned it off. A failure is not worth a word: the gesture worked.
#[inline(never)]
pub(crate) fn gesture_feedback(theme: &frus_widgets::Theme, kind: Option<Haptic>) {
    if !theme.enable_feedback {
        return;
    }
    let Some(kind) = kind else {
        return;
    };
    #[cfg(any(test, feature = "testing"))]
    GIVEN.with(|given| given.borrow_mut().push(kind));
    if let Err(err) = perform(kind) {
        if err != HapticsError::Unsupported {
            log::debug!("{err}");
        }
    }
}

pub(crate) fn perform(kind: Haptic) -> Result<(), HapticsError> {
    #[cfg(android)]
    {
        crate::android_device::haptic(kind)
    }
    #[cfg(web)]
    {
        crate::web_device::vibrate(&[kind.millis()])
    }
    #[cfg(any(desktop, ios))]
    {
        let _ = kind;
        Err(HapticsError::Unsupported)
    }
}

/// A light impact: a small thing happened — a key, a tap on a small control.
///
/// ```no_run
/// let _ = frus_shell::haptics::light();
/// ```
pub fn light() -> Result<(), HapticsError> {
    perform(Haptic::Light)
}

/// A medium impact: something clicked into place.
///
/// ```no_run
/// let _ = frus_shell::haptics::medium();
/// ```
pub fn medium() -> Result<(), HapticsError> {
    perform(Haptic::Medium)
}

/// A heavy impact: something large landed, a drag dropped.
///
/// ```no_run
/// let _ = frus_shell::haptics::heavy();
/// ```
pub fn heavy() -> Result<(), HapticsError> {
    perform(Haptic::Heavy)
}

/// A selection tick: a value moved one step — a picker's wheel, a slider's notch.
///
/// ```no_run
/// let _ = frus_shell::haptics::selection();
/// ```
pub fn selection() -> Result<(), HapticsError> {
    perform(Haptic::Selection)
}

/// A task succeeded.
///
/// ```no_run
/// let _ = frus_shell::haptics::success();
/// ```
pub fn success() -> Result<(), HapticsError> {
    perform(Haptic::Success)
}

/// A task produced a warning.
///
/// ```no_run
/// let _ = frus_shell::haptics::warning();
/// ```
pub fn warning() -> Result<(), HapticsError> {
    perform(Haptic::Warning)
}

/// A task failed.
///
/// ```no_run
/// let _ = frus_shell::haptics::error();
/// ```
pub fn error() -> Result<(), HapticsError> {
    perform(Haptic::Error)
}

/// Vibrates for `duration`: an alert rather than a touch's feedback, so the system's
/// touch-feedback setting does not silence it (its vibration settings still apply).
///
/// On Android it uses the vibrator, which needs `android.permission.VIBRATE` (declared by
/// `frus build` and `frus run`); on the web `navigator.vibrate`. The desktops answer
/// [`HapticsError::Unsupported`].
///
/// ```no_run
/// use std::time::Duration;
/// let _ = frus_shell::haptics::vibrate(Duration::from_millis(400));
/// ```
pub fn vibrate(duration: Duration) -> Result<(), HapticsError> {
    let millis = duration.as_millis().min(u32::MAX as u128) as u32;
    if millis == 0 {
        return Ok(());
    }
    #[cfg(android)]
    {
        crate::android_device::vibrate(millis)
    }
    #[cfg(web)]
    {
        crate::web_device::vibrate(&[millis])
    }
    #[cfg(any(desktop, ios))]
    {
        Err(HapticsError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frus_widgets::TargetPlatform as P;

    /// The system's effects, as the leading toolkit maps them, and the older effects that stand
    /// in for confirm and reject before Android 11.
    #[test]
    fn each_kind_takes_androids_own_effect() {
        assert_eq!(Haptic::Light.android_constant(29), VIRTUAL_KEY);
        assert_eq!(Haptic::Medium.android_constant(29), KEYBOARD_TAP);
        assert_eq!(Haptic::Heavy.android_constant(29), CONTEXT_CLICK);
        assert_eq!(Haptic::Selection.android_constant(29), CLOCK_TICK);
        assert_eq!(Haptic::LongPress.android_constant(29), LONG_PRESS);
        assert_eq!(Haptic::Success.android_constant(30), CONFIRM);
        assert_eq!(Haptic::Error.android_constant(30), REJECT);
        assert_eq!(Haptic::Warning.android_constant(30), KEYBOARD_TAP);
        assert_eq!(Haptic::Success.android_constant(29), CONTEXT_CLICK);
        assert_eq!(Haptic::Error.android_constant(29), LONG_PRESS);
    }

    #[test]
    fn the_web_vibrates_as_long_as_the_kind_is_strong() {
        assert_eq!(Haptic::Light.millis(), 10);
        assert_eq!(Haptic::Medium.millis(), 20);
        assert_eq!(Haptic::Heavy.millis(), 30);
        assert_eq!(Haptic::LongPress.millis(), 50);
    }

    #[test]
    fn a_long_press_vibrates_on_phones_only() {
        assert_eq!(long_press_feedback(P::Android), Some(Haptic::LongPress));
        assert_eq!(long_press_feedback(P::Ios), Some(Haptic::Heavy));
        for desktop in [P::Windows, P::MacOs, P::Linux] {
            assert_eq!(long_press_feedback(desktop), None);
        }
    }

    #[test]
    fn a_theme_can_turn_the_widgets_feedback_off() {
        let _ = take_given();
        gesture_feedback(&frus_widgets::Theme::default(), Some(Haptic::LongPress));
        assert_eq!(take_given(), vec![Haptic::LongPress]);
        gesture_feedback(
            &frus_widgets::Theme::default().with_feedback(false),
            Some(Haptic::LongPress),
        );
        assert_eq!(take_given(), vec![]);
    }

    #[cfg(desktop)]
    #[test]
    fn the_desktops_say_they_have_no_haptics() {
        assert_eq!(light(), Err(HapticsError::Unsupported));
        assert_eq!(
            vibrate(Duration::from_millis(100)),
            Err(HapticsError::Unsupported)
        );
        assert_eq!(vibrate(Duration::ZERO), Ok(()));
    }
}
