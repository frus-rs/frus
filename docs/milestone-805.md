# Milestone 805 — clippy for Android and the Web in CI

## Objective

CI ran clippy for the Linux build only. Code under `cfg(target_os = "android")` and
`cfg(target_arch = "wasm32")` was compiled for those targets (the Android, size and wasm
jobs) but never linted, so its warnings could pile up where nobody looked. Earlier milestones
ran the target lints by hand, crate by crate, and some reported warnings in code they did not
touch. This milestone makes the whole workspace lint-clean for both targets and adds a CI job
so it stays that way.

## What the lints found

Run with rustc 1.100.0, the latest stable and what CI installs:

```text
cargo clippy --workspace --target aarch64-linux-android --no-deps -- -D warnings
cargo clippy --workspace --exclude frus-cli --target wasm32-unknown-unknown --no-deps -- -D warnings
```

- **Android**: clippy itself was clean on master `d7cc1692`. The `missing_const_for_thread_local`
  warning reported in `frus-widgets` earlier did not come back with rustc 1.100.0, for the
  workspace or for `-p frus-shell -p frus-widgets`. Cargo 1.100's new
  `unused_dependencies` lint did warn, twice:
  - `frus-bench` listed `frus-gpu` as a dependency, but only `benches/batch.rs` uses it. It
    is now a dev-dependency.
  - `frus-shell` always built `async-lock`, which only the network image cache (feature
    `net`) uses. It is now optional and part of `net`.
- **wasm32**: one error and one warning.
  - `frus-transforms` did not compile for the Web at all: `main!` expands to a
    `#[wasm_bindgen(start)]` entry point, and the crate did not depend on `wasm-bindgen` for
    that target the way `frus-hello` does. The dependency is added.
  - `frus-shell`'s `async-channel` (features `sqlite` and `auth`) is used only natively. It
    moves to the native-only dependency table.
- `frus-cli` is left out of the wasm lint: it is the developer's command-line tool and uses
  `std::os::unix` and processes, which do not exist in a browser.

No `#[allow]` was added.

## CI

A new job, `clippy (Android, wasm32)`, installs both targets and the same NDK as the size
job (r26d, `CC`/`AR` for aarch64 Android, since some dependencies compile C), then runs the
two commands above. It runs in parallel with the other jobs and shares nothing with the size
job's release build, so the size result is not delayed.

## Size

Moving dependencies between tables changes what is compiled, not what is linked:
`frus-hello` uses neither `async-lock` nor `async-channel`, so its size is not expected to
move; the size job checks it on the pull request.

## How it was tested

- Both target lints above, clean on rustc 1.100.0 (Windows host, NDK 27 for the C parts).
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo clippy -p frus-demo --features shots --all-targets -- -D warnings`,
  `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` and
  `cargo test --workspace`. Every test passed except 19 golden comparisons in `frus-test`,
  which differ by a few pixels when rendered on a Windows host instead of the Linux
  software renderer that recorded them; this branch changes no rendering code.
- `frus-shell` with `auth` for wasm32 and with `auth,sqlite` natively, the features that
  use the moved dependencies.
- The new CI job on the pull request.

## Not done

- The lint does not cover `--all-targets` (tests and examples built for the phone or the
  browser) or optional features beyond what the workspace's feature unification turns on.
- iOS is still built by its own job but not linted.
- The `missing_const_for_thread_local` warning seen before was not reproduced, so its exact
  toolchain is not known; the new job will catch it if a later rustc brings it back.
