//! The Android side: the Java class in `android/java`, compiled into a dex by `build.rs`, loaded
//! into the application's activity and called through JNI.
//!
//! frus's own Android bridges work the same way: a dex carried by the library and loaded with an
//! `InMemoryDexClassLoader` whose parent is the activity's class loader, so no Gradle project and
//! no change to the application's package are needed. Every JNI failure clears the Java
//! exception it left: a pending one would take the process down on the next, unrelated call.

use jni::objects::{JClass, JObject, JString, JValue};
use jni::{JNIEnv, JavaVM};

use crate::PluginError;

/// The Java side, compiled for this build.
static DEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/classes.dex"));

/// The class `android/java` declares.
const CLASS: &str = "{{org}}.{{crate_name}}.{{ClassName}}Plugin";

pub(crate) fn platform_version() -> Result<String, PluginError> {
    let context = ndk_context::android_context();
    // SAFETY: the pointers android-activity registered for the running activity.
    let vm = unsafe { JavaVM::from_raw(context.vm().cast()) }.map_err(failed)?;
    let mut env = vm.attach_current_thread().map_err(failed)?;
    let activity = unsafe { JObject::from_raw(context.context().cast()) };
    let result = call(&mut env, &activity);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    result.map_err(failed)
}

fn call(env: &mut JNIEnv, activity: &JObject) -> jni::errors::Result<String> {
    // SAFETY: `DEX` is a static, alive for as long as the loader that reads it.
    let buffer = unsafe { env.new_direct_byte_buffer(DEX.as_ptr() as *mut u8, DEX.len()) }?;
    let parent = env
        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let loader = env.new_object(
        "dalvik/system/InMemoryDexClassLoader",
        "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
        &[JValue::Object(&buffer), JValue::Object(&parent)],
    )?;
    let name = env.new_string(CLASS)?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?;
    let class = JClass::from(class);
    let version = env
        .call_static_method(
            &class,
            "platformVersion",
            "(Landroid/content/Context;)Ljava/lang/String;",
            &[JValue::Object(activity)],
        )?
        .l()?;
    let version: String = env.get_string(&JString::from(version))?.into();
    Ok(version)
}

fn failed(error: jni::errors::Error) -> PluginError {
    PluginError::Failed(error.to_string())
}
