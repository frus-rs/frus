// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The video player screen (milestone 797): a clip played by the platform's player, shown by a
//! `VideoPlayer` under the default `VideoControls`, with the controller's settings below.
//!
//! Three sources: a short clip built into the application, an MP4 file streamed from the
//! network, and an HLS stream. Leaving the screen drops the controller, which releases the
//! player.

use std::time::Duration;

use crate::prelude::*;
use frus_shell::{VideoControls, VideoPlayer, VideoPlayerController};
use frus_widgets::{column, row, StackFit};

/// A 320×240 H.264 clip, four seconds of a generated test picture, no sound.
static CLIP: &[u8] = include_bytes!("../../assets/texture-demo.mp4");

/// The sources the screen offers.
const SOURCES: [(&str, Option<&str>); 3] = [
    ("Built-in clip", None),
    (
        "Network MP4",
        Some("https://test-videos.co.uk/vids/bigbuckbunny/mp4/h264/360/Big_Buck_Bunny_360_10s_1MB.mp4"),
    ),
    (
        "HLS stream",
        Some("https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8"),
    ),
];

/// The screen.
pub(crate) struct PlayerPage;

/// What the screen keeps: the controller of the source chosen.
pub(crate) struct PlayerState {
    source: usize,
    controller: VideoPlayerController,
}

impl StatefulWidget for PlayerPage {
    type State = PlayerState;

    fn create_state(&self) -> PlayerState {
        PlayerState {
            source: 0,
            controller: open(0),
        }
    }
}

/// A controller for source `index`, opening, playing once it is open, looping.
fn open(index: usize) -> VideoPlayerController {
    // `FRUS_VIDEO_URL` plays another address instead of the built-in clip.
    let custom = std::env::var("FRUS_VIDEO_URL").ok().filter(|_| index == 0);
    let controller = match (custom, SOURCES[index].1) {
        (Some(url), _) => VideoPlayerController::network_url(url),
        (None, Some(url)) => VideoPlayerController::network_url(url),
        (None, None) => VideoPlayerController::asset(CLIP),
    };
    controller.set_looping(true);
    controller.play();
    let opening = controller.clone();
    frus_widgets::spawn_local(async move {
        match opening.initialize().await {
            Ok(()) => {
                let v = opening.value();
                log::info!(
                    "video: open, {}×{}, {:.1} s",
                    v.width,
                    v.height,
                    v.duration.as_secs_f32()
                );
            }
            Err(err) => log::warn!("video: {err}"),
        }
    });
    controller
}

impl State for PlayerState {
    type Widget = PlayerPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let c = &self.controller;
        let v = c.value();
        let router = cx.router();

        let picture: Box<dyn Widget> = match &v.error_description {
            Some(why) => Box::new(
                Container::new()
                    .color(Color::BLACK)
                    .padding(16.0)
                    .child(text(why.clone()).size(14.0).color(Color::WHITE).wrap()),
            ),
            None => Box::new(
                Stack::new()
                    .fit(StackFit::Expand)
                    .layer(VideoPlayer::new(c).background(Color::BLACK).into_widget())
                    .layer(Positioned::fill(VideoControls::new(c).into_widget())),
            ),
        };

        let mut sources = SegmentedButton::new(
            self.source,
            cx.handler(|state: &mut PlayerState, index: usize| {
                if state.source != index {
                    state.controller.dispose();
                    state.source = index;
                    state.controller = open(index);
                }
            }),
        );
        for (label, _) in SOURCES {
            sources = sources.segment(label);
        }

        let speed = v.playback_speed;
        let faster = {
            let c = c.clone();
            Callback::new(move || {
                let next = if speed >= 2.0 { 0.5 } else { speed * 2.0 };
                let _ = c.set_playback_speed(next);
            })
        };
        let looping = {
            let c = c.clone();
            let on = v.is_looping;
            Callback::new(move || c.set_looping(!on))
        };
        let mute = {
            let c = c.clone();
            let volume = v.volume;
            Callback::new(move || c.set_volume(if volume > 0.0 { 0.0 } else { 1.0 }))
        };
        let back = {
            let c = c.clone();
            Callback::new(move || {
                let at = c.position().saturating_sub(Duration::from_secs(5));
                c.seek_to(at);
            })
        };
        let settings = row![
            button(format!("{speed}×"), faster),
            button(if v.is_looping { "Loop" } else { "Once" }, looping),
            button(if v.volume > 0.0 { "Mute" } else { "Sound" }, mute),
            button("−5 s", back),
        ]
        .gap(8.0)
        .wrap();
        let status = text(format!(
            "{}×{} · {} · {}{}",
            v.width,
            v.height,
            if v.is_playing { "playing" } else { "paused" },
            if v.is_initialized { "open" } else { "opening" },
            if v.is_buffering {
                " · waiting for data"
            } else {
                ""
            },
        ))
        .size(13.0)
        .color(theme.muted);

        let content = column![
            sources,
            Expanded::new(Container::new().child(picture)),
            settings,
            status,
        ]
        .gap(12.0)
        .padding(16.0)
        .flex(1.0);
        let screen = column![
            NavigationBar::new("Video player").on_back(on(move || {
                router.pop();
            })),
            content
        ]
        .flex(1.0);
        let Size { width, height } = surface();
        Box::new(
            Container::new()
                .width(width)
                .height(height)
                .color(theme.background)
                .child(SafeArea::new(screen)),
        )
    }
}
