// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus` — the framework's **facade**: a **single dependency** (`frus`) is all an
//! application needs. It re-exports the framework layer (`frus-shell`) and the widgets
//! (`frus-widgets`), plus the **single entry point** [`main!`].
//!
//! ```ignore
//! use frus::{Application, Command, Widget, Theme, button, column};
//!
//! #[derive(Default)]
//! struct App { count: i32 }
//! # #[derive(Clone)] enum Msg { Inc }
//! impl Application for App {
//!     type Message = Msg;
//!     fn update(&mut self, _m: Msg) -> Command<Msg> { Command::none() }
//!     fn view(&self, _t: &Theme) -> Box<dyn Widget<Msg>> { todo!() }
//! }
//!
//! // One entry point, for every platform.
//! frus::main!(App::default());
//! ```
//!
//! The thin desktop binary calls the generated `run()`: `fn main() -> frus::anyhow::Result<()>
//! { my_app::run() }`. On the web, the application keeps a `wasm-bindgen` dependency (targeted
//! at `wasm32`), which the generated `#[wasm_bindgen(start)]` entry point needs.

#![deny(missing_docs)]

// The README's example is compiled by `cargo test --doc`, so it cannot rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

// So are the guide's (`docs/guide`): every example in it is a complete program.
#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/layout.md")]
pub struct GuideLayout;
#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/style.md")]
pub struct GuideStyle;
#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/input.md")]
pub struct GuideInput;
#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/lists.md")]
pub struct GuideLists;
#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/navigation.md")]
pub struct GuideNavigation;
#[cfg(all(doctest, feature = "net", feature = "json"))]
#[doc = include_str!("../../../docs/guide/network.md")]
pub struct GuideNetwork;
#[cfg(all(doctest, feature = "net", feature = "sqlite", feature = "auth"))]
#[doc = include_str!("../../../docs/guide/data.md")]
pub struct GuideData;
#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/animation.md")]
pub struct GuideAnimation;
#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/shaders.md")]
pub struct GuideShaders;
#[doc = include_str!("../../../docs/guide/native-views.md")]
pub struct GuideNativeViews;
#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/build-variants.md")]
pub struct GuideBuildVariants;
#[cfg(all(doctest, feature = "testing"))]
#[doc = include_str!("../../../docs/guide/testing.md")]
pub struct GuideTesting;
#[cfg(doctest)]
#[doc = include_str!("../../../docs/guide/textures.md")]
pub struct GuideTextures;
#[cfg(all(
    doctest,
    feature = "file-dialogs",
    feature = "notifications",
    feature = "tray",
    feature = "taskbar",
    feature = "global-shortcuts",
    feature = "secure-store"
))]
#[doc = include_str!("../../../docs/guide/platform.md")]
pub struct GuidePlatform;

// Widgets, theming, layout, and the DSL (`row!` / `column!`) — all of `frus-widgets`.
pub use frus_widgets::*;

// Framework layer: the [`Application`] trait and its companions (effects, subscriptions,
// lifecycle).
pub use frus_shell::{
    app_data_dir, AppIcon, Application, Command, FrusApp, GlobalShortcut, Lifecycle, RemoteData,
    Subscription, Taskbar, TaskbarProgress, Tray, DECORATIVE_FRAME_RATE,
};

// Utility re-exports for the thin binary and for the entry-point macro (so the application
// does not have to declare `anyhow` / `log` itself).
pub use frus_shell::anyhow;

/// **Messages for the developer**: `frus::log::info!`, `warn!`, `error!`, `debug!`.
///
/// They reach the console with nothing to set up: the terminal on desktop, logcat on Android
/// (tagged with the application's crate name), the browser's console on the web. From `info`
/// up by default, and from `debug` up for the application's own crate in a debug build. On
/// desktop, `RUST_LOG` decides over that default.
///
/// ```
/// frus::log::info!("signed in as {}", "ada");
/// frus::log::warn!("the cache is {}% full", 93);
/// ```
pub use frus_shell::log;

// Macros: a glob (`frus_widgets::*`) does **not** re-export `#[macro_export]` macros, so we
// name them explicitly — hence `frus::main!`, `frus::column!`, `frus::row!`.
/// A value given at build time: `frus run --define API_URL=https://dev.example.com`, read with
/// `frus::define!("API_URL")` (milestone 790).
///
/// ```
/// const API_URL: &str = frus::define!("API_URL", "https://example.com");
/// # let _ = API_URL;
/// ```
pub use frus_shell::define;
pub use frus_shell::main;
/// What was built: the mode (`--debug`, `--profile`, `--release`), the flavor (`--flavor`) and
/// the version (`--build-name`, `--build-number`), milestone 790.
///
/// ```
/// if frus::build_mode() == frus::BuildMode::Release {
///     frus::log::info!("{:?} {:?}", frus::flavor(), frus::app_version());
/// }
/// ```
pub use frus_shell::{app_version, build_mode, flavor, AppVersion, BuildMode};
pub use frus_widgets::{asset, column, row};

/// Registering the application's own faces, and naming the families text resolves
/// to. An application that ships its own can drop the bundled ones through the
/// `bundled-*` features — see the "Shipping" section of the getting-started guide.
pub mod fonts {
    pub use frus_text::{add_font, set_default_family, set_monospace_family};
}

// External textures (milestone 793): `textures()`, `Texture` and `ExternalTexture` come with
// the widgets; the platform surface a decoder or camera draws into is the shell's.
pub use frus_shell::{SurfaceFormat, TextureSurface};

// The system's file dialogs (feature `file-dialogs`, desktop only): `FileDialog`.
#[cfg(all(
    feature = "file-dialogs",
    not(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))
))]
pub use frus_shell::FileDialog;

/// **frus's own wgpu**: the version the renderer draws with, for a [`GpuPass`]. An application
/// uses `frus::wgpu` rather than depending on `wgpu` itself, so the two never disagree.
pub use frus_shell::wgpu;
/// **Drawing on the GPU yourself** (milestone 792): a [`GpuPass`] box is given frus's device,
/// queue and a picture to draw into, then composited with the widgets around it.
pub use frus_shell::{GpuCallback, GpuPass, GpuPassContext, PassState, GPU_PASS_FORMAT};

/// A page driven by a test, with no window (feature `testing`).
#[cfg(feature = "testing")]
pub use frus_shell::Tester;

// The system's notifications (feature `notifications`, desktop, Android and the Web):
// `SystemNotification`.
#[cfg(all(feature = "notifications", not(target_os = "ios")))]
pub use frus_shell::{NotificationError, NotificationResponse, SystemNotification};

// Secrets kept by the system (feature `secure-store`, milestone 776): `secure_store()`. The
// preferences, `prefs()`, come with the widgets.
/// **Haptic feedback** (milestone 777): `frus::haptics::light()`, `medium()`, `heavy()`,
/// `selection()`, `success()`, `warning()`, `error()` and `vibrate(duration)`.
///
/// ```no_run
/// let _ = frus::haptics::success();
/// ```
pub use frus_shell::haptics;
pub use frus_shell::HapticsError;
#[cfg(feature = "secure-store")]
pub use frus_shell::{secure_store, SecureStore, SecureStoreError};

/// Asking the person for the camera, the microphone, the location, notifications
/// (milestone 730).
pub use frus_shell::{open_permission_settings, Permission, PermissionStatus};

/// Opening an address in the system's handler — the browser, the mail application, the
/// dialer — and sharing through the system's share sheet (milestone 775).
pub use frus_shell::{open_url, share, OpenUrlError, Share, ShareError, ShareOutcome};

/// What another application shares into this one, from the system's share sheet (milestone
/// 801): handed to [`Application::on_share`] or to [`FrusApp::on_share`].
pub use frus_shell::SharedContent;

/// Native views inside the page (milestones 794, 802): a plugin registers the factory that
/// makes its views — a Java class on Android, a Rust function on Windows and the web — and the
/// application embeds them with [`PlatformView`].
pub use frus_shell::{
    register_android_view_factory, register_platform_view_factory, NativeView, PlatformViewChannel,
    PlatformViewContext, PlatformViewError,
};

// Cross-platform HTTP (feature `net`): the `frus::fetch(url).await` shorthand and the
// `frus::Request` builder (method / headers / body / timeout) — see [`frus::net`].
#[cfg(feature = "net")]
pub use frus_shell::{
    fetch, fetch_bytes, net, FetchError, Method, Request, Response, MAX_RESPONSE_BYTES,
};

#[cfg(all(feature = "net", not(target_arch = "wasm32")))]
pub use frus_shell::DEFAULT_DISK_CACHE_BYTES;
/// **The cache of network images** (feature `net`, milestone 795): `frus::image_cache()`
/// clears, sizes and measures what `Image::network` keeps in memory and on disk.
///
/// ```no_run
/// frus::image_cache().clear();
/// ```
#[cfg(feature = "net")]
pub use frus_shell::{image_cache, ImageCache};

/// **A SQLite database** in the application's data folder (feature `sqlite`, milestone 795):
/// versioned migrations, typed parameters and rows, transactions, every call off the
/// interface's thread.
///
/// ```no_run
/// # async fn run() -> Result<(), frus::DatabaseError> {
/// let db = frus::database("notes.db")
///     .migrations(&["CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL)"])
///     .open()
///     .await?;
/// db.insert("INSERT INTO notes (body) VALUES (?1)", frus::params!["milk"]).await?;
/// # Ok(()) }
/// ```
#[cfg(feature = "sqlite")]
pub use frus_shell::{database, params, Database, DatabaseBuilder, DatabaseError};

#[cfg(all(feature = "auth", feature = "secure-store"))]
pub use frus_shell::stored_refresh_token;
/// **Signing in through the system's browser** (feature `auth`, milestone 795): OAuth 2 and
/// OpenID Connect, the authorization code with PKCE.
///
/// ```no_run
/// # async fn run() -> Result<(), frus::AuthError> {
/// let tokens = frus::auth_session(
///     frus::AuthRequest::new("https://id.example/authorize", "client", "http://127.0.0.1/cb")
///         .token_url("https://id.example/token")
///         .scope("openid"),
/// )
/// .await?;
/// # let _ = tokens; Ok(()) }
/// ```
#[cfg(feature = "auth")]
pub use frus_shell::{auth_session, AuthError, AuthRequest, AuthTokens, DEFAULT_AUTH_TIMEOUT};
