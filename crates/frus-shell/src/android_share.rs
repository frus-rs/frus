// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Opening addresses and sharing on Android** (milestone 775): [`crate::open_url`] and
//! [`crate::Share`] through `FrusShare` in the bundled dex.
//!
//! As with the other bridges, every failure clears the Java exception it left, and a bridge
//! that did not load answers `Unsupported`.

use std::path::PathBuf;
use std::sync::OnceLock;

use jni::objects::{GlobalRef, JClass, JObject, JValue};
use jni::{JNIEnv, JavaVM};

use crate::{OpenUrlError, ShareError};

struct Handle {
    vm: JavaVM,
    class: GlobalRef,
    activity: GlobalRef,
}

// SAFETY: JavaVM and the GlobalRefs are shareable across threads, per the JNI contract.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

static HANDLE: OnceLock<Handle> = OnceLock::new();

/// Loads `FrusShare` from the bundled dex, through the loader the input bridge made. A failure
/// costs these two only.
pub(crate) fn install(env: &mut JNIEnv, loader: &JObject, activity: &JObject) {
    match try_install(env, loader, activity) {
        Ok(handle) => {
            let _ = HANDLE.set(handle);
        }
        Err(err) => {
            clear_exception(env);
            log::warn!("opening addresses and sharing unavailable ({err})");
        }
    }
}

fn try_install(
    env: &mut JNIEnv,
    loader: &JObject,
    activity: &JObject,
) -> Result<Handle, jni::errors::Error> {
    let name = env.new_string("dev.frus.input.FrusShare")?;
    let class = env
        .call_method(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?;
    Ok(Handle {
        vm: env.get_java_vm()?,
        class: env.new_global_ref(&class)?,
        activity: env.new_global_ref(activity)?,
    })
}

fn clear_exception(env: &mut JNIEnv) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

/// Runs `work` with the bridge; `None` if the bridge did not load or JNI failed.
fn with_bridge<T>(
    work: impl FnOnce(&mut JNIEnv, &JClass, &JObject) -> Result<T, jni::errors::Error>,
) -> Option<T> {
    let handle = HANDLE.get()?;
    let mut env = handle.vm.attach_current_thread_permanently().ok()?;
    let class: &JClass = handle.class.as_obj().into();
    let result = work(&mut env, class, handle.activity.as_obj());
    if result.is_err() {
        clear_exception(&mut env);
    }
    result.ok()
}

/// A Java string, or `null` for `None`.
fn string<'a>(
    env: &mut JNIEnv<'a>,
    value: Option<&str>,
) -> Result<JObject<'a>, jni::errors::Error> {
    match value {
        Some(value) => Ok(env.new_string(value)?.into()),
        None => Ok(JObject::null()),
    }
}

pub(crate) fn open_url(url: &str) -> Result<(), OpenUrlError> {
    let code = with_bridge(|env, class, activity| {
        let url = env.new_string(url)?;
        env.call_static_method(
            class,
            "openUrl",
            "(Landroid/app/Activity;Ljava/lang/String;)I",
            &[JValue::Object(activity), JValue::Object(&url)],
        )?
        .i()
    });
    match code {
        Some(0) => Ok(()),
        Some(1) => Err(OpenUrlError::NoHandler),
        Some(_) => Err(OpenUrlError::Failed("see the log".to_string())),
        None => Err(OpenUrlError::Unsupported),
    }
}

pub(crate) fn share(
    text: Option<&str>,
    subject: Option<&str>,
    files: &[PathBuf],
) -> Result<(), ShareError> {
    let code = with_bridge(|env, class, activity| {
        let text = string(env, text)?;
        let subject = string(env, subject)?;
        let paths =
            env.new_object_array(files.len() as i32, "java/lang/String", JObject::null())?;
        for (index, path) in files.iter().enumerate() {
            let path = env.new_string(path.to_string_lossy())?;
            env.set_object_array_element(&paths, index as i32, path)?;
        }
        env.call_static_method(
            class,
            "share",
            "(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;)I",
            &[
                JValue::Object(activity),
                JValue::Object(&text),
                JValue::Object(&subject),
                JValue::Object(&paths),
            ],
        )?
        .i()
    });
    match code {
        Some(0) => Ok(()),
        Some(1) => Err(ShareError::Failed(
            "no application can share this".to_string(),
        )),
        Some(2) => Err(ShareError::Unsupported),
        Some(_) => Err(ShareError::Failed("see the log".to_string())),
        None => Err(ShareError::Unsupported),
    }
}

/// Opens a sign-in page (milestone 795): in a **Custom Tab** when the browser has them — the
/// intent carries the Custom Tabs session extra, which is all it takes, with no library — and
/// in the browser otherwise. Not in a task of its own: the tab stays over the application,
/// which [`return_to_app`] brings back over it once the loopback answer is in.
#[cfg(feature = "auth")]
pub(crate) fn open_auth_tab(url: &str) -> Result<(), OpenUrlError> {
    let opened = with_bridge(|env, _class, activity| {
        let text = env.new_string(url)?;
        let uri = env
            .call_static_method(
                "android/net/Uri",
                "parse",
                "(Ljava/lang/String;)Landroid/net/Uri;",
                &[JValue::Object(&text)],
            )?
            .l()?;
        let action = env.new_string("android.intent.action.VIEW")?;
        let intent = env.new_object(
            "android/content/Intent",
            "(Ljava/lang/String;Landroid/net/Uri;)V",
            &[JValue::Object(&action), JValue::Object(&uri)],
        )?;
        let extras = env.new_object("android/os/Bundle", "()V", &[])?;
        let session = env.new_string("android.support.customtabs.extra.SESSION")?;
        env.call_method(
            &extras,
            "putBinder",
            "(Ljava/lang/String;Landroid/os/IBinder;)V",
            &[JValue::Object(&session), JValue::Object(&JObject::null())],
        )?;
        env.call_method(
            &intent,
            "putExtras",
            "(Landroid/os/Bundle;)Landroid/content/Intent;",
            &[JValue::Object(&extras)],
        )?;
        env.call_method(
            activity,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(&intent)],
        )?;
        Ok(())
    });
    opened.ok_or(OpenUrlError::NoHandler)
}

/// Brings the application's activity back over what was started above it in its task — the
/// sign-in's Custom Tab (milestone 795): an intent for its own activity with
/// `FLAG_ACTIVITY_CLEAR_TOP | FLAG_ACTIVITY_SINGLE_TOP`, which finishes everything above it and
/// hands the intent to the running activity instead of making another. Android 10 lets an
/// application do this from the background while one of its activities is in the foreground
/// task, which it is, under the tab.
#[cfg(feature = "auth")]
pub(crate) fn return_to_app() {
    let done = with_bridge(|env, _class, activity| {
        let component = env
            .call_method(
                activity,
                "getComponentName",
                "()Landroid/content/ComponentName;",
                &[],
            )?
            .l()?;
        let intent = env.new_object("android/content/Intent", "()V", &[])?;
        env.call_method(
            &intent,
            "setComponent",
            "(Landroid/content/ComponentName;)Landroid/content/Intent;",
            &[JValue::Object(&component)],
        )?;
        // FLAG_ACTIVITY_CLEAR_TOP | FLAG_ACTIVITY_SINGLE_TOP.
        env.call_method(
            &intent,
            "addFlags",
            "(I)Landroid/content/Intent;",
            &[JValue::Int(0x0400_0000 | 0x2000_0000)],
        )?;
        env.call_method(
            activity,
            "startActivity",
            "(Landroid/content/Intent;)V",
            &[JValue::Object(&intent)],
        )?;
        Ok(())
    });
    if done.is_none() {
        log::warn!("the application could not come back over the sign-in tab");
    }
}
