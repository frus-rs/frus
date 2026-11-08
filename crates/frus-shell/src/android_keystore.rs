// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The secure store on **Android** (milestone 776): an AES-256-GCM key made in the Android
//! Keystore — it never leaves it, and lives in the phone's secure hardware where there is some —
//! encrypts each value, and the ciphertext is kept in the application's own folder, in
//! `frus-secure-store.json`.
//!
//! Every call is public platform API (`KeyStore`, `KeyGenerator`, `KeyGenParameterSpec`,
//! `Cipher`), reached over JNI from whichever thread asks: no dex, no bridge class. Each value
//! is bound to its key as the cipher's associated data, so a ciphertext copied under another key
//! does not decrypt.
//!
//! As everywhere on this side, **every failed JNI call clears its exception**: one left pending
//! takes the process down on the next, unrelated call.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use frus_widgets::__serde_json as json;
use jni::objects::{JByteArray, JObject, JValue};
use jni::JNIEnv;

use crate::secure_store::SecureStoreError;

/// The Keystore's name for the key.
const ALIAS: &str = "frus.secure-store";
/// `KeyProperties.PURPOSE_ENCRYPT | PURPOSE_DECRYPT`.
const PURPOSES: i32 = 1 | 2;
/// `Cipher.ENCRYPT_MODE`, `Cipher.DECRYPT_MODE`.
const ENCRYPT: i32 = 1;
const DECRYPT: i32 = 2;
/// The GCM tag, in bits.
const TAG_BITS: i32 = 128;

/// One writer of the file at a time, in this process.
static FILE: Mutex<()> = Mutex::new(());

/// What the file keeps for a value: the nonce and the ciphertext with its tag, in hex.
type Kept = BTreeMap<String, (String, String)>;

fn failed(why: impl std::fmt::Display) -> SecureStoreError {
    SecureStoreError::Platform(why.to_string())
}

fn path() -> Result<PathBuf, SecureStoreError> {
    crate::storage::android::files_dir()
        .map(|files| files.join("frus-secure-store.json"))
        .ok_or_else(|| failed("the application's files folder is not known yet"))
}

fn read_file() -> Result<Kept, SecureStoreError> {
    match std::fs::read(path()?) {
        Ok(bytes) => json::from_slice(&bytes).map_err(|err| failed(format!("unreadable: {err}"))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Kept::new()),
        Err(err) => Err(failed(err)),
    }
}

fn write_file(kept: &Kept) -> Result<(), SecureStoreError> {
    let bytes = json::to_vec(kept).map_err(failed)?;
    crate::storage::write_atomically(&path()?, &bytes).map_err(failed)
}

pub(crate) fn set(_service: &str, key: &str, value: &str) -> Result<(), SecureStoreError> {
    let (nonce, sealed) = with_env(|env| encrypt(env, key.as_bytes(), value.as_bytes()))?;
    let _guard = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let mut kept = read_file()?;
    kept.insert(key.to_owned(), (hex(&nonce), hex(&sealed)));
    write_file(&kept)
}

pub(crate) fn get(_service: &str, key: &str) -> Result<Option<String>, SecureStoreError> {
    let entry = {
        let _guard = FILE.lock().unwrap_or_else(|e| e.into_inner());
        read_file()?.get(key).cloned()
    };
    let Some((nonce, sealed)) = entry else {
        return Ok(None);
    };
    let (nonce, sealed) = (
        unhex(&nonce).ok_or_else(|| failed("a damaged nonce"))?,
        unhex(&sealed).ok_or_else(|| failed("a damaged ciphertext"))?,
    );
    let plain = with_env(|env| decrypt(env, key.as_bytes(), &nonce, &sealed))?;
    String::from_utf8(plain)
        .map(Some)
        .map_err(|_| failed("the value is not text"))
}

pub(crate) fn delete(_service: &str, key: &str) -> Result<(), SecureStoreError> {
    let _guard = FILE.lock().unwrap_or_else(|e| e.into_inner());
    let mut kept = read_file()?;
    if kept.remove(key).is_some() {
        write_file(&kept)?;
    }
    Ok(())
}

/// Runs `work` with this thread's JNI environment, clearing any exception it leaves behind and
/// turning it into the error's text.
fn with_env<T>(
    work: impl FnOnce(&mut JNIEnv) -> Result<T, jni::errors::Error>,
) -> Result<T, SecureStoreError> {
    let vm = crate::storage::android::vm().ok_or_else(|| failed("no Java VM yet"))?;
    let mut env = vm.attach_current_thread_permanently().map_err(failed)?;
    // A frame of its own: this thread stays attached, and its local references would
    // otherwise never be freed.
    match env.with_local_frame(64, |env| work(env)) {
        Ok(value) => Ok(value),
        Err(err) => {
            let mut why = err.to_string();
            if env.exception_check().unwrap_or(false) {
                if let Ok(thrown) = env.exception_occurred() {
                    let _ = env.exception_clear();
                    if let Ok(text) = env
                        .call_method(&thrown, "toString", "()Ljava/lang/String;", &[])
                        .and_then(|v| v.l())
                    {
                        if let Ok(text) = env.get_string(&text.into()) {
                            why = text.into();
                        }
                    }
                }
                let _ = env.exception_clear();
            }
            Err(failed(format!("the Android Keystore: {why}")))
        }
    }
}

/// The Keystore's key, made the first time.
fn key<'a>(env: &mut JNIEnv<'a>) -> Result<JObject<'a>, jni::errors::Error> {
    let kind = env.new_string("AndroidKeyStore")?;
    let store = env
        .call_static_method(
            "java/security/KeyStore",
            "getInstance",
            "(Ljava/lang/String;)Ljava/security/KeyStore;",
            &[JValue::Object(&kind)],
        )?
        .l()?;
    env.call_method(
        &store,
        "load",
        "(Ljava/security/KeyStore$LoadStoreParameter;)V",
        &[JValue::Object(&JObject::null())],
    )?;
    let alias = env.new_string(ALIAS)?;
    let has = env
        .call_method(
            &store,
            "containsAlias",
            "(Ljava/lang/String;)Z",
            &[JValue::Object(&alias)],
        )?
        .z()?;
    if !has {
        make_key(env, &alias, &kind)?;
    }
    env.call_method(
        &store,
        "getKey",
        "(Ljava/lang/String;[C)Ljava/security/Key;",
        &[JValue::Object(&alias), JValue::Object(&JObject::null())],
    )?
    .l()
}

fn make_key(
    env: &mut JNIEnv,
    alias: &JObject,
    provider: &JObject,
) -> Result<(), jni::errors::Error> {
    const BUILDER: &str = "android/security/keystore/KeyGenParameterSpec$Builder";
    const RETURNS: &str = "Landroid/security/keystore/KeyGenParameterSpec$Builder;";
    let builder = env.new_object(
        BUILDER,
        "(Ljava/lang/String;I)V",
        &[JValue::Object(alias), JValue::Int(PURPOSES)],
    )?;
    // A one-element `String[]`. A function rather than a closure: the array must carry the
    // environment's lifetime, which a closure's signature cannot name.
    fn one<'local>(
        env: &mut JNIEnv<'local>,
        text: &str,
    ) -> Result<JObject<'local>, jni::errors::Error> {
        let array = env.new_object_array(1, "java/lang/String", JObject::null())?;
        let text = env.new_string(text)?;
        env.set_object_array_element(&array, 0, text)?;
        Ok(array.into())
    }
    let modes = one(env, "GCM")?;
    env.call_method(
        &builder,
        "setBlockModes",
        format!("([Ljava/lang/String;){RETURNS}"),
        &[JValue::Object(&modes)],
    )?;
    let paddings = one(env, "NoPadding")?;
    env.call_method(
        &builder,
        "setEncryptionPaddings",
        format!("([Ljava/lang/String;){RETURNS}"),
        &[JValue::Object(&paddings)],
    )?;
    env.call_method(
        &builder,
        "setKeySize",
        format!("(I){RETURNS}"),
        &[JValue::Int(256)],
    )?;
    let spec = env
        .call_method(
            &builder,
            "build",
            "()Landroid/security/keystore/KeyGenParameterSpec;",
            &[],
        )?
        .l()?;
    let aes = env.new_string("AES")?;
    let generator = env
        .call_static_method(
            "javax/crypto/KeyGenerator",
            "getInstance",
            "(Ljava/lang/String;Ljava/lang/String;)Ljavax/crypto/KeyGenerator;",
            &[JValue::Object(&aes), JValue::Object(provider)],
        )?
        .l()?;
    env.call_method(
        &generator,
        "init",
        "(Ljava/security/spec/AlgorithmParameterSpec;)V",
        &[JValue::Object(&spec)],
    )?;
    env.call_method(&generator, "generateKey", "()Ljavax/crypto/SecretKey;", &[])?;
    log::info!("secure store: made its key in the Android Keystore");
    Ok(())
}

fn cipher<'a>(env: &mut JNIEnv<'a>) -> Result<JObject<'a>, jni::errors::Error> {
    let name = env.new_string("AES/GCM/NoPadding")?;
    env.call_static_method(
        "javax/crypto/Cipher",
        "getInstance",
        "(Ljava/lang/String;)Ljavax/crypto/Cipher;",
        &[JValue::Object(&name)],
    )?
    .l()
}

fn finish(
    env: &mut JNIEnv,
    cipher: &JObject,
    aad: &[u8],
    input: &[u8],
) -> Result<Vec<u8>, jni::errors::Error> {
    let aad = env.byte_array_from_slice(aad)?;
    env.call_method(cipher, "updateAAD", "([B)V", &[JValue::Object(&aad)])?;
    let input = env.byte_array_from_slice(input)?;
    let output: JByteArray = env
        .call_method(cipher, "doFinal", "([B)[B", &[JValue::Object(&input)])?
        .l()?
        .into();
    env.convert_byte_array(output)
}

/// Encrypts `plain`, bound to `aad`: the nonce the Keystore chose, and the ciphertext with its
/// tag.
fn encrypt(
    env: &mut JNIEnv,
    aad: &[u8],
    plain: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), jni::errors::Error> {
    let key = key(env)?;
    let cipher = cipher(env)?;
    // The Keystore picks the nonce: a key made for GCM refuses one from the caller.
    env.call_method(
        &cipher,
        "init",
        "(ILjava/security/Key;)V",
        &[JValue::Int(ENCRYPT), JValue::Object(&key)],
    )?;
    let nonce: JByteArray = env.call_method(&cipher, "getIV", "()[B", &[])?.l()?.into();
    let nonce = env.convert_byte_array(nonce)?;
    let sealed = finish(env, &cipher, aad, plain)?;
    Ok((nonce, sealed))
}

fn decrypt(
    env: &mut JNIEnv,
    aad: &[u8],
    nonce: &[u8],
    sealed: &[u8],
) -> Result<Vec<u8>, jni::errors::Error> {
    let key = key(env)?;
    let cipher = cipher(env)?;
    let nonce = env.byte_array_from_slice(nonce)?;
    let spec = env.new_object(
        "javax/crypto/spec/GCMParameterSpec",
        "(I[B)V",
        &[JValue::Int(TAG_BITS), JValue::Object(&nonce)],
    )?;
    env.call_method(
        &cipher,
        "init",
        "(ILjava/security/Key;Ljava/security/spec/AlgorithmParameterSpec;)V",
        &[
            JValue::Int(DECRYPT),
            JValue::Object(&key),
            JValue::Object(&spec),
        ],
    )?;
    finish(env, &cipher, aad, sealed)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect()
}
