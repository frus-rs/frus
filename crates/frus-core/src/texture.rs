// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **External textures** (milestone 793): pixels made outside the widget tree — a video
//! decoder, a camera, a map engine, a native view — shown by a `Texture` widget.
//!
//! A producer, on any thread, [creates](TextureRegistry::create) a texture in the
//! process-wide [registry](textures) and pushes frames into it. The widget names the
//! texture by its [`TextureId`] and shows the **latest** frame at its layout size. Nothing
//! polls: a push asks the shell for one frame (and only one, however many pushes arrive
//! before it is drawn), so a texture that receives nothing costs nothing.
//!
//! ```
//! use frus_core::textures;
//!
//! let texture = textures().create();
//! let id = texture.id(); // what `Texture::new(id)` shows
//! std::thread::spawn(move || {
//!     let pixels = vec![255u8; 64 * 48 * 4];
//!     texture.push_rgba(64, 48, &pixels).unwrap();
//!     // ... one push per frame; dropping `texture` unregisters it.
//! })
//! .join()
//! .unwrap();
//! # let _ = id;
//! ```
//!
//! # Frames
//!
//! A push **copies** the pixels into one of a few buffers the texture keeps for the
//! purpose and recycles: after the first frames, pushing allocates nothing. Only the newest
//! frame is kept. A producer faster than the display has its older frames **dropped**
//! rather than queued (see [`TextureStats`]), which is what keeps a 120 fps source on a
//! 60 Hz screen from falling ever further behind.
//!
//! Four layouts are accepted: RGBA and BGRA (8 bits a channel, sRGB) and the two YUV 4:2:0
//! layouts decoders and cameras produce, NV12 and I420. YUV is converted to RGB on the
//! GPU, with the [colour space](YuvColorSpace) the producer names.
//!
//! A fifth kind of frame is **not copied at all**: a [`HardwareBuffer`] (Android's
//! `AHardwareBuffer`) that a decoder or a camera wrote, which the renderer samples where it
//! lies (milestone 800). Only a renderer that can import such buffers accepts them: see
//! [`TextureRegistry::hardware_buffers`].
//!
//! # Orientation
//!
//! A camera's sensor is mounted at an angle to the screen, and a video may say it was filmed
//! sideways. [`ExternalTexture::set_orientation`] turns the frames pushed after it by quarter
//! turns, mirrored or not ([`FrameOrientation`]); the GPU turns them in the same pass that
//! converts them, so it costs the producer nothing. [`FrameInfo`] gives the size as drawn.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

/// How many frame buffers a texture keeps for reuse besides the one on show: one being
/// filled by the producer while another is read by the renderer.
const SPARE_FRAMES: usize = 2;

/// The identity of an external texture: what a `Texture` widget is given to show it.
///
/// Cheap to copy and to send; it stays valid while the [`ExternalTexture`] it came from is
/// alive. A widget naming a texture that is gone draws nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextureId(u64);

impl TextureId {
    /// The id as a number — to hand to code on the other side of a language boundary
    /// (a Java plugin, a JavaScript page), which hands it back with
    /// [`TextureId::from_u64`].
    ///
    /// ```
    /// let texture = frus_core::textures().create();
    /// let raw = texture.id().as_u64();
    /// assert_eq!(frus_core::TextureId::from_u64(raw), texture.id());
    /// ```
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// The id a number stands for: see [`TextureId::as_u64`].
    pub fn from_u64(raw: u64) -> Self {
        Self(raw)
    }
}

/// The layout of a frame's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// 8-bit red, green, blue, alpha, sRGB; one plane, `width * height * 4` bytes.
    Rgba8,
    /// 8-bit blue, green, red, alpha, sRGB: what many platform APIs hand out.
    Bgra8,
    /// YUV 4:2:0, two planes: luma, then interleaved U and V at half the size each way.
    Nv12,
    /// YUV 4:2:0, three planes: luma, then U, then V, each at half the size each way.
    I420,
}

impl PixelFormat {
    /// Whether the GPU converts it from YUV.
    ///
    /// ```
    /// assert!(frus_core::PixelFormat::Nv12.is_yuv());
    /// assert!(!frus_core::PixelFormat::Rgba8.is_yuv());
    /// ```
    pub fn is_yuv(self) -> bool {
        matches!(self, PixelFormat::Nv12 | PixelFormat::I420)
    }

    /// The planes of a `width × height` frame, tightly packed one after another: for
    /// each, its width and height in texels, its bytes per texel and its offset in the
    /// frame's bytes. The second value is the frame's whole length.
    fn planes(self, width: u32, height: u32) -> ([PlaneLayout; 3], usize, usize) {
        let (w, h) = (width as usize, height as usize);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let none = PlaneLayout::default();
        match self {
            PixelFormat::Rgba8 | PixelFormat::Bgra8 => {
                let p = PlaneLayout::new(w, h, 4, 0);
                ([p, none, none], 1, w * h * 4)
            }
            PixelFormat::Nv12 => {
                let y = PlaneLayout::new(w, h, 1, 0);
                let uv = PlaneLayout::new(cw, ch, 2, w * h);
                ([y, uv, none], 2, w * h + cw * ch * 2)
            }
            PixelFormat::I420 => {
                let y = PlaneLayout::new(w, h, 1, 0);
                let u = PlaneLayout::new(cw, ch, 1, w * h);
                let v = PlaneLayout::new(cw, ch, 1, w * h + cw * ch);
                ([y, u, v], 3, w * h + cw * ch * 2)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct PlaneLayout {
    width: usize,
    height: usize,
    texel: usize,
    offset: usize,
}

impl PlaneLayout {
    fn new(width: usize, height: usize, texel: usize, offset: usize) -> Self {
        Self {
            width,
            height,
            texel,
            offset,
        }
    }

    fn len(&self) -> usize {
        self.width * self.height * self.texel
    }
}

/// How YUV values become RGB: the matrix, and whether the values use the full 0–255
/// range or the "limited" 16–235 one video is usually coded in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum YuvColorSpace {
    /// Standard definition video and most cameras, limited range: the default.
    #[default]
    Bt601Limited,
    /// The BT.601 matrix over the full range (JPEG, many camera previews).
    Bt601Full,
    /// High definition video, limited range.
    Bt709Limited,
    /// The BT.709 matrix over the full range.
    Bt709Full,
}

/// One plane of a YUV frame as a producer has it: bytes, the distance between rows, and the
/// distance between two samples of a row (`1` packed, `2` for a plane that interleaves U
/// and V, as Android's planes often do).
///
/// ```
/// let luma = [16u8; 4 * 2];
/// let plane = frus_core::YuvPlane::packed(&luma, 4);
/// assert_eq!(plane.pixel_stride, 1);
/// ```
#[derive(Clone, Copy, Debug)]
pub struct YuvPlane<'a> {
    /// The plane's bytes, from its first sample.
    pub data: &'a [u8],
    /// Bytes from the start of one row to the start of the next.
    pub row_stride: usize,
    /// Bytes from one sample to the next within a row.
    pub pixel_stride: usize,
}

impl<'a> YuvPlane<'a> {
    /// A plane whose samples are packed, rows `row_stride` bytes apart.
    pub fn packed(data: &'a [u8], row_stride: usize) -> Self {
        Self {
            data,
            row_stride,
            pixel_stride: 1,
        }
    }

    /// A plane with any strides.
    pub fn new(data: &'a [u8], row_stride: usize, pixel_stride: usize) -> Self {
        Self {
            data,
            row_stride,
            pixel_stride,
        }
    }

    /// Copies `width × height` samples of the plane, packed, into `out`.
    fn gather(&self, width: usize, height: usize, out: &mut [u8]) -> Result<(), TextureError> {
        let needed = if width == 0 || height == 0 {
            0
        } else {
            (height - 1) * self.row_stride + (width - 1) * self.pixel_stride + 1
        };
        if self.data.len() < needed || self.pixel_stride == 0 {
            return Err(TextureError::WrongLength {
                expected: needed,
                got: self.data.len(),
            });
        }
        for (row, dst) in out.chunks_exact_mut(width).take(height).enumerate() {
            let src = &self.data[row * self.row_stride..];
            if self.pixel_stride == 1 {
                dst.copy_from_slice(&src[..width]);
            } else {
                for (x, d) in dst.iter_mut().enumerate() {
                    *d = src[x * self.pixel_stride];
                }
            }
        }
        Ok(())
    }
}

/// Why a push or a platform request failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureError {
    /// The bytes given do not hold a frame of the size given.
    WrongLength {
        /// The bytes a frame of that size and layout needs.
        expected: usize,
        /// The bytes given.
        got: usize,
    },
    /// A frame of no pixels, or larger than a texture can be.
    BadSize {
        /// The width asked for.
        width: u32,
        /// The height asked for.
        height: u32,
    },
    /// The platform cannot do this (a native surface on a platform without one).
    Unsupported(&'static str),
    /// The platform refused, with its reason.
    Platform(String),
}

impl std::fmt::Display for TextureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextureError::WrongLength { expected, got } => {
                write!(
                    f,
                    "a frame of this size needs {expected} bytes, {got} given"
                )
            }
            TextureError::BadSize { width, height } => {
                write!(f, "a {width}×{height} frame cannot be shown")
            }
            TextureError::Unsupported(what) => write!(f, "unsupported on this platform: {what}"),
            TextureError::Platform(why) => write!(f, "the platform refused: {why}"),
        }
    }
}

impl std::error::Error for TextureError {}

/// How a texture's frames are turned when they are drawn (milestone 798): a number of
/// clockwise quarter turns, after a left-right mirror. A camera's frames come from a sensor
/// mounted at an angle to the screen (and the front camera's are mirrored, as a mirror
/// would show the person); a phone's video may have been filmed sideways.
///
/// ```
/// use frus_core::FrameOrientation;
///
/// let sideways = FrameOrientation::new(1, false);
/// assert_eq!(sideways.size(640, 480), (480, 640));
/// assert_eq!(FrameOrientation::new(-1, false).quarter_turns(), 3);
/// assert!(FrameOrientation::UPRIGHT.is_upright());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FrameOrientation {
    quarter_turns: u8,
    mirrored: bool,
}

impl FrameOrientation {
    /// As pushed: no turn, no mirror.
    pub const UPRIGHT: Self = Self {
        quarter_turns: 0,
        mirrored: false,
    };

    /// `quarter_turns` clockwise quarter turns (any integer; only the remainder modulo 4
    /// counts, a negative number turns the other way), after a left-right mirror when
    /// `mirrored`.
    pub fn new(quarter_turns: i32, mirrored: bool) -> Self {
        Self {
            quarter_turns: quarter_turns.rem_euclid(4) as u8,
            mirrored,
        }
    }

    /// The clockwise quarter turns, `0..4`.
    pub fn quarter_turns(self) -> u32 {
        self.quarter_turns as u32
    }

    /// Whether the frame is mirrored left to right (before it is turned).
    pub fn mirrored(self) -> bool {
        self.mirrored
    }

    /// Whether frames are drawn as pushed.
    pub fn is_upright(self) -> bool {
        self == Self::UPRIGHT
    }

    /// The size a `width × height` frame is drawn at: its sides swapped by an odd number of
    /// quarter turns.
    pub fn size(self, width: u32, height: u32) -> (u32, u32) {
        if self.quarter_turns % 2 == 1 {
            (height, width)
        } else {
            (width, height)
        }
    }

    /// Where the point `(x, y)` of a `width × height` frame, as pushed, lands once the frame
    /// is turned: what a scanner's corners, found in the pushed pixels, become on screen.
    ///
    /// ```
    /// use frus_core::FrameOrientation;
    /// // A quarter turn clockwise: the top-left corner goes to the top right.
    /// let turned = FrameOrientation::new(1, false);
    /// assert_eq!(turned.map_point(0.0, 0.0, 640.0, 480.0), (480.0, 0.0));
    /// ```
    pub fn map_point(self, x: f32, y: f32, width: f32, height: f32) -> (f32, f32) {
        let x = if self.mirrored { width - x } else { x };
        match self.quarter_turns {
            1 => (height - y, x),
            2 => (width - x, height - y),
            3 => (y, width - x),
            _ => (x, y),
        }
    }

    fn pack(self) -> u8 {
        self.quarter_turns | ((self.mirrored as u8) << 2)
    }

    fn unpack(bits: u8) -> Self {
        Self {
            quarter_turns: bits & 3,
            mirrored: bits & 4 != 0,
        }
    }
}

/// The largest side a frame may have: what every GPU frus runs on can sample.
pub const MAX_TEXTURE_SIDE: u32 = 8192;

/// **A frame in a platform GPU buffer**, shown without copying its pixels (milestone 800):
/// on Android an `AHardwareBuffer` a video decoder, a camera or a canvas wrote, which the
/// renderer imports and samples where it lies.
///
/// It holds the buffer until it is dropped, and then runs the `release` it was made with:
/// the producer's way to take the buffer back (an image reader's `AImage_delete`). The
/// renderer keeps it until the GPU has finished reading it, so the producer never writes
/// into a buffer still being drawn.
///
/// ```
/// use std::sync::atomic::{AtomicBool, Ordering};
/// use std::sync::Arc;
/// use frus_core::HardwareBuffer;
///
/// let released = Arc::new(AtomicBool::new(false));
/// let flag = released.clone();
/// let mut fake = 0u8; // stands for an `AHardwareBuffer*`
/// let buffer = unsafe {
///     HardwareBuffer::new((&mut fake as *mut u8).cast(), 640, 480, move || {
///         flag.store(true, Ordering::SeqCst)
///     })
/// }
/// .unwrap()
/// .with_crop(0, 0, 640, 360);
/// assert_eq!(buffer.visible_size(), (640, 360));
/// drop(buffer);
/// assert!(released.load(Ordering::SeqCst));
/// ```
pub struct HardwareBuffer {
    buffer: NonNull<c_void>,
    width: u32,
    height: u32,
    crop: [u32; 4],
    release: Option<Box<dyn FnOnce() + Send>>,
}

// The pointer is only handed to the renderer, which reads the buffer through it; the release
// runs once, from `Drop`, which has the buffer to itself.
unsafe impl Send for HardwareBuffer {}
unsafe impl Sync for HardwareBuffer {}

impl std::fmt::Debug for HardwareBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HardwareBuffer")
            .field("buffer", &self.buffer)
            .field("size", &(self.width, self.height))
            .field("crop", &self.crop)
            .finish()
    }
}

impl HardwareBuffer {
    /// A buffer of `width × height` pixels; `None` for a null pointer. `release` runs when
    /// the frame is no longer needed, on whichever thread drops it last.
    ///
    /// # Safety
    ///
    /// `buffer` must be a valid `AHardwareBuffer*` of that size, and the producer must
    /// neither free it nor write into it until `release` has run.
    pub unsafe fn new(
        buffer: *mut c_void,
        width: u32,
        height: u32,
        release: impl FnOnce() + Send + 'static,
    ) -> Option<Self> {
        Some(Self {
            buffer: NonNull::new(buffer)?,
            width,
            height,
            crop: [0, 0, width, height],
            release: Some(Box::new(release)),
        })
    }

    /// Shows only the rectangle from `(left, top)` to `(right, bottom)`, in the buffer's
    /// pixels: decoders often hand out buffers larger than the picture. Clamped to the
    /// buffer; an empty rectangle keeps the whole buffer.
    ///
    /// ```
    /// # let mut fake = 0u8;
    /// let buffer = unsafe {
    ///     frus_core::HardwareBuffer::new((&mut fake as *mut u8).cast(), 32, 32, || {})
    /// }
    /// .unwrap()
    /// .with_crop(4, 4, 100, 20);
    /// assert_eq!(buffer.crop(), [4, 4, 32, 20]);
    /// ```
    pub fn with_crop(mut self, left: u32, top: u32, right: u32, bottom: u32) -> Self {
        let (right, bottom) = (right.min(self.width), bottom.min(self.height));
        if left < right && top < bottom {
            self.crop = [left, top, right, bottom];
        }
        self
    }

    /// The `AHardwareBuffer*`.
    pub fn as_ptr(&self) -> *mut c_void {
        self.buffer.as_ptr()
    }

    /// The whole buffer's size, in pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The rectangle shown: left, top, right, bottom, in the buffer's pixels.
    pub fn crop(&self) -> [u32; 4] {
        self.crop
    }

    /// The size of the picture shown: the crop's.
    pub fn visible_size(&self) -> (u32, u32) {
        (self.crop[2] - self.crop[0], self.crop[3] - self.crop[1])
    }
}

impl Drop for HardwareBuffer {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release();
        }
    }
}

/// A frame, as the registry keeps it and the GPU layer reads it.
#[derive(Debug)]
pub struct TextureFrame {
    width: u32,
    height: u32,
    format: PixelFormat,
    color_space: YuvColorSpace,
    orientation: FrameOrientation,
    generation: u64,
    data: Vec<u8>,
    /// A frame that lies in a platform buffer: no bytes, no planes.
    hardware: Option<HardwareBuffer>,
}

/// One plane of a [`TextureFrame`], packed.
#[derive(Clone, Copy, Debug)]
pub struct FramePlane<'a> {
    /// The plane's bytes, row after row, with no padding.
    pub data: &'a [u8],
    /// Its width, in texels.
    pub width: u32,
    /// Its height, in texels.
    pub height: u32,
    /// Bytes per texel: `4` for RGBA and BGRA, `1` for a luma or chroma plane, `2` for
    /// NV12's interleaved chroma.
    pub bytes_per_texel: u32,
}

impl TextureFrame {
    fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            format: PixelFormat::Rgba8,
            color_space: YuvColorSpace::default(),
            orientation: FrameOrientation::UPRIGHT,
            generation: 0,
            data: Vec::new(),
            hardware: None,
        }
    }

    /// Width, in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height, in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The layout of the pixels.
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// How a YUV frame becomes RGB; meaningless for RGBA and BGRA.
    pub fn color_space(&self) -> YuvColorSpace {
        self.color_space
    }

    /// How the frame is turned when drawn (see [`ExternalTexture::set_orientation`]).
    pub fn orientation(&self) -> FrameOrientation {
        self.orientation
    }

    /// The size the frame is drawn at: [`width`](Self::width) and
    /// [`height`](Self::height), swapped by an odd number of quarter turns.
    pub fn display_size(&self) -> (u32, u32) {
        self.orientation.size(self.width, self.height)
    }

    /// The frame's number within its texture: `1` for the first pushed, rising by one for
    /// each push.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The platform buffer the frame lies in, for a frame pushed with
    /// [`ExternalTexture::push_hardware_buffer`]; `None` for pixels in memory.
    pub fn hardware_buffer(&self) -> Option<&HardwareBuffer> {
        self.hardware.as_ref()
    }

    /// How many planes the layout has: one, two or three; none for a frame in a
    /// [hardware buffer](Self::hardware_buffer).
    pub fn plane_count(&self) -> usize {
        if self.hardware.is_some() {
            return 0;
        }
        self.format.planes(self.width, self.height).1
    }

    /// Plane `index`, or `None` past the last (and always for a frame in a hardware
    /// buffer, whose pixels are not in memory the CPU reads).
    pub fn plane(&self, index: usize) -> Option<FramePlane<'_>> {
        let (planes, count, _) = self.format.planes(self.width, self.height);
        if index >= count || self.hardware.is_some() {
            return None;
        }
        let p = planes[index];
        Some(FramePlane {
            data: &self.data[p.offset..p.offset + p.len()],
            width: p.width as u32,
            height: p.height as u32,
            bytes_per_texel: p.texel as u32,
        })
    }
}

/// What has happened to a texture's frames: what [`ExternalTexture::stats`] answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextureStats {
    /// Frames pushed.
    pub pushed: u64,
    /// Frames replaced by a newer one before any window drew them.
    pub dropped: u64,
    /// Frames a window uploaded to draw.
    pub shown: u64,
}

/// The size and number of a texture's latest frame: what the widget lays the picture out
/// by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameInfo {
    /// Width, in pixels, as drawn: after the frame's [orientation](FrameOrientation).
    pub width: u32,
    /// Height, in pixels, as drawn.
    pub height: u32,
    /// The frame's [generation](TextureFrame::generation).
    pub generation: u64,
}

#[derive(Default)]
struct Slot {
    latest: Option<Arc<TextureFrame>>,
    spare: Vec<Arc<TextureFrame>>,
}

struct Entry {
    id: TextureId,
    slot: Mutex<Slot>,
    next_generation: AtomicU64,
    /// The newest generation a window has taken.
    shown_generation: AtomicU64,
    pushed: AtomicU64,
    dropped: AtomicU64,
    shown: AtomicU64,
    /// The [`FrameOrientation`] the next frames are stamped with, packed.
    orientation: AtomicU8,
}

type WakeFn = Arc<dyn Fn() + Send + Sync>;

/// The process-wide table of external textures: see [`textures`].
pub struct TextureRegistry {
    entries: Mutex<Vec<Arc<Entry>>>,
    wake: Mutex<Option<WakeFn>>,
    /// A frame has been asked for since the shell last started one: further pushes ask
    /// for nothing more.
    pending: AtomicBool,
    /// A renderer able to import hardware buffers is running.
    hardware: AtomicBool,
}

static REGISTRY: TextureRegistry = TextureRegistry::new();

/// **The texture registry**: where a producer creates a texture to feed, and where the
/// widget and the GPU layer find its frames.
///
/// ```
/// let texture = frus_core::textures().create();
/// assert!(frus_core::textures().contains(texture.id()));
/// let id = texture.id();
/// drop(texture);
/// assert!(!frus_core::textures().contains(id));
/// ```
pub fn textures() -> &'static TextureRegistry {
    &REGISTRY
}

impl TextureRegistry {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            wake: Mutex::new(None),
            pending: AtomicBool::new(false),
            hardware: AtomicBool::new(false),
        }
    }

    /// Whether frames in [hardware buffers](HardwareBuffer) are shown: the renderer says so
    /// when its GPU can import them (on Android, a Vulkan device with
    /// `VK_ANDROID_external_memory_android_hardware_buffer`). A producer that can hand out
    /// either kind asks this first, and copies planes when the answer is no.
    ///
    /// ```
    /// // No renderer in a doctest: frames are copied.
    /// assert!(!frus_core::textures().hardware_buffers());
    /// ```
    pub fn hardware_buffers(&self) -> bool {
        self.hardware.load(Ordering::Acquire)
    }

    /// Says whether the running renderer imports hardware buffers: see
    /// [`TextureRegistry::hardware_buffers`]. Called by the renderer.
    ///
    /// ```
    /// let registry = frus_core::textures();
    /// let before = registry.hardware_buffers();
    /// registry.set_hardware_buffers(before);
    /// assert_eq!(registry.hardware_buffers(), before);
    /// ```
    pub fn set_hardware_buffers(&self, accepted: bool) {
        self.hardware.store(accepted, Ordering::Release);
    }

    /// Creates a texture, empty until its first push. It lives until the returned handle
    /// is dropped.
    ///
    /// ```
    /// let texture = frus_core::textures().create();
    /// assert_eq!(frus_core::textures().frame_info(texture.id()), None);
    /// ```
    pub fn create(&'static self) -> ExternalTexture {
        let entry = Arc::new(Entry {
            id: TextureId(crate::image::next_image_id()),
            slot: Mutex::new(Slot::default()),
            next_generation: AtomicU64::new(1),
            shown_generation: AtomicU64::new(0),
            pushed: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            shown: AtomicU64::new(0),
            orientation: AtomicU8::new(0),
        });
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(entry.clone());
        }
        ExternalTexture {
            entry,
            registry: self,
        }
    }

    fn entry(&self, id: TextureId) -> Option<Arc<Entry>> {
        let entries = self.entries.lock().ok()?;
        entries.iter().find(|e| e.id == id).cloned()
    }

    /// Whether `id` names a live texture.
    pub fn contains(&self, id: TextureId) -> bool {
        self.entry(id).is_some()
    }

    /// How many textures are alive.
    ///
    /// ```
    /// let before = frus_core::textures().len();
    /// let texture = frus_core::textures().create();
    /// assert!(frus_core::textures().len() >= 1);
    /// # let _ = (before, texture);
    /// ```
    pub fn len(&self) -> usize {
        self.entries.lock().map(|e| e.len()).unwrap_or(0)
    }

    /// Whether no texture is alive.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The size and number of `id`'s latest frame; `None` for a texture that is gone or has
    /// received nothing yet.
    pub fn frame_info(&self, id: TextureId) -> Option<FrameInfo> {
        let entry = self.entry(id)?;
        let slot = entry.slot.lock().ok()?;
        slot.latest.as_ref().map(|f| {
            let (width, height) = f.display_size();
            FrameInfo {
                width,
                height,
                generation: f.generation,
            }
        })
    }

    /// `id`'s latest frame, for the GPU layer to upload; counts it as shown the first time
    /// it is taken. The frame's buffer goes back to the producer once every holder has
    /// dropped it, so a holder should drop it as soon as it is uploaded.
    pub fn latest(&self, id: TextureId) -> Option<Arc<TextureFrame>> {
        let entry = self.entry(id)?;
        let frame = entry.slot.lock().ok()?.latest.clone()?;
        if entry
            .shown_generation
            .fetch_max(frame.generation, Ordering::AcqRel)
            < frame.generation
        {
            entry.shown.fetch_add(1, Ordering::Relaxed);
        }
        Some(frame)
    }

    /// What has happened to `id`'s frames; `None` for a texture that is gone.
    pub fn stats(&self, id: TextureId) -> Option<TextureStats> {
        self.entry(id).map(|e| e.stats())
    }

    /// How to ask for a frame, from any thread: the shell installs it with its window.
    /// Until one is installed, pushed frames wait for a frame something else brings.
    pub fn set_wake(&self, wake: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut slot) = self.wake.lock() {
            *slot = Some(Arc::new(wake));
        }
        self.pending.store(false, Ordering::Release);
    }

    /// The shell starts a frame: a push from now on asks for the next one. Called before
    /// the frame reads any texture, so a frame pushed while it is drawn is not missed.
    pub fn begin_frame(&self) {
        self.pending.store(false, Ordering::Release);
    }

    /// Asks the shell for a frame, unless one is already asked for.
    fn request_frame(&self) {
        if self.pending.swap(true, Ordering::AcqRel) {
            return;
        }
        let wake = self.wake.lock().ok().and_then(|w| w.clone());
        if let Some(wake) = wake {
            wake();
        }
    }

    fn remove(&self, id: TextureId) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.retain(|e| e.id != id);
        }
        // What showed it must stop showing it.
        self.request_frame();
    }
}

impl Entry {
    fn stats(&self) -> TextureStats {
        TextureStats {
            pushed: self.pushed.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            shown: self.shown.load(Ordering::Relaxed),
        }
    }
}

/// **A texture a producer feeds**: made by [`TextureRegistry::create`], shown by a
/// `Texture` widget given its [`id`](ExternalTexture::id). `Send` and `Sync`, so a decoder
/// thread can own it; dropping it unregisters the texture.
///
/// ```
/// let texture = frus_core::textures().create();
/// let pixels = vec![0u8; 2 * 2 * 4];
/// texture.push_rgba(2, 2, &pixels).unwrap();
/// assert_eq!(texture.stats().pushed, 1);
/// ```
pub struct ExternalTexture {
    entry: Arc<Entry>,
    registry: &'static TextureRegistry,
}

impl std::fmt::Debug for ExternalTexture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalTexture")
            .field("id", &self.entry.id)
            .finish()
    }
}

impl ExternalTexture {
    /// The id a `Texture` widget is given to show this texture.
    pub fn id(&self) -> TextureId {
        self.entry.id
    }

    /// What has happened to the frames pushed so far.
    pub fn stats(&self) -> TextureStats {
        self.entry.stats()
    }

    /// Turns the frames pushed from now on by `orientation` when they are drawn: what a
    /// camera whose sensor is mounted sideways, or a video filmed sideways, asks for. The GPU
    /// turns them while it converts them, so it costs the producer nothing. Frames already
    /// pushed keep the orientation they had. A frame in a [hardware buffer](HardwareBuffer)
    /// is drawn as it lies: the orientation is for frames pushed as pixels.
    ///
    /// ```
    /// use frus_core::FrameOrientation;
    ///
    /// let texture = frus_core::textures().create();
    /// texture.set_orientation(FrameOrientation::new(1, false));
    /// texture.push_rgba(4, 2, &[0; 32]).unwrap();
    /// let info = frus_core::textures().frame_info(texture.id()).unwrap();
    /// assert_eq!((info.width, info.height), (2, 4));
    /// ```
    pub fn set_orientation(&self, orientation: FrameOrientation) {
        self.entry
            .orientation
            .store(orientation.pack(), Ordering::Relaxed);
    }

    /// The orientation the next frames are pushed with.
    pub fn orientation(&self) -> FrameOrientation {
        FrameOrientation::unpack(self.entry.orientation.load(Ordering::Relaxed))
    }

    /// The size of the latest frame, as pushed (before its orientation), if one was pushed.
    pub fn size(&self) -> Option<(u32, u32)> {
        let slot = self.entry.slot.lock().ok()?;
        slot.latest.as_ref().map(|f| (f.width, f.height))
    }

    /// Pushes a frame of RGBA pixels (8 bits a channel, sRGB, rows packed top to bottom).
    ///
    /// ```
    /// let texture = frus_core::textures().create();
    /// assert!(texture.push_rgba(2, 2, &[0; 16]).is_ok());
    /// assert!(texture.push_rgba(2, 2, &[0; 15]).is_err());
    /// ```
    pub fn push_rgba(&self, width: u32, height: u32, rgba: &[u8]) -> Result<(), TextureError> {
        self.push_packed(width, height, PixelFormat::Rgba8, rgba)
    }

    /// Pushes a frame of RGBA pixels whose rows are `row_stride` bytes apart (a padded
    /// buffer, as a platform's image reader hands out).
    ///
    /// ```
    /// let texture = frus_core::textures().create();
    /// // Two pixels a row, rows padded to 12 bytes.
    /// let padded = [0u8; 12 + 8];
    /// texture.push_rgba_strided(2, 2, &padded, 12).unwrap();
    /// ```
    pub fn push_rgba_strided(
        &self,
        width: u32,
        height: u32,
        rgba: &[u8],
        row_stride: usize,
    ) -> Result<(), TextureError> {
        let row = width as usize * 4;
        if row_stride == row {
            return self.push_rgba(width, height, rgba);
        }
        if row_stride < row {
            return Err(TextureError::WrongLength {
                expected: row,
                got: row_stride,
            });
        }
        let plane = YuvPlane::packed(rgba, row_stride);
        self.push_with(
            width,
            height,
            PixelFormat::Rgba8,
            YuvColorSpace::default(),
            |out| plane.gather(row, height as usize, out),
        )
    }

    /// Pushes a frame of BGRA pixels, as Windows and many capture APIs produce them.
    ///
    /// ```
    /// let texture = frus_core::textures().create();
    /// texture.push_bgra(1, 1, &[255, 0, 0, 255]).unwrap(); // one blue pixel
    /// ```
    pub fn push_bgra(&self, width: u32, height: u32, bgra: &[u8]) -> Result<(), TextureError> {
        self.push_packed(width, height, PixelFormat::Bgra8, bgra)
    }

    /// Pushes an **NV12** frame: a luma plane, then a plane of interleaved U and V at half
    /// the size each way. Rows may be padded (`row_stride`).
    ///
    /// ```
    /// use frus_core::{YuvColorSpace, YuvPlane};
    /// let texture = frus_core::textures().create();
    /// let (y, uv) = ([128u8; 4 * 4], [128u8; 2 * 2 * 2]);
    /// texture
    ///     .push_nv12(4, 4, YuvPlane::packed(&y, 4), YuvPlane::packed(&uv, 4), YuvColorSpace::default())
    ///     .unwrap();
    /// ```
    pub fn push_nv12(
        &self,
        width: u32,
        height: u32,
        y: YuvPlane<'_>,
        uv: YuvPlane<'_>,
        color_space: YuvColorSpace,
    ) -> Result<(), TextureError> {
        let (cw, ch) = (width.div_ceil(2) as usize, height.div_ceil(2) as usize);
        let (w, h) = (width as usize, height as usize);
        self.push_with(width, height, PixelFormat::Nv12, color_space, |out| {
            let (luma, chroma) = out.split_at_mut(w * h);
            y.gather(w, h, luma)?;
            // The chroma plane is `cw` texels of two bytes a row.
            YuvPlane::new(uv.data, uv.row_stride, uv.pixel_stride).gather(cw * 2, ch, chroma)
        })
    }

    /// Pushes an **I420** frame: luma, then U, then V, the chroma planes at half the size
    /// each way.
    ///
    /// ```
    /// use frus_core::{YuvColorSpace, YuvPlane};
    /// let texture = frus_core::textures().create();
    /// let (y, u, v) = ([128u8; 4 * 4], [128u8; 2 * 2], [128u8; 2 * 2]);
    /// texture
    ///     .push_i420(
    ///         4,
    ///         4,
    ///         YuvPlane::packed(&y, 4),
    ///         YuvPlane::packed(&u, 2),
    ///         YuvPlane::packed(&v, 2),
    ///         YuvColorSpace::default(),
    ///     )
    ///     .unwrap();
    /// ```
    pub fn push_i420(
        &self,
        width: u32,
        height: u32,
        y: YuvPlane<'_>,
        u: YuvPlane<'_>,
        v: YuvPlane<'_>,
        color_space: YuvColorSpace,
    ) -> Result<(), TextureError> {
        self.push_yuv420(width, height, y, u, v, color_space)
    }

    /// Pushes any **YUV 4:2:0** frame, whatever its strides: three planes as a camera or
    /// a decoder describes them (Android's `YUV_420_888` is this). Planes whose U and V
    /// interleave — a pixel stride of two — are kept interleaved (NV12, or NV21 swapped
    /// on the way); others are packed as I420.
    ///
    /// ```
    /// use frus_core::{YuvColorSpace, YuvPlane};
    /// let texture = frus_core::textures().create();
    /// let y = [128u8; 4 * 4];
    /// let vu = [128u8; 2 * 2 * 2]; // V, U, V, U...: NV21
    /// let (v, u) = (YuvPlane::new(&vu, 4, 2), YuvPlane::new(&vu[1..], 4, 2));
    /// texture
    ///     .push_yuv420(4, 4, YuvPlane::packed(&y, 4), u, v, YuvColorSpace::Bt601Full)
    ///     .unwrap();
    /// ```
    pub fn push_yuv420(
        &self,
        width: u32,
        height: u32,
        y: YuvPlane<'_>,
        u: YuvPlane<'_>,
        v: YuvPlane<'_>,
        color_space: YuvColorSpace,
    ) -> Result<(), TextureError> {
        let (w, h) = (width as usize, height as usize);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        // U and V in one interleaved plane, U first: copied as NV12, rows at a time.
        let u_first = u.pixel_stride == 2
            && v.pixel_stride == 2
            && u.row_stride == v.row_stride
            && (v.data.as_ptr() as usize).wrapping_sub(u.data.as_ptr() as usize) == 1;
        if u_first {
            // Each row is `cw * 2` bytes from U's first sample; the U view ends a byte
            // short of the row's last V sample, which is read through V's.
            let row = cw * 2;
            let needed = (ch - 1) * u.row_stride + row - 1;
            if u.data.len() >= needed && v.data.len() >= needed {
                return self.push_with(width, height, PixelFormat::Nv12, color_space, |out| {
                    let (luma, chroma) = out.split_at_mut(w * h);
                    y.gather(w, h, luma)?;
                    for (r, dst) in chroma.chunks_exact_mut(row).enumerate() {
                        let start = r * u.row_stride;
                        dst[..row - 1].copy_from_slice(&u.data[start..start + row - 1]);
                        dst[row - 1] = v.data[start + row - 2];
                    }
                    Ok(())
                });
            }
        }
        self.push_with(width, height, PixelFormat::I420, color_space, |out| {
            let (luma, chroma) = out.split_at_mut(w * h);
            let (cu, cv) = chroma.split_at_mut(cw * ch);
            y.gather(w, h, luma)?;
            u.gather(cw, ch, cu)?;
            v.gather(cw, ch, cv)
        })
    }

    /// Pushes a frame that lies in a platform GPU buffer: shown **without copying** its
    /// pixels (milestone 800). The buffer is released (its `release` run) once the frame is
    /// replaced and the GPU has finished reading it. `color_space` says how a YUV buffer
    /// becomes RGB; an RGBA buffer ignores it.
    ///
    /// Answers [`TextureError::Unsupported`], and releases the buffer, when no renderer
    /// imports hardware buffers ([`TextureRegistry::hardware_buffers`]): copy planes then.
    ///
    /// ```
    /// use frus_core::{HardwareBuffer, TextureError, YuvColorSpace};
    ///
    /// let texture = frus_core::textures().create();
    /// let mut fake = 0u8;
    /// let buffer = unsafe { HardwareBuffer::new((&mut fake as *mut u8).cast(), 64, 64, || {}) };
    /// let pushed = texture.push_hardware_buffer(buffer.unwrap(), YuvColorSpace::Bt709Limited);
    /// // No renderer imports them in a doctest.
    /// assert!(matches!(pushed, Err(TextureError::Unsupported(_))));
    /// ```
    pub fn push_hardware_buffer(
        &self,
        buffer: HardwareBuffer,
        color_space: YuvColorSpace,
    ) -> Result<(), TextureError> {
        if !self.registry.hardware_buffers() {
            return Err(TextureError::Unsupported(
                "this renderer cannot import hardware buffers: push planes",
            ));
        }
        let (width, height) = buffer.visible_size();
        if width == 0 || height == 0 || width > MAX_TEXTURE_SIDE || height > MAX_TEXTURE_SIDE {
            return Err(TextureError::BadSize { width, height });
        }
        let mut frame = TextureFrame::empty();
        frame.width = width;
        frame.height = height;
        frame.color_space = color_space;
        frame.hardware = Some(buffer);
        self.publish(Arc::new(frame))
    }

    fn push_packed(
        &self,
        width: u32,
        height: u32,
        format: PixelFormat,
        bytes: &[u8],
    ) -> Result<(), TextureError> {
        let expected = format.planes(width, height).2;
        if bytes.len() != expected {
            return Err(TextureError::WrongLength {
                expected,
                got: bytes.len(),
            });
        }
        self.push_with(
            width,
            height,
            format,
            YuvColorSpace::default(),
            |out: &mut [u8]| {
                out.copy_from_slice(bytes);
                Ok(())
            },
        )
    }

    /// Fills a recycled buffer with `fill` and makes it the latest frame.
    fn push_with(
        &self,
        width: u32,
        height: u32,
        format: PixelFormat,
        color_space: YuvColorSpace,
        fill: impl FnOnce(&mut [u8]) -> Result<(), TextureError>,
    ) -> Result<(), TextureError> {
        if width == 0 || height == 0 || width > MAX_TEXTURE_SIDE || height > MAX_TEXTURE_SIDE {
            return Err(TextureError::BadSize { width, height });
        }
        let len = format.planes(width, height).2;
        // A spare nobody else holds any more, or a new one while the ring warms up. Taken
        // under the lock, filled outside it: the renderer is never kept waiting on a copy.
        let mut frame = {
            let mut slot = self.entry.slot.lock().map_err(poisoned)?;
            let free = slot
                .spare
                .iter()
                .position(|f| Arc::strong_count(f) == 1 && Arc::weak_count(f) == 0);
            match free {
                Some(i) => slot.spare.swap_remove(i),
                None => Arc::new(TextureFrame::empty()),
            }
        };
        let buffer = Arc::get_mut(&mut frame).expect("a spare is only taken when unshared");
        // Same size as last time: no allocation.
        buffer.data.resize(len, 0);
        fill(&mut buffer.data)?;
        buffer.width = width;
        buffer.height = height;
        buffer.format = format;
        buffer.color_space = color_space;
        buffer.orientation =
            FrameOrientation::unpack(self.entry.orientation.load(Ordering::Relaxed));
        self.publish(frame)
    }

    /// Makes `frame` the latest, numbered; the frame it replaces goes back to the spares
    /// (or, for a hardware buffer, is released once nobody holds it any more).
    fn publish(&self, mut frame: Arc<TextureFrame>) -> Result<(), TextureError> {
        let replaced = {
            let mut slot = self.entry.slot.lock().map_err(poisoned)?;
            if let Some(f) = Arc::get_mut(&mut frame) {
                f.generation = self.entry.next_generation.fetch_add(1, Ordering::AcqRel);
            }
            let old = slot.latest.replace(frame);
            if let Some(old) = &old {
                if old.generation > self.entry.shown_generation.load(Ordering::Acquire) {
                    self.entry.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
            match old {
                Some(old) if slot.spare.len() < SPARE_FRAMES && old.hardware.is_none() => {
                    slot.spare.push(old);
                    None
                }
                other => other,
            }
        };
        // A hardware buffer is released outside the lock: its producer's release may take
        // locks of its own.
        drop(replaced);
        self.entry.pushed.fetch_add(1, Ordering::Relaxed);
        self.registry.request_frame();
        Ok(())
    }
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> TextureError {
    TextureError::Platform("a producer panicked while pushing".into())
}

impl Drop for ExternalTexture {
    fn drop(&mut self) {
        self.registry.remove(self.entry.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn registry() -> &'static TextureRegistry {
        Box::leak(Box::new(TextureRegistry::new()))
    }

    #[test]
    fn a_dropped_texture_leaves_the_registry_and_shows_nothing() {
        let reg = registry();
        let texture = reg.create();
        let id = texture.id();
        texture.push_rgba(1, 1, &[1, 2, 3, 4]).unwrap();
        assert!(reg.contains(id));
        assert!(reg.latest(id).is_some());
        drop(texture);
        assert!(!reg.contains(id));
        assert!(reg.latest(id).is_none());
        assert_eq!(reg.frame_info(id), None);
        assert!(reg.is_empty());
    }

    #[test]
    fn a_frame_taken_after_its_texture_is_gone_stays_readable() {
        let reg = registry();
        let texture = reg.create();
        texture.push_rgba(1, 1, &[9, 9, 9, 9]).unwrap();
        let frame = reg.latest(texture.id()).unwrap();
        drop(texture);
        assert_eq!(frame.plane(0).unwrap().data, &[9, 9, 9, 9]);
    }

    #[test]
    fn a_producer_faster_than_the_display_has_its_older_frames_dropped() {
        let reg = registry();
        let texture = reg.create();
        let id = texture.id();
        for i in 0..5u8 {
            texture.push_rgba(1, 1, &[i, 0, 0, 255]).unwrap();
        }
        // Only the newest is kept: four never reached a window.
        let frame = reg.latest(id).unwrap();
        assert_eq!(frame.plane(0).unwrap().data[0], 4);
        assert_eq!(frame.generation(), 5);
        assert_eq!(
            texture.stats(),
            TextureStats {
                pushed: 5,
                dropped: 4,
                shown: 1
            }
        );
        // Taking the same frame again is not a second showing.
        drop(frame);
        let _ = reg.latest(id);
        assert_eq!(texture.stats().shown, 1);
        // A frame drawn before the next push is not dropped.
        texture.push_rgba(1, 1, &[7, 0, 0, 255]).unwrap();
        let _ = reg.latest(id);
        texture.push_rgba(1, 1, &[8, 0, 0, 255]).unwrap();
        assert_eq!(texture.stats().dropped, 4);
    }

    #[test]
    fn many_pushes_before_a_frame_ask_for_one_frame() {
        let reg = registry();
        let asked = Arc::new(AtomicUsize::new(0));
        let counter = asked.clone();
        reg.set_wake(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let texture = reg.create();
        for _ in 0..10 {
            texture.push_rgba(1, 1, &[0; 4]).unwrap();
        }
        assert_eq!(asked.load(Ordering::SeqCst), 1, "one frame for ten pushes");
        // The shell starts that frame: the next push asks for another.
        reg.begin_frame();
        texture.push_rgba(1, 1, &[0; 4]).unwrap();
        assert_eq!(asked.load(Ordering::SeqCst), 2);
        // Nothing pushed, nothing asked: an idle texture costs no frame.
        reg.begin_frame();
        assert_eq!(asked.load(Ordering::SeqCst), 2);
        // Unregistering asks for one, so the widget stops showing it.
        drop(texture);
        assert_eq!(asked.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn frames_reuse_their_buffers_once_the_ring_is_warm() {
        let reg = registry();
        let texture = reg.create();
        let pixels = vec![5u8; 64 * 64 * 4];
        let mut seen = std::collections::HashSet::new();
        for _ in 0..50 {
            texture.push_rgba(64, 64, &pixels).unwrap();
            // The renderer takes each frame and lets it go, as it does after uploading.
            let frame = reg.latest(texture.id()).unwrap();
            seen.insert(frame.plane(0).unwrap().data.as_ptr() as usize);
        }
        assert!(
            seen.len() <= SPARE_FRAMES + 1,
            "{} buffers for 50 frames: one allocated per frame",
            seen.len()
        );
    }

    #[test]
    fn a_frame_held_by_the_renderer_is_never_written_over() {
        let reg = registry();
        let texture = reg.create();
        texture.push_rgba(1, 1, &[1, 1, 1, 1]).unwrap();
        let held = reg.latest(texture.id()).unwrap();
        for i in 2..10u8 {
            texture.push_rgba(1, 1, &[i; 4]).unwrap();
        }
        assert_eq!(held.plane(0).unwrap().data, &[1, 1, 1, 1]);
    }

    #[test]
    fn wrong_lengths_and_sizes_are_errors_not_panics() {
        let reg = registry();
        let texture = reg.create();
        assert_eq!(
            texture.push_rgba(2, 2, &[0; 3]),
            Err(TextureError::WrongLength {
                expected: 16,
                got: 3
            })
        );
        assert!(matches!(
            texture.push_rgba(0, 2, &[]),
            Err(TextureError::BadSize { .. })
        ));
        let short = [0u8; 3];
        let err = texture.push_i420(
            4,
            4,
            YuvPlane::packed(&short, 4),
            YuvPlane::packed(&short, 2),
            YuvPlane::packed(&short, 2),
            YuvColorSpace::default(),
        );
        assert!(matches!(err, Err(TextureError::WrongLength { .. })));
        assert_eq!(texture.stats().pushed, 0);
    }

    #[test]
    fn padded_i420_planes_are_packed() {
        let reg = registry();
        let texture = reg.create();
        // A 3×2 frame whose rows are padded to 8 bytes: chroma is 2×1.
        let y = [1, 2, 3, 0, 0, 0, 0, 0, 4, 5, 6];
        let u = [10, 11];
        let v = [20, 21];
        texture
            .push_i420(
                3,
                2,
                YuvPlane::packed(&y, 8),
                YuvPlane::packed(&u, 8),
                YuvPlane::packed(&v, 8),
                YuvColorSpace::Bt709Limited,
            )
            .unwrap();
        let frame = reg.latest(texture.id()).unwrap();
        assert_eq!(frame.format(), PixelFormat::I420);
        assert_eq!(frame.color_space(), YuvColorSpace::Bt709Limited);
        assert_eq!(frame.plane(0).unwrap().data, &[1, 2, 3, 4, 5, 6]);
        assert_eq!(frame.plane(1).unwrap().data, &[10, 11]);
        assert_eq!(frame.plane(2).unwrap().data, &[20, 21]);
        assert!(frame.plane(3).is_none());
    }

    #[test]
    fn interleaved_chroma_stays_interleaved_and_nv21_is_swapped_to_i420() {
        let reg = registry();
        let texture = reg.create();
        let y = [0u8; 4 * 2];
        // U first (NV12): kept as one interleaved plane.
        let uv = [10, 20, 11, 21];
        texture
            .push_yuv420(
                4,
                2,
                YuvPlane::packed(&y, 4),
                YuvPlane::new(&uv, 4, 2),
                YuvPlane::new(&uv[1..], 4, 2),
                YuvColorSpace::default(),
            )
            .unwrap();
        let frame = reg.latest(texture.id()).unwrap();
        assert_eq!(frame.format(), PixelFormat::Nv12);
        assert_eq!(frame.plane(1).unwrap().data, &[10, 20, 11, 21]);
        drop(frame);
        // V first (NV21): gathered into separate planes, in the right order.
        let vu = [20, 10, 21, 11];
        texture
            .push_yuv420(
                4,
                2,
                YuvPlane::packed(&y, 4),
                YuvPlane::new(&vu[1..], 4, 2),
                YuvPlane::new(&vu, 4, 2),
                YuvColorSpace::default(),
            )
            .unwrap();
        let frame = reg.latest(texture.id()).unwrap();
        assert_eq!(frame.format(), PixelFormat::I420);
        assert_eq!(frame.plane(1).unwrap().data, &[10, 11]);
        assert_eq!(frame.plane(2).unwrap().data, &[20, 21]);
    }

    #[test]
    fn nv12_rows_may_be_padded() {
        let reg = registry();
        let texture = reg.create();
        let y = [1, 2, 0, 0, 3, 4];
        let uv = [7, 8];
        texture
            .push_nv12(
                2,
                2,
                YuvPlane::packed(&y, 4),
                YuvPlane::packed(&uv, 4),
                YuvColorSpace::default(),
            )
            .unwrap();
        let frame = reg.latest(texture.id()).unwrap();
        assert_eq!(frame.plane(0).unwrap().data, &[1, 2, 3, 4]);
        assert_eq!(frame.plane(1).unwrap().data, &[7, 8]);
    }

    #[test]
    fn producers_on_many_threads_do_not_corrupt_the_texture() {
        let reg = registry();
        let texture = Arc::new(reg.create());
        let id = texture.id();
        let threads: Vec<_> = (0..4u8)
            .map(|t| {
                let texture = texture.clone();
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        texture.push_rgba(2, 1, &[t; 8]).unwrap();
                    }
                })
            })
            .collect();
        for _ in 0..200 {
            if let Some(frame) = reg.latest(id) {
                let data = frame.plane(0).unwrap().data;
                assert!(data.iter().all(|&b| b == data[0]), "a torn frame");
            }
        }
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(texture.stats().pushed, 800);
    }

    #[test]
    fn an_orientation_turns_the_frames_pushed_after_it() {
        let registry = registry();
        let texture = registry.create();
        texture.push_rgba(4, 2, &[0; 32]).unwrap();
        texture.set_orientation(FrameOrientation::new(3, true));
        // Already pushed: as it was.
        let info = registry.frame_info(texture.id()).unwrap();
        assert_eq!((info.width, info.height), (4, 2));
        texture.push_rgba(4, 2, &[0; 32]).unwrap();
        let info = registry.frame_info(texture.id()).unwrap();
        assert_eq!((info.width, info.height), (2, 4));
        let frame = registry.latest(texture.id()).unwrap();
        assert_eq!(frame.orientation(), FrameOrientation::new(-1, true));
        assert_eq!(
            (frame.width(), frame.height()),
            (4, 2),
            "the planes stay as pushed"
        );
        assert_eq!(texture.size(), Some((4, 2)));
    }

    #[test]
    fn points_follow_the_turn() {
        let (w, h) = (640.0, 480.0);
        let o = FrameOrientation::new;
        // The pushed frame's top-left and bottom-right corners.
        assert_eq!(o(0, false).map_point(0.0, 0.0, w, h), (0.0, 0.0));
        assert_eq!(o(1, false).map_point(0.0, 0.0, w, h), (480.0, 0.0));
        assert_eq!(o(1, false).map_point(w, h, w, h), (0.0, 640.0));
        assert_eq!(o(2, false).map_point(0.0, 0.0, w, h), (640.0, 480.0));
        assert_eq!(o(3, false).map_point(0.0, 0.0, w, h), (0.0, 640.0));
        // Mirrored first: the top-left comes from the top-right.
        assert_eq!(o(0, true).map_point(0.0, 0.0, w, h), (640.0, 0.0));
        assert_eq!(o(1, true).map_point(0.0, 0.0, w, h), (480.0, 640.0));
        for turns in 0..4 {
            for mirrored in [false, true] {
                let (dw, dh) = o(turns, mirrored).size(640, 480);
                let (x, y) = o(turns, mirrored).map_point(10.0, 20.0, w, h);
                assert!(x >= 0.0 && x <= dw as f32 && y >= 0.0 && y <= dh as f32);
            }
        }
    }

    #[test]
    fn external_images_differ_by_frame() {
        let reg = registry();
        let texture = reg.create();
        let a = crate::ImageData::external(texture.id(), 1, 4, 4);
        let b = crate::ImageData::external(texture.id(), 2, 4, 4);
        assert_ne!(a, b);
        assert_eq!(a, crate::ImageData::external(texture.id(), 1, 4, 4));
        assert_eq!(a.id(), texture.id().as_u64());
    }

    /// A fake hardware buffer whose release counts into `released`.
    fn fake_buffer(released: &Arc<AtomicUsize>, w: u32, h: u32) -> HardwareBuffer {
        let released = released.clone();
        // Never read: the registry only carries the pointer.
        let ptr = std::ptr::NonNull::<u8>::dangling().as_ptr().cast();
        unsafe {
            HardwareBuffer::new(ptr, w, h, move || {
                released.fetch_add(1, Ordering::SeqCst);
            })
        }
        .unwrap()
    }

    #[test]
    fn hardware_buffers_are_refused_and_released_without_a_renderer_that_imports_them() {
        let reg = registry();
        let texture = reg.create();
        let released = Arc::new(AtomicUsize::new(0));
        let pushed =
            texture.push_hardware_buffer(fake_buffer(&released, 8, 8), YuvColorSpace::default());
        assert!(matches!(pushed, Err(TextureError::Unsupported(_))));
        assert_eq!(released.load(Ordering::SeqCst), 1);
        assert_eq!(reg.frame_info(texture.id()), None);
    }

    #[test]
    fn a_hardware_frame_is_released_when_replaced_unless_a_window_still_holds_it() {
        let reg = registry();
        reg.set_hardware_buffers(true);
        let texture = reg.create();
        let id = texture.id();
        let released = Arc::new(AtomicUsize::new(0));
        let first = fake_buffer(&released, 64, 48).with_crop(0, 0, 60, 40);
        texture
            .push_hardware_buffer(first, YuvColorSpace::Bt709Limited)
            .unwrap();
        let info = reg.frame_info(id).unwrap();
        assert_eq!((info.width, info.height, info.generation), (60, 40, 1));
        // The renderer takes it and holds it while the GPU reads it.
        let held = reg.latest(id).unwrap();
        assert!(held.hardware_buffer().is_some());
        assert_eq!(held.plane_count(), 0);
        assert!(held.plane(0).is_none());
        assert_eq!(held.color_space(), YuvColorSpace::Bt709Limited);
        // Replaced: not released while held, released once the holder lets go.
        texture
            .push_hardware_buffer(fake_buffer(&released, 64, 48), YuvColorSpace::default())
            .unwrap();
        assert_eq!(released.load(Ordering::SeqCst), 0);
        drop(held);
        assert_eq!(released.load(Ordering::SeqCst), 1);
        // Replaced before any window took it: released at once, and counted as dropped.
        texture
            .push_hardware_buffer(fake_buffer(&released, 64, 48), YuvColorSpace::default())
            .unwrap();
        assert_eq!(released.load(Ordering::SeqCst), 2);
        assert_eq!(texture.stats().dropped, 1);
        // A copied frame after hardware ones works, and releases the last buffer.
        texture.push_rgba(1, 1, &[1, 2, 3, 4]).unwrap();
        assert_eq!(released.load(Ordering::SeqCst), 3);
        assert_eq!(
            reg.latest(id).unwrap().plane(0).unwrap().data,
            &[1, 2, 3, 4]
        );
        drop(texture);
        assert_eq!(released.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn a_hardware_frame_never_becomes_a_spare() {
        let reg = registry();
        reg.set_hardware_buffers(true);
        let texture = reg.create();
        let released = Arc::new(AtomicUsize::new(0));
        for _ in 0..5 {
            texture
                .push_hardware_buffer(fake_buffer(&released, 4, 4), YuvColorSpace::default())
                .unwrap();
        }
        // Four replaced, none kept: a spare would hold the producer's buffer forever.
        assert_eq!(released.load(Ordering::SeqCst), 4);
        assert!(texture.entry.slot.lock().unwrap().spare.is_empty());
        drop(texture);
        assert_eq!(released.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn an_empty_or_huge_hardware_frame_is_refused() {
        let reg = registry();
        reg.set_hardware_buffers(true);
        let texture = reg.create();
        let released = Arc::new(AtomicUsize::new(0));
        let huge = fake_buffer(&released, MAX_TEXTURE_SIDE + 1, 4);
        assert!(matches!(
            texture.push_hardware_buffer(huge, YuvColorSpace::default()),
            Err(TextureError::BadSize { .. })
        ));
        assert_eq!(released.load(Ordering::SeqCst), 1);
    }
}
