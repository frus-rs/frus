// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **frus's own Android activity** (milestone 801): `dev.frus.FrusActivity`, the system's
//! native activity with `onNewIntent` handed to the application.
//!
//! An application opened again while it runs — a link, a share, a notification's tap, the
//! redirect a browser sends back at the end of a sign-in — is handed an intent. The native
//! activity forwards no new intent to native code, and with the standard launch mode a browser's
//! `FLAG_ACTIVITY_CLEAR_TOP` finishes the running activity and makes another, which ends the
//! application (milestone 795). frus's activity is declared `singleTop` and forwards the intent,
//! so the running activity takes it.
//!
//! cargo-apk writes a manifest naming `android.app.NativeActivity` and packages no code. Once
//! it has made the package, frus names its own activity in the manifest, with
//! `android:hasCode="true"` and the launch mode, and adds the activity's compiled class as the
//! package's `classes.dex` (see `app_links::finish_package`, which makes the package again with
//! the SDK's tools). An application whose manifest names an activity of its own
//! (`[package.metadata.android.application.activity] name`) is left as it is; one built by hand
//! with `cargo apk` runs in the native activity, as before.

/// The class of frus's activity.
pub(crate) const ACTIVITY: &str = "dev.frus.FrusActivity";

/// The activity, compiled (`scripts/build-activity-dex.sh`): the package's `classes.dex`.
pub(crate) const DEX: &[u8] = include_bytes!("../assets/frus_activity.dex");

/// The native activity's class, cargo-apk's default.
const NATIVE: &str = "android.app.NativeActivity";

/// cargo-apk's manifest with frus's activity in place of the native one: its class, the launch
/// mode that hands a new intent to the running activity (unless the manifest chose one), and
/// the code the package now carries. None when the manifest's activity is not the native one.
pub(crate) fn use_frus_activity(manifest: &str) -> Option<String> {
    let start = manifest.find("<activity")?;
    let end = start + manifest[start..].find('>')?;
    let tag = &manifest[start..end];
    let native = format!("android:name=\"{NATIVE}\"");
    if !tag.contains(&native) {
        return None;
    }
    let mut new_tag = tag.replace(&native, &format!("android:name=\"{ACTIVITY}\""));
    if !new_tag.contains("android:launchMode=") {
        let (body, closing) = match new_tag.strip_suffix('/') {
            Some(body) => (body.to_string(), "/"),
            None => (new_tag.clone(), ""),
        };
        new_tag = format!("{body} android:launchMode=\"singleTop\"{closing}");
    }
    let xml = format!("{}{new_tag}{}", &manifest[..start], &manifest[end..]);
    Some(crate::android_pack::set_attribute(
        &xml,
        "<application",
        "android:hasCode",
        "true",
    ))
}

/// Whether a manifest names frus's activity: its package must carry [`DEX`].
pub(crate) fn uses_frus_activity(manifest: &str) -> bool {
    manifest.contains(&format!("android:name=\"{ACTIVITY}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WRITTEN: &str = r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.acme.app"><application android:debuggable="true" android:hasCode="false" android:label="app"><activity android:configChanges="orientation|keyboardHidden" android:label="app" android:name="android.app.NativeActivity" android:exported="true"><meta-data android:name="android.app.lib_name" android:value="app"/><intent-filter><action android:name="android.intent.action.MAIN"/></intent-filter></activity></application></manifest>"#;

    #[test]
    fn the_native_activity_becomes_frus_s() {
        let patched = use_frus_activity(WRITTEN).unwrap();
        assert!(
            patched.contains("android:name=\"dev.frus.FrusActivity\""),
            "{patched}"
        );
        assert!(!patched.contains("android.app.NativeActivity"));
        assert!(patched.contains("android:launchMode=\"singleTop\""));
        assert!(patched.contains("android:hasCode=\"true\""));
        assert!(!patched.contains("android:hasCode=\"false\""));
        // The rest is kept: the library's name, the launcher's filter, the activity's attributes.
        assert!(patched.contains("android:value=\"app\""));
        assert!(patched.contains("android.intent.action.MAIN"));
        assert!(patched.contains("android:configChanges=\"orientation|keyboardHidden\""));
        assert!(uses_frus_activity(&patched));
        assert!(!uses_frus_activity(WRITTEN));
        // Done once: asked again, nothing to change.
        assert_eq!(use_frus_activity(&patched), None);
    }

    #[test]
    fn a_launch_mode_the_manifest_chose_is_kept() {
        let chosen = WRITTEN.replace(
            "android:exported=\"true\"",
            "android:exported=\"true\" android:launchMode=\"singleTask\"",
        );
        let patched = use_frus_activity(&chosen).unwrap();
        assert!(patched.contains("android:launchMode=\"singleTask\""));
        assert!(!patched.contains("singleTop"));
    }

    #[test]
    fn an_activity_of_the_application_s_own_is_left_alone() {
        let own = WRITTEN.replace("android.app.NativeActivity", "com.acme.app.Main");
        assert_eq!(use_frus_activity(&own), None);
        // An application with no `hasCode` attribute still gets it.
        let bare = WRITTEN.replace(" android:hasCode=\"false\"", "");
        assert!(use_frus_activity(&bare)
            .unwrap()
            .contains("<application android:debuggable=\"true\" android:label=\"app\" android:hasCode=\"true\">"));
    }

    #[test]
    fn the_dex_holds_the_activity() {
        assert!(DEX.starts_with(b"dex\n"));
        let class = b"Ldev/frus/FrusActivity;";
        assert!(DEX.windows(class.len()).any(|w| w == class));
        let method = b"nativeNewIntent";
        assert!(DEX.windows(method.len()).any(|w| w == method));
    }
}
