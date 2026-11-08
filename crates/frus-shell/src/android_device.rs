// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Haptics and connectivity on Android** (milestone 777), through `FrusHaptics` and
//! `FrusConnectivity` in the bundled dex.
//!
//! Haptics: the system's touch feedback (`View.performHapticFeedback`), performed on the Java
//! UI thread, the vibrator standing in where the system has no such effect. The person's
//! touch-feedback setting is read first, here, so that neither path vibrates when it is off.
//!
//! Connectivity: `ConnectivityManager`'s default-network callback, which calls
//! `nativeChanged` from a thread of the system's; the value goes to
//! [`frus_widgets::report_connectivity`], which asks for a frame.
//!
//! As with the other bridges, every failure clears the Java exception it left.

use std::sync::OnceLock;

use jni::objects::{GlobalRef, JClass, JObject, JValue};
use jni::sys::jint;
use jni::{JNIEnv, JavaVM};

use crate::haptics::{Haptic, HapticsError};
use frus_widgets::Connectivity;

struct Handle {
    vm: JavaVM,
    haptics: Option<GlobalRef>,
    connectivity: Option<GlobalRef>,
    activity: GlobalRef,
    sdk: i32,
}

// SAFETY: JavaVM and the GlobalRefs are shareable across threads, per the JNI contract.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

static HANDLE: OnceLock<Handle> = OnceLock::new();

fn clear_exception(env: &mut JNIEnv) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

fn load<'a>(
    env: &mut JNIEnv<'a>,
    loader: &JObject,
    name: &str,
) -> Result<JObject<'a>, jni::errors::Error> {
    let name = env.new_string(name)?;
    env.call_method(
        loader,
        "loadClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        &[JValue::Object(&name)],
    )?
    .l()
}

/// Loads both classes from the bundled dex, through the loader the input bridge made, and wires
/// the connectivity callback's native. A failure costs only the half that failed.
pub(crate) fn install(env: &mut JNIEnv, loader: &JObject, activity: &JObject) {
    let haptics = match load(env, loader, "dev.frus.input.FrusHaptics")
        .and_then(|class| env.new_global_ref(&class))
    {
        Ok(class) => Some(class),
        Err(err) => {
            clear_exception(env);
            log::warn!("haptics unavailable ({err})");
            None
        }
    };
    let connectivity =
        match load(env, loader, "dev.frus.input.FrusConnectivity").and_then(|class| {
            let class: JClass = class.into();
            env.register_native_methods(
                &class,
                &[jni::NativeMethod {
                    name: "nativeChanged".into(),
                    sig: "(I)V".into(),
                    fn_ptr: native_changed as *mut _,
                }],
            )?;
            env.new_global_ref(&class)
        }) {
            Ok(class) => Some(class),
            Err(err) => {
                clear_exception(env);
                log::warn!("connectivity unavailable ({err})");
                None
            }
        };
    let sdk = match env
        .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")
        .and_then(|v| v.i())
    {
        Ok(sdk) => sdk,
        Err(_) => {
            clear_exception(env);
            0
        }
    };
    let (Ok(vm), Ok(activity)) = (env.get_java_vm(), env.new_global_ref(activity)) else {
        clear_exception(env);
        return;
    };
    let _ = HANDLE.set(Handle {
        vm,
        haptics,
        connectivity,
        activity,
        sdk,
    });
}

/// Calls a static method of `class` with the activity, on the calling thread.
fn call_static(
    class: &GlobalRef,
    work: impl FnOnce(&mut JNIEnv, &JClass, &JObject) -> Result<i32, jni::errors::Error>,
) -> Option<i32> {
    let handle = HANDLE.get()?;
    let mut env = handle.vm.attach_current_thread_permanently().ok()?;
    let class: &JClass = class.as_obj().into();
    let result = work(&mut env, class, handle.activity.as_obj());
    if result.is_err() {
        clear_exception(&mut env);
    }
    result.ok()
}

/// What `FrusHaptics` answers: performed (or posted to be), turned off by the person, no
/// permission for the vibrator, no vibrator.
const DONE: i32 = 0;
const OFF: i32 = 1;
const NO_PERMISSION: i32 = 2;
const NO_VIBRATOR: i32 = 3;

fn answer(code: Option<i32>) -> Result<(), HapticsError> {
    match code {
        Some(DONE) | Some(OFF) => Ok(()),
        Some(NO_PERMISSION) => Err(HapticsError::Failed(
            "the manifest does not declare android.permission.VIBRATE".into(),
        )),
        Some(NO_VIBRATOR) => Err(HapticsError::Failed("the device has no vibrator".into())),
        Some(other) => Err(HapticsError::Failed(format!("unexpected answer {other}"))),
        None => Err(HapticsError::Failed(
            "the Android bridge is not loaded".into(),
        )),
    }
}

/// Performs `kind` with the system's touch feedback, the vibrator standing in.
pub(crate) fn haptic(kind: Haptic) -> Result<(), HapticsError> {
    let handle = HANDLE
        .get()
        .ok_or_else(|| HapticsError::Failed("the Android bridge is not loaded".into()))?;
    let class = handle.haptics.as_ref().ok_or(HapticsError::Unsupported)?;
    let constant = kind.android_constant(handle.sdk);
    let code = call_static(class, |env, class, activity| {
        env.call_static_method(
            class,
            "perform",
            "(Landroid/app/Activity;IJ)I",
            &[
                JValue::Object(activity),
                JValue::Int(constant),
                JValue::Long(kind.millis() as i64),
            ],
        )?
        .i()
    });
    log::debug!(
        "haptics: {kind:?} -> performHapticFeedback({}) answered {code:?}{}",
        Haptic::android_name(constant),
        if code == Some(OFF) {
            " (touch feedback is off in the system's settings)"
        } else {
            ""
        }
    );
    answer(code)
}

/// Vibrates the vibrator for `millis`.
pub(crate) fn vibrate(millis: u32) -> Result<(), HapticsError> {
    let handle = HANDLE
        .get()
        .ok_or_else(|| HapticsError::Failed("the Android bridge is not loaded".into()))?;
    let class = handle.haptics.as_ref().ok_or(HapticsError::Unsupported)?;
    let code = call_static(class, |env, class, activity| {
        env.call_static_method(
            class,
            "vibrate",
            "(Landroid/app/Activity;J)I",
            &[JValue::Object(activity), JValue::Long(millis as i64)],
        )?
        .i()
    });
    log::debug!("haptics: vibrate({millis} ms) answered {code:?}");
    answer(code)
}

/// The kind `FrusConnectivity` reports, as frus names it; `-1` is the system refusing.
pub(crate) fn connectivity_of(code: i32) -> Connectivity {
    match code {
        0 => Connectivity::None,
        1 => Connectivity::Wifi,
        2 => Connectivity::Cellular,
        3 => Connectivity::Ethernet,
        4 => Connectivity::Other,
        _ => Connectivity::Unknown,
    }
}

/// Starts watching the default network, and reports what it is now. `false` while the bridge
/// is not loaded, to be asked again.
pub(crate) fn start_connectivity() -> bool {
    let Some(handle) = HANDLE.get() else {
        return false;
    };
    let Some(class) = handle.connectivity.as_ref() else {
        return true;
    };
    let code = call_static(class, |env, class, activity| {
        env.call_static_method(
            class,
            "start",
            "(Landroid/app/Activity;)I",
            &[JValue::Object(activity)],
        )?
        .i()
    });
    match code {
        Some(code) if code >= 0 => {
            let now = connectivity_of(code);
            log::info!("connectivity: {now:?}");
            frus_widgets::report_connectivity(now);
        }
        Some(_) => log::warn!(
            "connectivity unknown: the manifest does not declare \
             android.permission.ACCESS_NETWORK_STATE"
        ),
        None => log::warn!("connectivity unknown: the system could not be asked"),
    }
    true
}

/// `FrusConnectivity.nativeChanged(int)`: the default network changed, on a thread of the
/// system's.
extern "system" fn native_changed(_env: JNIEnv, _class: JClass, code: jint) {
    let now = connectivity_of(code);
    if frus_widgets::report_connectivity(now) {
        log::info!("connectivity: {now:?}");
    }
}
