// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The battery's charge and whether it is charging, written the way a crate adds a platform
//! capability frus does not wrap: with `frus::platform` only, and no Java, Kotlin or
//! JavaScript.
//!
//! - **Android**: a `BroadcastReceiver` for `ACTION_BATTERY_CHANGED`, implemented in Rust with
//!   `JavaProxy::broadcast_receiver`. The broadcast is sticky, so the receiver hears the
//!   current state at once, then each change (plugged in, unplugged, a percent more or less).
//! - **The web**: `navigator.getBattery()` where the browser has it (Chromium), and its
//!   `levelchange` and `chargingchange` events.
//! - **Elsewhere**: [`watch`] answers `Unavailable`.
//!
//! Each reading is sent through a `frus::platform::channel` from the thread the system called
//! on, and arrives on the interface's thread at [`BatteryWatch::next`].

use frus::platform::{self, PlatformError};

/// One reading of the battery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    /// The charge, 0 to 100; `None` when the system does not say.
    pub percent: Option<u8>,
    /// Whether it is charging (or full on a charger).
    pub charging: bool,
}

impl Battery {
    /// The line the example shows.
    pub fn describe(&self) -> String {
        let charge = match self.percent {
            Some(percent) => format!("{percent} %"),
            None => "unknown charge".to_string(),
        };
        if self.charging {
            format!("{charge}, charging")
        } else {
            format!("{charge}, on battery")
        }
    }
}

/// The battery's readings, from [`watch`]. Dropping it stops listening.
pub struct BatteryWatch {
    readings: platform::Receiver<Battery>,
    #[cfg(target_os = "android")]
    _receiver: android::Registration,
}

impl BatteryWatch {
    /// The next reading, on the interface's thread; the first comes at once.
    pub async fn next(&mut self) -> Option<Battery> {
        self.readings.recv().await
    }
}

/// Starts listening to the battery.
pub fn watch() -> Result<BatteryWatch, PlatformError> {
    #[allow(unused_variables)]
    let (sender, readings) = platform::channel::<Battery>();
    #[cfg(target_os = "android")]
    {
        let receiver = android::register(sender)?;
        Ok(BatteryWatch {
            readings,
            _receiver: receiver,
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        web::listen(sender)?;
        Ok(BatteryWatch { readings })
    }
    #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
    {
        let _ = readings;
        Err(PlatformError::Unavailable(
            "this example reads the battery on Android and the web only".into(),
        ))
    }
}

/// Asks `BatteryManager` for the charge from a plain worker thread through `with_env`, and
/// logs it with the thread's Java name: the same number the broadcast gives, read on a
/// thread the system knows nothing of.
#[cfg(target_os = "android")]
pub fn check_from_a_worker() {
    let spawned = std::thread::Builder::new()
        .name("battery-worker".into())
        .spawn(|| match android::capacity() {
            Ok((percent, thread)) => frus::log::info!(
                "battery: BatteryManager says {percent} % (asked through with_env on the \
                 worker thread \"{thread}\")"
            ),
            Err(err) => frus::log::warn!("battery: the worker could not ask ({err})"),
        });
    if let Err(err) = spawned {
        frus::log::warn!("battery: no worker thread ({err})");
    }
}

/// `BatteryManager.BATTERY_STATUS_*` and the `EXTRA_*` names of the battery broadcast, read
/// into a [`Battery`]. Pure, so it is tested on any machine.
pub fn from_extras(level: i32, scale: i32, status: i32, plugged: i32) -> Battery {
    const CHARGING: i32 = 2;
    const FULL: i32 = 5;
    let percent = (level >= 0 && scale > 0).then(|| ((level * 100 + scale / 2) / scale) as u8);
    Battery {
        percent,
        charging: status == CHARGING || (status == FULL && plugged != 0),
    }
}

#[cfg(target_os = "android")]
mod android {
    use frus::platform::android::jni::objects::{JObject, JValue};
    use frus::platform::android::jni::JNIEnv;
    use frus::platform::android::{with_env, JavaProxy};
    use frus::platform::{PlatformError, Sender};

    use super::{from_extras, Battery};

    /// The registered receiver; dropping it unregisters it.
    pub(super) struct Registration {
        receiver: JavaProxy,
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            let receiver = self.receiver.as_obj();
            let _ = with_env(|env, activity| {
                env.call_method(
                    activity,
                    "unregisterReceiver",
                    "(Landroid/content/BroadcastReceiver;)V",
                    &[JValue::Object(receiver)],
                )?;
                Ok(())
            });
        }
    }

    /// One integer extra of the broadcast's intent, `-1` when missing.
    fn extra<'l>(
        env: &mut JNIEnv<'l>,
        intent: &JObject<'l>,
        name: &str,
    ) -> Result<i32, PlatformError> {
        let name = env.new_string(name)?;
        Ok(env
            .call_method(
                intent,
                "getIntExtra",
                "(Ljava/lang/String;I)I",
                &[JValue::Object(&name), JValue::Int(-1)],
            )?
            .i()?)
    }

    /// The Java name of the calling thread, for the log.
    fn java_thread(env: &mut JNIEnv) -> Result<String, PlatformError> {
        let thread = env
            .call_static_method(
                "java/lang/Thread",
                "currentThread",
                "()Ljava/lang/Thread;",
                &[],
            )?
            .l()?;
        let name = env
            .call_method(&thread, "getName", "()Ljava/lang/String;", &[])?
            .l()?;
        Ok(env.get_string(&name.into())?.into())
    }

    /// The charge as `BatteryManager.getIntProperty(BATTERY_PROPERTY_CAPACITY)` gives it, read
    /// on the calling thread (any thread: `with_env` attaches it for the call), and that
    /// thread's Java name.
    pub fn capacity() -> Result<(i32, String), PlatformError> {
        with_env(|env, activity| {
            const BATTERY_PROPERTY_CAPACITY: i32 = 4;
            let service = env.new_string("batterymanager")?;
            let manager = env
                .call_method(
                    activity,
                    "getSystemService",
                    "(Ljava/lang/String;)Ljava/lang/Object;",
                    &[JValue::Object(&service)],
                )?
                .l()?;
            let percent = env
                .call_method(
                    &manager,
                    "getIntProperty",
                    "(I)I",
                    &[JValue::Int(BATTERY_PROPERTY_CAPACITY)],
                )?
                .i()?;
            Ok((percent, java_thread(env)?))
        })
    }

    pub(super) fn register(sender: Sender<Battery>) -> Result<Registration, PlatformError> {
        with_env(move |env, activity| {
            // `onReceive(context, intent)`, on Android's main thread.
            let receiver = JavaProxy::broadcast_receiver(env, move |env, call| {
                if let Some(intent) = call.arg(1) {
                    let battery = from_extras(
                        extra(env, intent, "level")?,
                        extra(env, intent, "scale")?,
                        extra(env, intent, "status")?,
                        extra(env, intent, "plugged")?,
                    );
                    frus::log::info!(
                        "battery: {} (heard on the Java thread \"{}\")",
                        battery.describe(),
                        java_thread(env).unwrap_or_default()
                    );
                    sender.send(battery);
                }
                Ok(JObject::null())
            })?;
            let action = env.new_string("android.intent.action.BATTERY_CHANGED")?;
            let filter = env.new_object(
                "android/content/IntentFilter",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&action)],
            )?;
            env.call_method(
                activity,
                "registerReceiver",
                "(Landroid/content/BroadcastReceiver;Landroid/content/IntentFilter;)Landroid/content/Intent;",
                &[JValue::Object(receiver.as_obj()), JValue::Object(&filter)],
            )?;
            Ok(Registration { receiver })
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use frus::platform::web::{
        self, js_sys,
        wasm_bindgen::{closure::Closure, JsCast, JsValue},
        wasm_bindgen_futures::JsFuture,
        web_sys::BatteryManager,
    };
    use frus::platform::{PlatformError, Sender};

    use super::Battery;

    fn reading(manager: &BatteryManager) -> Battery {
        let level = manager.level();
        Battery {
            percent: level
                .is_finite()
                .then(|| (level * 100.0).round().clamp(0.0, 100.0) as u8),
            charging: manager.charging(),
        }
    }

    pub(super) fn listen(sender: Sender<Battery>) -> Result<(), PlatformError> {
        let navigator = web::window()?.navigator();
        // `getBattery` is not in web-sys's bindings (it is not on every browser): reached by
        // name, as any API the bindings lack.
        let get = js_sys::Reflect::get(&navigator, &JsValue::from_str("getBattery"))?;
        let Some(get) = get.dyn_ref::<js_sys::Function>() else {
            return Err(PlatformError::Unavailable(
                "this browser does not report the battery".into(),
            ));
        };
        let promise: js_sys::Promise = get.call0(&navigator)?.dyn_into()?;
        web::wasm_bindgen_futures::spawn_local(async move {
            let manager: BatteryManager = match JsFuture::from(promise).await {
                Ok(manager) => manager.unchecked_into(),
                Err(err) => {
                    frus::log::warn!("battery: {}", PlatformError::from(err));
                    return;
                }
            };
            let send = {
                let manager = manager.clone();
                move || {
                    sender.send(reading(&manager));
                }
            };
            send();
            let changed = Closure::<dyn Fn()>::new(send);
            manager.set_onlevelchange(Some(changed.as_ref().unchecked_ref()));
            manager.set_onchargingchange(Some(changed.as_ref().unchecked_ref()));
            // Kept for the page's life: the example watches until it is closed.
            changed.forget();
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_extras_read_as_a_battery() {
        assert_eq!(
            from_extras(57, 100, 3, 0),
            Battery {
                percent: Some(57),
                charging: false
            }
        );
        assert_eq!(
            from_extras(200, 255, 2, 1),
            Battery {
                percent: Some(78),
                charging: true
            }
        );
        // Full on the charger counts as charging; full off it does not.
        assert!(from_extras(100, 100, 5, 2).charging);
        assert!(!from_extras(100, 100, 5, 0).charging);
        assert_eq!(from_extras(-1, 100, 1, 0).percent, None);
    }

    #[test]
    fn a_desktop_build_says_the_battery_is_unavailable() {
        assert!(matches!(watch(), Err(PlatformError::Unavailable(_))));
    }

    #[test]
    fn a_reading_describes_itself() {
        let battery = Battery {
            percent: Some(80),
            charging: true,
        };
        assert_eq!(battery.describe(), "80 %, charging");
    }
}
