// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The system's memory signals** (milestone 840), turned into [`trim_caches`] calls.
//!
//! - Android: `onTrimMemory`, which the native activity keeps to itself, comes through
//!   `FrusMemory` in the bundled dex, registered on the application's context; its level says
//!   how much to give back ([`trim_level_of_android`]). The low-memory callback comes through
//!   the native activity and winit, as `memory_warning`.
//! - iOS: the memory warning, through winit, as `memory_warning`.
//! - The Web and the desktops: a hidden tab or a minimised window is winit's
//!   `WindowEvent::Occluded`, handled in the loop. Browsers have no memory signal of their own.
//!
//! [`trim_caches`]: frus_widgets::trim_caches

use frus_widgets::TrimLevel;

/// What an Android `onTrimMemory` level asks for (the `ComponentCallbacks2` constants):
///
/// - `RUNNING_MODERATE` (5) and `RUNNING_LOW` (10): the application is in use and the device
///   is getting short: [`TrimLevel::Moderate`];
/// - `RUNNING_CRITICAL` (15): in use, and the system is about to end background processes:
///   [`TrimLevel::Critical`];
/// - `UI_HIDDEN` (20), `BACKGROUND` (40), `MODERATE` (60), `COMPLETE` (80): the interface is
///   out of sight, and the process is on the list of those to end: [`TrimLevel::Hidden`].
///
/// Below 5, nothing: no such level is defined.
#[cfg_attr(not(android), allow(dead_code))]
pub(crate) fn trim_level_of_android(level: i32) -> Option<TrimLevel> {
    match level {
        i32::MIN..=4 => None,
        5..=14 => Some(TrimLevel::Moderate),
        15..=19 => Some(TrimLevel::Critical),
        _ => Some(TrimLevel::Hidden),
    }
}

/// Loads `FrusMemory` from the bundled dex and starts it listening, through the loader the
/// input bridge made. A failure costs only this: the low-memory callback still arrives.
#[cfg(android)]
pub(crate) fn install(
    env: &mut jni::JNIEnv,
    loader: &jni::objects::JObject,
    activity: &jni::objects::JObject,
) {
    use jni::objects::{JClass, JValue};
    let started = (|| -> Result<(), jni::errors::Error> {
        let name = env.new_string("dev.frus.input.FrusMemory")?;
        let class: JClass = env
            .call_method(
                loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)],
            )?
            .l()?
            .into();
        env.register_native_methods(
            &class,
            &[jni::NativeMethod {
                name: "nativeTrim".into(),
                sig: "(I)V".into(),
                fn_ptr: native_trim as *mut _,
            }],
        )?;
        env.call_static_method(
            &class,
            "start",
            "(Landroid/app/Activity;)V",
            &[JValue::Object(activity)],
        )?;
        Ok(())
    })();
    if let Err(err) = started {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        log::warn!("memory trim signals unavailable ({err})");
    }
}

/// `FrusMemory.nativeTrim(int)`: `onTrimMemory`, on the Java main thread. The decoded images
/// go at once (their stores are behind locks); the GPU's share is left for the loop.
#[cfg(android)]
extern "system" fn native_trim(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    level: jni::sys::jint,
) {
    log::info!("the system asks for memory back (onTrimMemory {level})");
    if let Some(trim) = trim_level_of_android(level) {
        frus_widgets::trim_caches(trim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every level the system defines asks for what its name says.
    #[test]
    fn android_levels_map_to_how_much_to_give_back() {
        let cases = [
            (0, None),
            (5, Some(TrimLevel::Moderate)),
            (10, Some(TrimLevel::Moderate)),
            (15, Some(TrimLevel::Critical)),
            (20, Some(TrimLevel::Hidden)),
            (40, Some(TrimLevel::Hidden)),
            (60, Some(TrimLevel::Hidden)),
            (80, Some(TrimLevel::Hidden)),
        ];
        for (android, wanted) in cases {
            assert_eq!(trim_level_of_android(android), wanted, "level {android}");
        }
    }
}
