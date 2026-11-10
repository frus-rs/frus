# Milestone 790 — Build modes, flavors, defines, versions and release artifacts

## Objective

Give `frus run` and `frus build` what a team shipping an application needs besides "debug or
release", with the flag names developers of cross-platform applications already know:

1. **A profile mode**, `--profile`: optimised as a release is, the frames timed, the symbols
   kept, no hot reload. `frus::build_mode()` says which of debug, profile and release the code
   was built as.
2. **Flavors**, `--flavor <name>`, declared in one place: the application's `Cargo.toml`. A
   flavor has a name people see, an Android application id suffix, Android resources laid over
   `res/` (another icon), cargo features and defines. `frus::flavor()` reads it.
3. **Defines**, `--define KEY=VALUE` (repeatable) and `--define-from-file file.json|.env`,
   read in code with `frus::define!("KEY")` as a compile-time constant, on desktop, Android and
   the web, and rebuilt when a value changes.
4. **The version**, `--build-name` and `--build-number`, written into Android's `versionName`
   and `versionCode`, the macOS `Info.plist`, the Windows installer and the `.deb`;
   `frus::app_version()` reads it.
5. **Release artifacts**: `frus build appbundle` (an Android App Bundle), `frus build apk
   --split-per-abi`, and `frus build installer` (a Windows setup program, a Linux `.deb`, a macOS
   disk image). The Android release key is configured as before; signing is documented in one
   place.

## The surface

```text
frus run   [--debug|--profile|--release] [--flavor <name>] [--define K=V]... \
           [--define-from-file <file>]... [--build-name <x.y.z>] [--build-number <n>]
frus build <apk|appbundle|web|desktop|installer> [same flags] [--split-per-abi]
```

```rust
frus::build_mode()   // BuildMode::{Debug, Profile, Release}
frus::flavor()       // Option<&'static str>
frus::app_version()  // Option<AppVersion { name, number }>
frus::define!("KEY") // Option<&'static str>; define!("KEY", "default") -> &'static str
```

`--dart-define` is accepted as a synonym of `--define`, for people's muscle memory; it is not
documented.

## Design

### How values reach the code: the build's environment

Every value is an environment variable of the build — `FRUS_BUILD_MODE=profile`,
`FRUS_FLAVOR`, `FRUS_BUILD_NAME`, `FRUS_BUILD_NUMBER`, `FRUS_DEFINE_<KEY>` — set in the `frus`
process before any build starts (`variant::export`), so that every program it runs inherits
them: cargo for the desktop, cargo-apk for Android, cargo and wasm-bindgen for the web, and the
hot-reload engine. The code reads them with `option_env!`, which makes each one a constant, and
rustc records each variable it read in the dependency file (`# env-dep:FRUS_DEFINE_API_URL=…`),
which cargo compares on the next build: a new value rebuilds the crate that read it, an absent
one too. This was checked directly with rustc before relying on it.

Alternatives weighed:

- *`--cfg` through `RUSTFLAGS`*: changes the flags of every crate, rebuilding the whole
  dependency graph on each change, and cannot carry a value.
- *A generated Rust file*: would have to be written into the application's sources and included
  by its code; another file to explain and to keep out of version control.
- *Reading the values at run time from a file beside the program*: not available on the web or
  inside an APK without packaging work per platform, and not a constant.

`frus::define!` is a `macro_rules!` that expands to
`option_env!(concat!("FRUS_DEFINE_", $key))` in the crate that uses it, so the application's
crate (not frus) is the one recompiled when a define changes. With a default
(`define!("KEY", "x")`) it is a `&'static str` usable in a `const`.

The flavor and the version are recorded the same way, by `frus::main!`, which expands in the
application's crate and hands the shell a `__BuildInfo` (the crate's `CARGO_PKG_VERSION`, and
the three variables) before the application is made: only the application's crate recompiles
when the flavor changes, and the version is the application's own, not frus's. In a test,
which does not start through `main!`, `flavor()` and `app_version()` are `None`.

`build_mode()` is decided in the shell: `FRUS_BUILD_MODE=profile` marks a profile build, and
otherwise debug assertions tell debug from release. The variable is set only for profile
builds, whose output lives in cargo's `target/profile/` anyway, so switching between `frus
build` and a hand-made `cargo build --release` does not rebuild the shell.

### Profile mode

A cargo profile named `profile`, which `frus create` now writes:

```toml
[profile.profile]
inherits = "release"
strip = false
debug = "line-tables-only"
```

Same optimisation as the release, so the timings measured are the shipped ones; the symbols a
profiler resolves are kept. A project made before this milestone has no such table: `frus`
then defines it through cargo's environment (`CARGO_PROFILE_PROFILE_INHERITS=release`, …),
which cargo reads as configuration, so `--profile` works without editing the manifest.

In a profile build the frame statistics of milestone 609 are on from the start (one line every
two seconds under `frus::frames`), `FRUS_FRAME_STATS=0` turns them off. Hot reload is refused
with `--profile` (`--hot --profile` is an error, and the default is off), as a profile build is
meant to run as it ships. frus has no debug banner, so there was nothing to hide.

On Android, a profile APK is built with `cargo apk build --profile profile` and signed with
`[package.metadata.android.signing.profile]` or `CARGO_APK_PROFILE_KEYSTORE` when present, and
with the debug key otherwise.

### Flavors

```toml
[package.metadata.frus.flavors.dev]
app_name = "My App Dev"
application_id_suffix = ".dev"
resources = "flavors/dev/res"
features = ["dev"]

[package.metadata.frus.flavors.dev.defines]
API_URL = "https://dev.example.com"
```

One place, read by the CLI's line-oriented manifest reader like every other key it reads (the
CLI has no dependencies). `features` defaults to the feature named like the flavor when the
package declares one. A flavor that is not declared is an error listing those that are. Defines
merge in order: the flavor's, then `--define-from-file`'s, then `--define`'s; a later value wins.

Flags are parsed once for both commands (`variant::parse_flag`), and resolved against the
project (`Variant::apply`), which also appends the id suffix to the project's Android package —
so every place `frus run` already used the package (install, start, logcat, `pidof`) follows
the flavor without change.

### Android: packaging again, with aapt2

cargo-apk cannot be told the version name and code (it panics when the manifest sets them, and
derives them from `[package] version`), nor another application id or label, except by editing
`Cargo.toml`. It makes no App Bundle and no per-ABI APKs. Rather than editing the developer's
manifest for the length of a build, or replacing cargo-apk, `frus` lets cargo-apk do what it
does — compile the library for each target and write its APK — and packages again from what it
leaves in its build directory: the text `AndroidManifest.xml` it generated and the
`lib/<abi>/*.so` it packaged (`android_pack.rs`):

1. the manifest is edited: `package`, `android:versionCode`, `android:versionName`, and the
   application's `android:label` for a flavor's `app_name`;
2. `aapt2 compile` compiles `res/` and the flavor's resources, which `aapt2 link -R
   --auto-add-overlay` lays over them;
3. an APK: `aapt2 link`, `aapt add` for the libraries, `zipalign`, `apksigner` — the steps
   cargo-apk takes, with aapt2 for the resources;
4. an App Bundle: `aapt2 link --proto-format`, unpacked with the JDK's `jar` (aapt2 compresses
   the manifest even with `--no-compress`, and its `--output-to-dir` fails to make the resource
   directories on Windows), then the base module is assembled (`manifest/AndroidManifest.xml`,
   `resources.pb`, `res/`, `lib/`, `assets/`) by a small stored-zip writer in the CLI,
   `bundletool build-bundle` makes the `.aab`, and `jarsigner` signs it with the first private
   key of the keystore (a bundle is signed as a JAR; `apksigner` does not sign bundles).

The split APKs and the bundle are packaged from cargo-apk's own output (`cargo::cargo_apk`),
never from the APK already packaged again for a flavor: the first runs did that, and named the
bundle `vapp-dev-dev.aab`.

This runs only when needed: a plain `frus build apk` still ships cargo-apk's APK untouched.
bundletool (Apache-2.0, Google) is fetched into `~/.frus/tools` the first time, following "a
command installs what it needs"; it runs on the JDK the Android build already finds or installs.

`--split-per-abi` makes one APK per `build_targets` entry, with version code
`abi * 1000 + code` (1 armeabi-v7a, 2 arm64-v8a, 3 x86, 4 x86_64), the numbering developers
know: a device that runs two ABIs installs the 64-bit one.

The signing key is now found in one function (`signing_key`), as cargo-apk finds it — the
`CARGO_APK_<PROFILE>_KEYSTORE` variables, then `[package.metadata.android.signing.<profile>]` —
and handed to cargo-apk explicitly, so both always sign with the same key. A release `frus
build` without a key stops with a message that says where to put one.

On Android, a flavor that changes the package (id, name, resources) or a version override turns
hot reload off for that run with a message: the hot-reload engine builds its own package.

### Installers

- **Windows: NSIS.** It produces a single setup program, installs per user (into
  `%LOCALAPPDATA%\Programs`, Start menu shortcut, uninstaller registered under `HKCU`), needs no
  administrator to make or to install, and its compiler is a 2.5 MB zip that runs where it is
  unpacked, fetched the first time. MSIX was set aside: a package only installs once its
  signing certificate is trusted, which needs an administrator or a bought certificate, and
  testing it here would need both. WiX/MSI needs a .NET tool or a larger toolset, and a
  per-user MSI is awkward. The setup program carries DirectX's shader compiler DLLs beside the
  program, as `frus build desktop` places them.
- **Linux: a `.deb`**, written by the CLI itself (an `ar` archive of `debian-binary`,
  `control.tar.gz`, `data.tar.gz`, the tarballs made with the system's `tar`), so it needs no
  `dpkg-deb`: the program in `/usr/bin`, a desktop entry, the launcher icon. AppImage was not
  done (it needs `appimagetool` and a runtime).
- **macOS: a disk image** with `hdiutil`, holding the `.app` frus already makes (whose
  `Info.plist` now carries the flavor's name, `--build-name` and `--build-number`).

The Windows program has no version resource (the template has no build script); the version is
carried by the installer's version information and its entry in the list of programs.

## How it was tested

Unit tests (`cargo test -p frus-cli`, `-p frus-shell --lib build_info`): flag parsing for both
commands and every mistake (two modes, a bad key, a number out of Android's range, an unknown
flavor), the manifest's flavors and defines tables, the merge order of defines, `.env` and JSON
define files, the environment exported (including the profile profile for an old project, and
the clearing of a previous variant), version names and codes, manifest editing (id, version,
label escaped, the activity's own label left alone, missing attributes added), ABI codes, the
keytool alias, signing-key resolution, the stored zip round trip and CRC, the NSIS script, the
`.deb` control file, desktop entry and `ar` layout, `build_mode`/`app_version` resolution, and a
define read as a constant with and without a default. The guide page's Rust examples are
doctests of the `frus` crate.

Real runs, on a project made by `frus create` against this branch (`vapp`, flavors `dev` and
`prod`, its text showing `build_mode()`, `flavor()`, `define!("API_URL")` and `app_version()`):

- **Windows, debug**: `frus run --flavor dev`, with a `--define API_URL=…` over the flavor's
  and a version given; the window read `mode=debug flavor=dev API_URL=http://localhost:3000
  version=2.5.0+77` (screenshot taken with `s`).
- **Windows, profile**: `frus run --profile --flavor prod --build-name 3.0.0 --build-number 12`;
  the window read `mode=profile flavor=prod API_URL=https://example.com version=3.0.0+12`, and
  the log carried a `frus::frames` line with no `FRUS_FRAME_STATS` in the environment.
- **App Bundle**: `frus build appbundle --flavor dev --build-name 1.4.0 --build-number 23`, for
  `aarch64-linux-android` and `x86_64-linux-android`, signed with a test release key from
  `CARGO_APK_RELEASE_KEYSTORE`: bundletool fetched on first use, `vapp-dev.aab` (13.1 MB),
  `jarsigner -verify` reports it signed by that key. The first three runs failed or misnamed
  the bundle (the compressed manifest, `--output-to-dir`, the doubled flavor), all fixed above.
- **Split APKs**: `frus build apk --split-per-abi --flavor dev --build-name 1.4.0
  --build-number 23` wrote `vapp-dev-arm64-v8a.apk` (6.4 MB) and `vapp-dev-x86_64.apk`
  (6.7 MB). `aapt2 dump badging`: the package's id with `.dev` appended, label `vapp dev`, version
  name `1.4.0`, version codes `2023` and `4023`, one native ABI each; `apksigner verify` names
  the release key.
- **Windows installer**: `frus build installer --flavor prod --build-name 3.1.0 --build-number
  7` fetched NSIS 3.11 and wrote `vapp-3.1.0-setup.exe` (12.1 MB, file and product version
  3.1.0). `setup.exe /S` installed without a question into `%LOCALAPPDATA%\Programs\vapp`
  (program, `dxcompiler.dll`, `dxil.dll`, uninstaller), a Start menu shortcut and an `HKCU`
  uninstall entry with `DisplayVersion` 3.1.0. The installed program started and read
  `mode=release flavor=prod … version=3.1.0+7`. `uninstall.exe /S` removed the folder, the
  shortcut and the entry.
- **Linux `.deb`** (WSL, Ubuntu): the same command wrote `vapp_3.1.0_amd64.deb` (8.7 MB).
  `dpkg-deb --info` reads it (`Version: 3.1.0+7`, `Architecture: amd64`), its contents are
  `/usr/bin/vapp`, the desktop entry and the 192 px icon; `dpkg -i` installed it, the installed
  program drew its first frame (llvmpipe), `apt-get remove` took it away. WSL had no network
  that day: built offline from `cargo vendor`.
- `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`;
  `cargo clippy -p frus-shell -p frus --no-deps --target aarch64-linux-android -- -D warnings`;
  `cargo test -p frus-cli` (150), `-p frus-shell --lib` (547), `-p frus --doc` (69): all pass.

## Not done

- **Installing the bundle and the split APKs on a device.** The test phone (and the two
  emulators) dropped off adb before the install, and did not come back while this was written:
  the packages were checked with `aapt2 dump badging`, `apksigner` and `jarsigner`, not
  installed. A profile or flavored APK was not run on Android either.
- **macOS**: the disk image and the `Info.plist` version are written by code that was not run
  (no Mac here); only the unit tests cover them.
- **The web**: defines reach the `.wasm` through the build's environment like any other build,
  but no web build was run with them.
- **Linux**: the installed program was seen to draw a frame; its text (flavor, version) was not
  read.
- **The example projects** (outside the workspace) were not rebuilt. `frus::main!` now records
  the build information; a project made by `frus create` builds with it, the eight examples were
  not compiled against this branch.
- **Signing installers**: neither the setup program nor the disk image is signed; SmartScreen
  and Gatekeeper warn. The Windows program itself has no version resource.
- **Other formats**: MSIX, MSI, AppImage, `.rpm`, Flatpak and Snap are not made.
- **Hot reload with a flavor that changes the Android package** stays off (restarts instead):
  the hot-reload engine builds its own package.
- **iOS**: flavors and versions do not reach an iOS package; frus does not package for iOS yet.
