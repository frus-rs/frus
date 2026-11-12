// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Video playback** (milestone 797, feature `video`): [`VideoPlayerController`], which plays
//! a clip from the application's bytes, a file or the network, and the widgets that show it —
//! [`VideoPlayer`], [`VideoProgressIndicator`] and a default overlay of controls,
//! [`VideoControls`].
//!
//! The platform's own player does the work, so sound, the clock that keeps the picture in step
//! with it, seeking and streaming come with it:
//!
//! - Android: `MediaPlayer`, its pictures drawn into a [`TextureSurface`](crate::TextureSurface);
//! - Windows: Media Foundation's media engine, each new picture copied into an external texture;
//! - the Web: a `<video>` element, each new picture drawn into the texture through a canvas;
//! - macOS, Linux and iOS: not yet — the controller's
//!   [`initialize`](VideoPlayerController::initialize) answers [`VideoError::Unsupported`].
//!
//! Nothing runs while no clip is open: no thread, no timer. While one plays, the platform's
//! player runs, and the controller hears from it four times a second (the position) and on each
//! change of state.

mod controller;
mod platform;
mod widgets;

pub(crate) use controller::lifecycle;
pub use controller::{
    DataSource, DurationRange, VideoError, VideoPlayerController, VideoPlayerValue,
};
pub use widgets::{
    VideoControls, VideoControlsStyle, VideoPlayer, VideoProgressColors, VideoProgressIndicator,
};

#[cfg(target_os = "android")]
pub(crate) use platform::remember;
