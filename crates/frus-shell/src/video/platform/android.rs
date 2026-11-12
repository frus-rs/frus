// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Android**: the system's `MediaPlayer`, through `FrusVideo` (a dex of its own), its pictures
//! drawn into a [`TextureSurface`].
//!
//! `MediaPlayer` reads files and `http(s)` addresses, HLS playlists among them, plays the sound
//! and keeps the pictures in step with it; frus only gives it a surface. The surface is made
//! when the clip is open and its size known (the player's `onPrepared`, reported as an event),
//! at that size, and handed to the player as an `android.view.Surface`: from then on the
//! decoder's pictures land in the texture surface's image reader and become the texture's,
//! with nothing of frus's in between.
//!
//! At start-up only the class loader is kept (by [`remember`]); the class is loaded and its
//! native wired the first time a clip is opened. As everywhere on this side, every failed JNI
//! call clears the exception it left.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use jni::objects::{GlobalRef, JClass, JObject, JValue};
use jni::sys::{jint, jlong};
use jni::{JNIEnv, JavaVM};

use crate::{SurfaceFormat, TextureSurface};
use frus_widgets::YuvColorSpace;

use super::super::controller::{DataSource, DurationRange, Event, Player, Sink, VideoError};

struct Origin {
    vm: JavaVM,
    loader: GlobalRef,
}

struct Handle {
    vm: JavaVM,
    class: GlobalRef,
}

// SAFETY: JavaVM and the GlobalRefs are shareable across threads, per the JNI contract.
unsafe impl Send for Origin {}
unsafe impl Sync for Origin {}
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

static ORIGIN: OnceLock<Origin> = OnceLock::new();
static HANDLE: OnceLock<Option<Handle>> = OnceLock::new();

/// What one clip's events need: where they go, its duration, and its surface once made.
struct Shared {
    sink: Arc<Sink>,
    duration: AtomicU64,
    surface: Mutex<Option<TextureSurface>>,
}

/// The clips open, by the handle their Java half reports with.
static CLIPS: Mutex<Option<HashMap<u64, Arc<Shared>>>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);

fn clear_exception(env: &mut JNIEnv) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

/// Keeps what loading the class will take, at start-up: nothing is loaded yet.
pub(crate) fn remember(env: &mut JNIEnv, loader: &JObject, _activity: &JObject) {
    let origin = (|| -> jni::errors::Result<Origin> {
        Ok(Origin {
            vm: env.get_java_vm()?,
            loader: env.new_global_ref(loader)?,
        })
    })();
    match origin {
        Ok(origin) => {
            let _ = ORIGIN.set(origin);
        }
        Err(err) => {
            clear_exception(env);
            log::warn!("video unavailable ({err})");
        }
    }
}

fn handle() -> Option<&'static Handle> {
    HANDLE
        .get_or_init(|| {
            let origin = ORIGIN.get()?;
            let mut env = origin.vm.attach_current_thread_permanently().ok()?;
            match load(&mut env, origin) {
                Ok(handle) => Some(handle),
                Err(err) => {
                    clear_exception(&mut env);
                    log::warn!("video unavailable ({err})");
                    None
                }
            }
        })
        .as_ref()
}

/// `FrusVideo`, in a dex of its own: compiled in with the `video` feature only, so an
/// application that plays no video carries none of it (`scripts/build-video-dex.sh`).
static VIDEO_DEX: &[u8] = include_bytes!("../../../assets/frus_video.dex");

fn load(env: &mut JNIEnv, origin: &Origin) -> jni::errors::Result<Handle> {
    // SAFETY: the bytes are static; the buffer only reads them.
    let buffer =
        unsafe { env.new_direct_byte_buffer(VIDEO_DEX.as_ptr() as *mut u8, VIDEO_DEX.len()) }?;
    let loader = env.new_object(
        "dalvik/system/InMemoryDexClassLoader",
        "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
        &[
            JValue::Object(&buffer),
            JValue::Object(origin.loader.as_obj()),
        ],
    )?;
    let name = env.new_string("dev.frus.video.FrusVideo")?;
    let class: JClass = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?
        .into();
    env.register_native_methods(
        &class,
        &[jni::NativeMethod {
            name: "nativeEvent".into(),
            sig: "(JIJJ)V".into(),
            fn_ptr: native_event as *mut _,
        }],
    )?;
    Ok(Handle {
        vm: env.get_java_vm()?,
        class: env.new_global_ref(&class)?,
    })
}

pub(crate) fn open(source: &DataSource, sink: Arc<Sink>) -> Result<Box<dyn Player>, VideoError> {
    let location = match source {
        DataSource::Asset(bytes) => super::asset_file(bytes)?.to_string_lossy().into_owned(),
        DataSource::File(path) => {
            if !path.exists() {
                return Err(VideoError::Source(format!(
                    "{}: no such file",
                    path.display()
                )));
            }
            path.to_string_lossy().into_owned()
        }
        DataSource::Network(url) => url.clone(),
    };
    let handle = handle().ok_or(VideoError::Unsupported(
        "the Android video bridge could not be loaded",
    ))?;
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let shared = Arc::new(Shared {
        sink,
        duration: AtomicU64::new(0),
        surface: Mutex::new(None),
    });
    // Known before the Java half can report anything.
    if let Ok(mut clips) = CLIPS.lock() {
        clips.get_or_insert_with(HashMap::new).insert(id, shared);
    }
    let opened = (|| -> Result<GlobalRef, String> {
        let mut env = handle
            .vm
            .attach_current_thread_permanently()
            .map_err(|e| e.to_string())?;
        let result = env.with_local_frame(8, |env| -> jni::errors::Result<GlobalRef> {
            let class: &JClass = handle.class.as_obj().into();
            let location = env.new_string(&location)?;
            let video = env
                .call_static_method(
                    class,
                    "open",
                    "(JLjava/lang/String;)Ldev/frus/video/FrusVideo;",
                    &[JValue::Long(id as jlong), JValue::Object(&location)],
                )?
                .l()?;
            env.new_global_ref(video)
        });
        result.map_err(|err| {
            let why = java_exception(&mut env).unwrap_or_else(|| err.to_string());
            clear_exception(&mut env);
            why
        })
    })();
    match opened {
        Ok(java) => Ok(Box::new(MediaPlayer { id, java })),
        Err(why) => {
            forget(id);
            Err(VideoError::Source(format!("{location}: {why}")))
        }
    }
}

/// The message of the Java exception pending, if any (the exception is cleared).
fn java_exception(env: &mut JNIEnv) -> Option<String> {
    if !env.exception_check().unwrap_or(false) {
        return None;
    }
    let exception = env.exception_occurred().ok()?;
    let _ = env.exception_clear();
    let text = env
        .call_method(&exception, "toString", "()Ljava/lang/String;", &[])
        .and_then(|v| v.l())
        .ok()?;
    let text: String = env.get_string(&text.into()).ok()?.into();
    Some(text)
}

fn forget(id: u64) -> Option<Arc<Shared>> {
    CLIPS
        .lock()
        .ok()
        .and_then(|mut clips| clips.as_mut().and_then(|c| c.remove(&id)))
}

fn shared(id: u64) -> Option<Arc<Shared>> {
    CLIPS
        .lock()
        .ok()
        .and_then(|clips| clips.as_ref().and_then(|c| c.get(&id).cloned()))
}

struct MediaPlayer {
    id: u64,
    java: GlobalRef,
}

impl MediaPlayer {
    fn call(&self, name: &str, sig: &str, args: &[JValue]) {
        let Some(handle) = handle() else {
            return;
        };
        let Ok(mut env) = handle.vm.attach_current_thread_permanently() else {
            return;
        };
        if let Err(err) = env.call_method(self.java.as_obj(), name, sig, args) {
            clear_exception(&mut env);
            log::warn!("video: {name} failed ({err})");
        }
    }
}

impl Player for MediaPlayer {
    fn play(&self) {
        self.call("play", "()V", &[]);
    }
    fn pause(&self) {
        self.call("pause", "()V", &[]);
    }
    fn seek_to(&self, position: Duration) {
        self.call(
            "seekTo",
            "(J)V",
            &[JValue::Long(position.as_millis() as jlong)],
        );
    }
    fn set_volume(&self, volume: f32) {
        self.call("setVolume", "(F)V", &[JValue::Float(volume)]);
    }
    fn set_looping(&self, looping: bool) {
        self.call("setLooping", "(Z)V", &[JValue::Bool(looping as u8)]);
    }
    fn set_playback_speed(&self, speed: f32) {
        self.call("setSpeed", "(F)V", &[JValue::Float(speed)]);
    }
}

impl Drop for MediaPlayer {
    fn drop(&mut self) {
        // The player first, so that nothing draws into the surface once it goes.
        self.call("release", "()V", &[]);
        if let Some(shared) = forget(self.id) {
            if let Ok(mut surface) = shared.surface.lock() {
                drop(surface.take());
            }
        }
    }
}

const OPEN: jint = 1;
const SIZE: jint = 2;
const POSITION: jint = 3;
const DOWNLOADED: jint = 4;
const WAITING: jint = 5;
const END: jint = 6;
const ERROR: jint = 7;

/// `FrusVideo.nativeEvent(long, int, long, long)`, on the main thread.
extern "system" fn native_event(
    mut env: JNIEnv,
    this: JObject,
    id: jlong,
    kind: jint,
    a: jlong,
    b: jlong,
) {
    let Some(clip) = shared(id as u64) else {
        return;
    };
    let ms = |ms: jlong| Duration::from_millis(ms.max(0) as u64);
    match kind {
        OPEN => {
            let (width, height) = ((b >> 32) as u32, (b & 0xffff_ffff) as u32);
            clip.duration.store(a.max(0) as u64, Ordering::Relaxed);
            let texture = if width > 0 && height > 0 {
                attach_surface(&mut env, &this, &clip, width, height)
            } else {
                None
            };
            clip.sink.send(Event::Ready {
                duration: ms(a),
                width,
                height,
                texture,
            });
        }
        SIZE => clip.sink.send(Event::Size {
            width: a as u32,
            height: b as u32,
        }),
        POSITION => clip.sink.send(Event::Position(ms(a))),
        DOWNLOADED => {
            let duration = clip.duration.load(Ordering::Relaxed);
            let end = duration * (a.clamp(0, 100) as u64) / 100;
            clip.sink.send(Event::Buffered(if end == 0 {
                Vec::new()
            } else {
                vec![DurationRange::new(Duration::ZERO, ms(end as jlong))]
            }));
        }
        WAITING => clip.sink.send(Event::Buffering(a != 0)),
        END => clip.sink.send(Event::Completed),
        ERROR => clip.sink.send(Event::Error(error_text(a as i32, b as i32))),
        _ => {}
    }
}

/// Makes the clip's texture surface at the picture's size and gives it to the player.
fn attach_surface(
    env: &mut JNIEnv,
    video: &JObject,
    clip: &Shared,
    width: u32,
    height: u32,
) -> Option<frus_widgets::TextureId> {
    let surface = match TextureSurface::new(width, height, SurfaceFormat::Yuv420) {
        Ok(surface) => surface,
        Err(err) => {
            clip.sink
                .send(Event::Error(format!("no surface for the video: {err}")));
            return None;
        }
    };
    // High-definition video is BT.709, standard-definition BT.601.
    surface.set_color_space(if height >= 720 {
        YuvColorSpace::Bt709Limited
    } else {
        YuvColorSpace::Bt601Limited
    });
    let java = match surface.java_surface() {
        Ok(java) => java,
        Err(err) => {
            clip.sink
                .send(Event::Error(format!("no surface for the video: {err}")));
            return None;
        }
    };
    // SAFETY: a global reference the texture surface owns, alive while it is; a `JObject`
    // deletes nothing when dropped.
    let java = unsafe { JObject::from_raw(java as jni::sys::jobject) };
    let set = env.call_method(
        video,
        "setSurface",
        "(Landroid/view/Surface;)V",
        &[JValue::Object(&java)],
    );
    if let Err(err) = set {
        clear_exception(env);
        clip.sink
            .send(Event::Error(format!("setSurface failed ({err})")));
        return None;
    }
    let id = surface.id();
    if let Ok(mut slot) = clip.surface.lock() {
        *slot = Some(surface);
    }
    Some(id)
}

/// `MediaPlayer`'s error codes, in words.
fn error_text(what: i32, extra: i32) -> String {
    let what = match what {
        100 => "the media server died",
        _ => "playback failed",
    };
    let extra = match extra {
        -1004 => "a read or network error",
        -1007 => "the stream is malformed",
        -1010 => "the format is not supported",
        -110 => "it timed out",
        _ => "",
    };
    if extra.is_empty() {
        format!("MediaPlayer: {what}")
    } else {
        format!("MediaPlayer: {what}: {extra}")
    }
}
