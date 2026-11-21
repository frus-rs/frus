// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The system's **text services** on Android (milestone 829): the spell checker
//! (`SpellCheckerSession`) and the "process text" actions (`ACTION_PROCESS_TEXT`), reached
//! through `FrusTextServices` in the bundled dex.
//!
//! A pipe, like the other bridges: requests go across when the shell decides
//! (`crate::textservices`), and answers come back on the Java UI thread, are queued, and
//! wake the loop. Nothing is asked of the platform until a field that wants spell checking
//! has focus, or a selection bar opens. Every failure is a log line and a cleared exception;
//! nothing on this path can panic.

use std::sync::{Mutex, OnceLock};

use jni::objects::{GlobalRef, JClass, JIntArray, JObject, JObjectArray, JString, JValue};
use jni::sys::jint;
use jni::{JNIEnv, JavaVM};

use crate::textservices::Utf16Word;

struct Handle {
    vm: JavaVM,
    class: GlobalRef,
    activity: GlobalRef,
}

// SAFETY: JavaVM and the GlobalRefs are shareable across threads, per the JNI contract.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

static HANDLE: OnceLock<Handle> = OnceLock::new();

/// An answer from the platform, waiting for the shell.
pub(crate) enum Answer {
    /// The spell checker's words for request `sequence`; `None` when there is no checker.
    Spelling {
        sequence: i32,
        words: Option<Vec<Utf16Word>>,
    },
    /// What a "process text" action returned for the selection, if anything.
    Processed(Option<String>),
}

static QUEUE: Mutex<Vec<Answer>> = Mutex::new(Vec::new());

/// Loads `FrusTextServices` through the loader the input bridge made, and wires its natives.
/// A failure costs these services only.
pub(crate) fn install(env: &mut JNIEnv, loader: &JObject, activity: &JObject) {
    match try_install(env, loader, activity) {
        Ok(handle) => {
            let _ = HANDLE.set(handle);
        }
        Err(err) => {
            clear_exception(env);
            log::warn!("text services unavailable ({err}): no spell check, no process text");
        }
    }
}

fn try_install(
    env: &mut JNIEnv,
    loader: &JObject,
    activity: &JObject,
) -> Result<Handle, jni::errors::Error> {
    let name = env.new_string("dev.frus.input.FrusTextServices")?;
    let class: JClass = env
        .call_method(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?
        .into();
    use jni::NativeMethod;
    env.register_native_methods(
        &class,
        &[
            NativeMethod {
                name: "nativeSpellResults".into(),
                sig: "(I[I[I[Ljava/lang/String;)V".into(),
                fn_ptr: native_spell_results as *mut _,
            },
            NativeMethod {
                name: "nativeProcessTextResult".into(),
                sig: "(Ljava/lang/String;)V".into(),
                fn_ptr: native_process_text_result as *mut _,
            },
        ],
    )?;
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

/// Runs `call` against the class and the activity, logging and clearing what fails.
fn with<T>(
    what: &str,
    call: impl FnOnce(&mut JNIEnv, &JClass, &JObject) -> jni::errors::Result<T>,
) -> Option<T> {
    let handle = HANDLE.get()?;
    let mut env = handle.vm.attach_current_thread_permanently().ok()?;
    let class: &JClass = handle.class.as_obj().into();
    match call(&mut env, class, handle.activity.as_obj()) {
        Ok(value) => Some(value),
        Err(err) => {
            clear_exception(&mut env);
            log::warn!("text services: {what} failed ({err})");
            None
        }
    }
}

/// Whether the services were installed.
pub(crate) fn installed() -> bool {
    HANDLE.get().is_some()
}

/// Asks the spell checker about `text`, as request `sequence`.
pub(crate) fn spell_check(sequence: i32, text: &str) {
    with("spellCheck", |env, class, activity| {
        let text = env.new_string(text)?;
        env.call_static_method(
            class,
            "spellCheck",
            "(Landroid/app/Activity;ILjava/lang/String;)V",
            &[
                JValue::Object(activity),
                JValue::Int(sequence),
                JValue::Object(&text),
            ],
        )
        .map(|_| ())
    });
}

/// The words of the installed "process text" actions, in the platform's order.
pub(crate) fn process_text_labels() -> Vec<String> {
    with("processTextLabels", |env, class, activity| {
        let array: JObjectArray = env
            .call_static_method(
                class,
                "processTextLabels",
                "(Landroid/app/Activity;)[Ljava/lang/String;",
                &[JValue::Object(activity)],
            )?
            .l()?
            .into();
        strings(env, &array)
    })
    .unwrap_or_default()
}

/// Hands `text` to action `index`; a read-only selection's action answers nothing.
pub(crate) fn process_text(index: u16, text: &str, read_only: bool) {
    with("processText", |env, class, activity| {
        let text = env.new_string(text)?;
        env.call_static_method(
            class,
            "processText",
            "(Landroid/app/Activity;ILjava/lang/String;Z)V",
            &[
                JValue::Object(activity),
                JValue::Int(i32::from(index)),
                JValue::Object(&text),
                JValue::Bool(u8::from(read_only)),
            ],
        )
        .map(|_| ())
    });
}

/// The answers that arrived since the last call, in order.
pub(crate) fn drain() -> Vec<Answer> {
    std::mem::take(&mut QUEUE.lock().unwrap())
}

fn push(answer: Answer) {
    QUEUE.lock().unwrap().push(answer);
    crate::android_ime::wake();
}

fn strings(env: &mut JNIEnv, array: &JObjectArray) -> jni::errors::Result<Vec<String>> {
    if array.is_null() {
        return Ok(Vec::new());
    }
    let count = env.get_array_length(array)?;
    let mut out = Vec::with_capacity(count.max(0) as usize);
    for i in 0..count {
        let item = env.get_object_array_element(array, i)?;
        if item.is_null() {
            out.push(String::new());
            continue;
        }
        let text = JString::from(item);
        out.push(env.get_string(&text)?.into());
    }
    Ok(out)
}

// --- Natives, called on the Java UI thread ------------------------------------------

extern "system" fn native_spell_results(
    mut env: JNIEnv,
    _class: JClass,
    sequence: jint,
    starts: JIntArray,
    lengths: JIntArray,
    suggestions: JObjectArray,
) {
    if starts.is_null() {
        push(Answer::Spelling {
            sequence,
            words: None,
        });
        return;
    }
    let read = (|| -> jni::errors::Result<Vec<Utf16Word>> {
        let count = env.get_array_length(&starts)?.max(0) as usize;
        let mut at = vec![0; count];
        let mut length = vec![0; count];
        env.get_int_array_region(&starts, 0, &mut at)?;
        env.get_int_array_region(&lengths, 0, &mut length)?;
        let offered = strings(&mut env, &suggestions)?;
        Ok((0..count)
            .map(|i| {
                let words = offered
                    .get(i)
                    .map(|joined| {
                        joined
                            .split('\n')
                            .filter(|word| !word.is_empty())
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                (at[i], length[i], words)
            })
            .collect())
    })();
    match read {
        Ok(words) => push(Answer::Spelling {
            sequence,
            words: Some(words),
        }),
        Err(err) => {
            clear_exception(&mut env);
            log::warn!("text services: the spell checker's answer could not be read ({err})");
        }
    }
}

extern "system" fn native_process_text_result(mut env: JNIEnv, _class: JClass, text: JString) {
    let text = if text.is_null() {
        None
    } else {
        env.get_string(&text).map(String::from).ok()
    };
    push(Answer::Processed(text));
}
