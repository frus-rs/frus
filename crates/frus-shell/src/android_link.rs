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
//! declared `singleTop`, whose `onNewIntent` calls a native method [`listen`] registers: the
//! link goes to the running loop as a second activity's does ([`crate::android_lifecycle`]),
//! the share to [`crate::received_share`]. A package that still names the native activity (one
//! built by hand with `cargo apk`) gets the launch's intent only, as before.
//!
//! As in [`crate::android_settings`], the read is best-effort and **every failure clears its
//! exception**: a pending Java exception left behind takes the process down on the next,
//! unrelated JNI call.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use jni::objects::{GlobalRef, JObject, JString, JValue};
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

/// Whether an intent that opens the application again reaches the running activity — so that a
/// link can be waited for (a sign-in's redirect) without the application being started over.
#[cfg_attr(not(feature = "auth"), allow(dead_code))]
pub(crate) fn receives_new_intents() -> bool {
    RECEIVING.load(Ordering::Acquire)
}

/// What an intent carries for the application.
#[derive(Debug, PartialEq)]
enum Opened {
    /// Its data: a link.
    Link(String),
    /// `ACTION_SEND` or `ACTION_SEND_MULTIPLE`: what was shared, and the `content://`
    /// addresses of the files still to copy.
    Share {
        shared: SharedContent,
        streams: Vec<String>,
    },
}

/// The intent actions a share comes as.
const SEND: &str = "android.intent.action.SEND";
const SEND_MULTIPLE: &str = "android.intent.action.SEND_MULTIPLE";

/// The classes that have no `nativeNewIntent`: where the search for it stops.
const PLATFORM_ACTIVITIES: [&str; 2] = ["android.app.NativeActivity", "android.app.Activity"];

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
    loop {
        let name = env
            .call_method(&class, "getName", "()Ljava/lang/String;", &[])
            .and_then(|name| name.l())
            .and_then(|name| string(&mut env, name));
        let Ok(Some(name)) = name else {
            clear_pending(&mut env);
            return;
        };
        if PLATFORM_ACTIVITIES.contains(&name.as_str()) {
            log::info!(
                "the package names the native activity: a link opened while the application \
                 runs starts a second activity, which hands it over (`frus build` names \
                 frus's own)"
            );
            return;
        }
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
            log::info!("{name} hands new intents to the running application");
            return;
        }
        clear_pending(&mut env);
        match env.get_superclass(&class) {
            Ok(Some(parent)) => class = parent,
            _ => {
                clear_pending(&mut env);
                return;
            }
        }
    }
}

/// `FrusActivity.nativeNewIntent`: the intent `onNewIntent` was handed, on the Java main
/// thread. Its link is queued for the loop, its share copied and queued; the loop is asked for
/// a frame.
extern "system" fn native_new_intent<'local>(
    mut env: JNIEnv<'local>,
    activity: JObject<'local>,
    intent: JObject<'local>,
) {
    if intent.is_null() {
        return;
    }
    match read_intent(&mut env, &intent) {
        Ok(Some(opened)) => {
            log::info!("the running activity was handed {}", describe(&opened));
            deliver(&mut env, &activity, opened);
        }
        Ok(None) => {}
        Err(err) => {
            clear_pending(&mut env);
            log::warn!("a new intent could not be read ({err})");
        }
    }
}

/// The share the activity was started with, if it was started by one: queued for the first
/// frames, once its files are copied.
pub(crate) fn receive_launch_share(app: &AndroidApp) {
    let Ok(vm) = (unsafe { JavaVM::from_raw(app.vm_as_ptr() as *mut jni::sys::JavaVM) }) else {
        return;
    };
    let Ok(mut env) = vm.attach_current_thread_permanently() else {
        return;
    };
    let activity = unsafe { JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject) };
    let opened = env
        .call_method(&activity, "getIntent", "()Landroid/content/Intent;", &[])
        .and_then(|intent| intent.l())
        .and_then(|intent| {
            if intent.is_null() {
                Ok(None)
            } else {
                read_intent(&mut env, &intent)
            }
        });
    match opened {
        Ok(Some(share @ Opened::Share { .. })) => {
            log::info!("the application was started with {}", describe(&share));
            deliver(&mut env, &activity, share);
        }
        Ok(_) => {}
        Err(err) => {
            clear_pending(&mut env);
            log::warn!("the launch intent's share could not be read ({err})");
        }
    }
}

/// A short description of what an intent carried, for the log: never a query, never the text.
fn describe(opened: &Opened) -> String {
    match opened {
        Opened::Link(link) => format!("a link ({})", link.split(['?', '#']).next().unwrap_or("")),
        Opened::Share { shared, streams } => format!(
            "a share ({}, {} file(s){})",
            shared.mime_type.as_deref().unwrap_or("no type"),
            streams.len(),
            if shared.text.is_some() { ", text" } else { "" }
        ),
    }
}

/// Hands what an intent carried to the loop: a link at once, a share once its files are copied
/// — on a thread of its own, as a video can take a while.
fn deliver(env: &mut JNIEnv, activity: &JObject, opened: Opened) {
    match opened {
        Opened::Link(link) => {
            crate::android_lifecycle::queue_link(link);
            crate::android_lifecycle::wake();
        }
        Opened::Share { shared, streams } if streams.is_empty() => {
            crate::received_share::push(shared);
            crate::android_lifecycle::wake();
        }
        Opened::Share {
            mut shared,
            streams,
        } => {
            let (Ok(vm), Ok(activity)) = (env.get_java_vm(), env.new_global_ref(activity)) else {
                clear_pending(env);
                return;
            };
            let spawned = std::thread::Builder::new()
                .name("frus-shared-files".into())
                .spawn(move || {
                    if let Ok(mut env) = vm.attach_current_thread() {
                        shared.files = copy_streams(&mut env, &activity, &streams);
                    }
                    crate::received_share::push(shared);
                    crate::android_lifecycle::wake();
                });
            if let Err(err) = spawned {
                log::warn!("the shared files could not be copied ({err})");
            }
        }
    }
}

/// Reads what `intent` carries for the application: a share, or a link; None for neither (the
/// launcher's intent, a notification's tap).
fn read_intent(env: &mut JNIEnv, intent: &JObject) -> Result<Option<Opened>, jni::errors::Error> {
    let action = env
        .call_method(intent, "getAction", "()Ljava/lang/String;", &[])?
        .l()?;
    let action = string(env, action)?;
    if matches!(action.as_deref(), Some(SEND | SEND_MULTIPLE)) {
        let text = char_sequence_extra(env, intent, "android.intent.extra.TEXT")?;
        let subject = char_sequence_extra(env, intent, "android.intent.extra.SUBJECT")?;
        let mime_type = env
            .call_method(intent, "getType", "()Ljava/lang/String;", &[])?
            .l()?;
        let mime_type = string(env, mime_type)?;
        let key = env.new_string("android.intent.extra.STREAM")?;
        let mut streams = Vec::new();
        if action.as_deref() == Some(SEND) {
            let uri = env
                .call_method(
                    intent,
                    "getParcelableExtra",
                    "(Ljava/lang/String;)Landroid/os/Parcelable;",
                    &[JValue::Object(&key)],
                )?
                .l()?;
            streams.extend(to_string(env, &uri)?);
        } else {
            let list = env
                .call_method(
                    intent,
                    "getParcelableArrayListExtra",
                    "(Ljava/lang/String;)Ljava/util/ArrayList;",
                    &[JValue::Object(&key)],
                )?
                .l()?;
            if !list.is_null() {
                let count = env.call_method(&list, "size", "()I", &[])?.i()?;
                for at in 0..count {
                    let uri = env
                        .call_method(&list, "get", "(I)Ljava/lang/Object;", &[JValue::Int(at)])?
                        .l()?;
                    streams.extend(to_string(env, &uri)?);
                }
            }
        }
        return Ok(Some(Opened::Share {
            shared: SharedContent {
                text,
                subject,
                mime_type,
                files: Vec::new(),
            },
            streams,
        }));
    }
    let data = env
        .call_method(intent, "getDataString", "()Ljava/lang/String;", &[])?
        .l()?;
    Ok(string(env, data)?.map(Opened::Link))
}

/// A text extra that may be a `CharSequence` (styled text) rather than a `String`.
fn char_sequence_extra(
    env: &mut JNIEnv,
    intent: &JObject,
    name: &str,
) -> Result<Option<String>, jni::errors::Error> {
    let key = env.new_string(name)?;
    let value = env
        .call_method(
            intent,
            "getCharSequenceExtra",
            "(Ljava/lang/String;)Ljava/lang/CharSequence;",
            &[JValue::Object(&key)],
        )?
        .l()?;
    to_string(env, &value)
}

/// `object.toString()`, or None for `null`.
fn to_string(env: &mut JNIEnv, object: &JObject) -> Result<Option<String>, jni::errors::Error> {
    if object.is_null() {
        return Ok(None);
    }
    let text = env
        .call_method(object, "toString", "()Ljava/lang/String;", &[])?
        .l()?;
    string(env, text)
}

/// Copies each `content://` stream into the cache folder's `shared/`, named after what the
/// sender calls the file. What cannot be read is left out, with a warning.
fn copy_streams(env: &mut JNIEnv, activity: &GlobalRef, streams: &[String]) -> Vec<PathBuf> {
    let mut copied = Vec::new();
    let dir = match cache_dir(env, activity.as_obj()) {
        Ok(Some(dir)) => dir.join("shared"),
        _ => {
            clear_pending(env);
            log::warn!("no cache folder to copy the shared files into");
            return copied;
        }
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        log::warn!("{}: {err}", dir.display());
        return copied;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    for (at, uri) in streams.iter().enumerate() {
        match copy_stream(env, activity.as_obj(), uri, &dir, &format!("{stamp}-{at}")) {
            Ok(path) => copied.push(path),
            Err(err) => {
                clear_pending(env);
                log::warn!("a shared file could not be copied ({err})");
            }
        }
    }
    copied
}

/// The application's cache folder: `Context.getCacheDir()`.
fn cache_dir(env: &mut JNIEnv, activity: &JObject) -> Result<Option<PathBuf>, jni::errors::Error> {
    let dir = env
        .call_method(activity, "getCacheDir", "()Ljava/io/File;", &[])?
        .l()?;
    if dir.is_null() {
        return Ok(None);
    }
    let path = env
        .call_method(&dir, "getAbsolutePath", "()Ljava/lang/String;", &[])?
        .l()?;
    Ok(string(env, path)?.map(PathBuf::from))
}

/// Copies one stream into `dir`, as `<prefix>-<its name>`.
fn copy_stream(
    env: &mut JNIEnv,
    activity: &JObject,
    uri: &str,
    dir: &Path,
    prefix: &str,
) -> Result<PathBuf, String> {
    use std::io::Write;
    let jni = |e: jni::errors::Error| e.to_string();
    let text = env.new_string(uri).map_err(jni)?;
    let parsed = env
        .call_static_method(
            "android/net/Uri",
            "parse",
            "(Ljava/lang/String;)Landroid/net/Uri;",
            &[JValue::Object(&text)],
        )
        .and_then(|v| v.l())
        .map_err(jni)?;
    let resolver = env
        .call_method(
            activity,
            "getContentResolver",
            "()Landroid/content/ContentResolver;",
            &[],
        )
        .and_then(|v| v.l())
        .map_err(jni)?;
    let name = match display_name(env, &resolver, &parsed) {
        Ok(Some(name)) => name,
        _ => {
            clear_pending(env);
            uri.rsplit('/').next().unwrap_or_default().to_string()
        }
    };
    let path = dir.join(format!(
        "{prefix}-{}",
        crate::received_share::safe_file_name(&name)
    ));
    let input = env
        .call_method(
            &resolver,
            "openInputStream",
            "(Landroid/net/Uri;)Ljava/io/InputStream;",
            &[JValue::Object(&parsed)],
        )
        .and_then(|v| v.l())
        .map_err(jni)?;
    if input.is_null() {
        return Err(format!("{uri} opened to nothing"));
    }
    let result = (|| -> Result<(), String> {
        let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        const CHUNK: usize = 64 * 1024;
        let buffer = env.new_byte_array(CHUNK as i32).map_err(jni)?;
        let mut bytes = vec![0i8; CHUNK];
        loop {
            let read = env
                .call_method(&input, "read", "([B)I", &[JValue::Object(&buffer)])
                .and_then(|v| v.i())
                .map_err(jni)?;
            if read < 0 {
                return Ok(());
            }
            let read = (read as usize).min(CHUNK);
            env.get_byte_array_region(&buffer, 0, &mut bytes[..read])
                .map_err(jni)?;
            let chunk: Vec<u8> = bytes[..read].iter().map(|&b| b as u8).collect();
            file.write_all(&chunk).map_err(|e| e.to_string())?;
        }
    })();
    if result.is_err() {
        clear_pending(env);
    }
    if env.call_method(&input, "close", "()V", &[]).is_err() {
        clear_pending(env);
    }
    match result {
        Ok(()) => Ok(path),
        Err(err) => {
            let _ = std::fs::remove_file(&path);
            Err(err)
        }
    }
}

/// The name the sender gives a `content://` stream: its `_display_name` column.
fn display_name(
    env: &mut JNIEnv,
    resolver: &JObject,
    uri: &JObject,
) -> Result<Option<String>, jni::errors::Error> {
    let column = env.new_string("_display_name")?;
    let columns = env.new_object_array(1, "java/lang/String", &column)?;
    let null = JObject::null();
    let cursor = env
        .call_method(
            resolver,
            "query",
            "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;\
             Ljava/lang/String;)Landroid/database/Cursor;",
            &[
                JValue::Object(uri),
                JValue::Object(&columns),
                JValue::Object(&null),
                JValue::Object(&null),
                JValue::Object(&null),
            ],
        )?
        .l()?;
    if cursor.is_null() {
        return Ok(None);
    }
    let name = if env.call_method(&cursor, "moveToFirst", "()Z", &[])?.z()? {
        let value = env
            .call_method(
                &cursor,
                "getString",
                "(I)Ljava/lang/String;",
                &[JValue::Int(0)],
            )?
            .l()?;
        string(env, value)?
    } else {
        None
    };
    env.call_method(&cursor, "close", "()V", &[])?;
    Ok(name)
}

/// Clears any Java exception left pending on this thread.
fn clear_pending(env: &mut JNIEnv) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}
