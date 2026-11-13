// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The camera screen (milestone 798): the preview, a picture, the torch, the zoom and the
//! other camera; and a code scanner.
//!
//! The camera opens when the screen does (asking for the permission the first time) and
//! closes when it is left, or when the scanner takes over: the scanner opens a camera of its
//! own, lighter, with no picture stream.

use crate::prelude::*;
use frus_shell::{
    available_cameras, BarcodeCapture, CameraController, CameraLensDirection, CameraPreview,
    FlashMode, ResolutionPreset, Scanner,
};
use frus_widgets::{column, row, Button, StateHandle};

/// The screen.
pub(crate) struct CameraPage;

/// What the screen shows.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    Camera,
    Scan,
}

/// What the screen keeps.
#[derive(Default)]
pub(crate) struct CameraState {
    mode: Mode,
    camera: Option<CameraController>,
    status: String,
    picture: Option<(ImageHandle, String)>,
    scanned: Option<String>,
    scans: u32,
}

impl StatefulWidget for CameraPage {
    type State = CameraState;

    fn create_state(&self) -> CameraState {
        CameraState {
            status: "Opening the camera…".into(),
            ..Default::default()
        }
    }
}

/// Opens the back camera (or the first there is) into the state.
fn open_camera(state: StateHandle<CameraState>, lens: CameraLensDirection) {
    state.clone().spawn(async move {
        let opened = async {
            let cameras = available_cameras().await?;
            let description = cameras
                .iter()
                .find(|c| c.lens_direction == lens)
                .or_else(|| cameras.first())
                .cloned()
                .ok_or(frus_shell::CameraError::NotFound)?;
            let camera = CameraController::new(description, ResolutionPreset::High);
            camera.initialize().await?;
            Ok::<_, frus_shell::CameraError>(camera)
        }
        .await;
        state.set_state(|s| match opened {
            Ok(camera) => {
                s.status = match camera.preview_size() {
                    Some((w, h)) => format!("Preview {w}×{h}"),
                    None => String::new(),
                };
                s.camera = Some(camera);
            }
            Err(err) => s.status = err.to_string(),
        });
    });
}

impl State for CameraState {
    type Widget = CameraPage;

    fn init_state(&mut self, cx: &StateContext<Self>) {
        open_camera(cx.handle(), CameraLensDirection::Back);
    }

    fn dispose(&mut self) {
        if let Some(camera) = self.camera.take() {
            camera.dispose();
        }
    }

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let handle = cx.handle();
        let picture_view: Box<dyn Widget> = match self.mode {
            Mode::Camera => Box::new(match self.camera.as_ref() {
                Some(camera) => CameraPreview::new(camera),
                None => CameraPreview::empty(),
            }),
            Mode::Scan => {
                let found = handle.clone();
                Box::new(
                    Scanner::new()
                        .on_detect(move |capture: BarcodeCapture| {
                            found.set_state(|s| {
                                let code = &capture.barcodes[0];
                                s.scans += 1;
                                s.scanned = Some(format!(
                                    "{:?}: {}",
                                    code.format,
                                    code.raw_value.clone().unwrap_or_default()
                                ));
                            });
                        })
                        .on_error({
                            let failed = handle.clone();
                            move |err| failed.set_state(|s| s.status = err.to_string())
                        })
                        .into_widget(),
                )
            }
        };
        let switch_mode = {
            let handle = handle.clone();
            Callback::new(move || {
                let (mode, camera) = handle.read(|s| (s.mode, s.camera.clone()));
                match mode {
                    Mode::Camera => {
                        if let Some(camera) = camera {
                            camera.dispose();
                        }
                        handle.set_state(|s| {
                            s.camera = None;
                            s.mode = Mode::Scan;
                            s.status = "Point the camera at a QR code".into();
                        });
                    }
                    Mode::Scan => {
                        handle.set_state(|s| {
                            s.mode = Mode::Camera;
                            s.status = "Opening the camera…".into();
                        });
                        open_camera(handle.clone(), CameraLensDirection::Back);
                    }
                }
            })
        };
        let mut controls = row![Button::new(match self.mode {
            Mode::Camera => "Scan a code",
            Mode::Scan => "Camera",
        })
        .on_press(switch_mode)]
        .gap(8.0)
        .wrap();
        if let (Mode::Camera, Some(camera)) = (self.mode, self.camera.clone()) {
            let shoot = handle.async_callback({
                let camera = camera.clone();
                move |state| {
                    let camera = camera.clone();
                    async move {
                        let taken = camera.take_picture().await;
                        state.set_state(|s| match taken {
                            Ok(picture) => {
                                let size = picture.bytes.len();
                                let place = picture
                                    .path
                                    .as_ref()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_default();
                                match frus_image::decode(&picture.bytes) {
                                    Ok(image) => {
                                        s.picture = Some((
                                            image.into_handle(),
                                            format!("{} KB {place}", size / 1024),
                                        ))
                                    }
                                    Err(err) => s.status = err.to_string(),
                                }
                            }
                            Err(err) => s.status = err.to_string(),
                        });
                    }
                }
            });
            let torch = {
                let (camera, state) = (camera.clone(), handle.clone());
                Callback::new(move || {
                    let next = if camera.flash_mode() == FlashMode::Torch {
                        FlashMode::Off
                    } else {
                        FlashMode::Torch
                    };
                    if let Err(err) = camera.set_flash_mode(next) {
                        state.set_state(|s| s.status = err.to_string());
                    }
                })
            };
            let zoom = |step: f32| {
                let (camera, state) = (camera.clone(), handle.clone());
                Callback::new(move || {
                    let level = camera.zoom_level() + step;
                    match camera.set_zoom_level(level) {
                        Ok(()) => state
                            .set_state(|s| s.status = format!("Zoom {:.1}×", camera.zoom_level())),
                        Err(err) => state.set_state(|s| s.status = err.to_string()),
                    }
                })
            };
            let focus = {
                let (camera, state) = (camera.clone(), handle.clone());
                Callback::new(move || {
                    if let Err(err) = camera.set_focus_point(Some((0.5, 0.5))) {
                        state.set_state(|s| s.status = err.to_string());
                    }
                })
            };
            let flip = {
                let (camera, state) = (camera.clone(), handle.clone());
                handle.async_callback(move |_| {
                    let (camera, state) = (camera.clone(), state.clone());
                    async move {
                        let lens = match camera.description().lens_direction {
                            CameraLensDirection::Front => CameraLensDirection::Back,
                            _ => CameraLensDirection::Front,
                        };
                        let result = async {
                            let cameras = available_cameras().await?;
                            match cameras.into_iter().find(|c| c.lens_direction == lens) {
                                Some(other) => camera.set_description(other).await,
                                None => Err(frus_shell::CameraError::NotFound),
                            }
                        }
                        .await;
                        state.set_state(|s| {
                            s.status = match result {
                                Ok(()) => format!("{lens:?} camera"),
                                Err(err) => err.to_string(),
                            }
                        });
                    }
                })
            };
            controls = controls
                .child(Button::new("Take a picture").on_press(shoot))
                .child(
                    Button::new("Torch")
                        .variant(Variant::Outlined)
                        .on_press(torch),
                )
                .child(
                    Button::new("Zoom +")
                        .variant(Variant::Outlined)
                        .on_press(zoom(0.5)),
                )
                .child(
                    Button::new("Zoom −")
                        .variant(Variant::Outlined)
                        .on_press(zoom(-0.5)),
                )
                .child(
                    Button::new("Focus centre")
                        .variant(Variant::Outlined)
                        .on_press(focus),
                )
                .child(
                    Button::new("Other camera")
                        .variant(Variant::Outlined)
                        .on_press(flip),
                );
        }
        let mut details = column![text(self.status.clone())
            .size(14.0)
            .color(theme.muted)
            .wrap()]
        .gap(8.0);
        if self.mode == Mode::Scan {
            details = details.child(
                text(match &self.scanned {
                    Some(value) => format!("Read {} time(s) — {value}", self.scans),
                    None => "Nothing read yet".into(),
                })
                .size(16.0)
                .weight(FontWeight::Bold)
                .wrap(),
            );
        }
        if let (Mode::Camera, Some((image, about))) = (self.mode, self.picture.as_ref()) {
            details = details.child(
                row![
                    Image::new(image.clone())
                        .width(96.0)
                        .height(96.0)
                        .fit(BoxFit::Contain),
                    text(about.clone()).size(12.0).color(theme.muted).wrap(),
                ]
                .gap(8.0),
            );
        }
        let Size { width, height } = surface();
        let router = cx.router();
        let content = column![
            Expanded::new(Container::new().child(picture_view)),
            details,
            controls,
        ]
        .gap(12.0)
        .padding(16.0)
        .flex(1.0);
        let screen = column![
            NavigationBar::new("Camera").on_back(on(move || {
                router.pop();
            })),
            content
        ]
        .flex(1.0);
        Box::new(
            Container::new()
                .width(width)
                .height(height)
                .color(theme.background)
                .child(SafeArea::new(screen)),
        )
    }
}
