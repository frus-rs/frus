// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! Google Play Billing in an application's package (milestone 807).
//!
//! frus's `purchases` feature talks to Google Play through Google's Billing Library, which is
//! Java: it must be in the package's own code, with an activity of its own declared in the
//! manifest. cargo-apk packages no Java, so this step does it, and only for an application
//! whose library uses the feature: frus-shell leaves a marker in the library
//! (`frus-play-billing-library=<version>;`), found here among the packaged libraries.
//!
//! The library and the one dependency its purchase flow runs on (`androidx.core`) are fetched
//! from Google's Maven repository into `~/.frus/android/play-billing-<version>`, once, and
//! shrunk with R8 (from the SDK's build tools) to what the library's public API reaches, under
//! the library's own keep rules: about 330 KB of dex rather than 1.4 MB. Its telemetry
//! dependency is left out (the library skips its logging without it), and so is
//! `androidx.activity`, which only the alternative billing programs' activity uses. frus's own
//! activity is merged into the same `classes.dex`.

use std::path::{Path, PathBuf};
use std::process::Command;

const MARKER: &[u8] = b"frus-play-billing-library=";
const MAVEN: &str = "https://dl.google.com/dl/android/maven2";
/// The `androidx.core` the library's purchase flow calls (`ContextCompat`, `BundleCompat`).
const ANDROIDX_CORE: &str = "1.13.1";
/// What R8 keeps: the library's public API, which frus's bridge calls by name, and the
/// activity the manifest names.
const KEEP: &str = "\
-keep public class com.android.billingclient.api.** { public *; }
-keep class com.android.billingclient.api.ProxyBillingActivity { *; }
-dontobfuscate
-dontwarn **
-ignorewarnings
";

/// The Billing Library version an application's libraries ask for, if one does.
pub(crate) fn version_in(dir: &Path, libraries: &[String]) -> Option<String> {
    libraries.iter().find_map(|library| {
        let bytes = std::fs::read(dir.join(library)).ok()?;
        version_in_bytes(&bytes)
    })
}

fn version_in_bytes(bytes: &[u8]) -> Option<String> {
    let at = bytes.windows(MARKER.len()).position(|w| w == MARKER)? + MARKER.len();
    let rest = &bytes[at..bytes.len().min(at + 32)];
    let end = rest.iter().position(|&b| b == b';')?;
    let version = std::str::from_utf8(&rest[..end]).ok()?;
    let valid = !version.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.');
    valid.then(|| version.to_string())
}

/// Whether a manifest already declares the Billing Library.
pub(crate) fn in_manifest(manifest: &str) -> bool {
    manifest.contains("com.android.billingclient.api.ProxyBillingActivity")
}

/// `manifest` with what the Billing Library declares: the `BILLING` permission, the store's
/// service among the packages it may see, its version, and its activity.
pub(crate) fn add_to_manifest(manifest: &str, version: &str) -> String {
    if in_manifest(manifest) {
        return manifest.to_string();
    }
    let outside = "<uses-permission android:name=\"com.android.vending.BILLING\"/>\
        <queries><intent><action android:name=\"com.android.vending.billing.InAppBillingService.BIND\"/></intent>\
        <intent><action android:name=\"com.google.android.apps.play.billingtestcompanion.BillingOverrideService.BIND\"/></intent></queries>";
    let inside = format!(
        "<meta-data android:name=\"com.google.android.play.billingclient.version\" android:value=\"{version}\"/>\
         <activity android:name=\"com.android.billingclient.api.ProxyBillingActivity\" \
         android:configChanges=\"keyboard|keyboardHidden|screenLayout|screenSize|orientation\" \
         android:exported=\"false\" android:theme=\"@android:style/Theme.Translucent.NoTitleBar\"/>"
    );
    let mut xml = manifest.to_string();
    if let Some(at) = xml.find("<application") {
        xml.insert_str(at, outside);
    }
    if let Some(at) = xml.rfind("</application>") {
        xml.insert_str(at, &inside);
    }
    xml
}

/// The package's `classes.dex`: the Billing Library, shrunk, with frus's activity when the
/// package names it. Made once per version and kept. `platform` is the `android.jar` the
/// package is built against.
pub(crate) fn dex(version: &str, with_activity: bool, platform: &Path) -> Result<Vec<u8>, String> {
    let dir = crate::setup::frus_home()
        .join("android")
        .join(format!("play-billing-{version}"));
    let name = if with_activity {
        "classes-with-activity.dex"
    } else {
        "classes.dex"
    };
    if let Ok(bytes) = std::fs::read(dir.join(name)) {
        return Ok(bytes);
    }
    crate::report::say(&format!(
        "Adding Google Play Billing Library {version} to the package (fetched once, about 1.8 MB)…"
    ));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let billing = fetch_aar(
        &dir,
        "com/android/billingclient/billing",
        "billing",
        version,
    )?;
    let core = fetch_aar(&dir, "androidx/core/core", "core", ANDROIDX_CORE)?;

    let sdk = crate::setup::existing_sdk().ok_or("no Android SDK")?;
    let build_tools = crate::setup::newest_version_dir(&sdk.join("build-tools"))
        .ok_or("the Android SDK has no build-tools")?;
    let d8 = build_tools.join("lib").join("d8.jar");
    if !d8.is_file() {
        return Err(format!("{} is not there", d8.display()));
    }
    let java =
        crate::setup::java()?
            .join("bin")
            .join(if cfg!(windows) { "java.exe" } else { "java" });

    let keep = dir.join("keep.pro");
    std::fs::write(&keep, KEEP).map_err(|e| e.to_string())?;
    let shrunk = dir.join("shrunk");
    let _ = std::fs::remove_dir_all(&shrunk);
    std::fs::create_dir_all(&shrunk).map_err(|e| e.to_string())?;
    let mut r8 = Command::new(&java);
    r8.arg("-cp")
        .arg(&d8)
        .args([
            "com.android.tools.r8.R8",
            "--release",
            "--min-api",
            "24",
            "--lib",
        ])
        .arg(platform)
        .arg("--pg-conf")
        .arg(&keep);
    let rules = billing.join("proguard.txt");
    if rules.is_file() {
        r8.arg("--pg-conf").arg(&rules);
    }
    r8.arg("--output")
        .arg(&shrunk)
        .arg(billing.join("classes.jar"))
        .arg(core.join("classes.jar"));
    run(&mut r8, "R8")?;
    let mut out = shrunk.join("classes.dex");
    if with_activity {
        let activity = dir.join("frus_activity.dex");
        std::fs::write(&activity, crate::android_activity::DEX).map_err(|e| e.to_string())?;
        let merged = dir.join("merged");
        let _ = std::fs::remove_dir_all(&merged);
        std::fs::create_dir_all(&merged).map_err(|e| e.to_string())?;
        run(
            Command::new(&java)
                .arg("-cp")
                .arg(&d8)
                .args([
                    "com.android.tools.r8.D8",
                    "--release",
                    "--min-api",
                    "24",
                    "--output",
                ])
                .arg(&merged)
                .arg(&activity)
                .arg(&out),
            "D8",
        )?;
        out = merged.join("classes.dex");
    }
    let bytes = std::fs::read(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    std::fs::write(dir.join(name), &bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

/// An AAR from Google's Maven repository, unpacked into `dir/<name>-<version>`.
fn fetch_aar(dir: &Path, path: &str, name: &str, version: &str) -> Result<PathBuf, String> {
    let unpacked = dir.join(format!("{name}-{version}"));
    if unpacked.join("classes.jar").is_file() {
        return Ok(unpacked);
    }
    // `.zip`, which every system's unpacker takes for one.
    let archive = dir.join(format!("{name}-{version}.zip"));
    crate::setup::download(
        &format!("{MAVEN}/{path}/{version}/{name}-{version}.aar"),
        &archive,
    )?;
    let _ = std::fs::remove_dir_all(&unpacked);
    crate::setup::extract(&archive, &unpacked)?;
    let _ = std::fs::remove_file(&archive);
    if unpacked.join("classes.jar").is_file() {
        Ok(unpacked)
    } else {
        Err(format!("{name} {version} has no classes.jar"))
    }
}

fn run(command: &mut Command, what: &str) -> Result<(), String> {
    let output = command
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("could not run {what}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{what} failed: {}{}",
            String::from_utf8_lossy(&output.stderr).trim(),
            String::from_utf8_lossy(&output.stdout).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_marker_names_the_version() {
        let mut library = vec![0u8; 100];
        library.extend_from_slice(b"frus-play-billing-library=8.3.0;in-app purchases");
        library.extend_from_slice(&[0; 10]);
        assert_eq!(version_in_bytes(&library).as_deref(), Some("8.3.0"));
        assert_eq!(version_in_bytes(b"no marker here"), None);
        // A marker cut short or holding something else is not one.
        assert_eq!(version_in_bytes(b"frus-play-billing-library=8.3"), None);
        assert_eq!(version_in_bytes(b"frus-play-billing-library=x;"), None);
    }

    #[test]
    fn the_manifest_gains_the_library_s_declarations_once() {
        let manifest = r#"<manifest package="com.acme.app"><uses-permission android:name="android.permission.INTERNET"/><application android:hasCode="true"><activity android:name="dev.frus.FrusActivity"/></application></manifest>"#;
        let patched = add_to_manifest(manifest, "8.3.0");
        assert!(in_manifest(&patched));
        let permission = patched.find("com.android.vending.BILLING").unwrap();
        assert!(permission < patched.find("<application").unwrap());
        assert!(patched.contains("InAppBillingService.BIND"));
        assert!(patched.contains(
            "android:name=\"com.google.android.play.billingclient.version\" android:value=\"8.3.0\""
        ));
        let activity = patched.find("ProxyBillingActivity").unwrap();
        assert!(activity > patched.find("dev.frus.FrusActivity").unwrap());
        assert!(activity < patched.find("</application>").unwrap());
        assert_eq!(add_to_manifest(&patched, "8.3.0"), patched);
    }
}
