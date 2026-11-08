// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The Android half of **accessibility** (milestone 782): the rows of
//! [`crate::a11y_android`] carried to `FrusAccessibility`, the view in the bundled dex
//! whose node provider shows them to TalkBack, and TalkBack's requests carried back.
//!
//! The Java side tells this one whether any accessibility service is on; while none is,
//! nothing is built or sent. A publication is one call with the whole tree in parallel
//! arrays, posted to the UI thread and swapped in whole there, so a screen reader asking
//! half-way through sees the last tree rather than half of the next. Requests arrive on
//! the UI thread, are queued, and the loop is woken to carry them out.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jfloat, jint, JNI_TRUE};
use jni::{JNIEnv, JavaVM};

use crate::a11y_android::{AndroidEvent, AndroidTree, Request};

struct Handle {
    vm: JavaVM,
    class: GlobalRef,
}

// SAFETY: JavaVM and the GlobalRefs are shareable across threads, per the JNI contract.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

static HANDLE: OnceLock<Handle> = OnceLock::new();
/// Whether an accessibility service is on, as the Java side last said.
static ENABLED: AtomicBool = AtomicBool::new(false);
/// The requests passed on since the last frame.
static REQUESTS: Mutex<Vec<Request>> = Mutex::new(Vec::new());

fn clear_exception(env: &mut JNIEnv) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

/// Loads `FrusAccessibility` from the bundled dex, wires its natives and adds its view to the
/// activity. A failure leaves the application as it was, without a screen reader's view of
/// it, and says so in the log.
pub(crate) fn install(env: &mut JNIEnv, loader: &JObject, activity: &JObject) {
    match try_install(env, loader, activity) {
        Ok(handle) => {
            let _ = HANDLE.set(handle);
        }
        Err(err) => {
            clear_exception(env);
            log::warn!("accessibility unavailable ({err})");
        }
    }
}

fn try_install(
    env: &mut JNIEnv,
    loader: &JObject,
    activity: &JObject,
) -> Result<Handle, jni::errors::Error> {
    let name = env.new_string("dev.frus.input.FrusAccessibility")?;
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
        &[
            jni::NativeMethod {
                name: "nativeEnabled".into(),
                sig: "(Z)V".into(),
                fn_ptr: native_enabled as *mut _,
            },
            jni::NativeMethod {
                name: "nativePerform".into(),
                sig: "(IILjava/lang/String;F)V".into(),
                fn_ptr: native_perform as *mut _,
            },
        ],
    )?;
    env.call_static_method(
        &class,
        "install",
        "(Landroid/app/Activity;)V",
        &[JValue::Object(activity)],
    )?;
    Ok(Handle {
        vm: env.get_java_vm()?,
        class: env.new_global_ref(&class)?,
    })
}

/// Whether an accessibility service is on: the tree is built and sent only then.
pub(crate) fn enabled() -> bool {
    HANDLE.get().is_some() && ENABLED.load(Ordering::Relaxed)
}

/// The requests passed on since the last call, in arrival order.
pub(crate) fn drain() -> Vec<Request> {
    std::mem::take(&mut REQUESTS.lock().unwrap())
}

/// Hands the Java side `tree`, the `events` that say what changed, and an `announce`ment to
/// read out (`None` for none).
pub(crate) fn publish(tree: &AndroidTree, events: &[AndroidEvent], announce: Option<&str>) {
    let Some(handle) = HANDLE.get() else {
        return;
    };
    let Ok(mut env) = handle.vm.attach_current_thread_permanently() else {
        return;
    };
    let class: &JClass = handle.class.as_obj().into();
    // Every row's strings in a frame of their own: the thread is attached for good, and a
    // local reference nobody frees would stay for as long as the process does.
    let count = tree.nodes.len();
    let capacity = 32 + 5 * count as i32 + 2 * events.len() as i32;
    let result = env.with_local_frame(capacity, |env| -> jni::errors::Result<()> {
        let n = count as i32;
        let ids = env.new_int_array(n)?;
        let parents = env.new_int_array(n)?;
        let bounds = env.new_int_array(4 * n)?;
        let flags = env.new_int_array(n)?;
        let actions = env.new_int_array(n)?;
        let ranges = env.new_float_array(3 * n)?;
        let scrolls = env.new_int_array(4 * n)?;
        let classes = env.new_object_array(n, "java/lang/String", JObject::null())?;
        let texts = env.new_object_array(n, "java/lang/String", JObject::null())?;
        let descriptions = env.new_object_array(n, "java/lang/String", JObject::null())?;
        let hints = env.new_object_array(n, "java/lang/String", JObject::null())?;

        let mut id_buf = Vec::with_capacity(count);
        let mut parent_buf = Vec::with_capacity(count);
        let mut bounds_buf = Vec::with_capacity(4 * count);
        let mut flag_buf = Vec::with_capacity(count);
        let mut action_buf = Vec::with_capacity(count);
        let mut range_buf = Vec::with_capacity(3 * count);
        let mut scroll_buf = Vec::with_capacity(4 * count);
        for (i, node) in tree.nodes.iter().enumerate() {
            id_buf.push(node.id);
            parent_buf.push(node.parent);
            bounds_buf.extend_from_slice(&node.bounds);
            flag_buf.push(node.flags);
            action_buf.push(node.actions);
            let (min, max, current) = node.range.unwrap_or((0.0, 0.0, 0.0));
            range_buf.extend_from_slice(&[min, max, current]);
            scroll_buf.extend_from_slice(&node.scroll.unwrap_or([0; 4]));
            let i = i as i32;
            let class_name = env.new_string(node.class_name)?;
            env.set_object_array_element(&classes, i, &class_name)?;
            for (array, value) in [
                (&texts, &node.text),
                (&descriptions, &node.description),
                (&hints, &node.hint),
            ] {
                if let Some(value) = value {
                    let value = env.new_string(value)?;
                    env.set_object_array_element(array, i, &value)?;
                }
            }
        }
        env.set_int_array_region(&ids, 0, &id_buf)?;
        env.set_int_array_region(&parents, 0, &parent_buf)?;
        env.set_int_array_region(&bounds, 0, &bounds_buf)?;
        env.set_int_array_region(&flags, 0, &flag_buf)?;
        env.set_int_array_region(&actions, 0, &action_buf)?;
        env.set_float_array_region(&ranges, 0, &range_buf)?;
        env.set_int_array_region(&scrolls, 0, &scroll_buf)?;

        // Six numbers an event: its type, its node and the four it carries.
        let e = events.len() as i32;
        let event_numbers = env.new_int_array(6 * e)?;
        let event_texts = env.new_object_array(e, "java/lang/String", JObject::null())?;
        let mut numbers = Vec::with_capacity(6 * events.len());
        for (i, event) in events.iter().enumerate() {
            numbers.push(event.kind);
            numbers.push(event.id);
            numbers.extend_from_slice(&event.numbers);
            if let Some(before) = &event.before {
                let before = env.new_string(before)?;
                env.set_object_array_element(&event_texts, i as i32, &before)?;
            }
        }
        env.set_int_array_region(&event_numbers, 0, &numbers)?;
        let announce = match announce {
            Some(words) => JObject::from(env.new_string(words)?),
            None => JObject::null(),
        };
        env.call_static_method(
            class,
            "publish",
            "([I[I[I[I[I[Ljava/lang/String;[Ljava/lang/String;[Ljava/lang/String;\
             [Ljava/lang/String;[F[I[I[Ljava/lang/String;Ljava/lang/String;)V",
            &[
                JValue::Object(&ids),
                JValue::Object(&parents),
                JValue::Object(&bounds),
                JValue::Object(&flags),
                JValue::Object(&actions),
                JValue::Object(&classes),
                JValue::Object(&texts),
                JValue::Object(&descriptions),
                JValue::Object(&hints),
                JValue::Object(&ranges),
                JValue::Object(&scrolls),
                JValue::Object(&event_numbers),
                JValue::Object(&event_texts),
                JValue::Object(&announce),
            ],
        )?;
        Ok(())
    });
    if let Err(err) = result {
        clear_exception(&mut env);
        log::warn!("accessibility: publishing the tree failed ({err})");
    }
}

/// The Java side's word on whether an accessibility service is on: at install, and whenever
/// one is turned on or off. A frame is asked for, which publishes the tree when one is.
extern "system" fn native_enabled(_env: JNIEnv, _class: JClass, on: jboolean) {
    let on = on == JNI_TRUE;
    let was = ENABLED.swap(on, Ordering::Relaxed);
    if on != was {
        log::info!(
            "accessibility {}",
            if on { "service on" } else { "services off" }
        );
    }
    crate::android_ime::wake();
}

/// A request TalkBack made of virtual view `id`, already checked against the published
/// tree on the Java side. Queued, and the loop woken: the widgets are the loop's to touch.
extern "system" fn native_perform(
    mut env: JNIEnv,
    _class: JClass,
    id: jint,
    action: jint,
    text: JString,
    value: jfloat,
) {
    let text = if text.is_null() {
        None
    } else {
        env.get_string(&text).map(Into::into).ok()
    };
    REQUESTS.lock().unwrap().push(Request {
        id,
        action,
        text,
        value,
    });
    crate::android_ime::wake();
}
