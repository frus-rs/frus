// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The external texture screen (milestone 793): frames made outside the widget tree, shown
//! by a `Texture`.
//!
//! - On the desktop a thread draws an animated pattern, 60 frames a second, and pushes each
//!   one as RGBA.
//! - On Android the platform's video decoder plays a short clip (generated, in `assets/`)
//!   into a texture surface: the decoder writes into the surface, the surface's frames
//!   become the texture's, and nothing in between is the application's.
//!
//! Leaving the screen stops the producer: the texture, and every cost with it, is gone.

use crate::prelude::*;
use frus_widgets::{column, TextureId};

/// The screen.
pub(crate) struct VideoPage;

/// What the screen keeps: the producer running while it is open.
#[derive(Default)]
pub(crate) struct VideoState {
    producer: Option<producer::Producer>,
    error: Option<String>,
}

impl StatefulWidget for VideoPage {
    type State = VideoState;

    fn create_state(&self) -> VideoState {
        VideoState::default()
    }
}

impl State for VideoState {
    type Widget = VideoPage;

    fn init_state(&mut self, _cx: &StateContext<Self>) {
        match producer::Producer::start() {
            Ok(producer) => self.producer = Some(producer),
            Err(err) => {
                log::warn!("external texture demo: {err}");
                self.error = Some(err);
            }
        }
    }

    fn dispose(&mut self) {
        // Stops the thread and drops the texture.
        self.producer = None;
    }

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let picture: Box<dyn Widget> = match self.producer.as_ref().map(|p| p.id()) {
            Some(id) => Box::new(view(id)),
            None => Box::new(
                text(self.error.clone().unwrap_or_default())
                    .size(14.0)
                    .color(theme.muted)
                    .wrap(),
            ),
        };
        let Size { width, height } = surface();
        let router = cx.router();
        let content = column![
            text(producer::DESCRIPTION)
                .size(14.0)
                .color(theme.muted)
                .wrap(),
            // The rest of the screen: a texture takes the room it is given.
            Expanded::new(Container::new().child(picture)),
        ]
        .gap(12.0)
        .padding(24.0)
        .flex(1.0);
        let screen = column![
            NavigationBar::new("External texture").on_back(on(move || {
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

fn view(id: TextureId) -> frus_widgets::Texture {
    frus_widgets::Texture::new(id)
        .fit(BoxFit::Contain)
        .background(Color::BLACK)
        .semantic_label("A moving test picture")
}

#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
mod producer {
    //! A thread drawing an animated pattern at 60 frames a second.

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use frus_widgets::TextureId;

    pub(super) const DESCRIPTION: &str =
        "A thread draws this pattern and pushes it as RGBA, 60 frames a second.";

    const W: u32 = 480;
    const H: u32 = 270;

    pub(crate) struct Producer {
        id: TextureId,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Producer {
        pub(super) fn start() -> Result<Self, String> {
            let texture = frus_widgets::textures().create();
            let id = texture.id();
            let stop = Arc::new(AtomicBool::new(false));
            let stopping = stop.clone();
            let thread = std::thread::Builder::new()
                .name("texture-demo".into())
                .spawn(move || run(texture, &stopping))
                .map_err(|e| e.to_string())?;
            Ok(Self {
                id,
                stop,
                thread: Some(thread),
            })
        }

        pub(super) fn id(&self) -> TextureId {
            self.id
        }
    }

    impl Drop for Producer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    fn run(texture: frus_widgets::ExternalTexture, stop: &AtomicBool) {
        let mut pixels = vec![0u8; (W * H * 4) as usize];
        let start = Instant::now();
        let tick = Duration::from_micros(16_667);
        let mut next = start;
        let mut reported = start;
        let mut frame = 0u32;
        // `FRUS_TEXTURE_DEMO_FRAMES=n` stops pushing after `n` frames, the texture still
        // shown: what an idle texture costs can then be measured.
        let limit = std::env::var("FRUS_TEXTURE_DEMO_FRAMES")
            .ok()
            .and_then(|n| n.parse::<u32>().ok());
        while !stop.load(Ordering::Acquire) {
            if limit.is_some_and(|n| frame >= n) {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            draw(&mut pixels, frame);
            if texture.push_rgba(W, H, &pixels).is_err() {
                return;
            }
            frame += 1;
            if reported.elapsed() >= Duration::from_secs(2) {
                let s = texture.stats();
                log::info!(
                    "texture demo: {} pushed, {} dropped, {} shown",
                    s.pushed,
                    s.dropped,
                    s.shown
                );
                reported = Instant::now();
            }
            next += tick;
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            } else {
                next = now;
            }
        }
    }

    /// Diagonal colour bands sliding along, and a white bar sweeping across.
    fn draw(pixels: &mut [u8], frame: u32) {
        let bar = (frame * 4) % W;
        let (texels, _) = pixels.as_chunks_mut::<4>();
        for (i, px) in texels.iter_mut().enumerate() {
            let (x, y) = (i as u32 % W, i as u32 / W);
            let t = x + y + frame * 3;
            *px = if x.abs_diff(bar) < 6 {
                [255, 255, 255, 255]
            } else {
                [
                    (t % 256) as u8,
                    ((t / 2 + 85) % 256) as u8,
                    (255 - (t % 256)) as u8,
                    255,
                ]
            };
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod producer {
    //! The page: one pattern frame, pushed once (a browser page has no thread to run a
    //! producer on here).

    use frus_widgets::TextureId;

    pub(super) const DESCRIPTION: &str = "One frame, pushed as RGBA when the screen opened.";

    pub(crate) struct Producer {
        texture: frus_widgets::ExternalTexture,
    }

    impl Producer {
        pub(super) fn start() -> Result<Self, String> {
            let texture = frus_widgets::textures().create();
            let (w, h) = (64u32, 36u32);
            let mut pixels = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h {
                for x in 0..w {
                    pixels.extend_from_slice(&[(x * 4) as u8, (y * 7) as u8, 160, 255]);
                }
            }
            texture
                .push_rgba(w, h, &pixels)
                .map_err(|e| e.to_string())?;
            Ok(Self { texture })
        }

        pub(super) fn id(&self) -> TextureId {
            self.texture.id()
        }
    }
}

#[cfg(target_os = "android")]
mod producer {
    //! The platform's decoder plays a clip into a texture surface, looping.

    use std::ffi::{c_char, c_int, c_void, CStr};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, OnceLock};
    use std::time::{Duration, Instant};

    use frus_shell::{SurfaceFormat, TextureSurface};
    use frus_widgets::TextureId;

    pub(super) const DESCRIPTION: &str =
        "The phone's video decoder plays a clip into a texture surface; the frames are shown here.";

    /// A 320×240 H.264 clip, four seconds of a generated test picture.
    static CLIP: &[u8] = include_bytes!("../../assets/texture-demo.mp4");
    /// The same picture at 1280×720, for measuring (milestone 800): played when the
    /// `debug.frus.texture_clip` property is `720`.
    static CLIP_720: &[u8] = include_bytes!("../../assets/texture-demo-720.mp4");

    /// A system property, `adb shell setprop <name> <value>`: how the demo is told, with no
    /// screen of settings, to play the larger clip or to copy frames rather than import them.
    fn property(name: &CStr) -> String {
        extern "C" {
            fn __system_property_get(name: *const c_char, value: *mut c_char) -> c_int;
        }
        let mut value = [0 as c_char; 92];
        let len = unsafe { __system_property_get(name.as_ptr(), value.as_mut_ptr()) };
        if len <= 0 {
            return String::new();
        }
        unsafe { CStr::from_ptr(value.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    pub(crate) struct Producer {
        id: TextureId,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Producer {
        pub(super) fn start() -> Result<Self, String> {
            // `adb shell setprop debug.frus.texture_copy 1`: frames copied from their planes
            // even where the GPU imports them, to compare the two (milestone 800).
            if property(c"debug.frus.texture_copy") == "1" {
                log::info!("texture demo: frames copied (debug.frus.texture_copy)");
                frus_widgets::textures().set_hardware_buffers(false);
            }
            let large = property(c"debug.frus.texture_clip") == "720";
            let (clip, width, height) = if large {
                (CLIP_720, 1280, 720)
            } else {
                (CLIP, 320, 240)
            };
            let surface = TextureSurface::new(width, height, SurfaceFormat::Yuv420)
                .map_err(|e| e.to_string())?;
            let id = surface.id();
            // The path a Java plugin would take: the same surface, as an `android.view.Surface`.
            match surface.java_surface() {
                Ok(java) => log::info!("texture demo: Java surface ready ({java:p})"),
                Err(err) => log::warn!("texture demo: no Java surface ({err})"),
            }
            let path = frus_shell::app_data_dir()
                .ok_or("no application folder")?
                .join("texture-demo.mp4");
            std::fs::write(&path, clip).map_err(|e| e.to_string())?;
            let stop = Arc::new(AtomicBool::new(false));
            let stopping = stop.clone();
            let thread = std::thread::Builder::new()
                .name("texture-demo".into())
                .spawn(move || {
                    if let Err(err) = unsafe { play(&surface, &path, &stopping) } {
                        log::warn!("texture demo: {err}");
                    }
                    let s = surface.texture().stats();
                    log::info!(
                        "texture demo stopped: {} pushed, {} dropped, {} shown",
                        s.pushed,
                        s.dropped,
                        s.shown
                    );
                })
                .map_err(|e| e.to_string())?;
            Ok(Self {
                id,
                stop,
                thread: Some(thread),
            })
        }

        pub(super) fn id(&self) -> TextureId {
            self.id
        }
    }

    impl Drop for Producer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    extern "C" {
        fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    #[repr(C)]
    #[derive(Default)]
    struct BufferInfo {
        offset: i32,
        size: i32,
        presentation_us: i64,
        flags: u32,
    }

    const END_OF_STREAM: u32 = 4;

    type P = *mut c_void;

    /// `libmediandk.so`'s extractor and codec.
    struct Media {
        ex_new: unsafe extern "C" fn() -> P,
        ex_delete: unsafe extern "C" fn(P) -> i32,
        ex_source_fd: unsafe extern "C" fn(P, c_int, i64, i64) -> i32,
        ex_tracks: unsafe extern "C" fn(P) -> usize,
        ex_track_format: unsafe extern "C" fn(P, usize) -> P,
        ex_select: unsafe extern "C" fn(P, usize) -> i32,
        ex_read: unsafe extern "C" fn(P, *mut u8, usize) -> isize,
        ex_time: unsafe extern "C" fn(P) -> i64,
        ex_advance: unsafe extern "C" fn(P) -> bool,
        ex_seek: unsafe extern "C" fn(P, i64, c_int) -> i32,
        fmt_string: unsafe extern "C" fn(P, *const c_char, *mut *const c_char) -> bool,
        fmt_delete: unsafe extern "C" fn(P) -> i32,
        codec_by_type: unsafe extern "C" fn(*const c_char) -> P,
        codec_configure: unsafe extern "C" fn(P, P, P, P, u32) -> i32,
        codec_start: unsafe extern "C" fn(P) -> i32,
        codec_stop: unsafe extern "C" fn(P) -> i32,
        codec_delete: unsafe extern "C" fn(P) -> i32,
        codec_flush: unsafe extern "C" fn(P) -> i32,
        codec_dequeue_in: unsafe extern "C" fn(P, i64) -> isize,
        codec_in_buffer: unsafe extern "C" fn(P, usize, *mut usize) -> *mut u8,
        codec_queue_in: unsafe extern "C" fn(P, usize, i64, usize, u64, u32) -> i32,
        codec_dequeue_out: unsafe extern "C" fn(P, *mut BufferInfo, i64) -> isize,
        codec_release_out: unsafe extern "C" fn(P, usize, bool) -> i32,
    }

    // Each symbol's type is the field it fills.
    #[allow(clippy::missing_transmute_annotations)]
    fn media() -> Option<&'static Media> {
        static MEDIA: OnceLock<Option<Media>> = OnceLock::new();
        MEDIA
            .get_or_init(|| unsafe {
                let lib = dlopen(c"libmediandk.so".as_ptr(), 2);
                if lib.is_null() {
                    return None;
                }
                macro_rules! sym {
                    ($name:literal) => {{
                        let p = dlsym(lib, $name.as_ptr());
                        if p.is_null() {
                            return None;
                        }
                        std::mem::transmute::<*mut c_void, _>(p)
                    }};
                }
                Some(Media {
                    ex_new: sym!(c"AMediaExtractor_new"),
                    ex_delete: sym!(c"AMediaExtractor_delete"),
                    ex_source_fd: sym!(c"AMediaExtractor_setDataSourceFd"),
                    ex_tracks: sym!(c"AMediaExtractor_getTrackCount"),
                    ex_track_format: sym!(c"AMediaExtractor_getTrackFormat"),
                    ex_select: sym!(c"AMediaExtractor_selectTrack"),
                    ex_read: sym!(c"AMediaExtractor_readSampleData"),
                    ex_time: sym!(c"AMediaExtractor_getSampleTime"),
                    ex_advance: sym!(c"AMediaExtractor_advance"),
                    ex_seek: sym!(c"AMediaExtractor_seekTo"),
                    fmt_string: sym!(c"AMediaFormat_getString"),
                    fmt_delete: sym!(c"AMediaFormat_delete"),
                    codec_by_type: sym!(c"AMediaCodec_createDecoderByType"),
                    codec_configure: sym!(c"AMediaCodec_configure"),
                    codec_start: sym!(c"AMediaCodec_start"),
                    codec_stop: sym!(c"AMediaCodec_stop"),
                    codec_delete: sym!(c"AMediaCodec_delete"),
                    codec_flush: sym!(c"AMediaCodec_flush"),
                    codec_dequeue_in: sym!(c"AMediaCodec_dequeueInputBuffer"),
                    codec_in_buffer: sym!(c"AMediaCodec_getInputBuffer"),
                    codec_queue_in: sym!(c"AMediaCodec_queueInputBuffer"),
                    codec_dequeue_out: sym!(c"AMediaCodec_dequeueOutputBuffer"),
                    codec_release_out: sym!(c"AMediaCodec_releaseOutputBuffer"),
                })
            })
            .as_ref()
    }

    /// Decodes the clip into the surface, looping, each frame released at its time.
    unsafe fn play(
        surface: &TextureSurface,
        path: &std::path::Path,
        stop: &AtomicBool,
    ) -> Result<(), String> {
        use std::os::fd::AsRawFd;
        let m = media().ok_or("the NDK media library could not be loaded")?;
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len() as i64;
        let ex = (m.ex_new)();
        if (m.ex_source_fd)(ex, file.as_raw_fd(), 0, len) != 0 {
            (m.ex_delete)(ex);
            return Err("the clip could not be opened".into());
        }
        let mut codec = std::ptr::null_mut();
        for track in 0..(m.ex_tracks)(ex) {
            let format = (m.ex_track_format)(ex, track);
            let mut mime = std::ptr::null();
            if (m.fmt_string)(format, c"mime".as_ptr(), &mut mime)
                && CStr::from_ptr(mime).to_bytes().starts_with(b"video/")
            {
                (m.ex_select)(ex, track);
                codec = (m.codec_by_type)(mime);
                if !codec.is_null() {
                    (m.codec_configure)(
                        codec,
                        format,
                        surface.native_window(),
                        std::ptr::null_mut(),
                        0,
                    );
                }
                (m.fmt_delete)(format);
                break;
            }
            (m.fmt_delete)(format);
        }
        if codec.is_null() {
            (m.ex_delete)(ex);
            return Err("no video decoder for the clip".into());
        }
        (m.codec_start)(codec);
        log::info!("texture demo: decoding into the texture surface");
        let mut clock = Instant::now();
        let mut input_done = false;
        let mut reported = Instant::now();
        while !stop.load(Ordering::Acquire) {
            if !input_done {
                let index = (m.codec_dequeue_in)(codec, 2_000);
                if index >= 0 {
                    let mut cap = 0usize;
                    let buffer = (m.codec_in_buffer)(codec, index as usize, &mut cap);
                    let size = (m.ex_read)(ex, buffer, cap);
                    if size < 0 {
                        (m.codec_queue_in)(codec, index as usize, 0, 0, 0, END_OF_STREAM);
                        input_done = true;
                    } else {
                        let at = (m.ex_time)(ex).max(0) as u64;
                        (m.codec_queue_in)(codec, index as usize, 0, size as usize, at, 0);
                        (m.ex_advance)(ex);
                    }
                }
            }
            let mut info = BufferInfo::default();
            let index = (m.codec_dequeue_out)(codec, &mut info, 5_000);
            if index >= 0 {
                // Released when its time comes, the clip at its own speed.
                let due = clock + Duration::from_micros(info.presentation_us.max(0) as u64);
                let now = Instant::now();
                if due > now {
                    std::thread::sleep(due - now);
                }
                (m.codec_release_out)(codec, index as usize, info.size > 0);
                if info.flags & END_OF_STREAM != 0 {
                    // Round again.
                    (m.ex_seek)(ex, 0, 0);
                    (m.codec_flush)(codec);
                    input_done = false;
                    clock = Instant::now();
                }
            }
            if reported.elapsed() >= Duration::from_secs(2) {
                let s = surface.texture().stats();
                log::info!(
                    "texture demo: {} pushed, {} dropped, {} shown",
                    s.pushed,
                    s.dropped,
                    s.shown
                );
                reported = Instant::now();
            }
        }
        (m.codec_stop)(codec);
        (m.codec_delete)(codec);
        (m.ex_delete)(ex);
        Ok(())
    }
}
