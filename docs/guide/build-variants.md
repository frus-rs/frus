# Build modes, flavors and releases

One application, built several ways: in debug while you work, in profile to measure it, in
release to ship it; as a `dev`, `staging` or `prod` flavor; with values the code reads at build
time; with the version a store expects. `frus run` and `frus build` take the same flags for all
of it, and the code reads the result with a few functions of `frus`.

## Modes

| flag | what it is | hot reload | frames timed |
| --- | --- | --- | --- |
| `--debug` (the default of `frus run`) | checks on, quick to build | yes | when asked |
| `--profile` | optimised as a release is, symbols kept | no | yes, in the log |
| `--release` (the default of `frus build`) | what ships | no | when asked |

```sh
frus run --profile               # measure: the frame timings appear in the log every 2 s
frus run -d android --profile
frus build apk --profile         # a package to try on a device, signed with the debug key
```

A profile build uses cargo's `profile` profile, which `frus create` writes into `Cargo.toml`:

```toml
[profile.profile]
inherits = "release"
strip = false
debug = "line-tables-only"
```

A project made before it existed builds in profile all the same: `frus` gives cargo that
profile through its environment. Its frame timings are the ones `FRUS_FRAME_STATS=1` turns on
in any build (`FRUS_FRAME_STATS=0` turns them off).

The code knows its mode:

```rust
if frus::build_mode() == frus::BuildMode::Debug {
    frus::log::debug!("a development build");
}
```

## Values for the code

```sh
frus run --define API_URL=http://localhost:3000 --define RETRIES=3
frus build web --define-from-file config/prod.json
frus build apk --define-from-file .env.production
```

`frus::define!` reads one. It is a constant of the build: nothing is read at run time, and a
build given another value compiles again by itself, for every platform.

```rust
// `Option<&'static str>`: None when the build was not given it.
let analytics = frus::define!("ANALYTICS_KEY");

// With a default, a `&'static str` that can be a constant.
const API_URL: &str = frus::define!("API_URL", "https://example.com");
# let _ = (analytics, API_URL);
```

A file is a JSON object (`{"API_URL": "https://…", "RETRIES": 3}`) or a `.env` file
(`API_URL=https://…`, `#` comments). A key is letters, digits and `_`. When a key is given
twice, a value typed on the command line wins over a file's, and a file's over a flavor's.

A define is compiled into the program: never put a secret there that the person running the
program must not read.

## Flavors

A flavor is declared once, in the application's `Cargo.toml`:

```toml
[package.metadata.frus.flavors.dev]
app_name = "Tides dev"            # the name people see: Android's label, the installers'
application_id_suffix = ".dev"    # installs beside the other flavors on Android
resources = "flavors/dev/res"     # Android resources laid over res/: another icon
features = ["dev"]                # cargo features it turns on (default: the one of its name)

[package.metadata.frus.flavors.dev.defines]
API_URL = "https://dev.example.com"

[package.metadata.frus.flavors.prod]
```

```sh
frus run --flavor dev
frus build appbundle --flavor prod
```

Every key is optional; an empty table declares a flavor with nothing of its own. Another icon
for a flavor is `flavors/dev/res/mipmap-*/ic_launcher.png`, the same five files as in `res/`.

```rust
let banner = match frus::flavor() {
    Some("dev") => "DEV",
    Some("staging") => "STAGING",
    _ => "",
};
# let _ = banner;
```

`frus::flavor()` is `None` in a test, which does not start through `frus::main!`.

On Android, a flavor that changes the package (its id, name, resources) runs with restarts
instead of hot reload: the hot-reload engine builds its own package.

## The version

```sh
frus build appbundle --build-name 1.4.0 --build-number 23
```

| | default | Android | macOS | Windows installer | Linux `.deb` |
| --- | --- | --- | --- | --- | --- |
| `--build-name` | `version` in `Cargo.toml` | `versionName` | `CFBundleShortVersionString` | `DisplayVersion`, file version | `Version` |
| `--build-number` | the version's build metadata (`1.4.0+23`) | `versionCode` | `CFBundleVersion` | the file version's fourth number | after `+` |

Without a number, Android's version code is derived from the version as before
(`1 << 24 | major << 16 | minor << 8 | patch`). With `--split-per-abi`, each processor's
package adds its own thousand: `1000` for `armeabi-v7a`, `2000` for `arm64-v8a`, `3000` for
`x86`, `4000` for `x86_64`, so a device that runs two of them installs the 64-bit one.

```rust
if let Some(version) = frus::app_version() {
    frus::log::info!("{} build {:?}", version.name, version.number);
}
```

## What you ship

```sh
frus build apk                    # target/release/apk/<name>.apk
frus build apk --split-per-abi    # target/release/apk/<name>-arm64-v8a.apk, one per target
frus build appbundle              # target/release/apk/<name>.aab, for a store
frus build web
frus build desktop
frus build installer              # Windows: a setup program; Linux: a .deb; macOS: a .dmg
```

**The App Bundle** is what Google Play takes. `frus` builds the libraries with cargo-apk as for
an APK, links the manifest and the resources with the SDK's `aapt2` in the form a bundle holds,
and makes the bundle with Google's `bundletool` (fetched the first time, about 30 MB; it runs on
the JDK the Android build uses). To try one on a phone before uploading it:

```sh
java -jar ~/.frus/tools/bundletool-all-1.18.3.jar build-apks --bundle=app.aab \
     --output=app.apks --ks=release.keystore --ks-pass=pass:… --connected-device
java -jar ~/.frus/tools/bundletool-all-1.18.3.jar install-apks --apks=app.apks
```

**One APK per processor** is smaller to download than one carrying every processor's library.
The manifest's `build_targets` say which.

**The installers** need no administrator, to be made or installed:

- *Windows*: a setup program made with NSIS (fetched the first time, about 2.5 MB). It installs
  for the current user into `%LOCALAPPDATA%\Programs`, adds a Start menu entry and an
  uninstaller to the system's list, and carries DirectX's fast shader compiler beside the
  program. `setup.exe /S` installs without a question.
- *Linux*: a `.deb` holding the program in `/usr/bin`, a desktop entry and the launcher icon.
  `sudo apt install ./<name>_<version>_amd64.deb`.
- *macOS*: the `.app` in a disk image with a link to Applications.

None of them is signed: Windows SmartScreen and macOS Gatekeeper warn the first time.

## Signing

An APK and an App Bundle in release are signed with your key, found where cargo-apk finds it:

1. `CARGO_APK_RELEASE_KEYSTORE` and `CARGO_APK_RELEASE_KEYSTORE_PASSWORD` in the environment
   (what a CI job wants), or
2. the manifest:

```toml
[package.metadata.android.signing.release]
path = "release.keystore"          # relative to Cargo.toml, or absolute
keystore_password = "…"
```

Make the key once, and keep it out of version control (`.gitignore` skips `*.keystore`):

```sh
keytool -genkey -v -keystore release.keystore -alias upload \
        -keyalg RSA -keysize 2048 -validity 10000
```

The key's password is the keystore's. The bundle is signed with the first private key in the
keystore. A profile build is signed with `[package.metadata.android.signing.profile]` (or
`CARGO_APK_PROFILE_KEYSTORE`) when there is one, and with the debug key otherwise; `frus run
--release` falls back to the debug key too, since it only installs the package.
