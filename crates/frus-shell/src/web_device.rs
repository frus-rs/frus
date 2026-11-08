// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Haptics and connectivity on the web** (milestone 777): `navigator.vibrate`, and
//! `navigator.onLine` with the `online` and `offline` events — plus the connection's type
//! where the browser tells it (`navigator.connection`, Chrome on Android).

use frus_widgets::{report_connectivity, Connectivity};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::haptics::HapticsError;

/// Vibrates the pattern, in milliseconds, through `navigator.vibrate`.
pub(crate) fn vibrate(pattern: &[u32]) -> Result<(), HapticsError> {
    let navigator = web_sys::window()
        .ok_or(HapticsError::Unsupported)?
        .navigator();
    let vibrate = js_sys::Reflect::get(&navigator, &JsValue::from_str("vibrate"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .ok_or(HapticsError::Unsupported)?;
    let millis = js_sys::Array::new();
    for ms in pattern {
        millis.push(&JsValue::from(*ms));
    }
    match vibrate.call1(&navigator, &millis).map(|ok| ok.as_bool()) {
        Ok(Some(true)) => Ok(()),
        _ => Err(HapticsError::Failed(
            "the browser refused: it vibrates only once the person has touched the page".into(),
        )),
    }
}

fn connection() -> Option<JsValue> {
    let navigator = web_sys::window()?.navigator();
    js_sys::Reflect::get(&navigator, &JsValue::from_str("connection"))
        .ok()
        .filter(|c| c.is_object())
}

fn read() -> Connectivity {
    let Some(window) = web_sys::window() else {
        return Connectivity::Unknown;
    };
    let kind = connection()
        .and_then(|c| js_sys::Reflect::get(&c, &JsValue::from_str("type")).ok())
        .and_then(|t| t.as_string());
    crate::connectivity::of_web_type(window.navigator().on_line(), kind.as_deref())
}

/// Reports what the page knows now, and again on every `online`, `offline` and connection
/// change.
pub(crate) fn start_connectivity() {
    let now = read();
    log::debug!("connectivity: {now:?}");
    report_connectivity(now);
    let Some(window) = web_sys::window() else {
        return;
    };
    let changed = Closure::<dyn Fn()>::new(|| {
        let now = read();
        if report_connectivity(now) {
            log::debug!("connectivity: {now:?}");
        }
    });
    let listener = changed.as_ref().unchecked_ref();
    let _ = window.add_event_listener_with_callback("online", listener);
    let _ = window.add_event_listener_with_callback("offline", listener);
    if let Some(connection) = connection().and_then(|c| c.dyn_into::<web_sys::EventTarget>().ok()) {
        let _ = connection.add_event_listener_with_callback("change", listener);
    }
    // Listened to for the life of the page.
    changed.forget();
}
