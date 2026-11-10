// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Verified links in the APK** (milestone 791): `android:autoVerify="true"` on the intent
//! filters of `https` links, which cargo-apk cannot write.
//!
//! An application that opens `https://example.com/…` links (`frus create --deeplink
//! https://example.com`) declares an intent filter for them. Android opens such a link in the
//! application without asking only if the filter asks Android to check the host
//! (`autoVerify`) and the host vouches for the application (`/.well-known/assetlinks.json`).
//! cargo-apk 0.10 writes the manifest from `Cargo.toml` and has no field for `autoVerify`.
//!
//! So once cargo-apk has made the package, frus makes it again around the same manifest with the
//! attribute added, with the SDK's own tools, as cargo-apk does: `aapt package` (manifest and
//! resources), `aapt add` (the libraries cargo-apk left beside it), `zipalign`, `apksigner`, with
//! the key cargo-apk signed with. A package with no `https` filter is left exactly as cargo-apk
//! made it.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::cargo::Mode;
use crate::project::Project;

const VERIFY: &str = "android:autoVerify=\"true\"";

/// The manifest with `autoVerify` on every intent filter that takes `https` links, or None when
/// none needs it.
pub(crate) fn add_auto_verify(manifest: &str) -> Option<String> {
    let mut out = String::with_capacity(manifest.len() + 64);
    let mut rest = manifest;
    let mut changed = false;
    while let Some(start) = rest.find("<intent-filter") {
        let (before, from) = rest.split_at(start);
        out.push_str(before);
        let end = from
            .find("</intent-filter>")
            .map_or(from.len(), |e| e + "</intent-filter>".len());
        let (filter, after) = from.split_at(end);
        let open_end = filter.find('>').unwrap_or(filter.len());
        let opening = &filter[..open_end];
        if filter.contains("android:scheme=\"https\"") && !opening.contains("autoVerify") {
            out.push_str("<intent-filter ");
            out.push_str(VERIFY);
            out.push_str(&filter["<intent-filter".len()..]);
            changed = true;
        } else {
            out.push_str(filter);
        }
        rest = after;
    }
    out.push_str(rest);
    changed.then_some(out)
}

/// The value of `key = "…"` in the table `[table]` of a manifest written the way `frus create`
/// writes one, a key a line.
fn manifest_value(manifest: &str, table: &str, key: &str) -> Option<String> {
    let header = format!("[{table}]");
    let mut inside = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == header;
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == key {
                let v = v.trim();
                return v
                    .strip_prefix('"')
                    .and_then(|v| v.split('"').next())
                    .map(str::to_string);
            }
        }
    }
    None
}

/// The key the package is signed with, as cargo-apk chose it: path and password.
fn signing_key(project: &Project, mode: Mode, manifest: &str) -> Result<(PathBuf, String), String> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_default();
    let debug_key = home.join(".android").join("debug.keystore");
    if mode == Mode::Debug {
        return Ok((debug_key, "android".into()));
    }
    if let (Some(path), Some(password)) = (
        std::env::var_os("CARGO_APK_RELEASE_KEYSTORE"),
        std::env::var("CARGO_APK_RELEASE_KEYSTORE_PASSWORD").ok(),
    ) {
        return Ok((PathBuf::from(path), password));
    }
    let table = "package.metadata.android.signing.release";
    if let (Some(path), Some(password)) = (
        manifest_value(manifest, table, "path"),
        manifest_value(manifest, table, "keystore_password"),
    ) {
        let path = PathBuf::from(path);
        let path = if path.is_relative() {
            project.root.join(path)
        } else {
            path
        };
        return Ok((path, password));
    }
    Err("no release key to sign the package again with".into())
}

/// A tool of the SDK's newest build-tools: `aapt`, `zipalign`, `apksigner`.
fn build_tool(sdk: &Path, name: &str) -> Result<PathBuf, String> {
    let dir = crate::setup::newest_version_dir(&sdk.join("build-tools"))
        .ok_or("the Android SDK has no build-tools")?;
    let exe = if cfg!(windows) {
        let bat = dir.join(format!("{name}.bat"));
        if bat.is_file() {
            bat
        } else {
            dir.join(format!("{name}.exe"))
        }
    } else {
        dir.join(name)
    };
    if exe.is_file() {
        Ok(exe)
    } else {
        Err(format!("{} is not there", exe.display()))
    }
}

/// Every file below `dir`, as paths relative to `base` with `/`.
fn files_below(base: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_below(base, &path, out);
        } else if let Ok(relative) = path.strip_prefix(base) {
            let parts: Vec<String> = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            out.push(parts.join("/"));
        }
    }
}

fn tool(command: &mut Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} failed: {}{}",
            String::from_utf8_lossy(&output.stderr).trim(),
            String::from_utf8_lossy(&output.stdout).trim()
        ))
    }
}

/// Makes `apk` again with `autoVerify` on its `https` intent filters, when it has any. True when
/// the package was made again.
pub(crate) fn verify_https_links(
    project: &Project,
    apk: &Path,
    mode: Mode,
) -> Result<bool, String> {
    let dir = apk.parent().ok_or("the package has no directory")?;
    let manifest_path = dir.join("AndroidManifest.xml");
    let Ok(manifest) = std::fs::read_to_string(&manifest_path) else {
        return Ok(false);
    };
    let Some(patched) = add_auto_verify(&manifest) else {
        return Ok(false);
    };
    crate::report::say("Asking Android to verify the https links (android:autoVerify)…");
    let sdk = crate::setup::existing_sdk().ok_or("no Android SDK")?;
    let jar = sdk
        .join("platforms")
        .join(format!("android-{}", project.android_target_sdk))
        .join("android.jar");
    let cargo_toml = std::fs::read_to_string(project.root.join("Cargo.toml")).unwrap_or_default();
    let (key, password) = signing_key(project, mode, &cargo_toml)?;
    let aapt = build_tool(&sdk, "aapt")?;
    let zipalign = build_tool(&sdk, "zipalign")?;
    let apksigner = build_tool(&sdk, "apksigner")?;

    // aapt reads a manifest only by its own name: the patched one goes in a folder of its own.
    let patched_dir = dir.join("frus-links");
    std::fs::create_dir_all(&patched_dir).map_err(|e| e.to_string())?;
    let patched_path = patched_dir.join("AndroidManifest.xml");
    std::fs::write(&patched_path, patched).map_err(|e| e.to_string())?;
    let unaligned = dir.join("frus-relinked-unaligned.apk.tmp");
    let aligned = dir.join("frus-relinked.apk.tmp");
    let _ = std::fs::remove_file(&unaligned);
    let mut package = Command::new(&aapt);
    package
        .args(["package", "-f", "-F"])
        .arg(&unaligned)
        .arg("-M")
        .arg(&patched_path)
        .arg("-I")
        .arg(&jar);
    let android = "package.metadata.android";
    if let Some(res) = manifest_value(&cargo_toml, android, "resources") {
        package.arg("-S").arg(project.root.join(res));
    }
    if let Some(assets) = manifest_value(&cargo_toml, android, "assets") {
        package.arg("-A").arg(project.root.join(assets));
    }
    // The libraries, under the same paths cargo-apk put them at (`lib/<abi>/…`), from a folder
    // that holds only them; stored, not compressed, as cargo-apk stores them: the system maps a
    // stored library from the package rather than unpacking it.
    let mut libraries = Vec::new();
    files_below(dir, &dir.join("lib"), &mut libraries);
    if libraries.is_empty() {
        return Err("cargo-apk left no lib/ beside the package to add again".into());
    }
    let raw = patched_dir.join("raw");
    let _ = std::fs::remove_dir_all(&raw);
    for library in &libraries {
        let to = raw.join(library);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(dir.join(library), &to).map_err(|e| format!("{library}: {e}"))?;
    }
    package.args(["-0", "so"]).arg(&raw);
    tool(&mut package)?;
    tool(
        Command::new(&zipalign)
            .args(["-f", "-p", "4"])
            .arg(&unaligned)
            .arg(&aligned),
    )?;
    tool(
        Command::new(&apksigner)
            .args(["sign", "--ks"])
            .arg(&key)
            .args(["--ks-pass", &format!("pass:{password}")])
            .arg(&aligned),
    )?;
    std::fs::rename(&aligned, apk).map_err(|e| format!("{}: {e}", apk.display()))?;
    let _ = std::fs::remove_file(&unaligned);
    let _ = std::fs::remove_dir_all(&patched_dir);
    let _ = std::fs::remove_file(dir.join("frus-relinked.apk.tmp.idsig"));
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_filters_ask_for_verification() {
        let manifest = r#"<activity><intent-filter><action android:name="android.intent.action.MAIN"/></intent-filter><intent-filter><action android:name="android.intent.action.VIEW"/><data android:scheme="https" android:host="tides.example"/></intent-filter><intent-filter><data android:scheme="myapp"/></intent-filter></activity>"#;
        let patched = add_auto_verify(manifest).unwrap();
        assert_eq!(patched.matches(VERIFY).count(), 1, "{patched}");
        assert!(patched.contains(
            "<intent-filter android:autoVerify=\"true\"><action android:name=\"android.intent.action.VIEW\"/><data android:scheme=\"https\""
        ));
        // The launcher's filter and the scheme's are left as they are.
        assert!(patched.starts_with(
            "<activity><intent-filter><action android:name=\"android.intent.action.MAIN\"/>"
        ));
        assert!(patched.ends_with(
            "<intent-filter><data android:scheme=\"myapp\"/></intent-filter></activity>"
        ));
        // Asked again: nothing to add.
        assert_eq!(add_auto_verify(&patched), None);
        assert_eq!(
            add_auto_verify("<intent-filter><data android:scheme=\"myapp\"/></intent-filter>"),
            None
        );
    }

    #[test]
    fn the_signing_key_is_read_from_the_manifest() {
        let toml = "[package.metadata.android]\nresources = \"res\"\n\n[package.metadata.android.signing.release]\npath = \"/keys/release.keystore\"\nkeystore_password = \"s3cret\"\n";
        assert_eq!(
            manifest_value(toml, "package.metadata.android", "resources").as_deref(),
            Some("res")
        );
        assert_eq!(
            manifest_value(
                toml,
                "package.metadata.android.signing.release",
                "keystore_password"
            )
            .as_deref(),
            Some("s3cret")
        );
        assert_eq!(
            manifest_value(toml, "package.metadata.android", "path"),
            None
        );
    }
}
