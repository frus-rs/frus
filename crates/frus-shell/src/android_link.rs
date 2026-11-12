// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! What an **Android** activity is opened with: a link, a share.
//!
//! Tapping `https://example.com/orders/42` in a mail — when the application declared an
//! intent filter for it — or `myapp://orders/42` starts the activity with an `Intent` whose
//! data is that string. It is one read from the activity, `getIntent().getDataString()`, and
//! [`crate::link::location_of_link`] turns it into a location. A share from another
//! application's share sheet is an `ACTION_SEND` intent, read into a
//! [`SharedContent`](crate::SharedContent).
//!
//! **While the application runs** (milestone 801), an intent that opens it again reaches the
//! running activity as `onNewIntent`, which the native activity does not forward. The package
//! names frus's own activity instead (`dev.frus.FrusActivity`, which `frus build` compiles in),
//! declared `singleTop`, whose `onNewIntent` calls a native method [`listen`] registers.
//! `FrusIntents` in the bundled dex reads the intent — and copies a share's files — in Java,
//! which costs the library nothing, and hands back strings: the link goes to the running loop
//! as a second activity's does ([`crate::android_lifecycle`]), the share to
//! [`crate::received_share`]. A package that still names the native activity (one
//! built by hand with `cargo apk`) gets the launch's intent only, as before.
//!
//! As in [`crate::android_settings`], the read is best-effort and **every failure clears its
//! exception**: a pending Java exception left behind takes the process down on the next,
//! unrelated JNI call.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use jni::objects::{GlobalRef, JClass, JObject, JObjectArray, JString, JValue};
use jni::{JNIEnv, JavaVM, NativeMethod};
use winit::platform::android::activity::AndroidApp;

use crate::SharedContent;

/// The link the activity was started with, if it was started with one.
pub(crate) fn launch_link(app: &AndroidApp) -> Option<String> {
    with_intent(app, |env, intent| {
        let data = env
            .call_method(intent, "getDataString", "()Ljava/lang/String;", &[])?
            .l()?;
        string(env, data)
    })
}

/// A text extra of the launch intent: `am start --es <name> <value>`. How `frus run` hands an
/// application the address it reports to (see [`crate::dev`]).
pub(crate) fn launch_extra(app: &AndroidApp, name: &str) -> Option<String> {
    with_intent(app, |env, intent| {
        let key = env.new_string(name)?;
        let value = env
            .call_method(
                intent,
                "getStringExtra",
                "(Ljava/lang/String;)Ljava/lang/String;",
                &[(&key).into()],
            )?
            .l()?;
        string(env, value)
    })
}

/// Runs `read` on the activity's intent; any failure is a warning and `None`.
fn with_intent(
    app: &AndroidApp,
    read: impl FnOnce(&mut JNIEnv, &JObject) -> Result<Option<String>, jni::errors::Error>,
) -> Option<String> {
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) }.ok()?;
    let mut env = vm.attach_current_thread_permanently().ok()?;
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
    let result = env
        .call_method(&activity, "getIntent", "()Landroid/content/Intent;", &[])
        .and_then(|intent| intent.l())
        .and_then(|intent| {
            if intent.is_null() {
                Ok(None)
            } else {
                read(&mut env, &intent)
            }
        });
    match result {
        Ok(value) => value,
        Err(err) => {
            log::warn!("the launch intent could not be read ({err})");
            clear_pending(&mut env);
            None
        }
    }
}

/// A Java string, or `None` for `null`.
fn string(env: &mut JNIEnv, value: JObject) -> Result<Option<String>, jni::errors::Error> {
    if value.is_null() {
        return Ok(None);
    }
    let value = JString::from(value);
    let text: String = env.get_string(&value)?.into();
    Ok(Some(text))
}

/// Whether the running activity hands its new intents to the native side: frus's activity, with
/// its native method registered.
static RECEIVING: AtomicBool = AtomicBool::new(false);

/// `FrusIntents` in the bundled dex, which reads an intent and hands it back as strings.
static INTENTS: OnceLock<GlobalRef> = OnceLock::new();

/// Whether an intent that opens the application again reaches the running activity — so that a
/// link can be waited for (a sign-in's redirect) without the application being started over.
#[cfg_attr(not(feature = "auth"), allow(dead_code))]
pub(crate) fn receives_new_intents() -> bool {
    RECEIVING.load(Ordering::Acquire)
}

/// Loads `FrusIntents` from the bundled dex, through the loader the input bridge made, and
/// registers the native method it answers through.
pub(crate) fn install(env: &mut JNIEnv, loader: &JObject) {
    let loaded = (|| -> jni::errors::Result<GlobalRef> {
        let name = env.new_string("dev.frus.input.FrusIntents")?;
        let class = env
            .call_method(
                loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)],
            )?
            .l()?;
        let class = JClass::from(class);
        env.register_native_methods(
            &class,
            &[NativeMethod {
                name: "nativeReceived".into(),
                sig: "([Ljava/lang/String;)V".into(),
                fn_ptr: native_received as *mut std::ffi::c_void,
            }],
        )?;
        env.new_global_ref(class)
    })();
    match loaded {
        Ok(class) => {
            let _ = INTENTS.set(class);
        }
        Err(err) => {
            clear_pending(env);
            log::warn!("new intents unavailable ({err})");
        }
    }
}

/// Registers the activity's `nativeNewIntent(Intent)`, from now on called by its
/// `onNewIntent`. The activity's own class is asked first, then the classes it extends, up to
/// the native activity, which has no such method: a package that names it receives no new
/// intent, and nothing is registered.
pub(crate) fn listen(app: &AndroidApp) {
    let Ok(vm) = (unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) }) else {
        return;
    };
    let Ok(mut env) = vm.attach_current_thread_permanently() else {
        return;
    };
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
    let Ok(mut class) = env.get_object_class(&activity) else {
        clear_pending(&mut env);
        return;
    };
    // Up to the native activity's own class, `Activity` and `Object`: a few tries at most.
    for _ in 0..4 {
        let registered = env.register_native_methods(
            &class,
            &[NativeMethod {
                name: "nativeNewIntent".into(),
                sig: "(Landroid/content/Intent;)V".into(),
                fn_ptr: native_new_intent as *mut std::ffi::c_void,
            }],
        );
        if registered.is_ok() {
            RECEIVING.store(true, Ordering::Release);
            log::info!("new intents reach the running application");
            return;
        }
        clear_pending(&mut env);
        match env.get_superclass(&class) {
            Ok(Some(parent)) => class = parent,
            _ => break,
        }
    }
    clear_pending(&mut env);
}

/// `FrusActivity.nativeNewIntent`: the intent `onNewIntent` was handed, on the Java main
/// thread, read by `FrusIntents`.
extern "system" fn native_new_intent<'local>(
    mut env: JNIEnv<'local>,
    activity: JObject<'local>,
    intent: JObject<'local>,
) {
    receive(&mut env, &activity, &intent, false);
}

/// The share the activity was started with, if it was started by one, handed over like a new
/// intent's; its link is [`launch_link`]'s.
pub(crate) fn receive_launch_share(app: &AndroidApp) {
    let Ok(vm) = (unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) }) else {
        return;
    };
    let Ok(mut env) = vm.attach_current_thread_permanently() else {
        return;
    };
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
    match env
        .call_method(&activity, "getIntent", "()Landroid/content/Intent;", &[])
        .and_then(|intent| intent.l())
    {
        Ok(intent) => receive(&mut env, &activity, &intent, true),
        Err(_) => clear_pending(&mut env),
    }
}

/// `FrusIntents.receive(activity, intent, launch)`.
fn receive(env: &mut JNIEnv, activity: &JObject, intent: &JObject, launch: bool) {
    let Some(class) = INTENTS.get() else {
        return;
    };
    let class: &JClass = class.as_obj().into();
    if env
        .call_static_method(
            class,
            "receive",
            "(Landroid/app/Activity;Landroid/content/Intent;Z)V",
            &[
                JValue::Object(activity),
                JValue::Object(intent),
                JValue::Bool(launch.into()),
            ],
        )
        .is_err()
    {
        clear_pending(env);
        log::warn!("an intent could not be read");
    }
}

/// `FrusIntents.nativeReceived`: `{"link", data}` or `{"share", text, subject, type, file…}`,
/// from the main thread or the thread that copied the files. Queued for the loop, which is
/// asked for a frame.
extern "system" fn native_received<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    fields: JObjectArray<'local>,
) {
    let count = env.get_array_length(&fields).unwrap_or(0);
    let mut values = Vec::with_capacity(count as usize);
    for at in 0..count {
        let value = env
            .get_object_array_element(&fields, at)
            .and_then(|value| string(&mut env, value))
            .unwrap_or(None);
        values.push(value);
    }
    clear_pending(&mut env);
    let mut values = values.into_iter();
    match values.next().flatten().as_deref() {
        Some("link") => {
            if let Some(link) = values.next().flatten() {
                crate::android_lifecycle::queue_link(link);
            }
        }
        Some("share") => {
            let text = values.next().flatten();
            let subject = values.next().flatten();
            let mime_type = values.next().flatten();
            crate::received_share::push(SharedContent {
                text,
                subject,
                mime_type,
                files: values.flatten().map(PathBuf::from).collect(),
            });
        }
        _ => return,
    }
    crate::android_lifecycle::wake();
}

/// Clears any Java exception left pending on this thread.
fn clear_pending(env: &mut JNIEnv) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}
