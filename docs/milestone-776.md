# Milestone 776 — Preferences, and a secure store for secrets

## Objective

Two things every real application keeps and frus could not:

1. **Preferences**: small settings that outlive the process — the theme the person chose,
   whether the onboarding was seen, the last tab. Synchronous, any serde value, and a way for
   the interface to follow a change.
2. **Secrets**: a session token, a password, kept by the system, encrypted at rest and
   readable by this application only.

Neither is state restoration (`use_restorable_state`, milestones 431 and later), which keeps
what was on screen across a process the *system* ended, in the activity's saved state on
Android and the tab's `sessionStorage` on the web, and is gone when the person closes the
application. Restoration was left as it is; preferences are a store of their own.

## API

```rust
use frus::prefs;

prefs().set("theme", "dark")?;                       // Result<(), PrefsError>
let theme: Option<String> = prefs().get("theme");     // None when absent or of another type
prefs().remove("theme")?;
prefs().clear()?;

// In a component: rebuilt whenever the preference changes, from any thread.
let dark = cx.use_pref("dark_mode", false);
let _ = dark.set(!dark.get());

// Feature `secure-store`.
let store = frus::secure_store();
store.set("token", "…")?;                              // Result<(), SecureStoreError>
let token: Option<String> = store.get("token")?;
store.delete("token")?;
```

## Design

### Preferences live in the widgets, their storage in the shell

The hook has to be a `BuildContext` method, and `BuildContext` is in `frus-widgets`, so the
store is there too (`prefs.rs`): an in-memory map behind a mutex, loaded from its storage at the
first use, and a `PrefsStorage` trait (hidden) for where it is kept. The shell installs the
platform's storage (`storage.rs`):

| Platform | Where | How it is written |
| --- | --- | --- |
| Windows | `%APPDATA%\frus\<program>\prefs.json` | whole, to `prefs.json.<pid>.tmp`, `sync_all`, renamed over |
| macOS | `~/Library/Application Support/frus/<program>/prefs.json` | the same |
| Linux | `$XDG_DATA_HOME/frus/<program>/prefs.json` (or `~/.local/share/…`) | the same |
| iOS | `~/Library/Application Support/frus/<program>/prefs.json`, inside the sandbox | the same |
| Android | `<files>/frus-prefs.json` (`Context.getFilesDir`) | the same |
| Web | `localStorage`, one entry per key: `frus.pref.<key>` | `setItem` / `removeItem` |

`<program>` is the executable's name, as the pipeline cache (milestones 685 and 697) and the
notifications' identity already use; the data folder is the cache folder's sibling, not the
cache folder, which the system or the person may empty.

**When it is installed.** `main!` installs the storage *before* it evaluates the application's
expression, so `App::default()` may already read a preference. On Android that needs the
`files` folder before `run_android` is called: the macro calls a hidden
`__android_started(&android_app)` first, which records the folder and the Java VM. `run`,
`run_web` and `run_android` install it too, for an application started without the macro.
`Tester` installs an in-memory store, so a test never writes to the device.

**No storage, no pretending.** Without one, `set` answers `PrefsError::Unavailable` and `get`
answers `None`; nothing is kept in memory as if it were saved.

**Changes are atomic in memory too.** A change is made on a copy of the map, saved, and only
then taken: a write the disk refuses changes nothing and says so. Setting the value already
kept writes nothing. A file that does not parse is moved aside to `prefs.json.bad` (never
overwritten by the next write) and the store starts empty.

**Following a change.** The interface is rebuilt as a whole (the component runtime has no
per-component dirty marking), so the hook needs no subscription: a change sets a process-wide
flag, asks the current thread for a rebuild and wakes the shell's loop through the same waker
the component tasks use (`set_local_wake`). The shell's frame takes the flag beside
`take_rebuild_request`, so a preference set from a background task rebuilds the interface too.
`use_pref` returns a `UsePref<T>` whose `get` reads the store (the value kept, or the default)
and whose `set` writes it. An Elm-style `view` that reads `prefs()` is rebuilt the same way. A
separate `Listenable` per key was not added: the notifiers are single-threaded (`Rc`), and the
rebuild already reaches everything that reads a preference.

### The secure store

`frus-shell/src/secure_store.rs`, behind the feature `secure-store` (off by default, like the
other system services, since on Linux it brings a D-Bus client and its crypto, and on Apple
systems Security.framework's bindings):

- **Windows**: `CredWriteW` / `CredReadW` / `CredDeleteW` (windows-sys, already a dependency;
  one more feature, `Win32_Security_Credentials`). A generic credential named
  `frus.<program>/<key>`, persisted for the local machine, the value as UTF-8 (at most 2560
  bytes, the Credential Manager's limit, refused with a clear error above it).
- **macOS and iOS**: the Keychain's generic passwords through `security-framework` 3
  (MIT OR Apache-2.0, maintained by kornelski, the crate rustls' native certificates use);
  service `frus.<program>`, account the key.
- **Linux**: the Secret Service through `secret-service` 5.2 (MIT OR Apache-2.0; pure Rust over
  zbus 5, which was already in the tree; session encrypted with `crypto-rust`, so no OpenSSL or
  libdbus to build against). The default collection is unlocked first, which may show the
  desktop's prompt.
- **Android**: JNI to public platform API only, no dex change: `KeyGenerator` makes an
  AES-256 key in `AndroidKeyStore` the first time (`KeyGenParameterSpec`, GCM, no padding), the
  Keystore picks each nonce, the key name is bound as associated data, and the nonce and
  ciphertext are kept in hex in `<files>/frus-secure-store.json`, written atomically like the
  preferences. Each call runs in a JNI local frame (the thread stays attached), and every failed
  call clears its exception and reports the Java exception's text.
- **Web**: `SecureStoreError::Unsupported`, with the reason: a page has nowhere to keep a secret
  that every script running in it cannot read.

The `keyring` crate was considered. Its 4.x line is now a facade over `keyring-core` and one
crate per store; using it would have brought the same Secret Service and Security.framework
crates plus a store registry to set up at start, and its Android store needs `ndk-context`,
which frus does not initialise. The three direct calls per platform were smaller than the glue.

## How it was tested

- **Unit tests** (`frus-widgets` `prefs::tests`): every serde type round-trips; a second store
  on the same storage reads what the first kept; a refused write changes nothing and says so;
  the same value is not written twice; a value JSON cannot hold is refused; no storage, nothing
  kept; an unreadable storage starts empty; a change on another thread is seen by the shell's
  flag, and the hook reads the value or its default.
- **Unit tests** (`frus-shell`): the data folder per platform (`cache_dir`); the file storage
  round-trips and leaves no temporary file; a broken file is moved aside; an atomic write
  replaces, and a failed one leaves the old file and no temporary file (`storage`); the secret
  names, and on Windows a real Credential Manager round trip (`secure_store`).
- **Windows, for real**: a small application (`main!`, both stores, `use_pref` on screen) run
  twice; the second run read what the first wrote, and `cmdkey /list` showed the credential.
  See the report below.
- **Android**: the Android-only code (the Keystore bridge and the `files` folder) passes
  `cargo clippy -p frus-shell --features secure-store --no-deps --target aarch64-linux-android
  -- -D warnings`. This lint found two errors that no desktop build could see, both fixed: a
  closure that returned a JNI object without being able to name the environment's lifetime,
  which did not compile, and a manual `% 2`. The phone round trip was not run (see Not done).

### The Windows run

The check application keeps a launch counter and a theme in the preferences, and a token in
the secure store. Each run reads what the last one left, then writes new values. It started
from a clean state: no `prefs.json` and no credential.

```
run 1  prefs before: launches=None theme=None
       prefs set launches=1: Ok(())
       prefs set theme: Ok(())
       secure before: token=Ok(None)
       secure set token=secret-1: Ok(())
       secure after: token=Ok(Some("secret-1"))
       secure delete other: Ok(())          <- deleting a key never set is not an error
run 2  prefs before: launches=Some(1) theme=Some("dark")
       prefs set launches=2: Ok(())
       secure before: token=Ok(Some("secret-1"))
       secure set token=secret-2: Ok(())
       secure after: token=Ok(Some("secret-2"))
```

After both runs, `%APPDATA%\frus\prefcheck\prefs.json` held `{"launches": 2, "theme": "dark"}`.
`cmdkey /list` showed a generic credential with target `frus.prefcheck/token`, user `token`,
persisted on the local machine. The credential was deleted afterwards.

## Not done

- **Not run on an Android phone.** The test phone was disconnected for the whole session
  (`adb devices` listed nothing), so nothing was checked on a device: the Keystore key, the
  encrypted file, preferences in `files` across `am force-stop`, and `__android_started`
  running before the application's expression. The check application's APK compiled for
  `aarch64-linux-android`, but cargo-apk crashed with a bus error while packaging it, so no
  APK was produced either.
- **`use_pref` was not checked on screen.** The hook's rebuild was covered by unit tests only.
  No screenshot was taken during the Windows run.
- **Biometric unlock** of a secret (a key that needs a fingerprint or the device's PIN to
  use): out of scope.
- **macOS, iOS and Linux backends were not run**: they compile (`cargo check` for
  `x86_64-apple-darwin`, `aarch64-apple-ios` and Linux), but no Mac and no desktop with a
  Secret Service was at hand.
- **The web storage was not run in a browser**: it compiles for `wasm32`; `localStorage` and
  the `Unsupported` answer were not exercised in headless Edge.
- **Two processes** writing preferences at once: last writer wins (each writes its whole map);
  two tabs of a page see each other's changes only after a reload.
- **No change notification per key** beyond the rebuild (see Design).
- **iOS preferences** use the same file path as macOS inside the sandbox; not run on a device.
