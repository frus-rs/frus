// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Android**: Google Play Billing, through `FrusBilling` (a dex of its own, compiled in with
//! the `purchases` feature only).
//!
//! The Billing Library is Java and is not in frus-shell: `frus build` / `frus run` find
//! [`PLAY_BILLING_MARKER`] in the application's library and put the library into the package
//! (its code, its activity, the `BILLING` permission). `FrusBilling` reaches it through its
//! parent class loader, the package's own. An application packaged otherwise (plain
//! cargo-apk) has no library: the store then answers [`PurchaseError::Unsupported`] and says
//! why, instead of failing on the first call.
//!
//! At start-up only references are kept ([`remember`]); the class is loaded when the
//! application first asks for the store ([`Purchases::instance`](super::Purchases::instance)).
//! Each call carries a request number; `FrusBilling` answers it once, from the main thread,
//! through `nativeEvent`, which hands the answer to the call's [`Replier`]. Request 0 carries
//! purchase updates, which go to the [`Sink`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
use jni::sys::{jint, jlong};
use jni::{JNIEnv, JavaVM};

use super::{
    play, Product, ProductKind, ProductsResponse, PurchaseError, PurchaseUpdate, Replier, Reply,
    Sink, Store,
};

/// What `frus build` looks for in an application's library to put the Billing Library into its
/// package, with the version to put: the one `FrusBilling` is compiled against
/// (`scripts/build-billing-dex.sh`). It must be kept in the library: [`PlayStore::new`] logs
/// it.
pub(crate) const PLAY_BILLING_MARKER: &str = "frus-play-billing-library=8.3.0;";

struct Origin {
    vm: JavaVM,
    loader: GlobalRef,
    activity: GlobalRef,
}

// SAFETY: JavaVM and the GlobalRefs are shareable across threads, per the JNI contract.
unsafe impl Send for Origin {}
unsafe impl Sync for Origin {}

static ORIGIN: OnceLock<Origin> = OnceLock::new();

type Answer = Box<dyn FnOnce(i32, String, Option<String>) + Send>;

/// The calls waiting for their answer, by request number.
static WAITING: Mutex<Option<HashMap<u64, Answer>>> = Mutex::new(None);
static NEXT: AtomicU64 = AtomicU64::new(1);
/// Where purchase updates go: the store's.
static UPDATES: Mutex<Option<Arc<Sink>>> = Mutex::new(None);

fn clear_exception(env: &mut JNIEnv) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

/// Keeps what loading the class will take, at start-up: nothing is loaded yet.
pub(crate) fn remember(env: &mut JNIEnv, loader: &JObject, activity: &JObject) {
    let origin = (|| -> jni::errors::Result<Origin> {
        Ok(Origin {
            vm: env.get_java_vm()?,
            loader: env.new_global_ref(loader)?,
            activity: env.new_global_ref(activity)?,
        })
    })();
    match origin {
        Ok(origin) => {
            let _ = ORIGIN.set(origin);
        }
        Err(err) => {
            clear_exception(env);
            log::warn!("in-app purchases unavailable ({err})");
        }
    }
}

static BILLING_DEX: &[u8] = include_bytes!("../../assets/frus_billing.dex");

pub(crate) struct PlayStore {
    vm: JavaVM,
    billing: GlobalRef,
}

// SAFETY: as above.
unsafe impl Send for PlayStore {}
unsafe impl Sync for PlayStore {}

fn load_class<'a>(
    env: &mut JNIEnv<'a>,
    loader: &JObject,
    name: &str,
) -> jni::errors::Result<JClass<'a>> {
    let name = env.new_string(name)?;
    Ok(env
        .call_method(
            loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?
        .into())
}

impl PlayStore {
    pub(crate) fn new(sink: Arc<Sink>) -> Result<Self, &'static str> {
        log::info!(
            "in-app purchases: Google Play Billing ({})",
            std::hint::black_box(PLAY_BILLING_MARKER)
        );
        let origin = ORIGIN
            .get()
            .ok_or("the Android bridge to Google Play did not start")?;
        let mut env = origin
            .vm
            .attach_current_thread_permanently()
            .map_err(|_| "the Java VM could not be reached")?;
        // The Billing Library must be in the package.
        let library = load_class(
            &mut env,
            origin.loader.as_obj(),
            "com.android.billingclient.api.BillingClient",
        );
        if library.is_err() {
            clear_exception(&mut env);
            log::warn!(
                "in-app purchases: Google Play Billing Library is not in the package; build \
                 the application with `frus build` or `frus run`, which add it"
            );
            return Err(
                "Google Play Billing Library is not in the package (build with `frus build`)",
            );
        }
        let made = env.with_local_frame(8, |env| -> jni::errors::Result<GlobalRef> {
            // SAFETY: the bytes are static; the buffer only reads them.
            let buffer = unsafe {
                env.new_direct_byte_buffer(BILLING_DEX.as_ptr() as *mut u8, BILLING_DEX.len())
            }?;
            let loader = env.new_object(
                "dalvik/system/InMemoryDexClassLoader",
                "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
                &[
                    JValue::Object(&buffer),
                    JValue::Object(origin.loader.as_obj()),
                ],
            )?;
            let class = load_class(env, &loader, "dev.frus.billing.FrusBilling")?;
            env.register_native_methods(
                &class,
                &[jni::NativeMethod {
                    name: "nativeEvent".into(),
                    sig: "(JILjava/lang/String;Ljava/lang/String;)V".into(),
                    fn_ptr: native_event as *mut _,
                }],
            )?;
            let billing = env
                .call_static_method(
                    &class,
                    "create",
                    "(Landroid/app/Activity;)Ldev/frus/billing/FrusBilling;",
                    &[JValue::Object(origin.activity.as_obj())],
                )?
                .l()?;
            env.new_global_ref(billing)
        });
        match made {
            Ok(billing) => {
                if let Ok(mut updates) = UPDATES.lock() {
                    *updates = Some(sink);
                }
                Ok(PlayStore {
                    vm: env
                        .get_java_vm()
                        .map_err(|_| "the Java VM could not be reached")?,
                    billing,
                })
            }
            Err(err) => {
                clear_exception(&mut env);
                log::warn!("in-app purchases unavailable ({err})");
                Err("the Google Play bridge could not be loaded")
            }
        }
    }

    /// Sends a call; `answer` gets its response code, message and payload.
    fn call(
        &self,
        method: &str,
        sig: &str,
        args: impl FnOnce(&mut JNIEnv) -> jni::errors::Result<Vec<GlobalRef>>,
        answer: Answer,
    ) {
        let request = NEXT.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut waiting) = WAITING.lock() {
            waiting
                .get_or_insert_with(HashMap::new)
                .insert(request, answer);
        }
        let sent = (|| -> Result<(), String> {
            let mut env = self
                .vm
                .attach_current_thread_permanently()
                .map_err(|e| e.to_string())?;
            let result = env.with_local_frame(16, |env| -> jni::errors::Result<()> {
                let objects = args(env)?;
                let mut values = vec![JValue::Long(request as jlong)];
                values.extend(objects.iter().map(|o| JValue::Object(o.as_obj())));
                env.call_method(self.billing.as_obj(), method, sig, &values)?;
                Ok(())
            });
            result.map_err(|err| {
                clear_exception(&mut env);
                err.to_string()
            })
        })();
        if let Err(why) = sent {
            if let Some(answer) = take(request) {
                answer(6, why, None);
            }
        }
    }
}

fn take(request: u64) -> Option<Answer> {
    WAITING
        .lock()
        .ok()
        .and_then(|mut w| w.as_mut().and_then(|w| w.remove(&request)))
}

fn string(env: &mut JNIEnv, text: &str) -> jni::errors::Result<GlobalRef> {
    let text = env.new_string(text)?;
    env.new_global_ref(text)
}

/// The answer as a result: `Ok` for response code 0.
fn done(replier: Replier<Result<(), PurchaseError>>) -> Answer {
    Box::new(move |code, message, _| {
        replier.send(if code == play::OK {
            Ok(())
        } else {
            Err(play::error(code, &message))
        })
    })
}

impl Store for PlayStore {
    fn is_available(&self) -> Reply<bool> {
        let (reply, replier) = Reply::new();
        self.call(
            "available",
            "(J)V",
            |_| Ok(Vec::new()),
            Box::new(move |code, message, _| {
                if code != play::OK {
                    log::info!(
                        "in-app purchases: Google Play is not available ({code}: {message})"
                    );
                }
                replier.send(code == play::OK)
            }),
        );
        reply
    }

    fn query(&self, ids: Vec<String>) -> Reply<Result<ProductsResponse, PurchaseError>> {
        let (reply, replier) = Reply::new();
        let asked = ids.clone();
        self.call(
            "query",
            "(J[Ljava/lang/String;)V",
            move |env| {
                let array =
                    env.new_object_array(ids.len() as i32, "java/lang/String", JObject::null())?;
                for (i, id) in ids.iter().enumerate() {
                    let id = env.new_string(id)?;
                    env.set_object_array_element(&array, i as i32, id)?;
                }
                Ok(vec![env.new_global_ref(array)?])
            },
            Box::new(move |code, message, payload| {
                replier.send(if code == play::OK {
                    Ok(play::products(payload.as_deref().unwrap_or(""), &asked))
                } else {
                    Err(play::error(code, &message))
                })
            }),
        );
        reply
    }

    fn buy(&self, product: &Product, account_id: Option<&str>) -> Reply<Result<(), PurchaseError>> {
        let (reply, replier) = Reply::new();
        let id = product.id.clone();
        let offer = product.offer.clone().unwrap_or_default();
        let account = account_id.unwrap_or_default().to_string();
        self.call(
            "buy",
            "(JLjava/lang/String;Ljava/lang/String;Ljava/lang/String;)V",
            move |env| {
                Ok(vec![
                    string(env, &id)?,
                    string(env, &offer)?,
                    string(env, &account)?,
                ])
            },
            done(replier),
        );
        reply
    }

    fn restore(&self) -> Reply<Result<(), PurchaseError>> {
        let (reply, replier) = Reply::new();
        self.call(
            "owned",
            "(J)V",
            |_| Ok(Vec::new()),
            Box::new(move |code, message, payload| {
                if code == play::OK {
                    send_updates(play::purchases(payload.as_deref().unwrap_or(""), true));
                    replier.send(Ok(()))
                } else {
                    replier.send(Err(play::error(code, &message)))
                }
            }),
        );
        reply
    }

    fn complete(&self, purchase: &PurchaseUpdate) -> Reply<Result<(), PurchaseError>> {
        let (reply, replier) = Reply::new();
        let method = if purchase.kind == ProductKind::Consumable {
            "consume"
        } else {
            "acknowledge"
        };
        let token = purchase.server_verification_data.clone();
        self.call(
            method,
            "(JLjava/lang/String;)V",
            move |env| Ok(vec![string(env, &token)?]),
            done(replier),
        );
        reply
    }
}

fn send_updates(updates: Vec<PurchaseUpdate>) {
    let sink = UPDATES.lock().ok().and_then(|s| s.clone());
    if let Some(sink) = sink {
        sink.send(updates);
    }
}

extern "system" fn native_event<'a>(
    mut env: JNIEnv<'a>,
    _class: JClass<'a>,
    request: jlong,
    code: jint,
    message: JString<'a>,
    payload: JString<'a>,
) {
    let mut text = |s: &JString| -> Option<String> {
        if s.is_null() {
            return None;
        }
        env.get_string(s).ok().map(Into::into)
    };
    let message = text(&message).unwrap_or_default();
    let payload = text(&payload);
    if request == 0 {
        send_updates(play::updated(
            code,
            &message,
            payload.as_deref().unwrap_or(""),
        ));
    } else if let Some(answer) = take(request as u64) {
        answer(code, message, payload);
    }
}
