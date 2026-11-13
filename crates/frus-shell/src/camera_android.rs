// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The camera on **Android** (milestone 798): Camera2 through the NDK.
//!
//! - The preview is a repeating request into a [`TextureSurface`] (an image reader whose
//!   frames become the texture the `CameraPreview` shows), turned upright on the GPU by the
//!   texture's orientation. Its frames go to the image stream from the reader's thread, so
//!   the surface is always made readable (`TextureSurface::readable_with_texture`): with the
//!   `hardware-buffers` feature its frames are still copied, never imported as GPU buffers
//!   the image stream and the scanner could not read.
//! - A picture is a single request into a second image reader, of JPEG images, which tells
//!   the JPEG how to stand upright (its EXIF orientation).
//! - The flash, the zoom (a crop of the sensor, which every Camera2 device has) and the focus
//!   point are entries of those requests; changing one sends the repeating request again.
//!
//! Everything is looked up with `dlsym` (`libcamera2ndk.so`, Android 7) the first time a camera
//! is listed or opened, as the texture surface does: an application that never opens one loads
//! nothing.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::Duration;

use frus_widgets::{ExternalTexture, FrameOrientation, TextureId};

use super::{
    choose_size, lock, preview_orientation, CameraDescription, CameraError, CameraImage,
    CameraImageFormat, CameraLensDirection, CameraPlane, FlashMode, ResolutionPreset, Settings,
    Sink,
};
use crate::texture_surface::android::{ndk as image_ndk, ImageListener};
use crate::{SurfaceFormat, TextureSurface};

const RTLD_NOW: c_int = 2;

// Metadata tags (NdkCameraMetadataTags.h): section << 16 | index.
const CONTROL: u32 = 1 << 16;
const CONTROL_AE_MODE: u32 = CONTROL + 3;
const CONTROL_AE_REGIONS: u32 = CONTROL + 4;
const CONTROL_AE_PRECAPTURE_TRIGGER: u32 = CONTROL + 6;
const CONTROL_AF_MODE: u32 = CONTROL + 7;
const CONTROL_AF_REGIONS: u32 = CONTROL + 8;
const CONTROL_AF_TRIGGER: u32 = CONTROL + 9;
const CONTROL_AF_AVAILABLE_MODES: u32 = CONTROL + 0x17;
const CONTROL_MAX_REGIONS: u32 = CONTROL + 0x1c;
const FLASH_MODE: u32 = (4 << 16) + 2;
const FLASH_INFO_AVAILABLE: u32 = 5 << 16;
const JPEG_ORIENTATION: u32 = (7 << 16) + 3;
const JPEG_QUALITY: u32 = (7 << 16) + 4;
const LENS_FACING: u32 = (8 << 16) + 5;
const SCALER_CROP_REGION: u32 = 13 << 16;
const SCALER_MAX_DIGITAL_ZOOM: u32 = (13 << 16) + 4;
const SCALER_STREAM_CONFIGURATIONS: u32 = (13 << 16) + 10;
const SENSOR_ORIENTATION: u32 = (14 << 16) + 14;
const SENSOR_ACTIVE_ARRAY: u32 = 15 << 16;

const AE_MODE_ON: u8 = 1;
const AE_MODE_ON_AUTO_FLASH: u8 = 2;
const AE_MODE_ON_ALWAYS_FLASH: u8 = 3;
const AF_MODE_AUTO: u8 = 1;
const AF_MODE_CONTINUOUS_PICTURE: u8 = 4;
const AF_TRIGGER_IDLE: u8 = 0;
const AF_TRIGGER_START: u8 = 1;
const AF_TRIGGER_CANCEL: u8 = 2;
const FLASH_MODE_OFF: u8 = 0;
const FLASH_MODE_TORCH: u8 = 2;
const PRECAPTURE_START: u8 = 1;

const TEMPLATE_PREVIEW: i32 = 1;
const TEMPLATE_STILL_CAPTURE: i32 = 2;
const FORMAT_YUV_420_888: i32 = 0x23;
const FORMAT_JPEG: i32 = 0x100;

type Status = i32;
type P = *mut c_void;

#[repr(C)]
struct ConstEntry {
    tag: u32,
    kind: u8,
    count: u32,
    data: *const c_void,
}

#[repr(C)]
struct IdList {
    count: c_int,
    ids: *const *const c_char,
}

#[repr(C)]
struct DeviceCallbacks {
    context: P,
    on_disconnected: Option<unsafe extern "C" fn(P, P)>,
    on_error: Option<unsafe extern "C" fn(P, P, c_int)>,
}

#[repr(C)]
struct SessionCallbacks {
    context: P,
    on_closed: Option<unsafe extern "C" fn(P, P)>,
    on_ready: Option<unsafe extern "C" fn(P, P)>,
    on_active: Option<unsafe extern "C" fn(P, P)>,
}

/// The functions used, from `libcamera2ndk.so`.
struct Ndk {
    manager_create: unsafe extern "C" fn() -> P,
    manager_delete: unsafe extern "C" fn(P),
    id_list: unsafe extern "C" fn(P, *mut *mut IdList) -> Status,
    id_list_delete: unsafe extern "C" fn(*mut IdList),
    characteristics: unsafe extern "C" fn(P, *const c_char, *mut P) -> Status,
    metadata_entry: unsafe extern "C" fn(P, u32, *mut ConstEntry) -> Status,
    metadata_free: unsafe extern "C" fn(P),
    open: unsafe extern "C" fn(P, *const c_char, *mut DeviceCallbacks, *mut P) -> Status,
    device_close: unsafe extern "C" fn(P) -> Status,
    request_create: unsafe extern "C" fn(P, c_int, *mut P) -> Status,
    request_free: unsafe extern "C" fn(P),
    request_add_target: unsafe extern "C" fn(P, P) -> Status,
    request_u8: unsafe extern "C" fn(P, u32, u32, *const u8) -> Status,
    request_i32: unsafe extern "C" fn(P, u32, u32, *const i32) -> Status,
    target_create: unsafe extern "C" fn(P, *mut P) -> Status,
    target_free: unsafe extern "C" fn(P),
    container_create: unsafe extern "C" fn(*mut P) -> Status,
    container_free: unsafe extern "C" fn(P),
    container_add: unsafe extern "C" fn(P, P) -> Status,
    output_create: unsafe extern "C" fn(P, *mut P) -> Status,
    output_free: unsafe extern "C" fn(P),
    session_create: unsafe extern "C" fn(P, P, *const SessionCallbacks, *mut P) -> Status,
    session_close: unsafe extern "C" fn(P),
    session_repeating: unsafe extern "C" fn(P, P, c_int, *mut P, *mut c_int) -> Status,
    session_capture: unsafe extern "C" fn(P, P, c_int, *mut P, *mut c_int) -> Status,
    session_stop: unsafe extern "C" fn(P) -> Status,
}

unsafe impl Send for Ndk {}
unsafe impl Sync for Ndk {}

extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

fn ndk() -> Result<&'static Ndk, CameraError> {
    static NDK: OnceLock<Option<Ndk>> = OnceLock::new();
    NDK.get_or_init(|| unsafe { load() })
        .as_ref()
        .ok_or(CameraError::Unsupported(
            "the NDK camera library could not be loaded",
        ))
}

// Each symbol's type is the field it fills: spelling it again would only repeat that.
#[allow(clippy::missing_transmute_annotations)]
unsafe fn load() -> Option<Ndk> {
    let lib = dlopen(c"libcamera2ndk.so".as_ptr(), RTLD_NOW);
    if lib.is_null() {
        log::warn!("camera: libcamera2ndk.so not found");
        return None;
    }
    macro_rules! sym {
        ($name:literal) => {{
            let p = dlsym(lib, $name.as_ptr());
            if p.is_null() {
                log::warn!("camera: {:?} not found", $name);
                return None;
            }
            std::mem::transmute::<*mut c_void, _>(p)
        }};
    }
    Some(Ndk {
        manager_create: sym!(c"ACameraManager_create"),
        manager_delete: sym!(c"ACameraManager_delete"),
        id_list: sym!(c"ACameraManager_getCameraIdList"),
        id_list_delete: sym!(c"ACameraManager_deleteCameraIdList"),
        characteristics: sym!(c"ACameraManager_getCameraCharacteristics"),
        metadata_entry: sym!(c"ACameraMetadata_getConstEntry"),
        metadata_free: sym!(c"ACameraMetadata_free"),
        open: sym!(c"ACameraManager_openCamera"),
        device_close: sym!(c"ACameraDevice_close"),
        request_create: sym!(c"ACameraDevice_createCaptureRequest"),
        request_free: sym!(c"ACaptureRequest_free"),
        request_add_target: sym!(c"ACaptureRequest_addTarget"),
        request_u8: sym!(c"ACaptureRequest_setEntry_u8"),
        request_i32: sym!(c"ACaptureRequest_setEntry_i32"),
        target_create: sym!(c"ACameraOutputTarget_create"),
        target_free: sym!(c"ACameraOutputTarget_free"),
        container_create: sym!(c"ACaptureSessionOutputContainer_create"),
        container_free: sym!(c"ACaptureSessionOutputContainer_free"),
        container_add: sym!(c"ACaptureSessionOutputContainer_add"),
        output_create: sym!(c"ACaptureSessionOutput_create"),
        output_free: sym!(c"ACaptureSessionOutput_free"),
        session_create: sym!(c"ACameraDevice_createCaptureSession"),
        session_close: sym!(c"ACameraCaptureSession_close"),
        session_repeating: sym!(c"ACameraCaptureSession_setRepeatingRequest"),
        session_capture: sym!(c"ACameraCaptureSession_capture"),
        session_stop: sym!(c"ACameraCaptureSession_stopRepeating"),
    })
}

fn check(what: &str, status: Status) -> Result<(), CameraError> {
    if status == 0 {
        Ok(())
    } else {
        Err(CameraError::Platform(format!("{what} failed ({status})")))
    }
}

/// A camera manager, deleted on drop.
struct Manager(P);

impl Manager {
    fn new(ndk: &Ndk) -> Result<Self, CameraError> {
        let manager = unsafe { (ndk.manager_create)() };
        if manager.is_null() {
            return Err(CameraError::Platform("ACameraManager_create failed".into()));
        }
        Ok(Self(manager))
    }

    fn ids(&self, ndk: &Ndk) -> Result<Vec<String>, CameraError> {
        let mut list = std::ptr::null_mut();
        check("getCameraIdList", unsafe {
            (ndk.id_list)(self.0, &mut list)
        })?;
        if list.is_null() {
            return Ok(Vec::new());
        }
        let ids = unsafe {
            let list = &*list;
            (0..list.count.max(0) as usize)
                .map(|i| {
                    CStr::from_ptr(*list.ids.add(i))
                        .to_string_lossy()
                        .into_owned()
                })
                .collect()
        };
        unsafe { (ndk.id_list_delete)(list) };
        Ok(ids)
    }

    fn characteristics(&self, ndk: &Ndk, id: &str) -> Result<Characteristics, CameraError> {
        let name = CString::new(id).map_err(|_| CameraError::NotFound)?;
        let mut meta = std::ptr::null_mut();
        check("getCameraCharacteristics", unsafe {
            (ndk.characteristics)(self.0, name.as_ptr(), &mut meta)
        })?;
        let read = Characteristics::read(ndk, meta);
        unsafe { (ndk.metadata_free)(meta) };
        Ok(read)
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        if let Ok(ndk) = ndk() {
            unsafe { (ndk.manager_delete)(self.0) };
        }
    }
}

/// What the session needs to know of a camera.
#[derive(Clone, Debug, Default)]
struct Characteristics {
    facing: u8,
    sensor_orientation: u32,
    /// `left, top, width, height` of the sensor's active pixels.
    active: [i32; 4],
    max_zoom: f32,
    flash: bool,
    af_modes: Vec<u8>,
    /// How many focus regions the camera takes (and metering ones).
    max_af_regions: i32,
    max_ae_regions: i32,
    yuv_sizes: Vec<(u32, u32)>,
    jpeg_sizes: Vec<(u32, u32)>,
}

impl Characteristics {
    fn read(ndk: &Ndk, meta: P) -> Self {
        let entry = |tag| -> Option<ConstEntry> {
            let mut e = ConstEntry {
                tag: 0,
                kind: 0,
                count: 0,
                data: std::ptr::null(),
            };
            let status = unsafe { (ndk.metadata_entry)(meta, tag, &mut e) };
            (status == 0 && !e.data.is_null()).then_some(e)
        };
        let u8s = |tag| -> Vec<u8> {
            entry(tag).map_or_else(Vec::new, |e| unsafe {
                std::slice::from_raw_parts(e.data as *const u8, e.count as usize).to_vec()
            })
        };
        let i32s = |tag| -> Vec<i32> {
            entry(tag).map_or_else(Vec::new, |e| unsafe {
                std::slice::from_raw_parts(e.data as *const i32, e.count as usize).to_vec()
            })
        };
        let float = |tag| -> Option<f32> { entry(tag).map(|e| unsafe { *(e.data as *const f32) }) };
        let active = i32s(SENSOR_ACTIVE_ARRAY);
        let regions = i32s(CONTROL_MAX_REGIONS);
        let configs = i32s(SCALER_STREAM_CONFIGURATIONS);
        let sizes = |format: i32| -> Vec<(u32, u32)> {
            configs
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|c| c[0] == format && c[3] == 0 && c[1] > 0 && c[2] > 0)
                .map(|c| (c[1] as u32, c[2] as u32))
                .collect()
        };
        Self {
            facing: u8s(LENS_FACING).first().copied().unwrap_or(1),
            sensor_orientation: i32s(SENSOR_ORIENTATION)
                .first()
                .map_or(0, |&o| o.rem_euclid(360) as u32),
            active: if active.len() == 4 {
                [active[0], active[1], active[2], active[3]]
            } else {
                [0; 4]
            },
            max_zoom: float(SCALER_MAX_DIGITAL_ZOOM).unwrap_or(1.0).max(1.0),
            flash: u8s(FLASH_INFO_AVAILABLE).first() == Some(&1),
            af_modes: u8s(CONTROL_AF_AVAILABLE_MODES),
            max_ae_regions: regions.first().copied().unwrap_or(0),
            max_af_regions: regions.get(2).copied().unwrap_or(0),
            yuv_sizes: sizes(FORMAT_YUV_420_888),
            jpeg_sizes: sizes(FORMAT_JPEG),
        }
    }

    fn lens(&self) -> CameraLensDirection {
        match self.facing {
            0 => CameraLensDirection::Front,
            2 => CameraLensDirection::External,
            _ => CameraLensDirection::Back,
        }
    }
}

pub(super) fn cameras() -> Result<Vec<CameraDescription>, CameraError> {
    let ndk = ndk()?;
    let manager = Manager::new(ndk)?;
    let mut cameras = Vec::new();
    for id in manager.ids(ndk)? {
        match manager.characteristics(ndk, &id) {
            Ok(c) => cameras.push(CameraDescription {
                lens_direction: c.lens(),
                sensor_orientation: c.sensor_orientation,
                name: id,
            }),
            Err(err) => log::warn!("camera {id}: {err}"),
        }
    }
    Ok(cameras)
}

/// The display's rotation in degrees, as `Display.getRotation` gives it.
fn display_rotation() -> u32 {
    fn read() -> Result<i32, jni::errors::Error> {
        let context = ndk_context::android_context();
        let vm = unsafe { jni::JavaVM::from_raw(context.vm().cast()) }?;
        let mut env = vm.attach_current_thread_permanently()?;
        let activity = unsafe { jni::objects::JObject::from_raw(context.context().cast()) };
        let result = (|| {
            let manager = env
                .call_method(
                    &activity,
                    "getWindowManager",
                    "()Landroid/view/WindowManager;",
                    &[],
                )?
                .l()?;
            let display = env
                .call_method(
                    &manager,
                    "getDefaultDisplay",
                    "()Landroid/view/Display;",
                    &[],
                )?
                .l()?;
            env.call_method(&display, "getRotation", "()I", &[])?.i()
        })();
        if result.is_err() && env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        result
    }
    match read() {
        Ok(r) => (r.rem_euclid(4) as u32) * 90,
        Err(err) => {
            log::warn!("camera: the display's rotation is unknown ({err})");
            0
        }
    }
}

/// A surface feeding the preview, and the size it was made for.
type Feed = (Arc<TextureSurface>, (u32, u32));

/// The preview's texture, kept from one session to the next — a pause, another camera — so
/// its id does not change; and the surface feeding it, made again only for another size.
pub(super) struct Preview {
    id: TextureId,
    surface: Mutex<Option<Feed>>,
    /// The texture while no surface holds it.
    texture: Mutex<Option<ExternalTexture>>,
}

impl Preview {
    pub(super) fn new() -> Result<Self, CameraError> {
        let texture = frus_widgets::textures().create();
        Ok(Self {
            id: texture.id(),
            surface: Mutex::new(None),
            texture: Mutex::new(Some(texture)),
        })
    }

    pub(super) fn id(&self) -> TextureId {
        self.id
    }

    /// A surface of `size` feeding the texture: the one there when it is that size.
    fn surface(&self, size: (u32, u32)) -> Result<Arc<TextureSurface>, CameraError> {
        let mut slot = lock(&self.surface);
        if let Some((surface, made)) = slot.as_ref() {
            if *made == size {
                return Ok(surface.clone());
            }
        }
        let texture = match slot.take() {
            Some((old, _)) => match Arc::try_unwrap(old) {
                Ok(old) => old.into_texture(),
                Err(_) => {
                    return Err(CameraError::Platform(
                        "the previous session still holds the preview".into(),
                    ))
                }
            },
            None => lock(&self.texture)
                .take()
                .ok_or_else(|| CameraError::Platform("the preview texture is gone".into()))?,
        };
        let surface =
            TextureSurface::readable_with_texture(texture, size.0, size.1, SurfaceFormat::Yuv420)
                .map_err(|e| CameraError::Platform(e.to_string()))?;
        let surface = Arc::new(surface);
        *slot = Some((surface.clone(), size));
        Ok(surface)
    }
}

/// The JPEG reader's side: where its callback sends a picture's bytes.
struct StillShared {
    reader: P,
    waiting: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
}

unsafe impl Send for StillShared {}
unsafe impl Sync for StillShared {}

/// An open camera: its device, its session, the requests it sends.
pub(super) struct Session {
    ndk: &'static Ndk,
    _manager: Manager,
    chars: Characteristics,
    device: P,
    session: P,
    container: P,
    outputs: Vec<P>,
    preview_target: P,
    still_target: Option<P>,
    /// The repeating request, kept to be sent again with new settings.
    request: Mutex<P>,
    still: Option<Box<StillShared>>,
    surface: Arc<TextureSurface>,
    sink: Arc<Sink>,
    size: (u32, u32),
    /// The display's rotation the preview's orientation was worked out for.
    display: AtomicU32,
    _device_callbacks: Box<DeviceCallbacks>,
    _session_callbacks: Box<SessionCallbacks>,
}

unsafe impl Send for Session {}
unsafe impl Sync for Session {}

unsafe extern "C" fn on_disconnected(_: P, _: P) {
    log::warn!("camera: disconnected (another application took it)");
}

unsafe extern "C" fn on_error(_: P, _: P, error: c_int) {
    log::warn!("camera: device error {error}");
}

unsafe extern "C" fn on_session_state(_: P, _: P) {}

unsafe extern "C" fn on_still(context: P, reader: P) {
    let Ok(ndk) = image_ndk() else {
        return;
    };
    let shared = &*(context as *const StillShared);
    let mut image = std::ptr::null_mut();
    if (ndk.reader_latest)(reader, &mut image) != 0 || image.is_null() {
        return;
    }
    let (mut data, mut len) = (std::ptr::null_mut(), 0);
    if (ndk.plane_data)(image, 0, &mut data, &mut len) == 0 && !data.is_null() {
        let bytes = std::slice::from_raw_parts(data, len.max(0) as usize).to_vec();
        if let Some(sender) = shared.waiting.lock().ok().and_then(|mut w| w.take()) {
            let _ = sender.send(bytes);
        }
    }
    (ndk.image_delete)(image);
}

impl Session {
    pub(super) fn open(
        description: &CameraDescription,
        preset: ResolutionPreset,
        preview: &Preview,
        sink: Arc<Sink>,
        stills: bool,
    ) -> Result<Self, CameraError> {
        let ndk = ndk()?;
        let manager = Manager::new(ndk)?;
        let chars = manager.characteristics(ndk, &description.name)?;
        let size = choose_size(&chars.yuv_sizes, preset, None)
            .ok_or_else(|| CameraError::Platform("the camera offers no YUV preview size".into()))?;
        let surface = preview.surface(size)?;
        let display = display_rotation();
        let orientation = preview_orientation(chars.lens(), chars.sensor_orientation, display);
        surface.texture().set_orientation(orientation);
        surface.set_frame_observer(Some(observer(sink.clone(), chars.clone(), display)));

        let mut session = Self {
            ndk,
            _manager: manager,
            chars,
            device: std::ptr::null_mut(),
            session: std::ptr::null_mut(),
            container: std::ptr::null_mut(),
            outputs: Vec::new(),
            preview_target: std::ptr::null_mut(),
            still_target: None,
            request: Mutex::new(std::ptr::null_mut()),
            still: None,
            surface,
            sink,
            size,
            display: AtomicU32::new(display),
            _device_callbacks: Box::new(DeviceCallbacks {
                context: std::ptr::null_mut(),
                on_disconnected: Some(on_disconnected),
                on_error: Some(on_error),
            }),
            _session_callbacks: Box::new(SessionCallbacks {
                context: std::ptr::null_mut(),
                on_closed: Some(on_session_state),
                on_ready: Some(on_session_state),
                on_active: Some(on_session_state),
            }),
        };
        // Whatever fails from here, `Drop` frees what was made.
        session.start(description, preset, stills)?;
        log::info!(
            "camera {} open: preview {}×{}, sensor at {}°, display at {}°{}",
            description.name,
            size.0,
            size.1,
            session.chars.sensor_orientation,
            display,
            if session.still.is_some() {
                ", stills as JPEG"
            } else {
                ""
            }
        );
        Ok(session)
    }

    fn start(
        &mut self,
        description: &CameraDescription,
        preset: ResolutionPreset,
        stills: bool,
    ) -> Result<(), CameraError> {
        let ndk = self.ndk;
        let name = CString::new(description.name.as_str()).map_err(|_| CameraError::NotFound)?;
        check("openCamera", unsafe {
            (ndk.open)(
                self._manager.0,
                name.as_ptr(),
                &mut *self._device_callbacks,
                &mut self.device,
            )
        })
        .map_err(|e| match e {
            CameraError::Platform(why) if why.contains("(-10006)") || why.contains("(-10005)") => {
                CameraError::Platform(format!("{why}: the camera is in use or not allowed"))
            }
            other => other,
        })?;
        check("createCaptureSessionOutputContainer", unsafe {
            (ndk.container_create)(&mut self.container)
        })?;
        let window = self.surface.native_window();
        self.preview_target = self.add_output(window)?;

        // The JPEG stream, of the preview's shape, for stills; none for a scanner.
        let jpeg = choose_size(&self.chars.jpeg_sizes, preset, Some(self.size));
        if let Some(jpeg) = jpeg.filter(|_| stills) {
            match self.still_reader(jpeg) {
                Ok((shared, window)) => {
                    let target = self.add_output(window)?;
                    self.still_target = Some(target);
                    self.still = Some(shared);
                }
                Err(err) => log::warn!("camera: no stills ({err})"),
            }
        }

        check("createCaptureSession", unsafe {
            (ndk.session_create)(
                self.device,
                self.container,
                &*self._session_callbacks,
                &mut self.session,
            )
        })?;
        let mut request = std::ptr::null_mut();
        check("createCaptureRequest", unsafe {
            (ndk.request_create)(self.device, TEMPLATE_PREVIEW, &mut request)
        })?;
        *lock(&self.request) = request;
        check("addTarget", unsafe {
            (ndk.request_add_target)(request, self.preview_target)
        })?;
        self.apply(&Settings::default())
    }

    /// Adds an output for `window` to the container, and answers its request target.
    fn add_output(&mut self, window: P) -> Result<P, CameraError> {
        let ndk = self.ndk;
        let mut output = std::ptr::null_mut();
        check("createCaptureSessionOutput", unsafe {
            (ndk.output_create)(window, &mut output)
        })?;
        self.outputs.push(output);
        check("addOutput", unsafe {
            (ndk.container_add)(self.container, output)
        })?;
        let mut target = std::ptr::null_mut();
        check("createOutputTarget", unsafe {
            (ndk.target_create)(window, &mut target)
        })?;
        Ok(target)
    }

    fn still_reader(&self, size: (u32, u32)) -> Result<(Box<StillShared>, P), CameraError> {
        let images = image_ndk().map_err(|e| CameraError::Platform(e.to_string()))?;
        let mut reader = std::ptr::null_mut();
        check("AImageReader_new (JPEG)", unsafe {
            (images.reader_new)(size.0 as i32, size.1 as i32, FORMAT_JPEG, 2, &mut reader)
        })?;
        let mut shared = Box::new(StillShared {
            reader,
            waiting: Mutex::new(None),
        });
        let mut listener = ImageListener {
            context: (&mut *shared as *mut StillShared).cast(),
            on_image_available: Some(on_still),
        };
        let mut window = std::ptr::null_mut();
        let ok = unsafe {
            (images.reader_listener)(reader, &mut listener) == 0
                && (images.reader_window)(reader, &mut window) == 0
                && !window.is_null()
        };
        if !ok {
            unsafe { (images.reader_delete)(reader) };
            return Err(CameraError::Platform(
                "the JPEG reader has no window".into(),
            ));
        }
        Ok((shared, window))
    }

    pub(super) fn preview_size(&self) -> (u32, u32) {
        self.size
    }

    pub(super) fn orientation(&self) -> FrameOrientation {
        preview_orientation(
            self.chars.lens(),
            self.chars.sensor_orientation,
            self.display.load(Ordering::Relaxed),
        )
    }

    pub(super) fn zoom_range(&self) -> (f32, f32) {
        (1.0, self.chars.max_zoom)
    }

    /// The part of the sensor a zoom level shows: the active array, cropped about its middle.
    fn crop(&self, zoom: f32) -> [i32; 4] {
        let [left, top, width, height] = self.chars.active;
        let zoom = zoom.clamp(1.0, self.chars.max_zoom);
        let (w, h) = ((width as f32 / zoom) as i32, (height as f32 / zoom) as i32);
        [left + (width - w) / 2, top + (height - h) / 2, w, h]
    }

    /// The settings' entries, written into `request`.
    fn write(&self, request: P, settings: &Settings) -> Result<(), CameraError> {
        let ndk = self.ndk;
        let set_u8 = |tag, value: u8| unsafe { (ndk.request_u8)(request, tag, 1, &value) };
        let (ae, flash) = match (settings.flash, self.chars.flash) {
            (_, false) | (FlashMode::Off, _) => (AE_MODE_ON, FLASH_MODE_OFF),
            (FlashMode::Auto, _) => (AE_MODE_ON_AUTO_FLASH, FLASH_MODE_OFF),
            (FlashMode::Always, _) => (AE_MODE_ON_ALWAYS_FLASH, FLASH_MODE_OFF),
            (FlashMode::Torch, _) => (AE_MODE_ON, FLASH_MODE_TORCH),
        };
        check("AE mode", set_u8(CONTROL_AE_MODE, ae))?;
        check("flash mode", set_u8(FLASH_MODE, flash))?;
        let crop = self.crop(settings.zoom);
        if crop[2] > 0 && crop[3] > 0 {
            check("crop region", unsafe {
                (ndk.request_i32)(request, SCALER_CROP_REGION, 4, crop.as_ptr())
            })?;
        }
        let continuous = self.chars.af_modes.contains(&AF_MODE_CONTINUOUS_PICTURE);
        let af = match settings.focus {
            Some(_) if self.chars.af_modes.contains(&AF_MODE_AUTO) => AF_MODE_AUTO,
            _ if continuous => AF_MODE_CONTINUOUS_PICTURE,
            _ => self.chars.af_modes.first().copied().unwrap_or(0),
        };
        if !self.chars.af_modes.is_empty() {
            check("AF mode", set_u8(CONTROL_AF_MODE, af))?;
        }
        // The point's region, a tenth of the crop each way; a weight of nought where there
        // is no point, which the camera ignores.
        let region = match settings.focus {
            Some((x, y)) => {
                let (w, h) = ((crop[2] / 10).max(1), (crop[3] / 10).max(1));
                let cx = crop[0] + (x * crop[2] as f32) as i32;
                let cy = crop[1] + (y * crop[3] as f32) as i32;
                let (l, t) = (
                    (cx - w / 2).clamp(crop[0], crop[0] + crop[2] - w),
                    (cy - h / 2).clamp(crop[1], crop[1] + crop[3] - h),
                );
                [l, t, l + w, t + h, 1000]
            }
            None => [crop[0], crop[1], crop[0] + crop[2], crop[1] + crop[3], 0],
        };
        if self.chars.max_af_regions > 0 {
            check("AF regions", unsafe {
                (ndk.request_i32)(request, CONTROL_AF_REGIONS, 5, region.as_ptr())
            })?;
        }
        if self.chars.max_ae_regions > 0 {
            check("AE regions", unsafe {
                (ndk.request_i32)(request, CONTROL_AE_REGIONS, 5, region.as_ptr())
            })?;
        }
        Ok(())
    }

    pub(super) fn apply(&self, settings: &Settings) -> Result<(), CameraError> {
        let ndk = self.ndk;
        let mut request = *lock(&self.request);
        if request.is_null() || self.session.is_null() {
            return Err(CameraError::NotInitialized);
        }
        self.write(request, settings)?;
        check("AF trigger", unsafe {
            (ndk.request_u8)(request, CONTROL_AF_TRIGGER, 1, &AF_TRIGGER_IDLE)
        })?;
        check("setRepeatingRequest", unsafe {
            (ndk.session_repeating)(
                self.session,
                std::ptr::null_mut(),
                1,
                &mut request,
                std::ptr::null_mut(),
            )
        })?;
        if settings.focus.is_some() && self.chars.af_modes.contains(&AF_MODE_AUTO) {
            // One request that starts the scan at the point.
            self.once(request, |ndk, r| unsafe {
                (ndk.request_u8)(r, CONTROL_AF_TRIGGER, 1, &AF_TRIGGER_CANCEL);
                (ndk.request_u8)(r, CONTROL_AF_TRIGGER, 1, &AF_TRIGGER_START)
            })?;
        }
        Ok(())
    }

    /// Sends `request`, changed by `edit`, once; the repeating request goes on unchanged.
    fn once(
        &self,
        mut request: P,
        edit: impl FnOnce(&Ndk, P) -> Status,
    ) -> Result<(), CameraError> {
        check("trigger", edit(self.ndk, request))?;
        let sent = check("capture", unsafe {
            (self.ndk.session_capture)(
                self.session,
                std::ptr::null_mut(),
                1,
                &mut request,
                std::ptr::null_mut(),
            )
        });
        // Back to idle for the repeating request.
        unsafe { (self.ndk.request_u8)(request, CONTROL_AF_TRIGGER, 1, &AF_TRIGGER_IDLE) };
        sent
    }

    pub(super) fn take_picture(&self, settings: &Settings) -> Result<Vec<u8>, CameraError> {
        let ndk = self.ndk;
        let (Some(still), Some(target)) = (self.still.as_ref(), self.still_target) else {
            return Err(CameraError::Unsupported(
                "this camera takes no pictures (no JPEG stream, or opened to scan codes)",
            ));
        };
        if matches!(settings.flash, FlashMode::Auto | FlashMode::Always) && self.chars.flash {
            // Let the exposure measure with the flash before the picture: a precapture
            // sequence, given the time it usually takes.
            let request = *lock(&self.request);
            let _ = self.once(request, |ndk, r| unsafe {
                (ndk.request_u8)(r, CONTROL_AE_PRECAPTURE_TRIGGER, 1, &PRECAPTURE_START)
            });
            unsafe { (ndk.request_u8)(request, CONTROL_AE_PRECAPTURE_TRIGGER, 1, &0) };
            std::thread::sleep(Duration::from_millis(400));
        }
        let mut request = std::ptr::null_mut();
        check("createCaptureRequest (still)", unsafe {
            (ndk.request_create)(self.device, TEMPLATE_STILL_CAPTURE, &mut request)
        })?;
        let result = (|| {
            check("addTarget (still)", unsafe {
                (ndk.request_add_target)(request, target)
            })?;
            check("addTarget (preview)", unsafe {
                (ndk.request_add_target)(request, self.preview_target)
            })?;
            self.write(request, settings)?;
            // Turned upright the way the preview is, mirror aside: the JPEG says so in its
            // EXIF data.
            let degrees = (self.orientation().quarter_turns() * 90) as i32;
            check("JPEG orientation", unsafe {
                (ndk.request_i32)(request, JPEG_ORIENTATION, 1, &degrees)
            })?;
            check("JPEG quality", unsafe {
                (ndk.request_u8)(request, JPEG_QUALITY, 1, &95)
            })?;
            let (sender, receiver) = mpsc::channel();
            *lock(&still.waiting) = Some(sender);
            check("capture (still)", unsafe {
                (ndk.session_capture)(
                    self.session,
                    std::ptr::null_mut(),
                    1,
                    &mut request,
                    std::ptr::null_mut(),
                )
            })?;
            receiver
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| CameraError::Platform("the picture never came".into()))
        })();
        unsafe { (ndk.request_free)(request) };
        result
    }

    pub(super) fn display_changed(&self) {
        let display = display_rotation();
        if self.display.swap(display, Ordering::Relaxed) != display {
            self.surface.texture().set_orientation(self.orientation());
            self.surface.set_frame_observer(Some(observer(
                self.sink.clone(),
                self.chars.clone(),
                display,
            )));
        }
    }
}

/// The surface's observer: each frame, as a [`CameraImage`], to the sink when it listens.
fn observer(
    sink: Arc<Sink>,
    chars: Characteristics,
    display: u32,
) -> crate::texture_surface::FrameObserver {
    let orientation = preview_orientation(chars.lens(), chars.sensor_orientation, display);
    Box::new(move |frame| {
        if !sink.wants_frames() || frame.planes.len() < 3 {
            return;
        }
        let plane = |i: usize| CameraPlane {
            bytes: frame.planes[i].data,
            row_stride: frame.planes[i].row_stride,
            pixel_stride: frame.planes[i].pixel_stride,
        };
        let planes = [plane(0), plane(1), plane(2)];
        sink.deliver(&CameraImage {
            width: frame.width,
            height: frame.height,
            format: CameraImageFormat::Yuv420,
            planes: &planes,
            orientation,
        });
    })
}

impl Drop for Session {
    fn drop(&mut self) {
        let ndk = self.ndk;
        // No frame reaches the image stream past this.
        self.surface.set_frame_observer(None);
        unsafe {
            if !self.session.is_null() {
                (ndk.session_stop)(self.session);
                (ndk.session_close)(self.session);
            }
            let request = *lock(&self.request);
            if !request.is_null() {
                (ndk.request_free)(request);
            }
            if !self.device.is_null() {
                // Waits for the camera to finish: no callback runs past this.
                (ndk.device_close)(self.device);
            }
            if !self.preview_target.is_null() {
                (ndk.target_free)(self.preview_target);
            }
            if let Some(target) = self.still_target {
                (ndk.target_free)(target);
            }
            for &output in &self.outputs {
                (ndk.output_free)(output);
            }
            if !self.container.is_null() {
                (ndk.container_free)(self.container);
            }
            if let (Some(still), Ok(images)) = (self.still.as_ref(), image_ndk()) {
                (images.reader_listener)(still.reader, std::ptr::null_mut());
                (images.reader_delete)(still.reader);
            }
        }
        log::info!("camera closed");
    }
}
