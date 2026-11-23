// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The web**: the browser's API, from Rust (milestone 839).
//!
//! The browser is reached through `web-sys`, typed bindings generated from the web's own
//! interface definitions: no JavaScript to write, no message to encode. This module hands out
//! the page's `window` and `document`, and re-exports the crates frus is built with, so a
//! crate calling the browser matches frus's versions without declaring them:
//!
//! ```no_run
//! use frus_shell::platform::web;
//!
//! let title = web::document().map(|document| document.title());
//! let online = web::window().map(|window| window.navigator().on_line());
//! # let _ = (title, online, web::js_sys::Date::now());
//! ```
//!
//! **`web-sys` features.** `web-sys` compiles only the interfaces a build asks for. An
//! interface frus does not use itself (`BatteryManager`, `Geolocation`…) is turned on by the
//! crate that needs it, with its own `web-sys` dependency for the web target listing that
//! feature; Cargo merges it into the one `web-sys` frus re-exports here.
//!
//! **Threads.** The page has one: the interface's. A browser callback (a `Closure` given to an
//! event listener, a promise's continuation) runs on it too, between frames. Still, it runs
//! outside the frame, so it hands what it learned to the interface's state the same way as on
//! the other platforms, through [`channel`](super::channel), whose wake asks for the frame
//! that shows it. A promise is awaited directly in a task of the interface's thread, with
//! [`wasm_bindgen_futures::JsFuture`].

pub use js_sys;
pub use wasm_bindgen;
pub use wasm_bindgen_futures;
pub use web_sys;

use super::PlatformError;

/// The page's `window`; `Unavailable` in a worker, which has none.
pub fn window() -> Result<web_sys::Window, PlatformError> {
    web_sys::window().ok_or_else(|| PlatformError::Unavailable("no window (a worker?)".into()))
}

/// The page's `document`.
pub fn document() -> Result<web_sys::Document, PlatformError> {
    window()?
        .document()
        .ok_or_else(|| PlatformError::Unavailable("the window has no document".into()))
}

impl From<wasm_bindgen::JsValue> for PlatformError {
    /// A value a browser call threw: its text (an `Error`'s `toString()`).
    fn from(value: wasm_bindgen::JsValue) -> Self {
        use wasm_bindgen::JsCast;
        let text = match value.dyn_ref::<js_sys::Error>() {
            Some(error) => String::from(error.to_string()),
            None => value.as_string().unwrap_or_else(|| format!("{value:?}")),
        };
        PlatformError::Exception(text)
    }
}
