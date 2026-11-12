// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Windows**: Media Foundation's media engine, in frame-server mode.
//!
//! The engine opens the source (a file, an `http(s)` address, an HLS playlist on Windows 10 and
//! later), plays its sound, and keeps the clock. One thread per clip owns it: while the clip
//! plays, it asks the engine about every 8 ms whether a new picture is due
//! (`OnVideoStreamTick`), and when one is, has it drawn into a WIC bitmap
//! (`TransferVideoFrame`) and pushes its pixels into the clip's external texture. Paused, the
//! thread sleeps until it is told something: nothing runs.
//!
//! The engine's events arrive on Media Foundation's own threads and are handed to the clip's
//! thread, which turns them into the controller's reports.

use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::core::{implement, Interface, BSTR};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnLoad, WICBitmapLockRead, WICRect,
};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

use super::super::controller::{DataSource, DurationRange, Event, Player, Sink, VideoError};

/// How often a playing clip's thread asks whether a picture is due.
const TICK: Duration = Duration::from_millis(8);
/// How often the position is reported while the clip plays.
const POSITION_EVERY: Duration = Duration::from_millis(250);
/// `DXGI_FORMAT_B8G8R8A8_UNORM`: the pictures' pixels.
const BGRA: u32 = 87;

enum Cmd {
    Play,
    Pause,
    Seek(Duration),
    Volume(f32),
    Looping(bool),
    Speed(f32),
    /// An event of the engine's: its code and first parameter.
    Engine(u32, usize, u32),
    Quit,
}

pub(crate) fn open(source: &DataSource, sink: Arc<Sink>) -> Result<Box<dyn Player>, VideoError> {
    let url = match source {
        DataSource::Asset(bytes) => file_url(&super::asset_file(bytes)?),
        DataSource::File(path) => {
            if !path.exists() {
                return Err(VideoError::Source(format!(
                    "{}: no such file",
                    path.display()
                )));
            }
            file_url(&std::fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
        }
        DataSource::Network(url) => url.clone(),
    };
    let (tx, rx) = channel();
    let notify_tx = tx.clone();
    let thread = std::thread::Builder::new()
        .name("frus-video".into())
        .spawn(move || run(&url, notify_tx, rx, &sink))
        .map_err(|e| VideoError::Source(e.to_string()))?;
    Ok(Box::new(MediaEngine {
        tx,
        thread: Some(thread),
    }))
}

/// A `file:///` address for `path`, which the engine's source resolver opens.
fn file_url(path: &std::path::Path) -> String {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let mut url = String::from("file:///");
    for c in text.chars() {
        match c {
            '\\' => url.push('/'),
            ' ' => url.push_str("%20"),
            '#' => url.push_str("%23"),
            '%' => url.push_str("%25"),
            c => url.push(c),
        }
    }
    url
}

struct MediaEngine {
    tx: Sender<Cmd>,
    thread: Option<JoinHandle<()>>,
}

impl Player for MediaEngine {
    fn play(&self) {
        let _ = self.tx.send(Cmd::Play);
    }
    fn pause(&self) {
        let _ = self.tx.send(Cmd::Pause);
    }
    fn seek_to(&self, position: Duration) {
        let _ = self.tx.send(Cmd::Seek(position));
    }
    fn set_volume(&self, volume: f32) {
        let _ = self.tx.send(Cmd::Volume(volume));
    }
    fn set_looping(&self, looping: bool) {
        let _ = self.tx.send(Cmd::Looping(looping));
    }
    fn set_playback_speed(&self, speed: f32) {
        let _ = self.tx.send(Cmd::Speed(speed));
    }
}

impl Drop for MediaEngine {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The engine's callback: every event goes to the clip's thread.
#[implement(IMFMediaEngineNotify)]
struct Notify {
    tx: Mutex<Sender<Cmd>>,
}

impl IMFMediaEngineNotify_Impl for Notify_Impl {
    fn EventNotify(&self, event: u32, param1: usize, param2: u32) -> windows::core::Result<()> {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(Cmd::Engine(event, param1, param2));
        }
        Ok(())
    }
}

/// The clip's thread: Media Foundation up, the engine made, its loop run, everything down.
fn run(url: &str, notify: Sender<Cmd>, rx: Receiver<Cmd>, sink: &Sink) {
    unsafe {
        let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let mf = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET).is_ok();
        match create(url, notify) {
            Ok(engine) => {
                Clip::new(engine.clone()).run(&rx, sink);
                let _ = engine.Shutdown();
            }
            Err(err) => sink.send(Event::Error(format!("Media Foundation: {err}"))),
        }
        if mf {
            let _ = MFShutdown();
        }
        if com {
            CoUninitialize();
        }
    }
}

unsafe fn create(url: &str, notify: Sender<Cmd>) -> windows::core::Result<IMFMediaEngine> {
    let factory: IMFMediaEngineClassFactory =
        CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER)?;
    let mut attributes = None;
    MFCreateAttributes(&mut attributes, 2)?;
    let attributes = attributes.ok_or_else(windows::core::Error::empty)?;
    let callback: IMFMediaEngineNotify = Notify {
        tx: Mutex::new(notify),
    }
    .into();
    attributes.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &callback)?;
    // Frame-server mode: no window; the pictures are asked for.
    attributes.SetUINT32(&MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, BGRA)?;
    let engine = factory.CreateInstance(0, &attributes)?;
    engine.SetSource(&BSTR::from(url))?;
    Ok(engine)
}

/// What the clip's thread keeps.
struct Clip {
    engine: IMFMediaEngine,
    texture: Option<frus_widgets::ExternalTexture>,
    bitmap: Option<(IWICBitmap, u32, u32)>,
    wic: Option<IWICImagingFactory>,
    pixels: Vec<u8>,
    playing: bool,
    ready: bool,
    /// Pictures are asked for until then even while paused: the first one, the one a seek
    /// lands on.
    awake_until: Option<Instant>,
    last_position: Instant,
}

impl Clip {
    fn new(engine: IMFMediaEngine) -> Self {
        Self {
            engine,
            texture: None,
            bitmap: None,
            wic: None,
            pixels: Vec::new(),
            playing: false,
            ready: false,
            awake_until: None,
            last_position: Instant::now(),
        }
    }

    fn awake(&self) -> bool {
        self.playing || self.awake_until.is_some_and(|t| Instant::now() < t)
    }

    fn wake_for_a_while(&mut self) {
        self.awake_until = Some(Instant::now() + Duration::from_millis(500));
    }

    fn run(&mut self, rx: &Receiver<Cmd>, sink: &Sink) {
        loop {
            let cmd = if self.awake() {
                match rx.recv_timeout(TICK) {
                    Ok(cmd) => Some(cmd),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            } else {
                self.awake_until = None;
                match rx.recv() {
                    Ok(cmd) => Some(cmd),
                    Err(_) => return,
                }
            };
            if let Some(cmd) = cmd {
                if !self.handle(cmd, sink) {
                    return;
                }
            }
            if self.ready && self.awake() {
                self.tick(sink);
            }
        }
    }

    /// Carries out one command or event; `false` to stop.
    fn handle(&mut self, cmd: Cmd, sink: &Sink) -> bool {
        let engine = &self.engine;
        unsafe {
            match cmd {
                Cmd::Quit => return false,
                Cmd::Play => {
                    self.playing = true;
                    let _ = engine.Play();
                }
                Cmd::Pause => {
                    self.playing = false;
                    let _ = engine.Pause();
                    self.report_position(sink);
                }
                Cmd::Seek(at) => {
                    let _ = engine.SetCurrentTime(at.as_secs_f64());
                    self.wake_for_a_while();
                }
                Cmd::Volume(v) => {
                    let _ = engine.SetVolume(v as f64);
                }
                Cmd::Looping(on) => {
                    let _ = engine.SetLoop(on);
                }
                Cmd::Speed(s) => {
                    let _ = engine.SetPlaybackRate(s as f64);
                }
                Cmd::Engine(event, param1, param2) => self.event(event, param1, param2, sink),
            }
        }
        true
    }

    unsafe fn event(&mut self, event: u32, param1: usize, param2: u32, sink: &Sink) {
        let engine = self.engine.clone();
        let event = MF_MEDIA_ENGINE_EVENT(event as i32);
        match event {
            MF_MEDIA_ENGINE_EVENT_LOADEDMETADATA => {
                let (mut w, mut h) = (0u32, 0u32);
                let _ = engine.GetNativeVideoSize(Some(&mut w), Some(&mut h));
                let texture = (w > 0 && h > 0).then(|| frus_widgets::textures().create());
                let id = texture.as_ref().map(|t| t.id());
                self.texture = texture;
                self.ready = true;
                self.wake_for_a_while();
                sink.send(Event::Ready {
                    duration: seconds(engine.GetDuration()),
                    width: w,
                    height: h,
                    texture: id,
                });
            }
            MF_MEDIA_ENGINE_EVENT_FORMATCHANGE => {
                let (mut w, mut h) = (0u32, 0u32);
                if engine
                    .GetNativeVideoSize(Some(&mut w), Some(&mut h))
                    .is_ok()
                    && w > 0
                    && h > 0
                {
                    sink.send(Event::Size {
                        width: w,
                        height: h,
                    });
                }
            }
            MF_MEDIA_ENGINE_EVENT_FIRSTFRAMEREADY | MF_MEDIA_ENGINE_EVENT_LOADEDDATA => {
                self.wake_for_a_while();
            }
            MF_MEDIA_ENGINE_EVENT_SEEKED => {
                self.wake_for_a_while();
                self.report_position(sink);
            }
            MF_MEDIA_ENGINE_EVENT_WAITING | MF_MEDIA_ENGINE_EVENT_BUFFERINGSTARTED => {
                sink.send(Event::Buffering(true));
            }
            MF_MEDIA_ENGINE_EVENT_PLAYING
            | MF_MEDIA_ENGINE_EVENT_CANPLAY
            | MF_MEDIA_ENGINE_EVENT_CANPLAYTHROUGH
            | MF_MEDIA_ENGINE_EVENT_BUFFERINGENDED => {
                sink.send(Event::Buffering(false));
            }
            MF_MEDIA_ENGINE_EVENT_PROGRESS | MF_MEDIA_ENGINE_EVENT_DOWNLOADCOMPLETE => {
                if let Ok(ranges) = engine.GetBuffered() {
                    let mut out = Vec::new();
                    for i in 0..ranges.GetLength() {
                        if let (Ok(start), Ok(end)) = (ranges.GetStart(i), ranges.GetEnd(i)) {
                            out.push(DurationRange::new(seconds(start), seconds(end)));
                        }
                    }
                    sink.send(Event::Buffered(out));
                }
            }
            MF_MEDIA_ENGINE_EVENT_ENDED => {
                self.playing = false;
                self.report_position(sink);
                sink.send(Event::Completed);
            }
            MF_MEDIA_ENGINE_EVENT_ERROR => {
                self.playing = false;
                // `param1` is the MF_MEDIA_ENGINE_ERR code, `param2` the HRESULT.
                sink.send(Event::Error(error_text(param1, param2)));
            }
            _ => {}
        }
    }

    fn report_position(&mut self, sink: &Sink) {
        self.last_position = Instant::now();
        sink.send(Event::Position(seconds(unsafe {
            self.engine.GetCurrentTime()
        })));
    }

    /// Takes a picture if one is due, and reports the position now and then.
    fn tick(&mut self, sink: &Sink) {
        if self.playing && self.last_position.elapsed() >= POSITION_EVERY {
            self.report_position(sink);
        }
        let Some(texture) = self.texture.as_ref() else {
            return;
        };
        let mut pts = 0i64;
        // The raw call: S_FALSE (no new picture) and S_OK (one) are both successes, and only
        // the second is wanted.
        let hr = unsafe {
            (Interface::vtable(&self.engine).OnVideoStreamTick)(
                Interface::as_raw(&self.engine),
                &mut pts,
            )
        };
        if hr.0 != 0 {
            return;
        }
        if let Err(err) = unsafe { self.transfer(texture.id()) } {
            log::debug!("video: no picture ({err})");
        }
    }

    unsafe fn transfer(&mut self, _id: frus_widgets::TextureId) -> windows::core::Result<()> {
        let (mut w, mut h) = (0u32, 0u32);
        self.engine.GetNativeVideoSize(Some(&mut w), Some(&mut h))?;
        if w == 0 || h == 0 {
            return Ok(());
        }
        if self.wic.is_none() {
            self.wic = Some(CoCreateInstance(
                &CLSID_WICImagingFactory,
                None,
                CLSCTX_INPROC_SERVER,
            )?);
        }
        let stale = !matches!(&self.bitmap, Some((_, bw, bh)) if *bw == w && *bh == h);
        if stale {
            let wic = self.wic.as_ref().ok_or_else(windows::core::Error::empty)?;
            let bitmap =
                wic.CreateBitmap(w, h, &GUID_WICPixelFormat32bppBGRA, WICBitmapCacheOnLoad)?;
            self.bitmap = Some((bitmap, w, h));
        }
        let Some((bitmap, _, _)) = self.bitmap.as_ref() else {
            return Ok(());
        };
        let rect = RECT {
            left: 0,
            top: 0,
            right: w as i32,
            bottom: h as i32,
        };
        self.engine.TransferVideoFrame(bitmap, None, &rect, None)?;
        let lock = bitmap.Lock(
            &WICRect {
                X: 0,
                Y: 0,
                Width: w as i32,
                Height: h as i32,
            },
            WICBitmapLockRead.0 as u32,
        )?;
        let stride = lock.GetStride()? as usize;
        let mut size = 0u32;
        let mut data = std::ptr::null_mut();
        lock.GetDataPointer(&mut size, &mut data)?;
        if data.is_null() {
            return Ok(());
        }
        let source = std::slice::from_raw_parts(data, size as usize);
        let row = w as usize * 4;
        self.pixels.resize(row * h as usize, 0);
        for (y, out) in self.pixels.chunks_exact_mut(row).enumerate() {
            let Some(line) = source.get(y * stride..y * stride + row) else {
                break;
            };
            out.copy_from_slice(line);
        }
        // The engine leaves the alpha unset: a picture is opaque.
        let (texels, _) = self.pixels.as_chunks_mut::<4>();
        for px in texels {
            px[3] = 255;
        }
        drop(lock);
        if let Some(texture) = self.texture.as_ref() {
            let _ = texture.push_bgra(w, h, &self.pixels);
        }
        Ok(())
    }
}

/// A count of seconds as a duration: zero for what is not a finite, positive number (the
/// duration of a live stream is infinite, of an unknown one NaN).
fn seconds(s: f64) -> Duration {
    if s.is_finite() && s > 0.0 {
        Duration::from_secs_f64(s)
    } else {
        Duration::ZERO
    }
}

fn error_text(code: usize, hresult: u32) -> String {
    let what = match code {
        1 => "the loading was aborted",
        2 => "a network error",
        3 => "the media could not be decoded",
        4 => "the source is not supported",
        5 => "the media is encrypted",
        _ => "an unknown error",
    };
    format!("Media Foundation: {what} (0x{hresult:08X})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_addresses_are_escaped() {
        assert_eq!(
            file_url(std::path::Path::new(r"C:\Users\A B\clip #1.mp4")),
            "file:///C:/Users/A%20B/clip%20%231.mp4"
        );
        assert_eq!(
            file_url(std::path::Path::new(r"\\?\D:\v.mp4")),
            "file:///D:/v.mp4"
        );
    }

    #[test]
    fn durations_that_are_not_numbers_are_zero() {
        assert_eq!(seconds(f64::INFINITY), Duration::ZERO);
        assert_eq!(seconds(f64::NAN), Duration::ZERO);
        assert_eq!(seconds(1.5), Duration::from_millis(1500));
    }

    /// The real engine: the demo's clip opens, plays, its pictures reach the texture, and it
    /// ends. Run by hand: `cargo test -p frus-shell --features video --lib -- --ignored
    /// media_engine` (it needs Media Foundation, which Windows N editions lack).
    #[test]
    #[ignore]
    fn media_engine_plays_a_clip_into_its_texture() {
        static CLIP: &[u8] = include_bytes!("../../../../frus-demo/assets/texture-demo.mp4");
        let sink = Sink::new();
        let player = open(&DataSource::Asset(CLIP), sink.clone()).expect("opened");
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut take = || match sink.poll_take(&mut cx) {
            std::task::Poll::Ready(events) => events,
            std::task::Poll::Pending => Vec::new(),
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut texture = None;
        while texture.is_none() && Instant::now() < deadline {
            for event in take() {
                if let Event::Ready {
                    width,
                    height,
                    texture: t,
                    duration,
                } = event
                {
                    println!("ready: {width}x{height}, {duration:?}");
                    assert_eq!((width, height), (320, 240));
                    assert!(duration > Duration::from_secs(3));
                    texture = t;
                } else if let Event::Error(why) = event {
                    panic!("{why}");
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let texture = texture.expect("ready in time");
        player.set_playback_speed(2.0);
        player.play();
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut ended = false;
        let mut positions = 0;
        while !ended && Instant::now() < deadline {
            for event in take() {
                match event {
                    Event::Completed => ended = true,
                    Event::Position(_) => positions += 1,
                    Event::Error(why) => panic!("{why}"),
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let stats = frus_widgets::textures()
            .stats(texture)
            .expect("the texture");
        println!("{} pictures pushed, {positions} positions", stats.pushed);
        assert!(ended, "the clip ended");
        assert!(stats.pushed >= 30, "{} pictures", stats.pushed);
        let info = frus_widgets::textures()
            .frame_info(texture)
            .expect("a picture");
        println!("{info:?}");
        let frame = frus_widgets::textures().latest(texture).expect("a picture");
        let plane = frame.plane(0).expect("its pixels");
        let lit = plane.data.iter().filter(|b| **b > 40).count();
        assert!(lit > plane.data.len() / 4, "a picture, not black: {lit}");
        drop(frame);
        drop(player);
        assert!(
            !frus_widgets::textures().contains(texture),
            "released with the player"
        );
    }
}
