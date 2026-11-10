# {{project-name}}

A [frus](https://github.com/frus-rs/frus) plugin: something the platform does, called from Rust.

```rust
let version = {{crate_name}}::platform_version()?;   // "Android 10 (API 29)"
```

| platform | side | today |
|---|---|---|
| Android | `android/java/…/{{ClassName}}Plugin.java`, called from `src/android.rs` | answers |
| Windows, macOS, Linux, the web, iOS | Rust, in `src/lib.rs` | `PluginError::Unsupported` |

## Android

`build.rs` compiles `android/java` with `javac` (a JDK) against the SDK's `android.jar`, and
`d8` makes the dex the library carries. At run time the dex is loaded into the application's
activity and the Java methods are called through JNI: the application adds nothing to its
manifest or its build. `frus build apk` and `frus run -d android` find the SDK; by hand, set
`ANDROID_HOME` (and `JAVA_HOME` if `javac` is not on the `PATH`).

A new Java method is a `public static` one on the class, called from `src/android.rs` like
`platformVersion`. A method that needs a permission declares it in the application's
`Cargo.toml` (`[package.metadata.frus.permissions]`).

## Using it

```toml
[dependencies]
{{project-name}} = { path = "../{{project-name}}" }
```

`frus test` runs the tests in `tests/` on this computer; `cargo run --example demo` prints what
this platform answers.
