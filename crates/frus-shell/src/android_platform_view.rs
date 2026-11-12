// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The Android half of **native views** (milestone 794): what [`crate::platform_view`]
//! decided, carried to `FrusPlatformViews` in the bundled dex, and what the views send back —
//! a new image to draw, an event for the widget — carried home.
//!
//! Nothing here runs until the first [`AndroidView`](frus_widgets::AndroidView) is laid out:
//! at start-up only the class loader and the activity are kept (two global references); the
//! class is loaded, its natives wired and its container added to the activity the first time
//! a view is asked for. An application that embeds no native view pays nothing more.
//!
//! Images arrive on the UI thread as a note (`nativeFrame`) that wakes the loop; the loop
//! copies the newest one once per frame, straight into the pixels frus draws.

use std::sync::{Mutex, OnceLock};

use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use jni::sys::jint;
use jni::{JNIEnv, JavaVM};

use crate::platform_view::{Command, PixelRect, ViewTouch};

struct Origin {
    vm: JavaVM,
    loader: GlobalRef,
    activity: GlobalRef,
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
/// The views with a new image since the loop last looked.
static FRAMES: Mutex<Vec<i32>> = Mutex::new(Vec::new());
/// The events the views sent since the loop last looked.
static EVENTS: Mutex<Vec<(i32, String, String)>> = Mutex::new(Vec::new());
/// The factories registered before the class was loaded.
static PENDING: Mutex<Vec<Factory>> = Mutex::new(Vec::new());

struct Factory {
    view_type: String,
    class_name: String,
    dex: Option<&'static [u8]>,
}

fn clear_exception(env: &mut JNIEnv) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

/// Keeps what loading the class will take, at start-up: nothing is loaded yet.
pub(crate) fn remember(env: &mut JNIEnv, loader: &JObject, activity: &JObject) {
    let origin = (|| -> jni::errors::Result<Origin> {
        Ok(Origin {
            vm: env.get_java_vm()?,
            loader: env.new_global_ref(loader)?,
            activity: env.new_global_ref(activity)?,
        })
    })();
    match origin {
        Ok(origin) => {
            let _ = ORIGIN.set(origin);
        }
        Err(err) => {
            clear_exception(env);
            log::warn!("native views unavailable ({err})");
        }
    }
}

/// The class, loaded the first time it is needed; `None` when it cannot be.
fn handle() -> Option<&'static Handle> {
    HANDLE
        .get_or_init(|| {
            let origin = ORIGIN.get()?;
            let mut env = origin.vm.attach_current_thread_permanently().ok()?;
            match load(&mut env, origin) {
                Ok(handle) => {
                    log::info!("native views ready");
                    Some(handle)
                }
                Err(err) => {
                    clear_exception(&mut env);
                    log::warn!("native views unavailable ({err})");
                    None
                }
            }
        })
        .as_ref()
}

fn load(env: &mut JNIEnv, origin: &Origin) -> jni::errors::Result<Handle> {
    let name = env.new_string("dev.frus.input.FrusPlatformViews")?;
    let class: JClass = env
        .call_method(
            origin.loader.as_obj(),
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?
        .into();
    env.register_native_methods(
        &class,
        &[
            jni::NativeMethod {
                name: "nativeFrame".into(),
                sig: "(I)V".into(),
                fn_ptr: native_frame as *mut _,
            },
            jni::NativeMethod {
                name: "nativeEvent".into(),
                sig: "(ILjava/lang/String;Ljava/lang/String;)V".into(),
                fn_ptr: native_event as *mut _,
            },
        ],
    )?;
    env.call_static_method(
        &class,
        "install",
        "(Landroid/app/Activity;)V",
        &[JValue::Object(origin.activity.as_obj())],
    )?;
    let handle = Handle {
        vm: env.get_java_vm()?,
        class: env.new_global_ref(&class)?,
    };
    for factory in std::mem::take(&mut *PENDING.lock().unwrap()) {
        register_now(env, &handle, &factory);
    }
    Ok(handle)
}

/// Runs `call` against the class; a failure is logged and its exception cleared.
fn with_class(what: &str, call: impl FnOnce(&mut JNIEnv, &JClass) -> jni::errors::Result<()>) {
    let Some(handle) = handle() else {
        return;
    };
    let Ok(mut env) = handle.vm.attach_current_thread_permanently() else {
        return;
    };
    let class: &JClass = handle.class.as_obj().into();
    let result = env.with_local_frame(16, |env| call(env, class));
    if let Err(err) = result {
        clear_exception(&mut env);
        log::warn!("native views: {what} failed ({err})");
    }
}

/// A plugin's factory for `view_type`: `class_name`, loaded from `dex` (or from frus's own
/// dex, for `None`). Registered when the class is loaded, which may be now.
pub(crate) fn register(view_type: &str, class_name: &str, dex: Option<&'static [u8]>) {
    let factory = Factory {
        view_type: view_type.to_owned(),
        class_name: class_name.to_owned(),
        dex,
    };
    match HANDLE.get() {
        Some(Some(handle)) => {
            let Ok(mut env) = handle.vm.attach_current_thread_permanently() else {
                return;
            };
            register_now(&mut env, handle, &factory);
        }
        Some(None) => log::warn!(
            "native views: {} cannot be registered, native views are unavailable",
            factory.view_type
        ),
        None => PENDING.lock().unwrap().push(factory),
    }
}

fn register_now(env: &mut JNIEnv, handle: &Handle, factory: &Factory) {
    let class: &JClass = handle.class.as_obj().into();
    let result = env.with_local_frame(8, |env| -> jni::errors::Result<Option<String>> {
        let view_type = env.new_string(&factory.view_type)?;
        let class_name = env.new_string(&factory.class_name)?;
        let dex = match factory.dex {
            // SAFETY: the bytes are 'static and only read.
            Some(bytes) => JObject::from(unsafe {
                env.new_direct_byte_buffer(bytes.as_ptr() as *mut u8, bytes.len())
            }?),
            None => JObject::null(),
        };
        let failure = env
            .call_static_method(
                class,
                "register",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/nio/ByteBuffer;)Ljava/lang/String;",
                &[
                    JValue::Object(&view_type),
                    JValue::Object(&class_name),
                    JValue::Object(&dex),
                ],
            )?
            .l()?;
        if failure.is_null() {
            return Ok(None);
        }
        let failure: JString = failure.into();
        let why: String = env.get_string(&failure)?.into();
        Ok(Some(why))
    });
    match result {
        Ok(None) => log::info!("native views: {} registered", factory.view_type),
        Ok(Some(why)) => log::warn!(
            "native views: the factory for {} could not be made ({why})",
            factory.view_type
        ),
        Err(err) => {
            clear_exception(env);
            log::warn!(
                "native views: registering {} failed ({err})",
                factory.view_type
            );
        }
    }
}

/// Carries out the frame's commands, in order.
pub(crate) fn run(commands: &[Command]) {
    if commands.is_empty() {
        return;
    }
    with_class("a frame's changes", |env, class| {
        for command in commands {
            match command {
                Command::Create {
                    view,
                    view_type,
                    params,
                    at,
                } => {
                    let view_type = env.new_string(view_type)?;
                    let params = env.new_string(params)?;
                    env.call_static_method(
                        class,
                        "create",
                        "(ILjava/lang/String;Ljava/lang/String;IIII)V",
                        &[
                            JValue::Int(*view),
                            JValue::Object(&view_type),
                            JValue::Object(&params),
                            JValue::Int(at.x),
                            JValue::Int(at.y),
                            JValue::Int(at.w),
                            JValue::Int(at.h),
                        ],
                    )?;
                    env.delete_local_ref(view_type)?;
                    env.delete_local_ref(params)?;
                }
                Command::Place { view, at } => {
                    env.call_static_method(
                        class,
                        "place",
                        "(IIIII)V",
                        &[
                            JValue::Int(*view),
                            JValue::Int(at.x),
                            JValue::Int(at.y),
                            JValue::Int(at.w),
                            JValue::Int(at.h),
                        ],
                    )?;
                }
                Command::Dispose { view } => {
                    env.call_static_method(class, "dispose", "(I)V", &[JValue::Int(*view)])?;
                }
                Command::Invoke {
                    view,
                    method,
                    arguments,
                } => {
                    let method = env.new_string(method)?;
                    let arguments = env.new_string(arguments)?;
                    env.call_static_method(
                        class,
                        "invoke",
                        "(ILjava/lang/String;Ljava/lang/String;)V",
                        &[
                            JValue::Int(*view),
                            JValue::Object(&method),
                            JValue::Object(&arguments),
                        ],
                    )?;
                    env.delete_local_ref(method)?;
                    env.delete_local_ref(arguments)?;
                }
            }
        }
        Ok(())
    });
}

/// Hands a touch to its view.
pub(crate) fn touch(touch: &ViewTouch) {
    with_class("a touch", |env, class| {
        let ids = env.new_int_array(touch.ids.len() as i32)?;
        env.set_int_array_region(&ids, 0, &touch.ids)?;
        let xy = env.new_float_array(touch.xy.len() as i32)?;
        env.set_float_array_region(&xy, 0, &touch.xy)?;
        env.call_static_method(
            class,
            "touch",
            "(III[I[F)V",
            &[
                JValue::Int(touch.view),
                JValue::Int(touch.action),
                JValue::Int(touch.index as i32),
                JValue::Object(&ids),
                JValue::Object(&xy),
            ],
        )?;
        Ok(())
    });
}

/// The activity was resumed (`true`) or paused: the views hear it.
pub(crate) fn lifecycle(resumed: bool) {
    if !matches!(HANDLE.get(), Some(Some(_))) {
        return;
    }
    with_class("a pause or a resume", |env, class| {
        env.call_static_method(
            class,
            "lifecycle",
            "(Z)V",
            &[JValue::Bool(u8::from(resumed))],
        )?;
        Ok(())
    });
}

/// A press went to frus: a view holding the focus lets it go.
pub(crate) fn clear_focus() {
    if !matches!(HANDLE.get(), Some(Some(_))) {
        return;
    }
    with_class("letting the focus go", |env, class| {
        env.call_static_method(class, "clearFocus", "()V", &[])?;
        Ok(())
    });
}

/// The views with a new image since the last call.
pub(crate) fn take_frames() -> Vec<i32> {
    let mut frames = std::mem::take(&mut *FRAMES.lock().unwrap());
    frames.sort_unstable();
    frames.dedup();
    frames
}

/// The events the views sent since the last call: the view, the name, the data.
pub(crate) fn take_events() -> Vec<(i32, String, String)> {
    std::mem::take(&mut *EVENTS.lock().unwrap())
}

/// The newest image of view `view`, which is `at` large, as straight RGBA: `None` when there
/// is none (it was copied already, or is from before a resize).
pub(crate) fn copy_frame(view: i32, at: PixelRect) -> Option<frus_widgets::ImageHandle> {
    let (w, h) = (at.w.max(1) as usize, at.h.max(1) as usize);
    let mut pixels = vec![0u8; w * h * 4];
    let mut size = [0i32; 2];
    let mut copied = false;
    with_class("copying an image", |env, class| {
        // SAFETY: `pixels` outlives the call, and nothing else touches it meanwhile.
        let into = unsafe { env.new_direct_byte_buffer(pixels.as_mut_ptr(), pixels.len()) }?;
        let dims = env.new_int_array(2)?;
        copied = env
            .call_static_method(
                class,
                "copyFrame",
                "(ILjava/nio/ByteBuffer;[I)Z",
                &[
                    JValue::Int(view),
                    JValue::Object(&into),
                    JValue::Object(&dims),
                ],
            )?
            .z()?;
        if copied {
            env.get_int_array_region(&dims, 0, &mut size)?;
        }
        Ok(())
    });
    let (sw, sh) = (size[0].max(0) as usize, size[1].max(0) as usize);
    if !copied || sw == 0 || sh == 0 || sw * sh * 4 > pixels.len() {
        return None;
    }
    pixels.truncate(sw * sh * 4);
    crate::platform_view::packed_straight(&mut pixels);
    Some(frus_widgets::ImageData::from_rgba(sw as u32, sh as u32, pixels).into_handle())
}

/// The note that view `id` has a new image: queued, and the loop woken for a frame.
extern "system" fn native_frame(_env: JNIEnv, _class: JClass, id: jint) {
    FRAMES.lock().unwrap().push(id);
    crate::android_ime::wake();
}

/// An event view `id` sent its widget: queued, and the loop woken.
extern "system" fn native_event(
    mut env: JNIEnv,
    _class: JClass,
    id: jint,
    name: JString,
    data: JString,
) {
    let mut text = |s: &JString| -> String {
        if s.is_null() {
            String::new()
        } else {
            env.get_string(s).map(Into::into).unwrap_or_default()
        }
    };
    let name = text(&name);
    let data = text(&data);
    EVENTS.lock().unwrap().push((id, name, data));
    crate::android_ime::wake();
}
