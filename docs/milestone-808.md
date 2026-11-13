# Milestone 808 — INTERNET for every Android application

## Objective

An Android application that makes HTTP requests (`frus::fetch`, `Request`), shows network
video or opens any socket needs `android.permission.INTERNET` in its manifest. Without it the
system refuses every connection, and the application reads as a phone that is offline.

frus declared INTERNET only when the application turned on the `webview` feature
(milestone 799). An application using the `net` feature therefore worked under `frus run`,
whose hot-reload manifest names INTERNET, and failed every request once built with
`frus build --release`. The examples' HTTP project had to add the permission by hand.

## Change

- `permissions::ALWAYS`, the permissions `frus build` and `frus run` write into every
  application's `Cargo.toml` (as `[[package.metadata.android.uses_permission]]` tables, only
  the ones not already named), now holds INTERNET beside VIBRATE and ACCESS_NETWORK_STATE.
- The web-view detection (`uses_web_views` and its separate constant) is gone: nothing else
  used it, and INTERNET is now always declared.
- The `frus create` template declares the three, with a comment that says what each is for, so
  a new application's manifest is complete as written and `frus build` adds nothing to it.
- The guides (network, web view, native views, video) say INTERNET is declared for every
  application, and how to declare it in an APK built by hand with cargo-apk.

INTERNET is a normal permission: Android grants it at install, with no question asked, as it
does VIBRATE and ACCESS_NETWORK_STATE. Nearly every application declares it; the cost is one
line in the manifest.

### Other manifests

- The hot-reload Android project (`hot.rs`) already declared INTERNET, VIBRATE and
  ACCESS_NETWORK_STATE: unchanged.
- The App Bundle (`frus build appbundle`) is packed from the manifest cargo-apk writes, so it
  carries the same permissions as the APK.
- An application that already names INTERNET (frus-demo, frus-data-example, the examples' HTTP
  project) keeps its table: `android_additions` adds only names the manifest does not have, so
  nothing is declared twice.

## How it was tested

- `write_android_declares_what_frus_uses_without_being_asked` writes every name in `ALWAYS`
  exactly once into an empty manifest; `write_android_adds_to_the_manifest_once` now checks
  INTERNET is among them.
- `the_template_s_permissions_table_works_as_written` checks that the rendered template
  already names everything in `ALWAYS` (`android_additions` returns nothing). Against the old
  template, without INTERNET, it fails.
- The `webview`-detection test was removed with the function.
- `cargo test -p frus-cli`, `cargo clippy --workspace --all-targets -- -D warnings`.
- `frus create demo808` with the built CLI: the new `Cargo.toml` names VIBRATE,
  ACCESS_NETWORK_STATE and INTERNET, each once.

## Not done

- No release APK was built for this change and its manifest was not dumped with `aapt`: the
  manifest comes from the `Cargo.toml` tables checked above, which cargo-apk copies as written.
- An application can not opt out of INTERNET through frus; removing it would take a manifest
  built by hand.
