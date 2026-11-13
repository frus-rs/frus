// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The camera** (milestone 798): a live preview, still pictures, the frames themselves, the
//! flash, the zoom and the focus.
//!
//! ```no_run
//! use frus_shell::{available_cameras, CameraController, CameraLensDirection, ResolutionPreset};
//!
//! # async fn f() -> Result<(), frus_shell::CameraError> {
//! let cameras = available_cameras().await?;
//! let back = cameras
//!     .iter()
//!     .find(|c| c.lens_direction == CameraLensDirection::Back)
//!     .unwrap_or(&cameras[0]);
//! let camera = CameraController::new(back.clone(), ResolutionPreset::High);
//! camera.initialize().await?; // asks for the permission the first time
//! // Show it: `CameraPreview::new(&camera)`.
//! let picture = camera.take_picture().await?;
//! println!("{} bytes of JPEG at {:?}", picture.bytes.len(), picture.path);
//! # Ok(())
//! # }
//! ```
//!
//! What each platform does:
//!
//! - **Android**: Camera2, through the NDK. The preview streams into a
//!   [`TextureSurface`](crate::TextureSurface) whose frames the `Texture` widget shows, turned
//!   upright on the GPU; stills come from a second, JPEG stream. The application's manifest
//!   must ask for `android.permission.CAMERA` (with `frus build`:
//!   `[package.metadata.frus.permissions] camera = "..."`). The camera is closed while the
//!   application is in the background and opened again when it comes back.
//! - **Windows**: Media Foundation's capture devices (webcams), frames read on a thread of
//!   their own; a picture is the latest frame, encoded as JPEG. No flash, zoom or focus:
//!   those answer [`CameraError::Unsupported`].
//! - **The web**: `getUserMedia`, the frames drawn from a hidden `<video>` once per frame of
//!   the page. Needs a secure page (`https` or `localhost`).
//! - **macOS, Linux, iOS**: not yet; everything answers [`CameraError::Unsupported`].
//!
//! Nothing runs while no camera is open: no thread, no texture, no timer.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use frus_widgets::{FrameOrientation, TextureId};

#[cfg(android)]
#[path = "camera_android.rs"]
mod platform;
#[cfg(all(windows, not(android), not(web)))]
#[path = "camera_windows.rs"]
mod platform;
#[cfg(web)]
#[path = "camera_web.rs"]
mod platform;
#[cfg(not(any(android, windows, web)))]
mod platform {
    //! No camera here yet.

    use super::*;

    pub(super) enum Session {}

    pub(super) fn cameras() -> Result<Vec<CameraDescription>, CameraError> {
        Err(UNSUPPORTED)
    }

    pub(super) const UNSUPPORTED: CameraError =
        CameraError::Unsupported("the camera is not available on this platform yet");

    pub(super) struct Preview;

    impl Preview {
        pub(super) fn new() -> Result<Self, CameraError> {
            Err(UNSUPPORTED)
        }
        pub(super) fn id(&self) -> TextureId {
            unreachable!()
        }
    }

    impl Session {
        pub(super) fn open(
            _: &CameraDescription,
            _: ResolutionPreset,
            _: &Preview,
            _: Arc<Sink>,
            _: bool,
        ) -> Result<Self, CameraError> {
            Err(UNSUPPORTED)
        }
        pub(super) fn preview_size(&self) -> (u32, u32) {
            match *self {}
        }
        pub(super) fn orientation(&self) -> FrameOrientation {
            match *self {}
        }
        pub(super) fn zoom_range(&self) -> (f32, f32) {
            match *self {}
        }
        pub(super) fn apply(&self, _: &Settings) -> Result<(), CameraError> {
            match *self {}
        }
        pub(super) fn take_picture(&self, _: &Settings) -> Result<Vec<u8>, CameraError> {
            match *self {}
        }
        pub(super) fn display_changed(&self) {
            match *self {}
        }
    }
}

/// Which way a camera faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CameraLensDirection {
    /// The person's side: the selfie camera.
    Front,
    /// Away from the person.
    Back,
    /// Plugged in: a webcam.
    External,
}

/// A camera of the device, as [`available_cameras`] lists it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CameraDescription {
    /// The platform's name or identifier for it.
    pub name: String,
    /// Which way it faces.
    pub lens_direction: CameraLensDirection,
    /// How many degrees clockwise its picture must turn to stand upright on the device held in
    /// its natural orientation: `90` for most phones' back cameras, `270` for their front ones,
    /// `0` for a webcam.
    pub sensor_orientation: u32,
}

/// How large the preview and the pictures are: the camera picks the size closest to the
/// preset among those it offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ResolutionPreset {
    /// About 320×240.
    Low,
    /// About 720×480: enough to read a code, light on a phone.
    Medium,
    /// About 1280×720. The default.
    #[default]
    High,
    /// About 1920×1080.
    VeryHigh,
    /// About 3840×2160.
    UltraHigh,
    /// The largest the camera offers.
    Max,
}

impl ResolutionPreset {
    /// The size aimed at, the long side first.
    pub fn target(self) -> (u32, u32) {
        match self {
            ResolutionPreset::Low => (320, 240),
            ResolutionPreset::Medium => (720, 480),
            ResolutionPreset::High => (1280, 720),
            ResolutionPreset::VeryHigh => (1920, 1080),
            ResolutionPreset::UltraHigh => (3840, 2160),
            ResolutionPreset::Max => (u32::MAX, u32::MAX),
        }
    }
}

/// What the flash does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FlashMode {
    /// Never fires.
    Off,
    /// Fires for a picture when the scene is dark. The default.
    #[default]
    Auto,
    /// Fires for every picture.
    Always,
    /// Stays on, as a torch.
    Torch,
}

/// Why the camera could not do what was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CameraError {
    /// Not on this platform, or not on this camera (a webcam's flash, say).
    Unsupported(&'static str),
    /// The person did not allow the camera; the status says whether asking again can help.
    PermissionDenied(crate::PermissionStatus),
    /// No camera by that name.
    NotFound,
    /// [`CameraController::initialize`] has not succeeded yet, or the controller was disposed.
    NotInitialized,
    /// The platform refused, with its reason.
    Platform(String),
}

impl fmt::Display for CameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CameraError::Unsupported(why) => write!(f, "unsupported: {why}"),
            CameraError::PermissionDenied(status) => {
                write!(f, "the camera permission was not granted ({status:?})")
            }
            CameraError::NotFound => f.write_str("no such camera"),
            CameraError::NotInitialized => f.write_str("the camera is not initialized"),
            CameraError::Platform(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for CameraError {}

/// A picture [`CameraController::take_picture`] took.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// The picture, encoded as JPEG. On Android it is the camera's own file, which says in its
    /// EXIF data how to turn it upright.
    pub bytes: Vec<u8>,
    /// Where it was saved, in the application's cache folder; `None` on the web.
    pub path: Option<std::path::PathBuf>,
}

/// The layout of a [`CameraImage`]'s pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CameraImageFormat {
    /// YUV 4:2:0 in three planes, Y then U then V, each with its own strides (Android).
    Yuv420,
    /// NV12: Y, then U and V interleaved (Windows).
    Nv12,
    /// 8-bit RGBA, one plane (the web).
    Rgba8,
}

/// One plane of a [`CameraImage`].
#[derive(Clone, Copy, Debug)]
pub struct CameraPlane<'a> {
    /// The plane's bytes, from its first sample.
    pub bytes: &'a [u8],
    /// Bytes from one row to the next.
    pub row_stride: usize,
    /// Bytes from one sample to the next in a row.
    pub pixel_stride: usize,
}

/// A frame of the camera, as [`CameraController::start_image_stream`] hands it over: borrowed
/// from the camera for the time of the call.
#[derive(Clone, Copy, Debug)]
pub struct CameraImage<'a> {
    /// Width, in pixels, as the sensor gives it (before [`orientation`](Self::orientation)).
    pub width: u32,
    /// Height, in pixels.
    pub height: u32,
    /// The layout.
    pub format: CameraImageFormat,
    /// The planes, in the layout's order.
    pub planes: &'a [CameraPlane<'a>],
    /// How the frame must turn to stand upright as the preview shows it.
    pub orientation: FrameOrientation,
}

impl CameraImage<'_> {
    /// The brightness of pixel `(x, y)`, `0..=255`: the luma plane's sample for YUV, computed
    /// for RGBA. Out of range reads `0`.
    pub fn luma(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        let Some(plane) = self.planes.first() else {
            return 0;
        };
        let at = y as usize * plane.row_stride + x as usize * plane.pixel_stride;
        match self.format {
            CameraImageFormat::Yuv420 | CameraImageFormat::Nv12 => {
                plane.bytes.get(at).copied().unwrap_or(0)
            }
            CameraImageFormat::Rgba8 => match plane.bytes.get(at..at + 3) {
                // BT.601 weights, in integers.
                Some(p) => {
                    ((u32::from(p[0]) * 77 + u32::from(p[1]) * 150 + u32::from(p[2]) * 29) >> 8)
                        as u8
                }
                None => 0,
            },
        }
    }

    /// The brightness of the frame into `out`, shrunk by a whole factor until neither side is
    /// longer than `max_side`: what a code reader needs, and no more. Answers the size written
    /// and the factor (a pixel of `out` is the frame's pixel `(x * factor, y * factor)`).
    ///
    /// ```
    /// use frus_shell::{CameraImage, CameraImageFormat, CameraPlane};
    /// let y = [10u8; 8 * 4];
    /// let planes = [CameraPlane { bytes: &y, row_stride: 8, pixel_stride: 1 }];
    /// let image = CameraImage {
    ///     width: 8,
    ///     height: 4,
    ///     format: CameraImageFormat::Yuv420,
    ///     planes: &planes,
    ///     orientation: Default::default(),
    /// };
    /// let mut out = Vec::new();
    /// assert_eq!(image.luma_plane(4, &mut out), (4, 2, 2));
    /// assert_eq!(out, [10; 8]);
    /// ```
    pub fn luma_plane(&self, max_side: u32, out: &mut Vec<u8>) -> (u32, u32, u32) {
        let longest = self.width.max(self.height).max(1);
        let factor = longest.div_ceil(max_side.max(1)).max(1);
        let (w, h) = (self.width / factor, self.height / factor);
        out.clear();
        out.reserve(w as usize * h as usize);
        let packed_luma = matches!(
            self.format,
            CameraImageFormat::Yuv420 | CameraImageFormat::Nv12
        ) && self.planes.first().is_some_and(|p| p.pixel_stride == 1);
        for y in 0..h {
            if packed_luma && factor == 1 {
                // The common case, a row at a time.
                let plane = &self.planes[0];
                let start = y as usize * plane.row_stride;
                match plane.bytes.get(start..start + w as usize) {
                    Some(row) => out.extend_from_slice(row),
                    None => out.extend(std::iter::repeat_n(0, w as usize)),
                }
                continue;
            }
            for x in 0..w {
                out.push(self.luma(x * factor, y * factor));
            }
        }
        (w, h, factor)
    }
}

/// The settings a session is opened with again after a pause.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Settings {
    pub(crate) flash: FlashMode,
    pub(crate) zoom: f32,
    /// Where to focus and meter, in `0..1` of the frame as the sensor gives it.
    pub(crate) focus: Option<(f32, f32)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            flash: FlashMode::Auto,
            zoom: 1.0,
            focus: None,
        }
    }
}

type StreamFn = Box<dyn FnMut(&CameraImage<'_>) + Send>;

/// Where a session hands its frames: the image stream, when one is listening.
#[derive(Default)]
pub(crate) struct Sink {
    listening: AtomicBool,
    stream: Mutex<Option<StreamFn>>,
}

impl Sink {
    /// Whether anything wants the frames: the backends skip reading them otherwise.
    pub(crate) fn wants_frames(&self) -> bool {
        self.listening.load(Ordering::Acquire)
    }

    /// Hands `image` to the image stream.
    #[cfg_attr(not(any(android, windows, web)), allow(dead_code))]
    pub(crate) fn deliver(&self, image: &CameraImage<'_>) {
        if !self.wants_frames() {
            return;
        }
        if let Ok(mut stream) = self.stream.lock() {
            if let Some(stream) = stream.as_mut() {
                stream(image);
            }
        }
    }
}

/// **A camera**, opened by [`initialize`](Self::initialize) and shown by `CameraPreview`.
///
/// Cheap to clone: every clone is the same camera, which is closed when the last one is
/// dropped or [`dispose`](Self::dispose) is called.
///
/// ```no_run
/// use frus_shell::{CameraController, FlashMode, ResolutionPreset};
///
/// # async fn f(description: frus_shell::CameraDescription) -> Result<(), frus_shell::CameraError> {
/// let camera = CameraController::new(description, ResolutionPreset::Medium);
/// camera.initialize().await?;
/// camera.set_flash_mode(FlashMode::Torch)?;
/// camera.set_zoom_level(2.0)?;
/// camera.set_focus_point(Some((0.5, 0.5)))?;
/// camera.start_image_stream(|image| {
///     let _ = image.luma(0, 0); // on the camera's thread: keep it short
/// })?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct CameraController {
    inner: Arc<Inner>,
}

struct Inner {
    description: Mutex<CameraDescription>,
    preset: ResolutionPreset,
    preview: Mutex<Option<platform::Preview>>,
    session: Mutex<Option<platform::Session>>,
    settings: Mutex<Settings>,
    sink: Arc<Sink>,
    /// Closed by the application going to the background, to be opened again.
    suspended: AtomicBool,
    /// Whether pictures are taken: a scanner's camera opens no JPEG stream.
    stills: AtomicBool,
}

impl fmt::Debug for CameraController {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CameraController")
            .field("description", &self.description())
            .field("preset", &self.inner.preset)
            .field("initialized", &self.is_initialized())
            .finish()
    }
}

/// The cameras of the device: Android's back and front cameras, the webcams on Windows and
/// in a browser. [`CameraError::Unsupported`] where there is no camera support yet.
///
/// On the web, browsers name their cameras only once the page may use one, so the list
/// asks for the permission first.
pub async fn available_cameras() -> Result<Vec<CameraDescription>, CameraError> {
    #[cfg(web)]
    {
        platform::cameras().await
    }
    #[cfg(not(web))]
    {
        blocking::unblock(platform::cameras).await
    }
}

impl CameraController {
    /// A controller for `description`'s camera at about `preset`'s size. Nothing is opened
    /// before [`initialize`](Self::initialize).
    pub fn new(description: CameraDescription, preset: ResolutionPreset) -> Self {
        Self {
            inner: Arc::new(Inner {
                description: Mutex::new(description),
                preset,
                preview: Mutex::new(None),
                session: Mutex::new(None),
                settings: Mutex::new(Settings::default()),
                sink: Arc::default(),
                suspended: AtomicBool::new(false),
                stills: AtomicBool::new(true),
            }),
        }
    }

    /// A controller that takes no pictures: lighter, for reading codes.
    #[cfg_attr(not(feature = "scanner"), allow(dead_code))]
    pub(crate) fn without_stills(self) -> Self {
        self.inner.stills.store(false, Ordering::Relaxed);
        self
    }

    /// The camera this controls.
    pub fn description(&self) -> CameraDescription {
        lock(&self.inner.description).clone()
    }

    /// The preset it was made with.
    pub fn resolution_preset(&self) -> ResolutionPreset {
        self.inner.preset
    }

    /// Asks for the camera permission when it has not been given, then opens the camera and
    /// starts its preview.
    ///
    /// Call it from an `async` handler: the permission's question waits for the person.
    pub async fn initialize(&self) -> Result<(), CameraError> {
        let status = crate::Permission::Camera.request().await;
        if !status.is_granted() {
            return Err(CameraError::PermissionDenied(status));
        }
        self.open().await
    }

    async fn open(&self) -> Result<(), CameraError> {
        #[cfg(web)]
        {
            self.inner.close();
            let preview = platform::Preview::new()?;
            let session = platform::Session::open(
                &self.description(),
                self.inner.preset,
                &preview,
                self.inner.sink.clone(),
                self.inner.stills.load(Ordering::Relaxed),
            )
            .await?;
            self.inner.install(preview, session)
        }
        #[cfg(not(web))]
        {
            let inner = self.inner.clone();
            blocking::unblock(move || inner.open_blocking()).await?;
            Ok(())
        }
    }

    /// Whether the camera is open (or closed only while the application is in the background).
    pub fn is_initialized(&self) -> bool {
        lock(&self.inner.preview).is_some()
    }

    /// The texture the preview is shown in, for a `Texture` widget; `None` before
    /// [`initialize`](Self::initialize).
    pub fn texture_id(&self) -> Option<TextureId> {
        lock(&self.inner.preview).as_ref().map(|p| p.id())
    }

    /// The preview's size as it is shown, upright; `None` while the camera is not open.
    pub fn preview_size(&self) -> Option<(u32, u32)> {
        self.inner
            .with_session(|s| {
                let (w, h) = s.preview_size();
                Ok(s.orientation().size(w, h))
            })
            .ok()
    }

    /// Takes a picture, encoded as JPEG and saved in the application's cache folder.
    pub async fn take_picture(&self) -> Result<Picture, CameraError> {
        let settings = *lock(&self.inner.settings);
        #[cfg(web)]
        let bytes = self.inner.with_session(|s| s.take_picture(&settings))?;
        #[cfg(not(web))]
        let bytes = {
            let inner = self.inner.clone();
            blocking::unblock(move || inner.with_session(|s| s.take_picture(&settings))).await?
        };
        let path = save_picture(&bytes);
        Ok(Picture { bytes, path })
    }

    /// Hands every frame to `on_image`, **on the camera's thread**, until
    /// [`stop_image_stream`](Self::stop_image_stream): for an analysis of your own. The frame
    /// is borrowed for the time of the call; copy what you keep, and return quickly, or frames
    /// are dropped. Replaces a stream already listening.
    pub fn start_image_stream(
        &self,
        on_image: impl FnMut(&CameraImage<'_>) + Send + 'static,
    ) -> Result<(), CameraError> {
        if !self.is_initialized() {
            return Err(CameraError::NotInitialized);
        }
        *lock(&self.inner.sink.stream) = Some(Box::new(on_image));
        self.inner.sink.listening.store(true, Ordering::Release);
        Ok(())
    }

    /// Stops the image stream.
    pub fn stop_image_stream(&self) {
        self.inner.sink.listening.store(false, Ordering::Release);
        *lock(&self.inner.sink.stream) = None;
    }

    /// Whether an image stream is listening.
    pub fn is_streaming_images(&self) -> bool {
        self.inner.sink.wants_frames()
    }

    /// The flash mode set.
    pub fn flash_mode(&self) -> FlashMode {
        lock(&self.inner.settings).flash
    }

    /// Sets what the flash does: [`FlashMode::Torch`] lights it now.
    pub fn set_flash_mode(&self, mode: FlashMode) -> Result<(), CameraError> {
        self.inner.change(|s| s.flash = mode)
    }

    /// The smallest zoom level, `1.0`.
    pub fn min_zoom_level(&self) -> f32 {
        self.inner
            .with_session(|s| Ok(s.zoom_range().0))
            .unwrap_or(1.0)
    }

    /// The largest zoom level the camera offers; `1.0` without a zoom.
    pub fn max_zoom_level(&self) -> f32 {
        self.inner
            .with_session(|s| Ok(s.zoom_range().1))
            .unwrap_or(1.0)
    }

    /// The zoom level set.
    pub fn zoom_level(&self) -> f32 {
        lock(&self.inner.settings).zoom
    }

    /// Zooms in to `level`, between [`min_zoom_level`](Self::min_zoom_level) and
    /// [`max_zoom_level`](Self::max_zoom_level) (clamped to them). A camera without a zoom
    /// (a webcam, a browser's camera) answers [`CameraError::Unsupported`] for any level but
    /// `1.0`.
    pub fn set_zoom_level(&self, level: f32) -> Result<(), CameraError> {
        let (min, max) = self.inner.with_session(|s| Ok(s.zoom_range()))?;
        if !level.is_finite() {
            return Err(CameraError::Platform(format!("zoom level {level}")));
        }
        if max <= min && level != min {
            return Err(CameraError::Unsupported("this camera has no zoom"));
        }
        self.inner.change(|s| s.zoom = level.clamp(min, max))
    }

    /// Focuses and meters on `point`, in `0..1` of the preview as it is shown (`(0.5, 0.5)`
    /// its middle); `None` goes back to the whole picture.
    pub fn set_focus_point(&self, point: Option<(f32, f32)>) -> Result<(), CameraError> {
        let orientation = self.inner.with_session(|s| Ok(s.orientation()))?;
        let point =
            point.map(|(x, y)| sensor_point(orientation, x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)));
        self.inner.change(|s| s.focus = point)
    }

    /// Switches to another camera — the front one, say — keeping the preview's texture.
    pub async fn set_description(&self, description: CameraDescription) -> Result<(), CameraError> {
        *lock(&self.inner.description) = description;
        {
            let mut settings = lock(&self.inner.settings);
            settings.zoom = 1.0;
            settings.focus = None;
        }
        if self.is_initialized() {
            self.inner.close_session();
            self.open().await
        } else {
            Ok(())
        }
    }

    /// Closes the camera and lets the preview go dark, until [`resume_preview`]. The image
    /// stream stays set.
    ///
    /// [`resume_preview`]: Self::resume_preview
    pub fn pause_preview(&self) {
        self.inner.close_session();
    }

    /// Opens the camera again after [`pause_preview`](Self::pause_preview).
    pub async fn resume_preview(&self) -> Result<(), CameraError> {
        if !self.is_initialized() {
            return Err(CameraError::NotInitialized);
        }
        self.open().await
    }

    /// Closes the camera and drops the preview's texture. The controller may be initialized
    /// again.
    pub fn dispose(&self) {
        self.stop_image_stream();
        self.inner.close();
    }
}

impl Inner {
    #[cfg(not(web))]
    fn open_blocking(self: &Arc<Self>) -> Result<(), CameraError> {
        self.close_session();
        let mut preview = lock(&self.preview).take();
        if preview.is_none() {
            preview = Some(platform::Preview::new()?);
        }
        let preview = preview.expect("made above");
        let description = lock(&self.description).clone();
        let stills = self.stills.load(Ordering::Relaxed);
        match platform::Session::open(
            &description,
            self.preset,
            &preview,
            self.sink.clone(),
            stills,
        ) {
            Ok(session) => self.install(preview, session),
            Err(err) => {
                *lock(&self.preview) = Some(preview);
                Err(err)
            }
        }
    }

    fn install(
        self: &Arc<Self>,
        preview: platform::Preview,
        session: platform::Session,
    ) -> Result<(), CameraError> {
        let settings = *lock(&self.settings);
        if let Err(err) = session.apply(&settings) {
            log::warn!("camera: settings not applied ({err})");
        }
        *lock(&self.preview) = Some(preview);
        *lock(&self.session) = Some(session);
        self.suspended.store(false, Ordering::Release);
        register(self);
        Ok(())
    }

    fn with_session<T>(
        &self,
        work: impl FnOnce(&platform::Session) -> Result<T, CameraError>,
    ) -> Result<T, CameraError> {
        match lock(&self.session).as_ref() {
            Some(session) => work(session),
            None => Err(CameraError::NotInitialized),
        }
    }

    /// Changes a setting, and the open session with it.
    fn change(&self, edit: impl FnOnce(&mut Settings)) -> Result<(), CameraError> {
        let mut settings = *lock(&self.settings);
        edit(&mut settings);
        self.with_session(|s| s.apply(&settings))?;
        *lock(&self.settings) = settings;
        Ok(())
    }

    // Without a camera backend the session and preview hold nothing that needs dropping.
    #[cfg_attr(not(any(android, windows, web)), allow(clippy::drop_non_drop))]
    fn close_session(&self) {
        // Taken out of the lock first: closing a camera waits for its threads, which may be
        // asking for this lock.
        let session = lock(&self.session).take();
        drop(session);
    }

    #[cfg_attr(not(any(android, windows, web)), allow(clippy::drop_non_drop))]
    fn close(&self) {
        self.close_session();
        let preview = lock(&self.preview).take();
        drop(preview);
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.close();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// The point of the frame as the sensor gives it, in `0..1`, that shows at `(x, y)` of the
/// preview as it is drawn under `orientation`.
fn sensor_point(orientation: FrameOrientation, x: f32, y: f32) -> (f32, f32) {
    // The inverse of `FrameOrientation::map_point` on the unit square (the frame's own size
    // does not matter once both are normalised): undo the turn, then the mirror.
    let (sx, sy) = match orientation.quarter_turns() {
        1 => (y, 1.0 - x),
        2 => (1.0 - x, 1.0 - y),
        3 => (1.0 - y, x),
        _ => (x, y),
    };
    if orientation.mirrored() {
        (1.0 - sx, sy)
    } else {
        (sx, sy)
    }
}

/// The preview's orientation for a camera mounted `sensor` degrees off the device's natural
/// orientation, on a display turned `display` degrees (`0`, `90`, `180`, `270`, the way
/// Android counts them): the front camera's picture is mirrored, as a mirror shows a face.
#[cfg_attr(not(android), allow(dead_code))]
pub(crate) fn preview_orientation(
    lens: CameraLensDirection,
    sensor: u32,
    display: u32,
) -> FrameOrientation {
    let degrees = match lens {
        // The front camera faces the other way: the display's turn adds to the sensor's.
        CameraLensDirection::Front => (sensor + display) % 360,
        _ => (sensor + 360 - display % 360) % 360,
    };
    FrameOrientation::new((degrees / 90) as i32, lens == CameraLensDirection::Front)
}

/// The size among `sizes` closest to `preset`, the long side compared with the long side:
/// the largest that fits in it, else the smallest larger one; of the same shape as `like`
/// when one is.
#[cfg_attr(not(any(android, windows)), allow(dead_code))]
pub(crate) fn choose_size(
    sizes: &[(u32, u32)],
    preset: ResolutionPreset,
    like: Option<(u32, u32)>,
) -> Option<(u32, u32)> {
    let (tw, th) = preset.target();
    let landscape = |(w, h): (u32, u32)| (w.max(h), w.min(h));
    let same_shape = |s: (u32, u32)| match like {
        Some(l) => {
            let (a, b) = (landscape(s), landscape(l));
            (a.0 as u64 * b.1 as u64).abs_diff(b.0 as u64 * a.1 as u64) * 100
                <= a.0 as u64 * b.1 as u64
        }
        None => true,
    };
    let pick = |candidates: Vec<(u32, u32)>| {
        let area = |s: &(u32, u32)| s.0 as u64 * s.1 as u64;
        let fits = candidates
            .iter()
            .filter(|&&s| landscape(s).0 <= tw && landscape(s).1 <= th)
            .max_by_key(|s| area(s))
            .copied();
        fits.or_else(|| candidates.iter().min_by_key(|s| area(s)).copied())
    };
    let shaped: Vec<_> = sizes.iter().copied().filter(|&s| same_shape(s)).collect();
    if shaped.is_empty() {
        pick(sizes.to_vec())
    } else {
        pick(shaped)
    }
}

/// An NV12 frame as a JPEG (Windows' pictures): BT.601, limited range.
#[cfg(windows)]
pub(crate) fn nv12_to_jpeg(y: &[u8], uv: &[u8], width: u32, height: u32, pitch: usize) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut rgb = Vec::with_capacity(w * h * 3);
    for row in 0..h {
        for col in 0..w {
            let luma = f32::from(y.get(row * pitch + col).copied().unwrap_or(16)) - 16.0;
            let at = (row / 2) * pitch + (col / 2) * 2;
            let u = f32::from(uv.get(at).copied().unwrap_or(128)) - 128.0;
            let v = f32::from(uv.get(at + 1).copied().unwrap_or(128)) - 128.0;
            let l = 1.164 * luma;
            let px = [l + 1.596 * v, l - 0.392 * u - 0.813 * v, l + 2.017 * u];
            rgb.extend(px.map(|c| c.clamp(0.0, 255.0) as u8));
        }
    }
    let mut out = Vec::new();
    let encoded = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92).encode(
        &rgb,
        width,
        height,
        image::ExtendedColorType::Rgb8,
    );
    if let Err(err) = encoded {
        log::warn!("camera: the picture was not encoded ({err})");
    }
    out
}

/// Saves a picture's bytes in the cache folder's `camera` folder; `None` where there is no
/// disk, or when it cannot be written (the bytes are still handed back).
fn save_picture(bytes: &[u8]) -> Option<std::path::PathBuf> {
    let folder = crate::cache_dir::app_cache_dir()?.join("camera");
    std::fs::create_dir_all(&folder).ok()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let path = folder.join(format!("CAP_{stamp}.jpg"));
    match std::fs::write(&path, bytes) {
        Ok(()) => Some(path),
        Err(err) => {
            log::warn!("camera: the picture was not saved ({err})");
            None
        }
    }
}

/// The open cameras, for the lifecycle: closed in the background, opened again on return.
static OPEN: Mutex<Vec<Weak<Inner>>> = Mutex::new(Vec::new());

fn register(inner: &Arc<Inner>) {
    let mut open = lock(&OPEN);
    open.retain(|w| w.strong_count() > 0);
    if !open
        .iter()
        .any(|w| std::ptr::eq(w.as_ptr(), Arc::as_ptr(inner)))
    {
        open.push(Arc::downgrade(inner));
    }
}

fn open_cameras() -> Vec<Arc<Inner>> {
    let mut open = lock(&OPEN);
    open.retain(|w| w.strong_count() > 0);
    open.iter().filter_map(Weak::upgrade).collect()
}

/// The application went to the background (`false`) or came back (`true`): a camera open
/// when it left is closed, and opened again on its return, on a thread of its own.
pub(crate) fn lifecycle(resumed: bool) {
    let cameras = open_cameras();
    if cameras.is_empty() {
        return;
    }
    for inner in cameras {
        if !resumed {
            if lock(&inner.session).is_some() {
                log::info!("camera: closed while the application is in the background");
                inner.close_session();
                inner.suspended.store(true, Ordering::Release);
            }
        } else if inner.suspended.swap(false, Ordering::AcqRel) {
            #[cfg(not(web))]
            {
                let spawned = std::thread::Builder::new()
                    .name("frus-camera-resume".into())
                    .spawn(move || {
                        if lock(&inner.preview).is_none() {
                            return;
                        }
                        match inner.open_blocking() {
                            Ok(()) => log::info!("camera: opened again"),
                            Err(err) => log::warn!("camera: not opened again ({err})"),
                        }
                    });
                if let Err(err) = spawned {
                    log::warn!("camera: not opened again ({err})");
                }
            }
            #[cfg(web)]
            {
                let controller = CameraController { inner };
                wasm_bindgen_futures::spawn_local(async move {
                    if let Err(err) = controller.open().await {
                        log::warn!("camera: not opened again ({err})");
                    }
                });
            }
        }
    }
}

/// The display turned: the open cameras' previews turn with it.
#[cfg_attr(not(android), allow(dead_code))]
pub(crate) fn display_changed() {
    for inner in open_cameras() {
        if let Some(session) = lock(&inner.session).as_ref() {
            session.display_changed();
        }
    }
}

/// **The camera's preview**: the frames of a [`CameraController`], upright, in the room the
/// widget is given.
///
/// It shows the whole frame by default ([`BoxFit::Contain`](frus_widgets::BoxFit::Contain)),
/// with the bars around it painted black; [`fit`](Self::fit) with
/// [`BoxFit::Cover`](frus_widgets::BoxFit::Cover) fills the widget and crops instead. Before
/// the camera is open it paints its background.
///
/// ```no_run
/// use frus_shell::{CameraController, CameraPreview};
/// use frus_widgets::{BoxFit, Color};
///
/// # fn view(camera: &CameraController) {
/// let _preview = CameraPreview::new(camera).fit(BoxFit::Cover).background(Color::BLACK);
/// # }
/// ```
pub struct CameraPreview {
    texture: Option<frus_widgets::Texture>,
    fit: frus_widgets::BoxFit,
    background: frus_widgets::Color,
    semantic_label: String,
}

impl CameraPreview {
    /// The preview of `controller`'s camera.
    pub fn new(controller: &CameraController) -> Self {
        let mut preview = Self::empty();
        preview.texture = controller.texture_id().map(frus_widgets::Texture::new);
        preview
    }

    /// A preview of no camera: its background, until there is one.
    pub fn empty() -> Self {
        Self {
            texture: None,
            fit: frus_widgets::BoxFit::Contain,
            background: frus_widgets::Color::BLACK,
            semantic_label: "Camera preview".into(),
        }
    }

    /// How the frame fits the widget: [`BoxFit::Contain`](frus_widgets::BoxFit::Contain) by
    /// default.
    pub fn fit(mut self, fit: frus_widgets::BoxFit) -> Self {
        self.fit = fit;
        self
    }

    /// The colour around the frame and before it: black by default.
    pub fn background(mut self, color: frus_widgets::Color) -> Self {
        self.background = color;
        self
    }

    /// What a screen reader says the preview is: "Camera preview" by default.
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = label.into();
        self
    }

    fn texture(&self) -> Option<frus_widgets::Texture> {
        let id = self.texture.as_ref()?.id();
        Some(
            frus_widgets::Texture::new(id)
                .fit(self.fit)
                .background(self.background)
                .semantic_label(self.semantic_label.clone()),
        )
    }
}

impl<Msg> frus_widgets::Widget<Msg> for CameraPreview {
    fn style(&self) -> frus_widgets::LayoutStyle {
        // The room on offer, as the texture takes it.
        let texture = frus_widgets::Texture::new(TextureId::from_u64(0));
        frus_widgets::Widget::<Msg>::style(&texture)
    }

    fn children(&self) -> &[Box<dyn frus_widgets::Widget<Msg>>] {
        &[]
    }

    fn paint(
        &self,
        bounds: frus_widgets::Rect,
        status: frus_widgets::Status,
        theme: &frus_widgets::Theme,
        scene: &mut frus_widgets::Scene,
    ) {
        match self.texture() {
            Some(texture) => {
                frus_widgets::Widget::<Msg>::paint(&texture, bounds, status, theme, scene)
            }
            None => scene.fill_rect(bounds, self.background.fade(status.opacity)),
        }
    }

    fn paint_source(&self) -> Option<u64> {
        self.texture
            .as_ref()
            .and_then(frus_widgets::Widget::<Msg>::paint_source)
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn semantics(&self) -> Option<frus_widgets::SemanticsProperties> {
        Some(
            frus_widgets::SemanticsProperties::new(frus_widgets::Role::Image)
                .label(self.semantic_label.clone()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_back_camera_on_a_phone_held_upright_turns_a_quarter() {
        let o = preview_orientation(CameraLensDirection::Back, 90, 0);
        assert_eq!((o.quarter_turns(), o.mirrored()), (1, false));
        // Turned to landscape (the display at 90°): upright as it comes.
        let o = preview_orientation(CameraLensDirection::Back, 90, 90);
        assert_eq!(o.quarter_turns(), 0);
        let o = preview_orientation(CameraLensDirection::Back, 90, 270);
        assert_eq!(o.quarter_turns(), 2);
    }

    #[test]
    fn a_front_camera_is_mirrored_and_turns_with_the_display_the_other_way() {
        let o = preview_orientation(CameraLensDirection::Front, 270, 0);
        assert_eq!((o.quarter_turns(), o.mirrored()), (3, true));
        let o = preview_orientation(CameraLensDirection::Front, 270, 90);
        assert_eq!(o.quarter_turns(), 0);
        let o = preview_orientation(CameraLensDirection::External, 0, 0);
        assert!(o.is_upright());
    }

    #[test]
    fn a_point_of_the_preview_is_found_back_on_the_sensor() {
        for turns in 0..4 {
            for mirrored in [false, true] {
                let o = FrameOrientation::new(turns, mirrored);
                for (x, y) in [(0.1, 0.2), (0.9, 0.3), (0.5, 0.5)] {
                    // Where the sensor's point shows, then back.
                    let (w, h) = (640.0, 480.0);
                    let (dw, dh) = {
                        let (a, b) = o.size(640, 480);
                        (a as f32, b as f32)
                    };
                    let (px, py) = o.map_point(x * w, y * h, w, h);
                    let (sx, sy) = sensor_point(o, px / dw, py / dh);
                    assert!(
                        (sx - x).abs() < 1e-5 && (sy - y).abs() < 1e-5,
                        "{turns} {mirrored}: ({x}, {y}) came back as ({sx}, {sy})"
                    );
                }
            }
        }
    }

    #[test]
    fn the_luma_plane_is_shrunk_by_a_whole_factor() {
        let (w, h) = (10u32, 6u32);
        let y: Vec<u8> = (0..w * h).map(|i| i as u8).collect();
        // Padded rows, as cameras give them.
        let mut padded = Vec::new();
        for row in y.chunks(w as usize) {
            padded.extend_from_slice(row);
            padded.extend_from_slice(&[0xEE; 6]);
        }
        let planes = [CameraPlane {
            bytes: &padded,
            row_stride: 16,
            pixel_stride: 1,
        }];
        let image = CameraImage {
            width: w,
            height: h,
            format: CameraImageFormat::Yuv420,
            planes: &planes,
            orientation: FrameOrientation::UPRIGHT,
        };
        let mut out = Vec::new();
        assert_eq!(image.luma_plane(100, &mut out), (10, 6, 1));
        assert_eq!(out, y);
        assert_eq!(image.luma_plane(5, &mut out), (5, 3, 2));
        assert_eq!(out[..3], [0, 2, 4]);
        assert_eq!(out[5], 20, "the second row is the frame's third");
    }

    #[test]
    fn rgba_frames_have_a_brightness_too() {
        let px = [255u8, 255, 255, 255, 0, 0, 0, 255];
        let planes = [CameraPlane {
            bytes: &px,
            row_stride: 8,
            pixel_stride: 4,
        }];
        let image = CameraImage {
            width: 2,
            height: 1,
            format: CameraImageFormat::Rgba8,
            planes: &planes,
            orientation: FrameOrientation::UPRIGHT,
        };
        assert_eq!(image.luma(0, 0), 255);
        assert_eq!(image.luma(1, 0), 0);
        assert_eq!(image.luma(5, 0), 0, "out of range");
    }

    #[test]
    fn an_image_stream_is_only_fed_while_it_listens() {
        let sink = Sink::default();
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let planes = [CameraPlane {
            bytes: &[0],
            row_stride: 1,
            pixel_stride: 1,
        }];
        let image = CameraImage {
            width: 1,
            height: 1,
            format: CameraImageFormat::Yuv420,
            planes: &planes,
            orientation: FrameOrientation::UPRIGHT,
        };
        sink.deliver(&image);
        let counted = seen.clone();
        *sink.stream.lock().unwrap() = Some(Box::new(move |_| {
            counted.fetch_add(1, Ordering::Relaxed);
        }));
        sink.deliver(&image);
        assert_eq!(seen.load(Ordering::Relaxed), 0, "not listening yet");
        sink.listening.store(true, Ordering::Release);
        sink.deliver(&image);
        assert_eq!(seen.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn the_size_closest_to_the_preset_is_chosen() {
        let sizes = [
            (1920, 1080),
            (1280, 720),
            (640, 480),
            (320, 240),
            (4000, 3000),
        ];
        assert_eq!(
            choose_size(&sizes, ResolutionPreset::High, None),
            Some((1280, 720))
        );
        assert_eq!(
            choose_size(&sizes, ResolutionPreset::Medium, None),
            Some((640, 480))
        );
        assert_eq!(
            choose_size(&sizes, ResolutionPreset::Max, None),
            Some((4000, 3000))
        );
        // Of the preview's shape when there is one.
        assert_eq!(
            choose_size(&sizes, ResolutionPreset::Max, Some((1280, 720))),
            Some((1920, 1080))
        );
        assert_eq!(
            choose_size(&[(4000, 3000)], ResolutionPreset::Low, None),
            Some((4000, 3000))
        );
    }

    #[test]
    fn a_preview_of_no_camera_paints_its_background() {
        use frus_widgets::Widget;
        let preview = CameraPreview::empty().background(frus_widgets::Color::WHITE);
        let mut scene = frus_widgets::Scene::new();
        Widget::<()>::paint(
            &preview,
            frus_widgets::Rect::new(0.0, 0.0, 10.0, 10.0),
            frus_widgets::Status::default(),
            &frus_widgets::Theme::default(),
            &mut scene,
        );
        assert_eq!(scene.primitives().len(), 1);
        assert_eq!(Widget::<()>::paint_source(&preview), None);
        let semantics = Widget::<()>::semantics(&preview).expect("an image");
        assert_eq!(semantics.label.as_deref(), Some("Camera preview"));
    }

    #[test]
    fn a_controller_does_nothing_before_it_is_initialized() {
        let camera = CameraController::new(
            CameraDescription {
                name: "0".into(),
                lens_direction: CameraLensDirection::Back,
                sensor_orientation: 90,
            },
            ResolutionPreset::Low,
        );
        assert!(!camera.is_initialized());
        assert_eq!(camera.texture_id(), None);
        assert_eq!(camera.preview_size(), None);
        assert_eq!(
            camera.set_flash_mode(FlashMode::Torch),
            Err(CameraError::NotInitialized)
        );
        assert_eq!(camera.max_zoom_level(), 1.0);
        assert!(camera.start_image_stream(|_| {}).is_err());
        assert!(open_cameras().is_empty());
    }
}
