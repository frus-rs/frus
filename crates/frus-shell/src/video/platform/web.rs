// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The Web**: a `<video>` element the browser plays, each new picture drawn into a canvas
//! and pushed into the clip's external texture.
//!
//! The element is in the page (a pixel, transparent: some browsers do not decode a detached
//! one) and the browser plays its sound and keeps its clock. While it plays, each animation
//! frame checks whether the picture moved on and copies it if it did; paused, nothing runs.
//!
//! A clip from another origin is read only if its server allows it (CORS): the element asks
//! anonymously, and a server that does not answer leaves the canvas unreadable, which is
//! reported as an error.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Duration;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlVideoElement};

use super::super::controller::{DataSource, DurationRange, Event, Player, Sink, VideoError};

pub(crate) fn open(source: &DataSource, sink: Arc<Sink>) -> Result<Box<dyn Player>, VideoError> {
    let fail = |what: &str| VideoError::Source(what.to_owned());
    let window = web_sys::window().ok_or_else(|| fail("no window"))?;
    let document = window.document().ok_or_else(|| fail("no document"))?;
    let (src, object_url) = match source {
        DataSource::Asset(bytes) => {
            let url = blob_url(bytes).map_err(|_| fail("the clip could not be made a blob"))?;
            (url.clone(), Some(url))
        }
        DataSource::Network(url) => (url.clone(), None),
        DataSource::File(_) => {
            return Err(VideoError::Unsupported(
                "a page cannot read the device's files; use an asset or an address",
            ))
        }
    };
    let video: HtmlVideoElement = document
        .create_element("video")
        .map_err(|_| fail("no <video> element"))?
        .dyn_into()
        .map_err(|_| fail("no <video> element"))?;
    let canvas: HtmlCanvasElement = document
        .create_element("canvas")
        .map_err(|_| fail("no canvas"))?
        .dyn_into()
        .map_err(|_| fail("no canvas"))?;
    let ctx: CanvasRenderingContext2d = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|c| c.dyn_into().ok())
        .ok_or_else(|| fail("no 2D canvas"))?;
    if object_url.is_none() {
        video.set_cross_origin(Some("anonymous"));
    }
    video.set_preload("auto");
    let _ = video.set_attribute("playsinline", "");
    let _ = video.set_attribute(
        "style",
        "position:fixed;left:0;top:0;width:1px;height:1px;opacity:0;pointer-events:none",
    );
    if let Some(body) = document.body() {
        let _ = body.append_child(&video);
    }
    let inner = Rc::new(Inner {
        video,
        canvas,
        ctx,
        sink,
        texture: RefCell::new(None),
        listeners: RefCell::new(Vec::new()),
        frame: RefCell::new(None),
        frame_id: Cell::new(None),
        playing: Cell::new(false),
        last_time: Cell::new(-1.0),
        last_report: Cell::new(0.0),
        object_url,
    });
    Inner::listen(&inner);
    inner.video.set_src(&src);
    Ok(Box::new(VideoElement { inner }))
}

fn blob_url(bytes: &[u8]) -> Result<String, JsValue> {
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::of1(&array);
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(&format!("video/{}", super::extension(bytes)));
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)?;
    web_sys::Url::create_object_url_with_blob(&blob)
}

type Listener = Closure<dyn FnMut()>;
type Frame = Closure<dyn FnMut(f64)>;

struct Inner {
    video: HtmlVideoElement,
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    sink: Arc<Sink>,
    texture: RefCell<Option<frus_widgets::ExternalTexture>>,
    listeners: RefCell<Vec<(&'static str, Listener)>>,
    frame: RefCell<Option<Frame>>,
    frame_id: Cell<Option<i32>>,
    playing: Cell<bool>,
    /// The time of the last picture copied, in seconds.
    last_time: Cell<f64>,
    /// When the position was last reported, in the page's milliseconds.
    last_report: Cell<f64>,
    object_url: Option<String>,
}

fn seconds(s: f64) -> Duration {
    if s.is_finite() && s > 0.0 {
        Duration::from_secs_f64(s)
    } else {
        Duration::ZERO
    }
}

impl Inner {
    fn listen(this: &Rc<Self>) {
        let on = |name: &'static str, run: fn(&Inner)| {
            let weak: Weak<Inner> = Rc::downgrade(this);
            let closure: Listener = Closure::new(move || {
                if let Some(inner) = weak.upgrade() {
                    run(&inner);
                }
            });
            let _ = this
                .video
                .add_event_listener_with_callback(name, closure.as_ref().unchecked_ref());
            this.listeners.borrow_mut().push((name, closure));
        };
        on("loadedmetadata", |inner| inner.metadata());
        on("loadeddata", |inner| inner.draw(true));
        on("seeked", |inner| {
            inner.draw(true);
            inner.report_position();
        });
        on("resize", |inner| {
            let (w, h) = (inner.video.video_width(), inner.video.video_height());
            if w > 0 && h > 0 {
                inner.sink.send(Event::Size {
                    width: w,
                    height: h,
                });
            }
        });
        on("waiting", |inner| inner.sink.send(Event::Buffering(true)));
        on("playing", |inner| inner.sink.send(Event::Buffering(false)));
        on("canplay", |inner| inner.sink.send(Event::Buffering(false)));
        on("progress", |inner| inner.buffered());
        on("ended", |inner| {
            inner.stop_frames();
            inner.report_position();
            inner.sink.send(Event::Completed);
        });
        on("error", |inner| {
            inner.stop_frames();
            let why = inner
                .video
                .error()
                .map(|e| {
                    let message = e.message();
                    if message.is_empty() {
                        format!("media error {}", e.code())
                    } else {
                        message
                    }
                })
                .unwrap_or_else(|| "the clip could not be played".into());
            inner.sink.send(Event::Error(why));
        });
    }

    fn metadata(&self) {
        let (w, h) = (self.video.video_width(), self.video.video_height());
        let texture = (w > 0 && h > 0).then(|| frus_widgets::textures().create());
        let id = texture.as_ref().map(|t| t.id());
        *self.texture.borrow_mut() = texture;
        self.canvas.set_width(w);
        self.canvas.set_height(h);
        self.sink.send(Event::Ready {
            duration: seconds(self.video.duration()),
            width: w,
            height: h,
            texture: id,
        });
    }

    fn buffered(&self) {
        let ranges = self.video.buffered();
        let mut out = Vec::new();
        for i in 0..ranges.length() {
            if let (Ok(start), Ok(end)) = (ranges.start(i), ranges.end(i)) {
                out.push(DurationRange::new(seconds(start), seconds(end)));
            }
        }
        self.sink.send(Event::Buffered(out));
    }

    fn report_position(&self) {
        self.sink
            .send(Event::Position(seconds(self.video.current_time())));
    }

    /// Copies the picture into the texture, if it moved on since the last (or `always`).
    fn draw(&self, always: bool) {
        let texture = self.texture.borrow();
        let Some(texture) = texture.as_ref() else {
            return;
        };
        let now = self.video.current_time();
        if !always && now == self.last_time.get() {
            return;
        }
        let (w, h) = (self.video.video_width(), self.video.video_height());
        if w == 0 || h == 0 {
            return;
        }
        if self.canvas.width() != w || self.canvas.height() != h {
            self.canvas.set_width(w);
            self.canvas.set_height(h);
        }
        self.last_time.set(now);
        if self
            .ctx
            .draw_image_with_html_video_element_and_dw_and_dh(
                &self.video,
                0.0,
                0.0,
                w as f64,
                h as f64,
            )
            .is_err()
        {
            return;
        }
        match self.ctx.get_image_data(0.0, 0.0, w as f64, h as f64) {
            Ok(image) => {
                let _ = texture.push_rgba(w, h, &image.data().0);
            }
            Err(_) => {
                self.stop_frames();
                self.sink.send(Event::Error(
                    "the clip's pictures cannot be read: its server does not allow this page \
                     (CORS)"
                        .into(),
                ));
            }
        }
    }

    fn start_frames(this: &Rc<Self>) {
        if this.frame.borrow().is_none() {
            let weak = Rc::downgrade(this);
            let frame: Frame = Closure::new(move |at: f64| {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                inner.frame_id.set(None);
                if !inner.playing.get() {
                    return;
                }
                inner.draw(false);
                if at - inner.last_report.get() >= 250.0 {
                    inner.last_report.set(at);
                    inner.report_position();
                }
                inner.request_frame();
            });
            *this.frame.borrow_mut() = Some(frame);
        }
        this.request_frame();
    }

    fn request_frame(&self) {
        if self.frame_id.get().is_some() {
            return;
        }
        let (Some(window), Some(frame)) = (
            web_sys::window(),
            self.frame
                .borrow()
                .as_ref()
                .map(|f| f.as_ref().unchecked_ref::<js_sys::Function>().clone()),
        ) else {
            return;
        };
        if let Ok(id) = window.request_animation_frame(&frame) {
            self.frame_id.set(Some(id));
        }
    }

    fn stop_frames(&self) {
        self.playing.set(false);
        if let (Some(id), Some(window)) = (self.frame_id.take(), web_sys::window()) {
            let _ = window.cancel_animation_frame(id);
        }
    }
}

struct VideoElement {
    inner: Rc<Inner>,
}

impl Player for VideoElement {
    fn play(&self) {
        let inner = &self.inner;
        inner.playing.set(true);
        Inner::start_frames(inner);
        if let Ok(promise) = inner.video.play() {
            // A browser refuses to start a clip with sound before the page was touched.
            let weak = Rc::downgrade(inner);
            let refused = Closure::once(move |err: JsValue| {
                if let Some(inner) = weak.upgrade() {
                    inner.stop_frames();
                    log::warn!("video: the browser refused to play ({err:?})");
                    inner.sink.send(Event::Playing(false));
                }
            });
            let _ = promise.catch(&refused);
            refused.forget();
        }
    }
    fn pause(&self) {
        self.inner.stop_frames();
        let _ = self.inner.video.pause();
        self.inner.report_position();
    }
    fn seek_to(&self, position: Duration) {
        self.inner.video.set_current_time(position.as_secs_f64());
    }
    fn set_volume(&self, volume: f32) {
        self.inner.video.set_volume(volume as f64);
    }
    fn set_looping(&self, looping: bool) {
        self.inner.video.set_loop(looping);
    }
    fn set_playback_speed(&self, speed: f32) {
        self.inner.video.set_playback_rate(speed as f64);
    }
}

impl Drop for VideoElement {
    fn drop(&mut self) {
        let inner = &self.inner;
        inner.stop_frames();
        for (name, listener) in inner.listeners.borrow_mut().drain(..) {
            let _ = inner
                .video
                .remove_event_listener_with_callback(name, listener.as_ref().unchecked_ref());
        }
        let _ = inner.video.pause();
        let _ = inner.video.remove_attribute("src");
        inner.video.load();
        inner.video.remove();
        if let Some(url) = &inner.object_url {
            let _ = web_sys::Url::revoke_object_url(url);
        }
        inner.texture.borrow_mut().take();
    }
}
