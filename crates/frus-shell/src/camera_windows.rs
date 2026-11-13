// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The camera on **Windows** (milestone 798): Media Foundation's video capture devices.
//!
//! A thread of the session's own opens the device, asks its source reader for NV12 frames at
//! the size closest to the preset, and reads them one after the other, pushing each into the
//! preview's texture (converted to RGB on the GPU) and handing it to the image stream. A
//! picture is the next frame, converted and encoded as JPEG on that thread. The thread ends
//! with the session, at the latest one frame after it is asked to.
//!
//! Webcams have no flash, zoom or focus point Media Foundation reaches simply: those answer
//! [`CameraError::Unsupported`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use frus_widgets::{ExternalTexture, FrameOrientation, TextureId, YuvColorSpace, YuvPlane};
use windows::core::{GUID, PWSTR};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};

use super::{
    lock, CameraDescription, CameraError, CameraImage, CameraImageFormat, CameraLensDirection,
    CameraPlane, FlashMode, ResolutionPreset, Settings, Sink,
};

fn platform(what: &str, err: windows::core::Error) -> CameraError {
    CameraError::Platform(format!("{what}: {err}"))
}

/// Media Foundation, started once for the process, on a COM thread.
fn start() -> Result<(), CameraError> {
    unsafe {
        // Already initialised on this thread, in either model, is fine.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|e| platform("MFStartup", e))
    }
}

/// The video capture devices, with their friendly names and their symbolic links.
fn devices() -> Result<Vec<(String, String, IMFActivate)>, CameraError> {
    start()?;
    unsafe {
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 1).map_err(|e| platform("MFCreateAttributes", e))?;
        let attributes = attributes.ok_or_else(|| CameraError::Platform("no attributes".into()))?;
        attributes
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(|e| platform("SetGUID", e))?;
        let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        MFEnumDeviceSources(&attributes, &mut list, &mut count)
            .map_err(|e| platform("MFEnumDeviceSources", e))?;
        let mut found = Vec::new();
        for i in 0..count as usize {
            let Some(device) = (*list.add(i)).take() else {
                continue;
            };
            let name = string(&device, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME).unwrap_or_default();
            let link = string(
                &device,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
            )
            .unwrap_or_else(|| name.clone());
            found.push((name, link, device));
        }
        if !list.is_null() {
            CoTaskMemFree(Some(list as *const _));
        }
        Ok(found)
    }
}

unsafe fn string(device: &IMFActivate, key: &GUID) -> Option<String> {
    let mut value = PWSTR::null();
    let mut len = 0u32;
    device.GetAllocatedString(key, &mut value, &mut len).ok()?;
    let text = value.to_string().ok();
    CoTaskMemFree(Some(value.0 as *const _));
    text
}

pub(super) fn cameras() -> Result<Vec<CameraDescription>, CameraError> {
    Ok(devices()?
        .into_iter()
        .map(|(_, link, _)| CameraDescription {
            name: link,
            lens_direction: CameraLensDirection::External,
            sensor_orientation: 0,
        })
        .collect())
}

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

/// An open webcam: the thread reading it.
pub(super) struct Session {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    size: (u32, u32),
    /// Where the next frame goes as a picture, when one is wanted.
    picture: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
}

impl Session {
    pub(super) fn open(
        description: &CameraDescription,
        preset: ResolutionPreset,
        preview: &Preview,
        sink: Arc<Sink>,
        _stills: bool,
    ) -> Result<Self, CameraError> {
        let stop = Arc::new(AtomicBool::new(false));
        let picture: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>> = Arc::default();
        let (ready, opened) = mpsc::channel();
        let name = description.name.clone();
        let texture = preview.texture.clone();
        let (stopping, wanted) = (stop.clone(), picture.clone());
        let thread = std::thread::Builder::new()
            .name("frus-camera".into())
            .spawn(move || {
                let reader = match open_reader(&name, preset) {
                    Ok((reader, size)) => {
                        let _ = ready.send(Ok(size));
                        (reader, size)
                    }
                    Err(err) => {
                        let _ = ready.send(Err(err));
                        return;
                    }
                };
                read(reader.0, reader.1, &texture, &sink, &stopping, &wanted);
            })
            .map_err(|e| CameraError::Platform(e.to_string()))?;
        let size = opened
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| CameraError::Platform("the webcam did not open".into()))??;
        log::info!(
            "camera {} open: {}×{} NV12",
            description.name,
            size.0,
            size.1
        );
        Ok(Self {
            stop,
            thread: Some(thread),
            size,
            picture,
        })
    }

    pub(super) fn preview_size(&self) -> (u32, u32) {
        self.size
    }

    pub(super) fn orientation(&self) -> FrameOrientation {
        FrameOrientation::UPRIGHT
    }

    pub(super) fn zoom_range(&self) -> (f32, f32) {
        (1.0, 1.0)
    }

    pub(super) fn apply(&self, settings: &Settings) -> Result<(), CameraError> {
        if matches!(settings.flash, FlashMode::Always | FlashMode::Torch) {
            return Err(CameraError::Unsupported("a webcam has no flash"));
        }
        if settings.zoom != 1.0 {
            return Err(CameraError::Unsupported("no zoom on a webcam yet"));
        }
        if settings.focus.is_some() {
            return Err(CameraError::Unsupported("no focus point on a webcam yet"));
        }
        Ok(())
    }

    pub(super) fn take_picture(&self, _settings: &Settings) -> Result<Vec<u8>, CameraError> {
        let (sender, receiver) = mpsc::channel();
        *lock(&self.picture) = Some(sender);
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| CameraError::Platform("no frame came for the picture".into()))
    }

    pub(super) fn display_changed(&self) {}
}

impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        log::info!("camera closed");
    }
}

/// Opens the device named by its symbolic link, NV12 at the size closest to `preset`.
fn open_reader(
    link: &str,
    preset: ResolutionPreset,
) -> Result<(IMFSourceReader, (u32, u32)), CameraError> {
    let device = devices()?
        .into_iter()
        .find(|(_, l, _)| l == link)
        .map(|(_, _, d)| d)
        .ok_or(CameraError::NotFound)?;
    unsafe {
        let source: IMFMediaSource = device
            .ActivateObject()
            .map_err(|e| platform("ActivateObject", e))?;
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 1).map_err(|e| platform("MFCreateAttributes", e))?;
        let attributes = attributes.ok_or_else(|| CameraError::Platform("no attributes".into()))?;
        // Lets the reader convert the device's format (often MJPG or YUY2) to NV12.
        attributes
            .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
            .map_err(|e| platform("SetUINT32", e))?;
        let reader = MFCreateSourceReaderFromMediaSource(&source, &attributes)
            .map_err(|e| platform("MFCreateSourceReaderFromMediaSource", e))?;
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        let mut sizes = Vec::new();
        let mut index = 0;
        while let Ok(native) = reader.GetNativeMediaType(stream, index) {
            if let Ok(packed) = native.GetUINT64(&MF_MT_FRAME_SIZE) {
                sizes.push(((packed >> 32) as u32, packed as u32));
            }
            index += 1;
        }
        let size = super::choose_size(&sizes, preset, None).unwrap_or((640, 480));
        let wanted = MFCreateMediaType().map_err(|e| platform("MFCreateMediaType", e))?;
        let pack = ((size.0 as u64) << 32) | size.1 as u64;
        wanted
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .and_then(|_| wanted.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12))
            .and_then(|_| wanted.SetUINT64(&MF_MT_FRAME_SIZE, pack))
            .map_err(|e| platform("media type", e))?;
        reader
            .SetCurrentMediaType(stream, None, &wanted)
            .map_err(|e| platform("SetCurrentMediaType", e))?;
        // The size it settled on.
        let current = reader
            .GetCurrentMediaType(stream)
            .map_err(|e| platform("GetCurrentMediaType", e))?;
        let packed = current.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(pack);
        Ok((reader, ((packed >> 32) as u32, packed as u32)))
    }
}

/// Reads frames until `stop`.
fn read(
    reader: IMFSourceReader,
    size: (u32, u32),
    texture: &ExternalTexture,
    sink: &Sink,
    stop: &AtomicBool,
    picture: &Mutex<Option<mpsc::Sender<Vec<u8>>>>,
) {
    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    let (w, h) = size;
    while !stop.load(Ordering::Acquire) {
        let mut flags = 0u32;
        let mut sample = None;
        let read = unsafe {
            reader.ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))
        };
        if let Err(err) = read {
            log::warn!("camera: {err}");
            break;
        }
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0
            || flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0
        {
            log::warn!("camera: the webcam stopped (flags {flags:#x})");
            break;
        }
        let Some(sample) = sample else {
            continue;
        };
        let Ok(buffer) = (unsafe { sample.ConvertToContiguousBuffer() }) else {
            continue;
        };
        let (mut data, mut len) = (std::ptr::null_mut(), 0u32);
        if unsafe { buffer.Lock(&mut data, None, Some(&mut len)) }.is_err() || data.is_null() {
            continue;
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, len as usize) };
        // A plain buffer holds the rows packed; its length says the stride.
        let pitch = (len as usize * 2 / 3 / h.max(1) as usize).max(w as usize);
        let luma_len = pitch * h as usize;
        if bytes.len() >= luma_len + pitch * h.div_ceil(2) as usize {
            let (y, uv) = bytes.split_at(luma_len);
            let _ = texture.push_nv12(
                w,
                h,
                YuvPlane::packed(y, pitch),
                YuvPlane::packed(uv, pitch),
                YuvColorSpace::Bt601Limited,
            );
            let planes = [
                CameraPlane {
                    bytes: y,
                    row_stride: pitch,
                    pixel_stride: 1,
                },
                CameraPlane {
                    bytes: uv,
                    row_stride: pitch,
                    pixel_stride: 2,
                },
            ];
            sink.deliver(&CameraImage {
                width: w,
                height: h,
                format: CameraImageFormat::Nv12,
                planes: &planes,
                orientation: FrameOrientation::UPRIGHT,
            });
            if let Some(sender) = lock(picture).take() {
                let _ = sender.send(super::nv12_to_jpeg(y, uv, w, h, pitch));
            }
        }
        unsafe {
            let _ = buffer.Unlock();
        }
    }
}
