// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Background jobs in an application's package** (milestone 828).
//!
//! An application that declares headless entry points (`main!(app, headless: { .. })`) has a
//! native function the job service calls, `Java_dev_frus_FrusJobService_nativeRun`, in its
//! library's symbol table. Found there, this step declares frus's job service in the manifest,
//! with the library's name for it to load, and adds the service's compiled class to the
//! package as a dex of its own. An application without headless entry points has no such
//! symbol, and its package is left exactly as it was: no service, no permission, no code.

/// The service's class.
pub(crate) const SERVICE: &str = "dev.frus.FrusJobService";

/// The service, compiled (`scripts/build-jobs-dex.sh`).
pub(crate) const DEX: &[u8] = include_bytes!("../assets/frus_jobs.dex");

/// What `main!` exports when an application declares headless entry points.
const MARKER: &[u8] = b"Java_dev_frus_FrusJobService_nativeRun";

/// The library that declares headless entry points, among a package's libraries
/// (`lib/arm64-v8a/libapp.so`), as the name `System.loadLibrary` takes: `app`.
pub(crate) fn library_in(dir: &std::path::Path, libraries: &[String]) -> Option<String> {
    libraries.iter().find_map(|library| {
        let bytes = std::fs::read(dir.join(library)).ok()?;
        declares_entries(&bytes).then(|| library_name(library))?
    })
}

fn declares_entries(bytes: &[u8]) -> bool {
    bytes.windows(MARKER.len()).any(|w| w == MARKER)
}

/// `lib/arm64-v8a/libapp.so` → `app`.
fn library_name(path: &str) -> Option<String> {
    let file = path.rsplit('/').next()?;
    let stem = file.strip_suffix(".so")?;
    Some(stem.strip_prefix("lib").unwrap_or(stem).to_string())
}

/// Whether a manifest already declares the service.
pub(crate) fn in_manifest(manifest: &str) -> bool {
    manifest.contains(&format!("android:name=\"{SERVICE}\""))
}

/// `manifest` with the job service declared — bound only by the system's job scheduler, and
/// told which library to load — the permission a job kept across a restart needs, and code in
/// the package. Unchanged when it already declares the service.
pub(crate) fn add_to_manifest(manifest: &str, library: &str) -> String {
    if in_manifest(manifest) {
        return manifest.to_string();
    }
    let mut xml = manifest.to_string();
    let boot = "android.permission.RECEIVE_BOOT_COMPLETED";
    if !xml.contains(&format!("android:name=\"{boot}\"")) {
        if let Some(at) = xml.find("<application") {
            xml.insert_str(at, &format!("<uses-permission android:name=\"{boot}\"/>"));
        }
    }
    let service = format!(
        "<service android:name=\"{SERVICE}\" \
         android:permission=\"android.permission.BIND_JOB_SERVICE\" android:exported=\"false\">\
         <meta-data android:name=\"dev.frus.lib_name\" android:value=\"{library}\"/></service>"
    );
    if let Some(at) = xml.rfind("</application>") {
        xml.insert_str(at, &service);
    }
    crate::android_pack::set_attribute(&xml, "<application", "android:hasCode", "true")
}

/// The package's dex files, by name, once the service's code is added to `primary` (frus's
/// activity, the Billing Library): `classes2.dex` beside a `classes.dex`, which Android 5 and
/// later load together, or `classes.dex` alone in a package with no other code.
pub(crate) fn dex_files(primary: Option<Vec<u8>>, with_jobs: bool) -> Vec<(&'static str, Vec<u8>)> {
    match (primary, with_jobs) {
        (Some(primary), true) => vec![("classes.dex", primary), ("classes2.dex", DEX.to_vec())],
        (Some(primary), false) => vec![("classes.dex", primary)],
        (None, true) => vec![("classes.dex", DEX.to_vec())],
        (None, false) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WRITTEN: &str = r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.acme.app"><application android:hasCode="false" android:label="app"><activity android:name="dev.frus.FrusActivity"><meta-data android:name="android.app.lib_name" android:value="app"/></activity></application></manifest>"#;

    #[test]
    fn the_service_is_declared_once_with_its_library() {
        let patched = add_to_manifest(WRITTEN, "acme_app");
        assert!(in_manifest(&patched));
        assert!(patched.contains("android:permission=\"android.permission.BIND_JOB_SERVICE\""));
        assert!(patched.contains("android:exported=\"false\""));
        assert!(patched.contains(
            "<meta-data android:name=\"dev.frus.lib_name\" android:value=\"acme_app\"/></service></application>"
        ));
        assert!(patched.contains(
            "<uses-permission android:name=\"android.permission.RECEIVE_BOOT_COMPLETED\"/><application"
        ));
        assert!(patched.contains("android:hasCode=\"true\""));
        assert!(!patched.contains("android:hasCode=\"false\""));
        // Done once.
        assert_eq!(add_to_manifest(&patched, "acme_app"), patched);
        assert!(!in_manifest(WRITTEN));
    }

    #[test]
    fn a_permission_the_manifest_asks_for_is_not_asked_twice() {
        let asked = WRITTEN.replace(
            "<application",
            "<uses-permission android:name=\"android.permission.RECEIVE_BOOT_COMPLETED\"/><application",
        );
        let patched = add_to_manifest(&asked, "app");
        assert_eq!(patched.matches("RECEIVE_BOOT_COMPLETED").count(), 1);
    }

    #[test]
    fn only_a_library_with_entry_points_is_named() {
        let dir = std::env::temp_dir().join(format!("frus-jobs-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("lib/arm64-v8a")).unwrap();
        std::fs::write(
            dir.join("lib/arm64-v8a/libother.so"),
            b"\x7fELF nothing here",
        )
        .unwrap();
        let libraries = vec!["lib/arm64-v8a/libother.so".to_string()];
        assert_eq!(library_in(&dir, &libraries), None);
        std::fs::write(
            dir.join("lib/arm64-v8a/libacme_app.so"),
            b"\x7fELF ..\0Java_dev_frus_FrusJobService_nativeRun\0..",
        )
        .unwrap();
        let libraries = vec![
            "lib/arm64-v8a/libother.so".to_string(),
            "lib/arm64-v8a/libacme_app.so".to_string(),
        ];
        assert_eq!(library_in(&dir, &libraries).as_deref(), Some("acme_app"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_service_s_code_follows_the_primary_dex() {
        assert!(dex_files(None, false).is_empty());
        let alone = dex_files(None, true);
        assert_eq!(alone.len(), 1);
        assert_eq!(alone[0].0, "classes.dex");
        let both = dex_files(Some(b"dex\nprimary".to_vec()), true);
        assert_eq!(both[0], ("classes.dex", b"dex\nprimary".to_vec()));
        assert_eq!(both[1].0, "classes2.dex");
        assert_eq!(dex_files(Some(b"p".to_vec()), false).len(), 1);
    }

    #[test]
    fn the_dex_holds_the_service() {
        assert!(DEX.starts_with(b"dex\n"));
        for name in [
            &b"Ldev/frus/FrusJobService;"[..],
            b"nativeRun",
            b"nativeStop",
            b"dev.frus.lib_name",
        ] {
            assert!(DEX.windows(name.len()).any(|w| w == name), "{name:?}");
        }
    }
}
