// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! [`TextureSurface`]: a **platform surface** whose frames become an external texture
//! (milestone 793) — what a video decoder, a camera or a native view draws into.
//!
//! On Android it is an image reader: its window is an `ANativeWindow`, and so an
//! `android.view.Surface` for Java, that `MediaCodec`, Camera2, a media player or a
//! `Canvas` can target. Every frame they queue is read from its planes on the reader's own
//! thread and pushed into the texture (YUV 4:2:0 for decoders and cameras, RGBA for a
//! canvas); the `Texture` widget shows it. Nothing runs between frames.
//!
//! Elsewhere there is no such surface, and [`TextureSurface::new`] answers
//! [`TextureError::Unsupported`]: push frames into an [`ExternalTexture`] directly.

use frus_widgets::{ExternalTexture, TextureError, TextureId, YuvColorSpace};

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
        #[cfg(target_os = "android")]
        {
            android::Reader::new(texture, width, height, format).map(|inner| Self { inner })
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (texture, width, height, format);
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
mod android {
    //! The NDK image reader, reached through `dlsym`: frus applications start at Android 7
    //! (API 24), and linking a symbol a later version added would stop the library loading
    //! on the earlier one. Nothing is looked up before a surface is made.

    use std::ffi::{c_char, c_int, c_void};
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::{Mutex, OnceLock};

    use frus_widgets::{ExternalTexture, TextureError, YuvColorSpace, YuvPlane};

    use super::SurfaceFormat;

    const RTLD_NOW: c_int = 2;
    const AIMAGE_FORMAT_RGBA_8888: i32 = 1;
    const AIMAGE_FORMAT_YUV_420_888: i32 = 0x23;
    /// Images the reader holds: one the producer fills, one being read, one spare.
    const MAX_IMAGES: i32 = 3;

    extern "C" {
        fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    #[repr(C)]
    struct ImageListener {
        context: *mut c_void,
        on_image_available: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
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

    /// The functions used, from `libmediandk.so` and `libandroid.so`.
    struct Ndk {
        reader_new: unsafe extern "C" fn(i32, i32, i32, i32, *mut *mut c_void) -> Status,
        reader_delete: unsafe extern "C" fn(*mut c_void),
        reader_window: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> Status,
        reader_listener: unsafe extern "C" fn(*mut c_void, *mut ImageListener) -> Status,
        reader_latest: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> Status,
        image_delete: unsafe extern "C" fn(*mut c_void),
        image_width: unsafe extern "C" fn(*const c_void, *mut i32) -> Status,
        image_height: unsafe extern "C" fn(*const c_void, *mut i32) -> Status,
        image_format: unsafe extern "C" fn(*const c_void, *mut i32) -> Status,
        image_crop: unsafe extern "C" fn(*const c_void, *mut CropRect) -> Status,
        plane_pixel_stride: unsafe extern "C" fn(*const c_void, c_int, *mut i32) -> Status,
        plane_row_stride: unsafe extern "C" fn(*const c_void, c_int, *mut i32) -> Status,
        plane_data: unsafe extern "C" fn(*const c_void, c_int, *mut *mut u8, *mut c_int) -> Status,
        /// API 26.
        to_surface: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void>,
    }

    fn ndk() -> Result<&'static Ndk, TextureError> {
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
        })
    }

    /// What the reader's thread reaches through the listener's context.
    struct Shared {
        texture: ExternalTexture,
        color_space: AtomicU8,
        reader: *mut c_void,
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

    unsafe impl Send for Reader {}
    unsafe impl Sync for Reader {}

    impl Reader {
        pub(super) fn new(
            texture: ExternalTexture,
            width: u32,
            height: u32,
            format: SurfaceFormat,
        ) -> Result<Self, TextureError> {
            let ndk = ndk()?;
            if width == 0 || height == 0 {
                return Err(TextureError::BadSize { width, height });
            }
            let code = match format {
                SurfaceFormat::Yuv420 => AIMAGE_FORMAT_YUV_420_888,
                SurfaceFormat::Rgba => AIMAGE_FORMAT_RGBA_8888,
            };
            let mut reader = std::ptr::null_mut();
            let status = unsafe {
                (ndk.reader_new)(width as i32, height as i32, code, MAX_IMAGES, &mut reader)
            };
            if status != 0 || reader.is_null() {
                return Err(TextureError::Platform(format!(
                    "AImageReader_new failed ({status})"
                )));
            }
            let mut window = std::ptr::null_mut();
            let status = unsafe { (ndk.reader_window)(reader, &mut window) };
            if status != 0 || window.is_null() {
                unsafe { (ndk.reader_delete)(reader) };
                return Err(TextureError::Platform(format!(
                    "AImageReader_getWindow failed ({status})"
                )));
            }
            let mut shared = Box::new(Shared {
                texture,
                color_space: AtomicU8::new(0),
                reader,
            });
            let mut listener = ImageListener {
                context: (&mut *shared as *mut Shared).cast(),
                on_image_available: Some(on_image),
            };
            let status = unsafe { (ndk.reader_listener)(reader, &mut listener) };
            if status != 0 {
                unsafe { (ndk.reader_delete)(reader) };
                return Err(TextureError::Platform(format!(
                    "AImageReader_setImageListener failed ({status})"
                )));
            }
            log::info!(
                "texture surface {width}×{height} ({format:?}): CPU planes through an image reader"
            );
            Ok(Self {
                shared,
                reader,
                window,
                java: Mutex::new(None),
            })
        }

        pub(super) fn texture(&self) -> &ExternalTexture {
            &self.shared.texture
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
            if let Ok(ndk) = ndk() {
                unsafe {
                    // No callback runs past this: the listener goes first, then the reader
                    // and every image it held.
                    (ndk.reader_listener)(self.reader, std::ptr::null_mut());
                    (ndk.reader_delete)(self.reader);
                }
            }
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
        if let Err(err) = push(ndk, shared, image) {
            log::warn!("texture surface: a frame was not shown ({err})");
        }
        (ndk.image_delete)(image);
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
                let space = match shared.color_space.load(Ordering::Relaxed) {
                    1 => YuvColorSpace::Bt601Full,
                    2 => YuvColorSpace::Bt709Limited,
                    3 => YuvColorSpace::Bt709Full,
                    _ => YuvColorSpace::Bt601Limited,
                };
                let (y, u, v) = (plane(0, 0)?, plane(1, 1)?, plane(2, 1)?);
                shared.texture.push_yuv420(width, height, y, u, v, space)
            }
            AIMAGE_FORMAT_RGBA_8888 => {
                let p = plane(0, 0)?;
                shared
                    .texture
                    .push_rgba_strided(width, height, p.data, p.row_stride)
            }
            other => Err(TextureError::Platform(format!(
                "image format {other:#x} is not read"
            ))),
        }
    }
}
