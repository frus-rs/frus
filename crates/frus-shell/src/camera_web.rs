// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The camera on **the web** (milestone 798): `getUserMedia`.
//!
//! The camera's stream plays in a `<video>` that is never put on the page. Once per frame of
//! the page, when the video has a new frame, it is drawn into a canvas of its size and read
//! back as RGBA, which becomes the preview texture's frame and the image stream's. A picture
//! is that canvas, encoded as JPEG by the browser. Everything stops with the session: the
//! stream's tracks end and no further frame is asked for.
//!
//! Browsers give no flash, zoom or focus point through this simple path: those answer
//! [`CameraError::Unsupported`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use frus_widgets::{ExternalTexture, FrameOrientation, TextureId};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use super::{
    CameraDescription, CameraError, CameraImage, CameraImageFormat, CameraLensDirection,
    CameraPlane, FlashMode, ResolutionPreset, Settings, Sink,
};

fn js(what: &str, err: JsValue) -> CameraError {
    CameraError::Platform(format!("{what}: {err:?}"))
}

fn media_devices() -> Result<web_sys::MediaDevices, CameraError> {
    web_sys::window()
        .ok_or(CameraError::Unsupported("no window"))?
        .navigator()
        .media_devices()
        .map_err(|_| CameraError::Unsupported("no camera access: the page must be secure"))
}

fn lens_of(label: &str) -> CameraLensDirection {
    let label = label.to_ascii_lowercase();
    if label.contains("front") || label.contains("user") || label.contains("facetime") {
        CameraLensDirection::Front
    } else if label.contains("back") || label.contains("rear") || label.contains("environment") {
        CameraLensDirection::Back
    } else {
        CameraLensDirection::External
    }
}

pub(super) async fn cameras() -> Result<Vec<CameraDescription>, CameraError> {
    // Browsers name their cameras to a page that may use one.
    let status = crate::Permission::Camera.request().await;
    if !status.is_granted() {
        return Err(CameraError::PermissionDenied(status));
    }
    let devices = media_devices()?;
    let list = JsFuture::from(
        devices
            .enumerate_devices()
            .map_err(|e| js("enumerateDevices", e))?,
    )
    .await
    .map_err(|e| js("enumerateDevices", e))?;
    let list: js_sys::Array = list.unchecked_into();
    Ok(list
        .iter()
        .filter_map(|d| d.dyn_into::<web_sys::MediaDeviceInfo>().ok())
        .filter(|d| d.kind() == web_sys::MediaDeviceKind::Videoinput)
        .map(|d| CameraDescription {
            lens_direction: lens_of(&d.label()),
            name: d.device_id(),
            sensor_orientation: 0,
        })
        .collect())
}

/// What the page's only thread holds. frus's web build has one thread, so it is never
/// touched from another: the camera's types only need to be `Send` to sit beside the native
/// ones.
struct PageOnly<T>(T);

unsafe impl<T> Send for PageOnly<T> {}
unsafe impl<T> Sync for PageOnly<T> {}

/// The preview's texture, kept from one session to the next.
pub(super) struct Preview {
    texture: Arc<ExternalTexture>,
}

impl Preview {
    pub(super) fn new() -> Result<Self, CameraError> {
        Ok(Self {
            texture: Arc::new(frus_widgets::textures().create()),
        })
    }

    pub(super) fn id(&self) -> TextureId {
        self.texture.id()
    }
}

/// The frame callback, which asks for itself again.
type Tick = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

struct Page {
    stream: web_sys::MediaStream,
    canvas: web_sys::HtmlCanvasElement,
    stopped: Rc<Cell<bool>>,
    /// The frame callback, kept alive while the session is.
    _tick: Tick,
}

/// An open camera: its stream, and the loop reading its frames.
pub(super) struct Session {
    page: PageOnly<Page>,
    size: (u32, u32),
    orientation: FrameOrientation,
}

impl Session {
    pub(super) async fn open(
        description: &CameraDescription,
        preset: ResolutionPreset,
        preview: &Preview,
        sink: Arc<Sink>,
        _stills: bool,
    ) -> Result<Self, CameraError> {
        let devices = media_devices()?;
        let (tw, th) = preset.target();
        let video = js_sys::Object::new();
        let set = |o: &js_sys::Object, k: &str, v: &JsValue| {
            let _ = js_sys::Reflect::set(o, &k.into(), v);
        };
        if !description.name.is_empty() {
            let exact = js_sys::Object::new();
            set(&exact, "exact", &description.name.as_str().into());
            set(&video, "deviceId", &exact);
        }
        if tw != u32::MAX {
            let ideal = |v: u32| {
                let o = js_sys::Object::new();
                set(&o, "ideal", &v.into());
                o
            };
            set(&video, "width", &ideal(tw));
            set(&video, "height", &ideal(th));
        }
        let constraints = web_sys::MediaStreamConstraints::new();
        constraints.set_video(&video);
        constraints.set_audio(&false.into());
        let stream = JsFuture::from(
            devices
                .get_user_media_with_constraints(&constraints)
                .map_err(|e| js("getUserMedia", e))?,
        )
        .await
        .map_err(|e| js("getUserMedia", e))?;
        let stream: web_sys::MediaStream = stream.unchecked_into();

        let document = web_sys::window()
            .and_then(|w| w.document())
            .ok_or(CameraError::Unsupported("no document"))?;
        let element: web_sys::HtmlVideoElement = document
            .create_element("video")
            .map_err(|e| js("video", e))?
            .unchecked_into();
        element.set_muted(true);
        element.set_autoplay(true);
        let _ = element.set_attribute("playsinline", "");
        element.set_src_object(Some(&stream));
        if let Ok(playing) = element.play() {
            JsFuture::from(playing).await.map_err(|e| js("play", e))?;
        }
        let size = (element.video_width(), element.video_height());
        let canvas: web_sys::HtmlCanvasElement = document
            .create_element("canvas")
            .map_err(|e| js("canvas", e))?
            .unchecked_into();
        let orientation =
            FrameOrientation::new(0, description.lens_direction == CameraLensDirection::Front);
        preview.texture.set_orientation(orientation);

        let stopped = Rc::new(Cell::new(false));
        let tick: Tick = Rc::default();
        {
            let (stopped, again, canvas) = (stopped.clone(), tick.clone(), canvas.clone());
            let texture = preview.texture.clone();
            let last = Cell::new(-1.0f64);
            *tick.borrow_mut() = Some(Closure::new(move || {
                if stopped.get() {
                    return;
                }
                let now = element.current_time();
                if element.ready_state() >= 2 && now != last.get() {
                    last.set(now);
                    read_frame(&element, &canvas, &texture, &sink, orientation);
                }
                if let (Some(window), Some(tick)) = (web_sys::window(), again.borrow().as_ref()) {
                    let _ = window.request_animation_frame(tick.as_ref().unchecked_ref());
                }
            }));
        }
        if let (Some(window), Some(first)) = (web_sys::window(), tick.borrow().as_ref()) {
            let _ = window.request_animation_frame(first.as_ref().unchecked_ref());
        }
        log::info!("camera open: {}×{} through getUserMedia", size.0, size.1);
        Ok(Self {
            page: PageOnly(Page {
                stream,
                canvas,
                stopped,
                _tick: tick,
            }),
            size,
            orientation,
        })
    }

    pub(super) fn preview_size(&self) -> (u32, u32) {
        self.size
    }

    pub(super) fn orientation(&self) -> FrameOrientation {
        self.orientation
    }

    pub(super) fn zoom_range(&self) -> (f32, f32) {
        (1.0, 1.0)
    }

    pub(super) fn apply(&self, settings: &Settings) -> Result<(), CameraError> {
        if matches!(settings.flash, FlashMode::Always | FlashMode::Torch) {
            return Err(CameraError::Unsupported("no flash through getUserMedia"));
        }
        if settings.zoom != 1.0 || settings.focus.is_some() {
            return Err(CameraError::Unsupported(
                "no zoom or focus point through getUserMedia",
            ));
        }
        Ok(())
    }

    pub(super) fn take_picture(&self, _settings: &Settings) -> Result<Vec<u8>, CameraError> {
        let url = self
            .page
            .0
            .canvas
            .to_data_url_with_type("image/jpeg")
            .map_err(|e| js("toDataURL", e))?;
        let data = url
            .split_once(',')
            .map(|(_, b)| b)
            .ok_or_else(|| CameraError::Platform("no picture yet".into()))?;
        decode_base64(data).ok_or_else(|| CameraError::Platform("a malformed picture".into()))
    }

    pub(super) fn display_changed(&self) {}
}

impl Drop for Session {
    fn drop(&mut self) {
        let page = &self.page.0;
        page.stopped.set(true);
        for track in page.stream.get_tracks().iter() {
            if let Ok(track) = track.dyn_into::<web_sys::MediaStreamTrack>() {
                track.stop();
            }
        }
        log::info!("camera closed");
    }
}

/// Draws the video's frame into the canvas and pushes its pixels.
fn read_frame(
    video: &web_sys::HtmlVideoElement,
    canvas: &web_sys::HtmlCanvasElement,
    texture: &ExternalTexture,
    sink: &Sink,
    orientation: FrameOrientation,
) {
    let (w, h) = (video.video_width(), video.video_height());
    if w == 0 || h == 0 {
        return;
    }
    if canvas.width() != w || canvas.height() != h {
        canvas.set_width(w);
        canvas.set_height(h);
    }
    let Some(context) = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
    else {
        return;
    };
    if context
        .draw_image_with_html_video_element(video, 0.0, 0.0)
        .is_err()
    {
        return;
    }
    let Ok(image) = context.get_image_data(0.0, 0.0, w as f64, h as f64) else {
        return;
    };
    let pixels = image.data();
    let _ = texture.push_rgba(w, h, &pixels);
    let planes = [CameraPlane {
        bytes: &pixels,
        row_stride: w as usize * 4,
        pixel_stride: 4,
    }];
    sink.deliver(&CameraImage {
        width: w,
        height: h,
        format: CameraImageFormat::Rgba8,
        planes: &planes,
        orientation,
    });
}

/// Standard base64, as a data URL holds it.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let bytes: Vec<u8> = text.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut acc = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            acc |= value(c)? << (18 - 6 * i);
        }
        let n = chunk.len() * 6 / 8;
        out.extend_from_slice(&acc.to_be_bytes()[1..1 + n]);
    }
    Some(out)
}
