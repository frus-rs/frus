// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus-platform-example`: **calling the platform yourself**, for a capability frus does not
//! wrap. The screen shows the battery's charge and whether it is charging, read by
//! [`battery`] through `frus::platform` alone: a broadcast receiver implemented in Rust on
//! Android, the browser's battery API on the web. On Android a button also shows a toast,
//! which must be made on Android's main thread (`run_on_main_thread`, a `Runnable` proxy).
//!
//! `frus run -d android` in `crates/frus-platform-example`; the readings are logged under
//! `battery:`.

#![deny(missing_docs)]

pub mod battery;

use frus::{
    button, column, text, Align, Container, FrusApp, Justify, MediaQuery, Size, State,
    StateContext, StatefulWidget, Variant, Widget,
};

/// The example's one screen.
#[derive(Clone)]
pub struct BatteryScreen;

/// What the screen keeps: the latest reading, or why there is none.
#[derive(Default)]
pub struct BatteryState {
    /// The latest reading.
    pub battery: Option<battery::Battery>,
    /// Why the battery cannot be read here, when it cannot.
    pub unavailable: Option<String>,
}

impl StatefulWidget for BatteryScreen {
    type State = BatteryState;

    fn create_state(&self) -> BatteryState {
        BatteryState::default()
    }
}

impl State for BatteryState {
    type Widget = BatteryScreen;

    fn init_state(&mut self, cx: &StateContext<Self>) {
        #[cfg(target_os = "android")]
        battery::check_from_a_worker();
        match battery::watch() {
            Ok(mut watch) => {
                // A task of the interface's thread, owned by this state: each reading the
                // system reports, on whatever thread, wakes it, and it changes the state.
                let state = cx.handle();
                state.clone().spawn(async move {
                    while let Some(reading) = watch.next().await {
                        state.set_state(|s| s.battery = Some(reading));
                    }
                });
            }
            Err(err) => {
                frus::log::warn!("battery: {err}");
                self.unavailable = Some(err.to_string());
            }
        }
    }

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let Size { width, height } = MediaQuery::of().size;
        let theme = cx.theme();
        let line = match (&self.battery, &self.unavailable) {
            (Some(battery), _) => battery.describe(),
            (None, Some(why)) => why.clone(),
            (None, None) => "Reading the battery…".to_string(),
        };
        let content = column![
            text("frus · platform").size(14.0).color(theme.muted),
            text("Battery").size(18.0),
            text(line).size(28.0),
            button("Show a toast", cx.callback(|_: &mut BatteryState| toast()))
                .variant(Variant::Outlined),
        ]
        .gap(16.0)
        .align(Align::Center);

        let centered = column![content]
            .width(width)
            .height(height)
            .justify(Justify::Center)
            .align(Align::Center);
        Box::new(
            Container::new()
                .width(width)
                .height(height)
                .color(theme.background)
                .child(centered),
        )
    }
}

/// A toast on Android, made on Android's main thread; a log line elsewhere.
fn toast() {
    #[cfg(target_os = "android")]
    {
        use frus::platform::android::{jni::objects::JValue, run_on_main_thread};
        let posted = run_on_main_thread(|env, activity| {
            let thread = env
                .call_static_method(
                    "android/os/Looper",
                    "myLooper",
                    "()Landroid/os/Looper;",
                    &[],
                )?
                .l()?;
            let main = env
                .call_static_method(
                    "android/os/Looper",
                    "getMainLooper",
                    "()Landroid/os/Looper;",
                    &[],
                )?
                .l()?;
            let on_main = env.is_same_object(&thread, &main)?;
            frus::log::info!("toast: running on Android's main thread: {on_main}");
            let message = env.new_string("Hello from Rust, on Android's main thread")?;
            let toast = env
                .call_static_method(
                    "android/widget/Toast",
                    "makeText",
                    "(Landroid/content/Context;Ljava/lang/CharSequence;I)Landroid/widget/Toast;",
                    &[
                        JValue::Object(activity),
                        JValue::Object(&message),
                        JValue::Int(0),
                    ],
                )?
                .l()?;
            env.call_method(&toast, "show", "()V", &[])?;
            Ok(())
        });
        if let Err(err) = posted {
            frus::log::warn!("toast: {err}");
        }
    }
    #[cfg(not(target_os = "android"))]
    frus::log::info!("toast: toasts are an Android thing");
}

// **A single entry point**: it generates the desktop, Android and Web entries.
frus::main!(FrusApp::stateful(BatteryScreen).title("frus — platform"));
