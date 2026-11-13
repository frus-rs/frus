// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`TextureSurface`]: a **platform surface** whose frames become an external texture
//! (milestone 793) — what a video decoder, a camera or a native view draws into.
//!
//! On Android it is an image reader: its window is an `ANativeWindow`, and so an
//! `android.view.Surface` for Java, that `MediaCodec`, Camera2, a media player or a
//! `Canvas` can target. The `Texture` widget shows every frame they queue. Nothing runs
//! between frames.
//!
//! Where the renderer can import hardware buffers (a Vulkan GPU with
//! `VK_ANDROID_external_memory_android_hardware_buffer`: milestone 800), the reader hands
//! out GPU buffers in the producer's own format and each frame is **shown where the
//! producer wrote it**, without a copy. Elsewhere (OpenGL ES, an older GPU, Android 7) each
//! frame is read from its planes on the reader's own thread and pushed: YUV 4:2:0 for
//! decoders and cameras, RGBA for a canvas. The choice is made when the surface is made.
//!
//! Elsewhere there is no such surface, and [`TextureSurface::new`] answers
//! [`TextureError::Unsupported`]: push frames into an [`ExternalTexture`] directly.

use frus_widgets::{ExternalTexture, TextureError, TextureId, YuvColorSpace, YuvPlane};

/// A frame a [`TextureSurface`] received, as its producer queued it: handed to a
/// [frame observer](TextureSurface::set_frame_observer) on the surface's own thread, after it
/// has become the texture's.
#[derive(Clone, Copy, Debug)]
pub struct SurfaceFrame<'a> {
    /// Width, in pixels (the crop applied).
    pub width: u32,
    /// Height, in pixels.
    pub height: u32,
    /// The layout: three planes (Y, U, V) for [`SurfaceFormat::Yuv420`], one for
    /// [`SurfaceFormat::Rgba`].
    pub format: SurfaceFormat,
    /// The planes, each from its first sample in the crop, with its strides.
    pub planes: &'a [YuvPlane<'a>],
}

/// What a [`TextureSurface`] hands each frame to besides its texture.
pub type FrameObserver = Box<dyn FnMut(&SurfaceFrame<'_>) + Send>;

/// The pixels a [`TextureSurface`] asks its producer for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SurfaceFormat {
    /// YUV 4:2:0: what video decoders and cameras produce. The default.
    #[default]
    Yuv420,
    /// 8-bit RGBA: what a `Canvas` or a GL producer draws.
    Rgba,
}

/// A platform surface feeding an external texture: on Android an image reader whose window a
/// decoder, a camera or a canvas draws into, every frame becoming the texture's;
/// [`TextureError::Unsupported`] elsewhere.
///
/// ```no_run
/// use frus_shell::{SurfaceFormat, TextureSurface};
///
/// match TextureSurface::new(1280, 720, SurfaceFormat::Yuv420) {
///     // Give `surface.java_surface()` to MediaCodec, show `Texture::new(surface.id())`.
///     Ok(surface) => drop(surface),
///     // Not Android: push frames into an `ExternalTexture` yourself.
///     Err(err) => eprintln!("{err}"),
/// }
/// ```
pub struct TextureSurface {
    #[cfg(target_os = "android")]
    inner: android::Reader,
    #[cfg(not(target_os = "android"))]
    texture: ExternalTexture,
}

impl std::fmt::Debug for TextureSurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextureSurface")
            .field("id", &self.id())
            .finish()
    }
}

impl TextureSurface {
    /// A surface of `width × height` pixels (the producer may queue another size; each
    /// frame is shown at the size it has), feeding a new texture.
    pub fn new(width: u32, height: u32, format: SurfaceFormat) -> Result<Self, TextureError> {
        Self::with_texture(frus_widgets::textures().create(), width, height, format)
    }

    /// The same, feeding `texture`.
    pub fn with_texture(
        texture: ExternalTexture,
        width: u32,
        height: u32,
        format: SurfaceFormat,
    ) -> Result<Self, TextureError> {
        Self::make(texture, width, height, format, false)
    }

    /// The same, its frames always **read from their planes by the CPU**, never handed to
    /// the GPU as hardware buffers: what a [frame observer](Self::set_frame_observer) needs
    /// to see the pixels (milestone 798; a camera's image stream, a code reader). It costs
    /// the copy that hardware buffers save.
    pub fn readable_with_texture(
        texture: ExternalTexture,
        width: u32,
        height: u32,
        format: SurfaceFormat,
    ) -> Result<Self, TextureError> {
        Self::make(texture, width, height, format, true)
    }

    fn make(
        texture: ExternalTexture,
        width: u32,
        height: u32,
        format: SurfaceFormat,
        readable: bool,
    ) -> Result<Self, TextureError> {
        #[cfg(target_os = "android")]
        {
            android::Reader::new(texture, width, height, format, readable)
                .map(|inner| Self { inner })
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (texture, width, height, format, readable);
            Err(TextureError::Unsupported(
                "a texture surface exists on Android only; push frames into an ExternalTexture",
            ))
        }
    }

    /// The texture's id, for a `Texture` widget.
    pub fn id(&self) -> TextureId {
        self.texture().id()
    }

    /// The texture fed, for its [stats](ExternalTexture::stats).
    pub fn texture(&self) -> &ExternalTexture {
        #[cfg(target_os = "android")]
        {
            self.inner.texture()
        }
        #[cfg(not(target_os = "android"))]
        {
            &self.texture
        }
    }

    /// How YUV frames become RGB: BT.601 limited range unless set (high-definition video
    /// is usually BT.709).
    pub fn set_color_space(&self, space: YuvColorSpace) {
        #[cfg(target_os = "android")]
        self.inner.set_color_space(space);
        #[cfg(not(target_os = "android"))]
        let _ = space;
    }

    /// The surface's `ANativeWindow*`, for a native producer (`AMediaCodec_configure`,
    /// `ACameraOutputTarget_create`). Valid while the surface lives; null where there is no
    /// surface.
    pub fn native_window(&self) -> *mut std::ffi::c_void {
        #[cfg(target_os = "android")]
        {
            self.inner.window()
        }
        #[cfg(not(target_os = "android"))]
        {
            std::ptr::null_mut()
        }
    }

    /// Hands every frame, once it is the texture's, to `observer`, on the surface's own
    /// thread (milestone 798): what a camera's image stream or a code reader reads the
    /// pixels through, with no copy of its own. The frame is borrowed for the time of the
    /// call; keep it short, the next frame waits. `None` stops it. Only frames read from
    /// their planes are observed: a surface handing its frames to the GPU as hardware
    /// buffers has no pixels to show, so make it with
    /// [`readable_with_texture`](Self::readable_with_texture).
    pub fn set_frame_observer(&self, observer: Option<FrameObserver>) {
        #[cfg(target_os = "android")]
        self.inner.set_observer(observer);
        #[cfg(not(target_os = "android"))]
        let _ = observer;
    }

    /// Closes the surface and hands back the texture it fed, with the frame it shows: a camera
    /// switching to a size the surface was not made for keeps its texture's id.
    pub fn into_texture(self) -> ExternalTexture {
        #[cfg(target_os = "android")]
        {
            self.inner.into_texture()
        }
        #[cfg(not(target_os = "android"))]
        {
            self.texture
        }
    }

    /// The surface as a **JNI global reference to an `android.view.Surface`**, for Java or
    /// Kotlin code: `MediaCodec.configure(format, surface, null, 0)`,
    /// `player.setVideoSurface(surface)`, a capture session's target, `lockHardwareCanvas`.
    /// Valid while this surface lives; pass it to your Java method as a `jobject`.
    /// Needs Android 8 (API 26); [`TextureError::Unsupported`] before.
    pub fn java_surface(&self) -> Result<*mut std::ffi::c_void, TextureError> {
        #[cfg(target_os = "android")]
        {
            self.inner.java_surface()
        }
        #[cfg(not(target_os = "android"))]
        {
            Err(TextureError::Unsupported("no Java surface off Android"))
        }
    }
}

#[cfg(target_os = "android")]
pub(crate) mod android {
    //! The NDK image reader, reached through `dlsym`: frus applications start at Android 7
    //! (API 24), and linking a symbol a later version added would stop the library loading
    //! on the earlier one. Nothing is looked up before a surface is made.

    use std::ffi::{c_char, c_int, c_void};
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};

    use frus_widgets::{ExternalTexture, HardwareBuffer, TextureError, YuvColorSpace, YuvPlane};

    use super::{FrameObserver, SurfaceFormat, SurfaceFrame};

    const RTLD_NOW: c_int = 2;
    const AIMAGE_FORMAT_RGBA_8888: i32 = 1;
    const AIMAGE_FORMAT_YUV_420_888: i32 = 0x23;
    /// The producer's own format, readable only by the GPU: what a decoder or a camera
    /// writes best.
    const AIMAGE_FORMAT_PRIVATE: i32 = 0x22;
    /// `AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE`.
    const USAGE_GPU_SAMPLED_IMAGE: u64 = 1 << 8;
    /// Images the reader holds when frames are copied: one the producer fills, one being
    /// read, one spare.
    const MAX_IMAGES: i32 = 3;
    /// When they are imported: the newest, waiting for a frame; the one the GPU may still be
    /// reading; one free for `acquireLatestImage`. No more: a decoder sizes its own buffer
    /// pool by adding these to what it needs, and the test phone's refuses a pool past 25
    /// (with 5 it failed to start).
    const MAX_HARDWARE_IMAGES: i32 = 3;

    extern "C" {
        fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    #[repr(C)]
    pub(crate) struct ImageListener {
        pub(crate) context: *mut c_void,
        pub(crate) on_image_available: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    }

    /// `AHardwareBuffer_Desc`.
    #[repr(C)]
    #[derive(Default)]
    struct BufferDesc {
        width: u32,
        height: u32,
        layers: u32,
        format: u32,
        usage: u64,
        stride: u32,
        rfu0: u32,
        rfu1: u64,
    }

    #[repr(C)]
    #[derive(Default)]
    struct CropRect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    type Status = i32;

    /// The functions used, from `libmediandk.so` and `libandroid.so`. The camera's JPEG
    /// reader uses them too (milestone 798).
    pub(crate) struct Ndk {
        pub(crate) reader_new: unsafe extern "C" fn(i32, i32, i32, i32, *mut *mut c_void) -> Status,
        pub(crate) reader_delete: unsafe extern "C" fn(*mut c_void),
        pub(crate) reader_window: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> Status,
        pub(crate) reader_listener: unsafe extern "C" fn(*mut c_void, *mut ImageListener) -> Status,
        pub(crate) reader_latest: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> Status,
        pub(crate) image_delete: unsafe extern "C" fn(*mut c_void),
        image_width: unsafe extern "C" fn(*const c_void, *mut i32) -> Status,
        image_height: unsafe extern "C" fn(*const c_void, *mut i32) -> Status,
        image_format: unsafe extern "C" fn(*const c_void, *mut i32) -> Status,
        image_crop: unsafe extern "C" fn(*const c_void, *mut CropRect) -> Status,
        plane_pixel_stride: unsafe extern "C" fn(*const c_void, c_int, *mut i32) -> Status,
        plane_row_stride: unsafe extern "C" fn(*const c_void, c_int, *mut i32) -> Status,
        pub(crate) plane_data:
            unsafe extern "C" fn(*const c_void, c_int, *mut *mut u8, *mut c_int) -> Status,
        /// API 26.
        to_surface: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void>,
        /// API 26: what handing out hardware buffers needs.
        hardware: Option<HardwareNdk>,
    }

    /// The functions that hand frames out as hardware buffers (API 26).
    struct HardwareNdk {
        reader_new: unsafe extern "C" fn(i32, i32, i32, u64, i32, *mut *mut c_void) -> Status,
        image_buffer: unsafe extern "C" fn(*const c_void, *mut *mut c_void) -> Status,
        describe: unsafe extern "C" fn(*const c_void, *mut BufferDesc),
    }

    pub(crate) fn ndk() -> Result<&'static Ndk, TextureError> {
        static NDK: OnceLock<Option<Ndk>> = OnceLock::new();
        NDK.get_or_init(|| unsafe { load() })
            .as_ref()
            .ok_or(TextureError::Unsupported(
                "the NDK image reader could not be loaded",
            ))
    }

    // Each symbol's type is the field it fills: spelling it again would only repeat that.
    #[allow(clippy::missing_transmute_annotations)]
    unsafe fn load() -> Option<Ndk> {
        let media = dlopen(c"libmediandk.so".as_ptr(), RTLD_NOW);
        let android = dlopen(c"libandroid.so".as_ptr(), RTLD_NOW);
        if media.is_null() {
            log::warn!("texture surface: libmediandk.so not found");
            return None;
        }
        macro_rules! sym {
            ($lib:expr, $name:literal) => {{
                let p = if $lib.is_null() {
                    std::ptr::null_mut()
                } else {
                    dlsym($lib, $name.as_ptr())
                };
                if p.is_null() {
                    None
                } else {
                    Some(std::mem::transmute::<*mut c_void, _>(p))
                }
            }};
        }
        Some(Ndk {
            reader_new: sym!(media, c"AImageReader_new")?,
            reader_delete: sym!(media, c"AImageReader_delete")?,
            reader_window: sym!(media, c"AImageReader_getWindow")?,
            reader_listener: sym!(media, c"AImageReader_setImageListener")?,
            reader_latest: sym!(media, c"AImageReader_acquireLatestImage")?,
            image_delete: sym!(media, c"AImage_delete")?,
            image_width: sym!(media, c"AImage_getWidth")?,
            image_height: sym!(media, c"AImage_getHeight")?,
            image_format: sym!(media, c"AImage_getFormat")?,
            image_crop: sym!(media, c"AImage_getCropRect")?,
            plane_pixel_stride: sym!(media, c"AImage_getPlanePixelStride")?,
            plane_row_stride: sym!(media, c"AImage_getPlaneRowStride")?,
            plane_data: sym!(media, c"AImage_getPlaneData")?,
            to_surface: sym!(android, c"ANativeWindow_toSurface"),
            // Not even looked up without the `hardware-buffers` feature: no byte of it ships.
            hardware: if !cfg!(feature = "hardware-buffers") {
                None
            } else {
                (|| {
                    Some(HardwareNdk {
                        reader_new: sym!(media, c"AImageReader_newWithUsage")?,
                        image_buffer: sym!(media, c"AImage_getHardwareBuffer")?,
                        describe: sym!(android, c"AHardwareBuffer_describe")?,
                    })
                })()
            },
        })
    }

    /// The reader itself, deleted once the surface is gone **and** every image it handed
    /// out is back: a frame the GPU still reads holds it.
    struct ReaderHandle(*mut c_void);

    unsafe impl Send for ReaderHandle {}
    unsafe impl Sync for ReaderHandle {}

    impl Drop for ReaderHandle {
        fn drop(&mut self) {
            if let Ok(ndk) = ndk() {
                unsafe { (ndk.reader_delete)(self.0) };
            }
        }
    }

    /// What the reader's thread reaches through the listener's context.
    struct Shared {
        /// Taken out only by `into_texture`, once no callback can run.
        texture: Option<ExternalTexture>,
        color_space: AtomicU8,
        reader: *mut c_void,
        /// Frames are handed out as hardware buffers, not copied.
        hardware: bool,
        handle: Arc<ReaderHandle>,
        observer: Mutex<Option<FrameObserver>>,
    }

    // The reader pointer is only used by the reader's own callback, which the NDK runs on
    // one thread at a time, and by `Drop`, after the listener is removed.
    unsafe impl Send for Shared {}
    unsafe impl Sync for Shared {}

    pub(super) struct Reader {
        shared: Box<Shared>,
        reader: *mut c_void,
        window: *mut c_void,
        java: Mutex<Option<jni::objects::GlobalRef>>,
    }

    /// An image handed out with its buffer: deleted (the buffer back to the producer) when
    /// the frame is let go.
    struct HeldImage {
        image: *mut c_void,
        _reader: Arc<ReaderHandle>,
    }

    // `AImage_delete` may be called from any thread.
    unsafe impl Send for HeldImage {}

    impl Drop for HeldImage {
        fn drop(&mut self) {
            if let Ok(ndk) = ndk() {
                unsafe { (ndk.image_delete)(self.image) };
            }
            // Then the reader, if this was the last thing holding it.
        }
    }

    unsafe impl Send for Reader {}
    unsafe impl Sync for Reader {}

    impl Reader {
        pub(super) fn new(
            texture: ExternalTexture,
            width: u32,
            height: u32,
            format: SurfaceFormat,
            readable: bool,
        ) -> Result<Self, TextureError> {
            let ndk = ndk()?;
            if width == 0 || height == 0 {
                return Err(TextureError::BadSize { width, height });
            }
            // Hardware buffers where the renderer imports them, planes copied elsewhere.
            // Only with the `hardware-buffers` feature: without it no renderer imports them.
            // Never for a surface whose frames must stay readable.
            let hardware = if cfg!(feature = "hardware-buffers") && !readable {
                ndk.hardware
                    .as_ref()
                    .filter(|_| frus_widgets::textures().hardware_buffers())
            } else {
                None
            };
            let mut reader = std::ptr::null_mut();
            let status = match hardware {
                Some(hw) => {
                    let code = match format {
                        SurfaceFormat::Yuv420 => AIMAGE_FORMAT_PRIVATE,
                        SurfaceFormat::Rgba => AIMAGE_FORMAT_RGBA_8888,
                    };
                    unsafe {
                        (hw.reader_new)(
                            width as i32,
                            height as i32,
                            code,
                            USAGE_GPU_SAMPLED_IMAGE,
                            MAX_HARDWARE_IMAGES,
                            &mut reader,
                        )
                    }
                }
                None => {
                    let code = match format {
                        SurfaceFormat::Yuv420 => AIMAGE_FORMAT_YUV_420_888,
                        SurfaceFormat::Rgba => AIMAGE_FORMAT_RGBA_8888,
                    };
                    unsafe {
                        (ndk.reader_new)(width as i32, height as i32, code, MAX_IMAGES, &mut reader)
                    }
                }
            };
            if status != 0 || reader.is_null() {
                return Err(TextureError::Platform(format!(
                    "AImageReader_new failed ({status})"
                )));
            }
            let handle = Arc::new(ReaderHandle(reader));
            let mut window = std::ptr::null_mut();
            let status = unsafe { (ndk.reader_window)(reader, &mut window) };
            if status != 0 || window.is_null() {
                return Err(TextureError::Platform(format!(
                    "AImageReader_getWindow failed ({status})"
                )));
            }
            let mut shared = Box::new(Shared {
                texture: Some(texture),
                color_space: AtomicU8::new(0),
                reader,
                hardware: hardware.is_some(),
                handle,
                observer: Mutex::new(None),
            });
            let mut listener = ImageListener {
                context: (&mut *shared as *mut Shared).cast(),
                on_image_available: Some(on_image),
            };
            let status = unsafe { (ndk.reader_listener)(reader, &mut listener) };
            if status != 0 {
                return Err(TextureError::Platform(format!(
                    "AImageReader_setImageListener failed ({status})"
                )));
            }
            if cfg!(feature = "hardware-buffers") && shared.hardware {
                log::info!(
                    "texture surface {width}×{height} ({format:?}): hardware buffers, imported \
                     by the GPU without a copy"
                );
            } else {
                log::info!(
                    "texture surface {width}×{height} ({format:?}): CPU planes through an image \
                     reader"
                );
            }
            Ok(Self {
                shared,
                reader,
                window,
                java: Mutex::new(None),
            })
        }

        pub(super) fn texture(&self) -> &ExternalTexture {
            self.shared
                .texture
                .as_ref()
                .expect("only taken by into_texture, which consumes the reader")
        }

        pub(super) fn set_observer(&self, observer: Option<FrameObserver>) {
            *self
                .shared
                .observer
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = observer;
        }

        pub(super) fn into_texture(mut self) -> ExternalTexture {
            self.shut_down();
            self.shared.texture.take().expect("only taken here, once")
        }

        /// No callback runs past this. The reader itself goes when the last image it handed
        /// out is back (`ReaderHandle`).
        fn shut_down(&mut self) {
            if self.reader.is_null() {
                return;
            }
            if let Ok(ndk) = ndk() {
                unsafe {
                    (ndk.reader_listener)(self.reader, std::ptr::null_mut());
                }
            }
            self.reader = std::ptr::null_mut();
        }

        pub(super) fn window(&self) -> *mut c_void {
            self.window
        }

        pub(super) fn set_color_space(&self, space: YuvColorSpace) {
            let code = match space {
                YuvColorSpace::Bt601Limited => 0,
                YuvColorSpace::Bt601Full => 1,
                YuvColorSpace::Bt709Limited => 2,
                YuvColorSpace::Bt709Full => 3,
            };
            self.shared.color_space.store(code, Ordering::Relaxed);
        }

        pub(super) fn java_surface(&self) -> Result<*mut c_void, TextureError> {
            let ndk = ndk()?;
            let to_surface = ndk.to_surface.ok_or(TextureError::Unsupported(
                "a Java surface needs Android 8 (API 26)",
            ))?;
            let mut slot = self
                .java
                .lock()
                .map_err(|_| TextureError::Platform("poisoned".into()))?;
            if let Some(global) = slot.as_ref() {
                return Ok(global.as_obj().as_raw().cast());
            }
            let context = ndk_context::android_context();
            let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast()) }
                .map_err(|e| TextureError::Platform(e.to_string()))?;
            let env = vm
                .attach_current_thread_permanently()
                .map_err(|e| TextureError::Platform(e.to_string()))?;
            let local = unsafe { to_surface(env.get_raw().cast(), self.window) };
            if local.is_null() {
                return Err(TextureError::Platform(
                    "ANativeWindow_toSurface answered nothing".into(),
                ));
            }
            let local = unsafe { jni::objects::JObject::from_raw(local.cast()) };
            let global = env
                .new_global_ref(&local)
                .map_err(|e| TextureError::Platform(e.to_string()))?;
            let _ = env.delete_local_ref(local);
            let raw = global.as_obj().as_raw().cast();
            *slot = Some(global);
            Ok(raw)
        }
    }

    impl Drop for Reader {
        fn drop(&mut self) {
            self.shut_down();
            // The Java surface is released from a thread the VM knows: the surface may well
            // be dropped on the producer's own, which never touched Java.
            if let Some(global) = self.java.get_mut().ok().and_then(|java| java.take()) {
                let context = ndk_context::android_context();
                if let Ok(vm) = unsafe { jni::JavaVM::from_raw(context.vm().cast()) } {
                    if let Ok(_env) = vm.attach_current_thread() {
                        drop(global);
                    }
                }
            }
        }
    }

    /// The reader's callback: the newest image becomes the texture's frame.
    unsafe extern "C" fn on_image(context: *mut c_void, reader: *mut c_void) {
        let Ok(ndk) = ndk() else {
            return;
        };
        let shared = &*(context as *const Shared);
        debug_assert_eq!(shared.reader, reader);
        let mut image = std::ptr::null_mut();
        if (ndk.reader_latest)(reader, &mut image) != 0 || image.is_null() {
            return;
        }
        if cfg!(feature = "hardware-buffers") && shared.hardware {
            if let Err(err) = push_buffer(ndk, shared, image) {
                log::warn!("texture surface: a frame was not shown ({err})");
            }
            return;
        }
        if let Err(err) = push(ndk, shared, image) {
            log::warn!("texture surface: a frame was not shown ({err})");
        }
        (ndk.image_delete)(image);
    }

    fn color_space(shared: &Shared) -> YuvColorSpace {
        match shared.color_space.load(Ordering::Relaxed) {
            1 => YuvColorSpace::Bt601Full,
            2 => YuvColorSpace::Bt709Limited,
            3 => YuvColorSpace::Bt709Full,
            _ => YuvColorSpace::Bt601Limited,
        }
    }

    /// Hands `image`'s buffer to the texture as it is: the image is deleted (and the buffer
    /// goes back to the producer) once the frame is replaced and the GPU has read it.
    unsafe fn push_buffer(
        ndk: &Ndk,
        shared: &Shared,
        image: *mut c_void,
    ) -> Result<(), TextureError> {
        let held = HeldImage {
            image,
            _reader: shared.handle.clone(),
        };
        let hw = ndk.hardware.as_ref().ok_or(TextureError::Unsupported(
            "no hardware buffers before Android 8",
        ))?;
        let mut buffer = std::ptr::null_mut();
        if (hw.image_buffer)(image, &mut buffer) != 0 || buffer.is_null() {
            return Err(TextureError::Platform(
                "the image has no hardware buffer".into(),
            ));
        }
        let mut desc = BufferDesc::default();
        (hw.describe)(buffer, &mut desc);
        let mut crop = CropRect::default();
        let cropped = (ndk.image_crop)(image, &mut crop) == 0
            && crop.right > crop.left
            && crop.bottom > crop.top;
        let frame = HardwareBuffer::new(buffer, desc.width, desc.height, move || drop(held))
            .ok_or(TextureError::Platform("a null hardware buffer".into()))?;
        let frame = if cropped {
            frame.with_crop(
                crop.left.max(0) as u32,
                crop.top.max(0) as u32,
                crop.right.max(0) as u32,
                crop.bottom.max(0) as u32,
            )
        } else {
            frame
        };
        match shared.texture.as_ref() {
            Some(texture) => texture.push_hardware_buffer(frame, color_space(shared)),
            None => Ok(()),
        }
    }

    unsafe fn push(ndk: &Ndk, shared: &Shared, image: *mut c_void) -> Result<(), TextureError> {
        let (mut w, mut h, mut format) = (0, 0, 0);
        (ndk.image_width)(image, &mut w);
        (ndk.image_height)(image, &mut h);
        (ndk.image_format)(image, &mut format);
        let mut crop = CropRect::default();
        if (ndk.image_crop)(image, &mut crop) != 0 || crop.right <= crop.left {
            crop = CropRect {
                left: 0,
                top: 0,
                right: w,
                bottom: h,
            };
        }
        let Some(texture) = shared.texture.as_ref() else {
            return Ok(());
        };
        let (left, top) = (crop.left.max(0) as usize, crop.top.max(0) as usize);
        let width = (crop.right.min(w) - crop.left.max(0)).max(0) as u32;
        let height = (crop.bottom.min(h) - crop.top.max(0)).max(0) as u32;
        let plane = |index: c_int, shift: usize| -> Result<YuvPlane<'static>, TextureError> {
            let (mut data, mut len, mut pixel, mut row) = (std::ptr::null_mut(), 0, 0, 0);
            if (ndk.plane_data)(image, index, &mut data, &mut len) != 0 || data.is_null() {
                return Err(TextureError::Platform(format!("no plane {index}")));
            }
            (ndk.plane_pixel_stride)(image, index, &mut pixel);
            (ndk.plane_row_stride)(image, index, &mut row);
            let (pixel, row) = (pixel.max(1) as usize, row.max(0) as usize);
            // The crop, in this plane's samples.
            let skip = (top >> shift) * row + (left >> shift) * pixel;
            let all = std::slice::from_raw_parts(data, len.max(0) as usize);
            let data = all.get(skip..).unwrap_or(&[]);
            Ok(YuvPlane::new(data, row, pixel))
        };
        match format {
            AIMAGE_FORMAT_YUV_420_888 => {
                let planes = [plane(0, 0)?, plane(1, 1)?, plane(2, 1)?];
                let [y, u, v] = planes;
                texture.push_yuv420(width, height, y, u, v, color_space(shared))?;
                observe(shared, width, height, SurfaceFormat::Yuv420, &planes);
                Ok(())
            }
            AIMAGE_FORMAT_RGBA_8888 => {
                let p = plane(0, 0)?;
                texture.push_rgba_strided(width, height, p.data, p.row_stride)?;
                observe(shared, width, height, SurfaceFormat::Rgba, &[p]);
                Ok(())
            }
            other => Err(TextureError::Platform(format!(
                "image format {other:#x} is not read"
            ))),
        }
    }

    /// Hands the frame to the observer, if there is one.
    fn observe(
        shared: &Shared,
        width: u32,
        height: u32,
        format: SurfaceFormat,
        planes: &[YuvPlane<'_>],
    ) {
        let Ok(mut observer) = shared.observer.lock() else {
            return;
        };
        if let Some(observer) = observer.as_mut() {
            observer(&SurfaceFrame {
                width,
                height,
                format,
                planes,
            });
        }
    }
}
