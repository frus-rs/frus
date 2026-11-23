// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Android**: the Java VM, from Rust (milestone 839).
//!
//! # Calling Java
//!
//! [`with_env`] runs a closure with a `JNIEnv` and the application's activity:
//!
//! ```no_run
//! use frus_shell::platform::android::{jni::objects::JValue, with_env};
//!
//! // The battery's charge, in percent, from `BatteryManager`.
//! let percent = with_env(|env, activity| {
//!     let name = env.new_string("batterymanager")?;
//!     let manager = env
//!         .call_method(
//!             activity,
//!             "getSystemService",
//!             "(Ljava/lang/String;)Ljava/lang/Object;",
//!             &[JValue::Object(&name)],
//!         )?
//!         .l()?;
//!     // BatteryManager.BATTERY_PROPERTY_CAPACITY
//!     Ok(env.call_method(&manager, "getIntProperty", "(I)I", &[JValue::Int(4)])?.i()?)
//! });
//! ```
//!
//! # Threads
//!
//! - **Any thread may call [`with_env`].** The interface's thread (the one running the
//!   application, `android_main`'s) is attached to the VM for its whole life. Any other
//!   thread — a worker, a thread of the shell's executor — is attached for the call and
//!   detached when it returns, unless it was attached already, in which case it is left so.
//! - **The interface's thread is not Android's main thread.** frus runs on a thread of its
//!   own; Android's main ("UI") thread is the one its looper and its views live on. An API
//!   that must be called there — a `Toast`, a `View`, anything that says "must be called
//!   from the main thread" — goes through [`run_on_main_thread`].
//! - **Never block Android's main thread waiting for the interface's thread**, nor the
//!   reverse: each may be waiting for the other (the keyboard and the screen reader cross
//!   between them).
//! - **References are local to the call.** Every local reference made inside the closure is
//!   freed when it returns (it runs in a local frame), so a loop of calls does not fill the
//!   reference table. Keep a Java object beyond the call with `env.new_global_ref`.
//! - **Exceptions are caught.** A Java exception the closure leaves pending is cleared and
//!   returned as [`PlatformError::Exception`] with its text; one left pending would end the
//!   process at the next JNI call anybody makes.
//!
//! # Java calling Rust
//!
//! [`JavaProxy`] implements any Java **interface** in Rust (`java.lang.reflect.Proxy`): a
//! listener, an observer, a `Runnable`. [`JavaProxy::broadcast_receiver`] does the same for a
//! `BroadcastReceiver`, which is a class. No crate ships Java or a dex for it: the one
//! forwarding class is in the dex frus already carries. The handler runs on the thread Java
//! calls it on; what it learns goes to the interface's thread through
//! [`channel`](super::channel).

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// The `jni` crate frus is built with, for the types [`with_env`] and [`JavaProxy`] hand
/// out: a crate calling Java needs no `jni` dependency of its own.
pub use jni;

use jni::objects::{GlobalRef, JClass, JObject, JObjectArray, JString, JValue};
use jni::sys::{jlong, jobject};
use jni::{JNIEnv, JavaVM};

use super::PlatformError;

impl From<jni::errors::Error> for PlatformError {
    fn from(err: jni::errors::Error) -> Self {
        PlatformError::Binding(err.to_string())
    }
}

/// The Java VM of the process, once there is an activity.
fn java_vm() -> Result<&'static JavaVM, PlatformError> {
    static VM: OnceLock<JavaVM> = OnceLock::new();
    if let Some(vm) = VM.get() {
        return Ok(vm);
    }
    let (vm, _) = context()?;
    // SAFETY: the pointer is the process's `JavaVM`, which lives as long as the process.
    let vm = unsafe { JavaVM::from_raw(vm.cast()) }?;
    Ok(VM.get_or_init(|| vm))
}

/// The VM and the activity, as the native activity recorded them; `Unavailable` before it
/// has, and after it was destroyed.
fn context() -> Result<(*mut std::ffi::c_void, jobject), PlatformError> {
    // `android_context` panics when nothing recorded one (a background job before any
    // activity); asked here as a question.
    let context = catch_unwind(ndk_context::android_context)
        .map_err(|_| PlatformError::Unavailable("no activity has started".into()))?;
    if context.vm().is_null() || context.context().is_null() {
        return Err(PlatformError::Unavailable("no activity has started".into()));
    }
    Ok((context.vm(), context.context().cast()))
}

/// Runs `f` with a `JNIEnv` for this thread and the application's activity (an
/// `android.app.Activity`, also its `Context`).
///
/// See the [module's notes](self) for the threading rules: any thread, attached for the call
/// if need be; local references freed when `f` returns; a Java exception cleared and returned
/// as [`PlatformError::Exception`].
pub fn with_env<R>(
    f: impl FnOnce(&mut JNIEnv, &JObject) -> Result<R, PlatformError>,
) -> Result<R, PlatformError> {
    let vm = java_vm()?;
    let (_, activity) = context()?;
    // Attached for the call when the thread was not; left attached when it was.
    let mut guard = vm.attach_current_thread()?;
    let env: &mut JNIEnv = &mut guard;
    // SAFETY: a global reference the native activity holds for the activity's lifetime; this
    // wrapper does not delete it.
    let activity = unsafe { JObject::from_raw(activity) };
    env.with_local_frame(16, |env| -> Result<R, PlatformError> {
        let result = f(env, &activity);
        // An exception left pending, whatever `f` answered, is the platform's answer.
        if let Some(text) = take_exception(env) {
            return Err(PlatformError::Exception(text));
        }
        result
    })
}

/// Clears a pending Java exception and gives its text.
fn take_exception(env: &mut JNIEnv) -> Option<String> {
    if !env.exception_check().unwrap_or(false) {
        return None;
    }
    let throwable = env.exception_occurred().ok();
    let _ = env.exception_clear();
    let text = throwable.and_then(|throwable| {
        let text = env
            .call_method(&throwable, "toString", "()Ljava/lang/String;", &[])
            .and_then(|value| value.l())
            .ok()?;
        env.get_string(&JString::from(text)).ok().map(String::from)
    });
    // `toString` itself may throw.
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    Some(text.unwrap_or_else(|| "a Java exception".into()))
}

/// Runs `f` on **Android's main thread** (its looper's), where views and other main-thread
/// APIs must be called, and returns at once; `f` runs later, in order with the main thread's
/// other work.
///
/// ```no_run
/// use frus_shell::platform::android::{jni::objects::JValue, run_on_main_thread};
///
/// // A toast must be made on the main thread.
/// run_on_main_thread(|env, activity| {
///     let text = env.new_string("Saved")?;
///     let toast = env
///         .call_static_method(
///             "android/widget/Toast",
///             "makeText",
///             "(Landroid/content/Context;Ljava/lang/CharSequence;I)Landroid/widget/Toast;",
///             &[JValue::Object(activity), JValue::Object(&text), JValue::Int(0)],
///         )?
///         .l()?;
///     env.call_method(&toast, "show", "()V", &[])?;
///     Ok(())
/// })
/// .ok();
/// ```
///
/// Do not wait on the interface's thread for `f` to have run: see the [threading
/// rules](self#threads).
pub fn run_on_main_thread(
    f: impl FnOnce(&mut JNIEnv, &JObject) -> Result<(), PlatformError> + Send + 'static,
) -> Result<(), PlatformError> {
    let work = Mutex::new(Some(f));
    // The proxy keeps itself until it has run: it is dropped from its own handler.
    let keep: Arc<Mutex<Option<JavaProxy>>> = Arc::new(Mutex::new(None));
    let kept = keep.clone();
    with_env(move |env, _| {
        let runnable = JavaProxy::new(env, &["java.lang.Runnable"], move |env, _call| {
            let work = work.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(work) = work {
                let (_, activity) = context()?;
                // SAFETY: as in `with_env`.
                let activity = unsafe { JObject::from_raw(activity) };
                if let Err(err) = work(env, &activity) {
                    let text = take_exception(env).unwrap_or_else(|| err.to_string());
                    log::warn!("platform: work on the main thread failed ({text})");
                }
            }
            // Run once: let the proxy go.
            kept.lock().unwrap_or_else(|e| e.into_inner()).take();
            Ok(JObject::null())
        })?;
        let looper = env
            .call_static_method(
                "android/os/Looper",
                "getMainLooper",
                "()Landroid/os/Looper;",
                &[],
            )?
            .l()?;
        let handler = env.new_object(
            "android/os/Handler",
            "(Landroid/os/Looper;)V",
            &[JValue::Object(&looper)],
        )?;
        // Kept before it is posted: the main thread may run it before `post` returns.
        let object = runnable.global_ref().clone();
        *keep.lock().unwrap_or_else(|e| e.into_inner()) = Some(runnable);
        let posted = env
            .call_method(
                &handler,
                "post",
                "(Ljava/lang/Runnable;)Z",
                &[JValue::Object(object.as_obj())],
            )
            .and_then(|posted| posted.z());
        match posted {
            Ok(true) => Ok(()),
            Ok(false) => {
                keep.lock().unwrap_or_else(|e| e.into_inner()).take();
                Err(PlatformError::Unavailable(
                    "the main thread's looper is exiting".into(),
                ))
            }
            Err(err) => {
                keep.lock().unwrap_or_else(|e| e.into_inner()).take();
                Err(err.into())
            }
        }
    })
}

// --- Java calling Rust ---------------------------------------------------------------------

/// What a [`JavaProxy`] runs for each call Java makes on it.
type Handler = dyn for<'l> Fn(&mut JNIEnv<'l>, &ProxyCall<'l>) -> Result<JObject<'l>, PlatformError>
    + Send
    + Sync;

/// The handlers of the live proxies, by the number their Java half carries. A number whose
/// proxy was dropped is found missing, and the call answers the default.
static HANDLERS: Mutex<Option<HashMap<jlong, Arc<Handler>>>> = Mutex::new(None);
static NEXT_ID: AtomicI64 = AtomicI64::new(1);

/// The bundled dex's loader, kept by the shell at start-up; the forwarding class is loaded
/// from it by the first proxy.
static DEX_LOADER: OnceLock<GlobalRef> = OnceLock::new();
/// `dev.frus.input.FrusProxy`, loaded, its native wired.
static PROXY_CLASS: Mutex<Option<GlobalRef>> = Mutex::new(None);

/// Keeps the loader of the bundled dex, for the first [`JavaProxy`]. Called by the shell
/// when it loads the dex; nothing is loaded until a proxy is made.
pub(crate) fn remember(env: &mut JNIEnv, loader: &JObject) {
    match env.new_global_ref(loader) {
        Ok(loader) => {
            let _ = DEX_LOADER.set(loader);
        }
        Err(err) => {
            let _ = take_exception(env);
            log::warn!("platform proxies unavailable ({err})");
        }
    }
}

/// `FrusProxy`, loaded from the bundled dex the first time, with its native registered.
fn proxy_class(env: &mut JNIEnv) -> Result<GlobalRef, PlatformError> {
    let mut slot = PROXY_CLASS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(class) = slot.as_ref() {
        return Ok(class.clone());
    }
    let loader = DEX_LOADER.get().ok_or_else(|| {
        PlatformError::Unavailable("the bridge's dex is not loaded (no window yet)".into())
    })?;
    let loaded = (|| -> jni::errors::Result<GlobalRef> {
        let name = env.new_string("dev.frus.input.FrusProxy")?;
        let class = env
            .call_method(
                loader.as_obj(),
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[JValue::Object(&name)],
            )?
            .l()?;
        let class = JClass::from(class);
        env.register_native_methods(
            &class,
            &[jni::NativeMethod {
                name: "nativeInvoke".into(),
                sig: "(JLjava/lang/Object;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/Object;"
                    .into(),
                fn_ptr: native_invoke as *mut _,
            }],
        )?;
        env.new_global_ref(&class)
    })();
    let class = match loaded {
        Ok(class) => class,
        Err(err) => {
            let text = take_exception(env).unwrap_or_else(|| err.to_string());
            return Err(PlatformError::Unavailable(format!(
                "the proxy class could not be loaded: {text}"
            )));
        }
    };
    *slot = Some(class.clone());
    Ok(class)
}

/// One call Java made on a [`JavaProxy`]: the method's name and its arguments.
///
/// Arguments of a primitive type arrive boxed (`java.lang.Integer` for an `int`); the
/// `*_arg` methods unbox them.
pub struct ProxyCall<'l> {
    method: String,
    proxy: JObject<'l>,
    args: Vec<JObject<'l>>,
}

impl<'l> ProxyCall<'l> {
    /// The name of the method called, as declared in the interface (`"onSensorChanged"`); a
    /// broadcast receiver's is `"onReceive"`.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// The Java object the call was made on: the proxy, or the receiver.
    pub fn this(&self) -> &JObject<'l> {
        &self.proxy
    }

    /// Every argument, in order (`null` ones included).
    pub fn args(&self) -> &[JObject<'l>] {
        &self.args
    }

    /// Argument `index`, `None` past the last.
    pub fn arg(&self, index: usize) -> Option<&JObject<'l>> {
        self.args.get(index)
    }

    fn unbox(
        &self,
        env: &mut JNIEnv<'l>,
        index: usize,
        method: &str,
        sig: &str,
    ) -> jni::errors::Result<jni::objects::JValueOwned<'l>> {
        let arg = self
            .args
            .get(index)
            .ok_or(jni::errors::Error::NullPtr("no argument at that index"))?;
        if arg.is_null() {
            return Err(jni::errors::Error::NullPtr("a null argument"));
        }
        env.call_method(arg, method, sig, &[])
    }

    /// Argument `index`, a boxed `int` (or `short`, `byte`: any `Number`).
    pub fn int_arg(&self, env: &mut JNIEnv<'l>, index: usize) -> jni::errors::Result<i32> {
        self.unbox(env, index, "intValue", "()I")?.i()
    }

    /// Argument `index`, a boxed `long`.
    pub fn long_arg(&self, env: &mut JNIEnv<'l>, index: usize) -> jni::errors::Result<i64> {
        self.unbox(env, index, "longValue", "()J")?.j()
    }

    /// Argument `index`, a boxed `float`.
    pub fn float_arg(&self, env: &mut JNIEnv<'l>, index: usize) -> jni::errors::Result<f32> {
        self.unbox(env, index, "floatValue", "()F")?.f()
    }

    /// Argument `index`, a boxed `double`.
    pub fn double_arg(&self, env: &mut JNIEnv<'l>, index: usize) -> jni::errors::Result<f64> {
        self.unbox(env, index, "doubleValue", "()D")?.d()
    }

    /// Argument `index`, a boxed `boolean`.
    pub fn bool_arg(&self, env: &mut JNIEnv<'l>, index: usize) -> jni::errors::Result<bool> {
        self.unbox(env, index, "booleanValue", "()Z")?.z()
    }

    /// Argument `index`, a `String` (or any `CharSequence`); `None` when it is `null`.
    pub fn string_arg(
        &self,
        env: &mut JNIEnv<'l>,
        index: usize,
    ) -> jni::errors::Result<Option<String>> {
        let Some(arg) = self.args.get(index) else {
            return Ok(None);
        };
        if arg.is_null() {
            return Ok(None);
        }
        let text = env
            .call_method(arg, "toString", "()Ljava/lang/String;", &[])?
            .l()?;
        Ok(Some(env.get_string(&JString::from(text))?.into()))
    }
}

/// A Java object implemented in Rust: a proxy for one or more **interfaces**
/// ([`new`](Self::new)), or a `BroadcastReceiver` ([`broadcast_receiver`](Self::broadcast_receiver)).
///
/// Each call Java makes on it runs the handler, **on the thread Java calls it on** (a
/// sensor's thread, Android's main thread for a receiver), with that call's [`ProxyCall`].
/// The handler answers a value for methods that return one (`JObject::null()` otherwise);
/// for a primitive return type, a boxed value, `null` standing for `false` or zero. An error
/// the handler returns is logged and the exception it left cleared: nothing is thrown into the
/// system's code. A panic is caught the same way.
///
/// The handler must not touch the interface's state: it sends what it learned through a
/// [`channel`](super::channel) to a task of the interface's thread.
///
/// Dropping the `JavaProxy` drops the handler. Unregister the listener first: calls Java
/// still makes then answer the default and run nothing.
///
/// ```no_run
/// use frus_shell::platform::{
///     self,
///     android::{jni::objects::{JFloatArray, JObject}, with_env, JavaProxy},
/// };
///
/// // A sensor's readings, sent to the interface's thread.
/// let (tx, mut rx) = platform::channel::<f32>();
/// let listener = with_env(|env, _activity| {
///     let listener = JavaProxy::new(
///         env,
///         &["android.hardware.SensorEventListener"],
///         move |env, call| {
///             if call.method() == "onSensorChanged" {
///                 if let Some(event) = call.arg(0) {
///                     let values = env.get_field(event, "values", "[F")?.l()?;
///                     let mut first = [0f32; 1];
///                     env.get_float_array_region(JFloatArray::from(values), 0, &mut first)?;
///                     tx.send(first[0]);
///                 }
///             }
///             Ok(JObject::null())
///         },
///     )?;
///     // ... then `SensorManager.registerListener(listener.as_obj(), sensor, delay)`.
///     Ok(listener)
/// });
/// frus_widgets::spawn_local(async move {
///     while let Some(reading) = rx.recv().await {
///         let _ = reading; // on the interface's thread
///     }
/// });
/// ```
pub struct JavaProxy {
    id: jlong,
    object: GlobalRef,
}

impl JavaProxy {
    /// A proxy implementing `interfaces` (Java names, `"android.view.View$OnClickListener"`
    /// for a nested one), each call running `handler`. The interfaces are looked up through
    /// the application's class loader, so the platform's and the package's are both found.
    ///
    /// `equals`, `hashCode` and `toString` are answered by identity without calling
    /// `handler`, so the proxy can be a key or sit in a listener list.
    pub fn new<F>(env: &mut JNIEnv, interfaces: &[&str], handler: F) -> Result<Self, PlatformError>
    where
        F: for<'l> Fn(&mut JNIEnv<'l>, &ProxyCall<'l>) -> Result<JObject<'l>, PlatformError>
            + Send
            + Sync
            + 'static,
    {
        let names: Vec<String> = interfaces.iter().map(|n| n.replace('/', ".")).collect();
        Self::make(env, Arc::new(handler), move |env, class, id| {
            let (_, activity) =
                context().map_err(|_| jni::errors::Error::NullPtr("the activity"))?;
            // SAFETY: as in `with_env`.
            let activity = unsafe { JObject::from_raw(activity) };
            let loader = env
                .call_method(
                    &activity,
                    "getClassLoader",
                    "()Ljava/lang/ClassLoader;",
                    &[],
                )?
                .l()?;
            let array =
                env.new_object_array(names.len() as i32, "java/lang/Class", JObject::null())?;
            for (index, name) in names.iter().enumerate() {
                let jname = env.new_string(name)?;
                let interface = env
                    .call_static_method(
                        "java/lang/Class",
                        "forName",
                        "(Ljava/lang/String;ZLjava/lang/ClassLoader;)Ljava/lang/Class;",
                        &[
                            JValue::Object(&jname),
                            JValue::Bool(0),
                            JValue::Object(&loader),
                        ],
                    )?
                    .l()?;
                env.set_object_array_element(&array, index as i32, interface)?;
            }
            env.call_static_method(
                class,
                "create",
                "(Ljava/lang/ClassLoader;[Ljava/lang/Class;J)Ljava/lang/Object;",
                &[
                    JValue::Object(&loader),
                    JValue::Object(&array),
                    JValue::Long(id),
                ],
            )?
            .l()
        })
    }

    /// A `BroadcastReceiver` whose `onReceive(context, intent)` runs `handler`, on the
    /// thread the receiver was registered for (Android's main thread by default); the call's
    /// method is `"onReceive"` and its arguments the context and the intent. Register it with
    /// `Context.registerReceiver(receiver.as_obj(), filter)`, and unregister it before
    /// dropping it.
    pub fn broadcast_receiver<F>(env: &mut JNIEnv, handler: F) -> Result<Self, PlatformError>
    where
        F: for<'l> Fn(&mut JNIEnv<'l>, &ProxyCall<'l>) -> Result<JObject<'l>, PlatformError>
            + Send
            + Sync
            + 'static,
    {
        Self::make(env, Arc::new(handler), |env, class, id| {
            env.call_static_method(
                class,
                "receiver",
                "(J)Landroid/content/BroadcastReceiver;",
                &[JValue::Long(id)],
            )?
            .l()
        })
    }

    /// Registers `handler` under a new number, and makes the Java half with `create`.
    fn make(
        env: &mut JNIEnv,
        handler: Arc<Handler>,
        create: impl for<'a> FnOnce(&mut JNIEnv<'a>, &JClass, jlong) -> jni::errors::Result<JObject<'a>>,
    ) -> Result<Self, PlatformError> {
        let class = proxy_class(env)?;
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        HANDLERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert_with(HashMap::new)
            .insert(id, handler);
        let made = env.with_local_frame(8, |env| -> jni::errors::Result<GlobalRef> {
            let object = create(env, class.as_obj().into(), id)?;
            env.new_global_ref(object)
        });
        match made {
            Ok(object) => Ok(JavaProxy { id, object }),
            Err(err) => {
                forget(id);
                Err(take_exception(env)
                    .map(PlatformError::Exception)
                    .unwrap_or_else(|| err.into()))
            }
        }
    }

    /// The Java object, to hand to the API that takes the listener.
    pub fn as_obj(&self) -> &JObject<'static> {
        self.object.as_obj()
    }

    /// A global reference to the Java object, for keeping it elsewhere.
    pub fn global_ref(&self) -> &GlobalRef {
        &self.object
    }
}

impl Drop for JavaProxy {
    fn drop(&mut self) {
        forget(self.id);
    }
}

impl std::fmt::Debug for JavaProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JavaProxy").field("id", &self.id).finish()
    }
}

fn forget(id: jlong) {
    if let Some(handlers) = HANDLERS.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        handlers.remove(&id);
    }
}

/// `FrusProxy.nativeInvoke(long, Object, String, Object[])`: a call Java made on a proxy, on
/// whichever thread made it.
extern "system" fn native_invoke<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    id: jlong,
    proxy: JObject<'l>,
    method: JString<'l>,
    args: JObjectArray<'l>,
) -> jobject {
    // Cloned out of the lock: the handler may make or drop proxies itself.
    let handler = HANDLERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|handlers| handlers.get(&id).cloned());
    let Some(handler) = handler else {
        // The proxy was dropped while Java still held it: answer the default.
        return std::ptr::null_mut();
    };
    let outcome = catch_unwind(AssertUnwindSafe(|| -> Result<jobject, PlatformError> {
        let method: String = env.get_string(&method)?.into();
        let count = if args.is_null() {
            0
        } else {
            env.get_array_length(&args)?
        };
        let mut list = Vec::with_capacity(count as usize);
        for index in 0..count {
            list.push(env.get_object_array_element(&args, index)?);
        }
        let call = ProxyCall {
            method,
            proxy,
            args: list,
        };
        Ok(handler(&mut env, &call)?.into_raw())
    }));
    match outcome {
        Ok(Ok(answer)) => {
            if let Some(text) = take_exception(&mut env) {
                log::warn!("platform: a proxy's handler left an exception ({text})");
                return std::ptr::null_mut();
            }
            answer
        }
        Ok(Err(err)) => {
            let text = take_exception(&mut env).unwrap_or_else(|| err.to_string());
            log::warn!("platform: a proxy's handler failed ({text})");
            std::ptr::null_mut()
        }
        Err(_) => {
            let _ = take_exception(&mut env);
            log::error!("platform: a proxy's handler panicked; the call answers the default");
            std::ptr::null_mut()
        }
    }
}
