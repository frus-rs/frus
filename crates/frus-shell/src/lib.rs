// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus-shell` — frus's **framework layer**.
//!
//! Creates a native window (through `winit`), initialises the `frus-gpu`
//! [`frus_gpu::Renderer`] and drives the `event → frame` loop for any [`Application`]. This
//! is the only platform-dependent layer.

#![deny(missing_docs)]

// The README's example is compiled by `cargo test --doc`, so it cannot rot.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

// The `desktop` / `android` / `ios` / `web` aliases come from `build.rs`. They hold
// for **this** crate only: the `main!` macro below expands in the user's crate, so
// it keeps explicit `target_os` / `target_arch` predicates.

// Accessibility: AccessKit on desktop, a hand-rolled DOM bridge on the web, and a node
// provider of our own on Android (milestone 782) — not iOS (see a11y.rs, a11y_web.rs,
// a11y_android.rs and android_accessibility.rs).
#[cfg(desktop)]
mod a11y;
#[cfg(any(android, test))]
mod a11y_android;
#[cfg(web)]
mod a11y_web;
#[cfg(web)]
mod address;
/// Selecting aligned and cut texts through the shell (milestone 742).
#[cfg(all(test, desktop))]
mod aligned_selection_tests;
#[cfg(android)]
mod android_accessibility;
#[cfg(android)]
mod android_autofill;
#[cfg(android)]
mod android_clipboard;
#[cfg(android)]
mod android_device;
#[cfg(android)]
mod android_ime;
#[cfg(all(android, feature = "secure-store"))]
mod android_keystore;
#[cfg(android)]
mod android_lifecycle;
#[cfg(android)]
mod android_link;
#[cfg(all(android, feature = "notifications"))]
mod android_notification;
#[cfg(android)]
mod android_permission;
#[cfg(android)]
mod android_platform_view;
#[cfg(android)]
mod android_settings;
#[cfg(android)]
mod android_share;
#[cfg(android)]
mod android_system_bars;
mod app;
mod application;
// Signing in through the system's browser (feature `auth`, milestone 795).
#[cfg(feature = "auth")]
mod auth;
#[cfg(any(android, test))]
mod autofill;
// The build mode, the flavor, the version and the build-time values (milestone 790).
/// A field's caret through text that mixes directions, through the shell (milestone 747).
#[cfg(all(test, desktop))]
mod bidi_caret_tests;
mod build_info;
mod cache_dir;
mod caret;
mod command;
// Watching the device's networks (milestone 777).
mod connectivity;
mod data_dir;
/// A SQLite database in the application's data folder (feature `sqlite`, milestone 795).
#[cfg(feature = "sqlite")]
pub mod database;
/// DevTools' service in the application (milestone 788): debug builds only.
#[cfg(debug_assertions)]
mod devtools;
#[cfg(all(desktop, feature = "file-dialogs"))]
mod file_dialog;
mod file_drop;
mod frame_stats;
mod gesture;
mod gpu_pass;
// Haptic feedback (milestone 777).
pub mod haptics;
#[cfg(any(web, test))]
mod history;
mod hover;
// The disk cache of fetched images (milestone 795).
#[cfg(all(feature = "net", not(web)))]
mod http_cache;
mod icon;
// Where network images come from (milestone 795).
#[cfg(feature = "net")]
mod image_cache;
#[cfg(any(android, test))]
mod ime;
#[cfg(desktop)]
mod instance;
#[cfg(any(
    all(desktop, feature = "file-dialogs"),
    all(any(desktop, android), feature = "notifications")
))]
mod interface_thread;
mod link;
mod logging;
// Opening an address in the system's handler (milestone 775).
/// Cross-platform `fetch` HTTP helper (behind the `net` feature).
#[cfg(feature = "net")]
pub mod net;
#[cfg(all(any(desktop, android, web), feature = "notifications"))]
mod notification;
#[cfg(all(web, feature = "notifications"))]
mod notification_web;
mod open_url;
#[cfg(test)]
mod restoration_tests;
// Asking the person for the camera, the microphone, the location (milestone 730).
#[cfg(target_os = "macos")]
mod macos_permission;
mod permission;
// Native views inside the page (milestone 794): decided here on every host, carried out on
// Android by android_platform_view.rs.
#[cfg_attr(not(android), allow(dead_code))]
mod platform_view;
// Whether the user asked for less motion, on the desktops, iOS and the Web (milestone 778).
mod reduce_motion;
mod reload;
// Where each platform keeps the restoration data (milestone 753).
mod restoration;
#[cfg(web)]
mod web_device;
#[cfg(web)]
mod web_permission;
// How long the application takes to show its first frame (milestone 683, #211).
// Global shortcuts (milestone 709).
mod shortcuts;
mod startup;
mod storage;
// The taskbar's badge and progress (milestone 708).
mod taskbar;
// A platform surface feeding an external texture (milestone 793).
mod texture_surface;
// The tray icon (milestone 706): its description everywhere, the system's own on the desktop.
mod tray;
// The Web side of a reload under `frus run`: the states kept across the page's reload.
#[cfg(web)]
mod web_dev;
// Hot reload: patches applied while the application runs (the `hot-reload` feature).
#[cfg(feature = "hot-reload")]
mod hot;
// The channel `frus run` restarts an application through.
#[cfg(any(desktop, android))]
mod dev;
// What the keys of `frus run` ask a running application, on every platform.
mod dev_commands;
mod remote;
/// The async runtime: one executor and a reactor, shared by every asynchronous
/// effect. Native only — on the Web the browser is the runtime.
#[cfg(not(web))]
pub(crate) mod runtime;
#[cfg(feature = "secure-store")]
mod secure_store;
mod selection;
// Sharing through the system's share sheet (milestone 775).
mod share;
mod simulated;
mod subscription;
mod theming;
#[cfg(windows)]
mod title_bar;
// Native views hosted over frus's surface (milestone 802): what the Windows and the web hosts
// share, and the factories plugins register.
#[cfg_attr(not(any(windows, web)), allow(dead_code))]
mod view_host;
#[cfg(web)]
mod web_clipboard;
#[cfg(any(web, test))]
mod web_editing;
#[cfg(any(web, test))]
mod web_failure;
#[cfg(web)]
mod web_icon;
#[cfg(any(web, test))]
mod web_keys;
// The web host of native views (milestone 802).
#[cfg(any(web, test))]
mod web_page;
#[cfg(web)]
mod web_platform_view;
mod widget_app;
// The desktop window's size, place and maximized state, kept between runs (milestone 783).
#[cfg(any(desktop, test))]
mod window_memory;
// The Windows host of native views (milestone 802).
#[cfg(windows)]
mod windows_platform_view;
// When the next frame comes, with decoration alone moving (milestone 783).
mod pacing;

/// A windowless driver for tests. Hidden: not part of what an application builds on.
#[cfg(any(test, feature = "testing"))]
#[doc(hidden)]
pub use app::testing;
#[cfg(any(test, feature = "testing"))]
mod tester;
pub use app::App;
pub use application::{Application, Lifecycle, LocationStrategy};
#[doc(hidden)]
pub use build_info::{__BuildInfo, __register_build};
pub use build_info::{app_version, build_mode, flavor, AppVersion, BuildMode};
pub use command::Command;
pub use data_dir::app_data_dir;
#[cfg(all(desktop, feature = "file-dialogs"))]
pub use file_dialog::FileDialog;
/// **frus's own wgpu**, for a [`GpuPass`]: the version the renderer draws with, so an
/// application never pins one of its own.
pub use frus_gpu::wgpu;
/// A box the application draws on frus's GPU device itself (milestone 792), and what it
/// draws with.
pub use frus_gpu::{GpuCallback, GpuPassContext, PassState, GPU_PASS_FORMAT};
pub use gpu_pass::GpuPass;
pub use haptics::HapticsError;
pub use icon::AppIcon;
pub use link::{link_among, location_of_link};
#[cfg(all(any(desktop, android, web), feature = "notifications"))]
pub use notification::{NotificationError, NotificationResponse, SystemNotification};
pub use open_url::{open_url, OpenUrlError};
pub use pacing::DECORATIVE_FRAME_RATE;
pub use permission::{open_permission_settings, Permission, PermissionStatus};
pub use platform_view::{register_android_view_factory, PlatformViewError};
pub use remote::RemoteData;
#[cfg(feature = "secure-store")]
pub use secure_store::{secure_store, SecureStore, SecureStoreError};
pub use share::{share, Share, ShareError, ShareOutcome};
pub use shortcuts::GlobalShortcut;
#[cfg(android)]
#[doc(hidden)]
pub use storage::__android_started;
#[doc(hidden)]
pub use storage::__install_storage;
pub use subscription::Subscription;
pub use taskbar::{Taskbar, TaskbarProgress};
#[cfg(any(test, feature = "testing"))]
pub use tester::Tester;
pub use texture_surface::{SurfaceFormat, TextureSurface};
pub use tray::Tray;
pub use view_host::{
    register_platform_view_factory, NativeView, PlatformViewChannel, PlatformViewContext,
};
pub use widget_app::FrusApp;

/// Cross-platform HTTP (`net` feature) — the [`fetch`] shorthand, the [`Request`]
/// builder (method / headers / body / timeout) and the [`FetchError`] error type.
#[cfg(feature = "net")]
pub use net::{fetch, fetch_bytes, FetchError, Method, Request, Response, MAX_RESPONSE_BYTES};

#[cfg(all(feature = "auth", feature = "secure-store"))]
pub use auth::stored_refresh_token;
/// Signing in through the system's browser (feature `auth`, milestone 795).
#[cfg(feature = "auth")]
pub use auth::{auth_session, AuthError, AuthRequest, AuthTokens, DEFAULT_AUTH_TIMEOUT};
/// A SQLite database (feature `sqlite`, milestone 795): [`database()`], [`Database`].
#[cfg(feature = "sqlite")]
pub use database::{database, Database, DatabaseBuilder, DatabaseError, Row};

/// The size the disk cache of network images is kept under by default.
#[cfg(all(feature = "net", not(web)))]
pub use http_cache::DEFAULT_DISK_CACHE_BYTES;
/// The cache of network images, in memory and on disk (`net` feature, milestone 795).
#[cfg(feature = "net")]
pub use image_cache::{image_cache, ImageCache};

/// Re-export: size classes and orientation, so the application can drive its own
/// responsive behaviour.
pub use frus_widgets::{Brightness, Locale, Orientation, ScrollPhysics, SizeClass, ThemeMode};

/// Re-export of the Android entry type (supplied by `winit` / `android-activity`),
/// so an application can type its `android_main` without depending on winit.
#[cfg(android)]
pub use winit::platform::android::activity::AndroidApp;

/// Re-exports **for the [`main!`] macro**: the application declares its single entry
/// point without depending on these crates directly.
#[doc(hidden)]
pub use anyhow;
#[doc(hidden)]
pub use log;
/// Names the application's crate for its logs; called by [`main!`] (milestone 604).
#[doc(hidden)]
pub use logging::name_app as __name_app;

/// With the `hot-reload` feature, what `main!` adds: a `main`, so that the application's
/// `src/lib.rs` can be compiled as the program a hot-reload patch replaces (`frus run --hot`,
/// milestone 670). Decided here, where the feature is known: the application's crate is never
/// asked about it.
#[doc(hidden)]
#[macro_export]
#[cfg(all(
    feature = "hot-reload",
    not(any(target_os = "android", target_arch = "wasm32"))
))]
macro_rules! __hot_main {
    () => {
        #[allow(dead_code)]
        fn main() -> $crate::anyhow::Result<()> {
            run()
        }
    };
}

/// On the web and on Android, the program a patch replaces is a binary too, which must have a
/// `main`; the application still starts from its `#[wasm_bindgen(start)]` function, or from
/// `android_main` on Android (milestone 680).
#[doc(hidden)]
#[macro_export]
#[cfg(all(
    feature = "hot-reload",
    any(target_arch = "wasm32", target_os = "android")
))]
macro_rules! __hot_main {
    () => {
        #[allow(dead_code)]
        fn main() {}
    };
}

/// Without the `hot-reload` feature, nothing.
#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "hot-reload"))]
macro_rules! __hot_main {
    () => {};
}

/// Declares a frus application's **single entry point**. Invoked **exactly once** (in
/// the application's library), it generates the entry point of **every platform**, all
/// delegating to the **same** application:
///
/// - **desktop and iOS**: a `run()` function, which the app's thin binary calls;
/// - **Android**: the native `android_main` symbol the activity expects;
/// - **web**: the `#[wasm_bindgen(start)]` function.
///
/// The `cfg`s below are written with **explicit** `target_os` / `target_arch`
/// predicates rather than the `desktop` / `android` / `web` aliases from `build.rs`:
/// the body of a `macro_rules!` expands in the **application's** crate, where those
/// aliases are not defined. A `#[cfg(desktop)]` there would always be false, and the
/// application would end up with no entry point at all.
///
/// The argument is an **expression** building the application (re-evaluated per
/// platform, never shared):
///
/// ```ignore
/// frus_shell::main!(App::default());
/// ```
///
/// The web platform keeps its own `wasm-bindgen` dependency (targeted at `wasm32`),
/// which the generated `#[wasm_bindgen(start)]` entry point refers to via
/// `::wasm_bindgen`.
#[macro_export]
macro_rules! main {
    ($app:expr $(,)?) => {
        /// **Desktop and iOS** entry point: opens the window and drives the loop
        /// (called by the application's thin binary, or by the `.app` bundle's `main`
        /// on iOS). Generated by [`frus_shell::main!`].
        #[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
        pub fn run() -> $crate::anyhow::Result<()> {
            $crate::__name_app(::core::module_path!());
            $crate::__register_build($crate::__build_info!());
            $crate::__install_storage();
            $crate::run($app)
        }

        // Hot reload under `frus run --hot`: a `main`, with the `hot-reload` feature only.
        $crate::__hot_main!();

        /// **Android** entry point: called by the native activity. Generated by
        /// [`frus_shell::main!`].
        #[cfg(target_os = "android")]
        #[no_mangle]
        fn android_main(android_app: $crate::AndroidApp) {
            $crate::__name_app(::core::module_path!());
            $crate::__register_build($crate::__build_info!());
            $crate::__android_started(&android_app);
            if let ::core::result::Result::Err(err) = $crate::run_android($app, android_app) {
                $crate::log::error!("frus (android) stopped: {:#}", err);
            }
        }

        /// **Web** entry point: called when the wasm module loads. Generated by
        /// [`frus_shell::main!`].
        #[cfg(target_arch = "wasm32")]
        #[::wasm_bindgen::prelude::wasm_bindgen(start)]
        pub fn start() {
            $crate::__name_app(::core::module_path!());
            $crate::__register_build($crate::__build_info!());
            $crate::__install_storage();
            if let ::core::result::Result::Err(err) = $crate::run_web($app) {
                $crate::log::error!("frus (web) stopped: {:#}", err);
            }
        }
    };
}

/// Runs an application: opens the window and drives the event loop.
///
/// ```no_run
/// # struct MyApp;
/// # impl frus_shell::Application for MyApp {
/// #     type Message = ();
/// #     fn update(&mut self, _m: ()) -> frus_shell::Command<()> { frus_shell::Command::none() }
/// #     fn view(&self, _t: &frus_widgets::Theme)
/// #         -> Box<dyn frus_widgets::Widget<()>> { unimplemented!() }
/// # }
/// frus_shell::run(MyApp).unwrap();
/// ```
#[cfg(desktop)]
pub fn run<A: Application>(mut app: A) -> anyhow::Result<()> {
    startup::begin();
    storage::__install_storage();
    // Where the renderer draws through Vulkan — Linux, or any desktop given `WGPU_BACKEND=vulkan`
    // — it keeps its compiled pipelines between launches: in the user's cache folder, under the
    // program's name (milestones 685 and 697, #217). Nothing is written elsewhere.
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| {
        cache_dir::desktop(
            std::env::consts::OS,
            |name| std::env::var_os(name),
            exe.file_stem()?,
        )
    }) {
        frus_gpu::set_pipeline_cache_dir(dir);
    }
    // The application's `log` output, in the terminal, with nothing to set up
    // (milestone 604). `RUST_LOG` still decides over the default.
    logging::init::<A>();

    // Live-reload (dev): rehydrate the state left behind by the previous binary,
    // before `init` — see [`Application::restore_state`].
    reload::restore_from_env(&mut app);

    // An application that means to be one window: the second process hands its link to the
    // first and ends; the first serves the ones that come after it.
    if let Some(id) = app.instance_id() {
        match instance::claim(&id, link::link_among(std::env::args()).as_deref()) {
            instance::Claim::Handed => return Ok(()),
            instance::Claim::First(server) => instance::serve_into_queue(server),
        }
    }

    // A loop whose **user events** are messages: asynchronous effects deliver their
    // result through an `EventLoopProxy<Message>`.
    let event_loop = winit::event_loop::EventLoop::<A::Message>::with_user_event().build()?;
    // Keep asking for frames while an animation runs; otherwise wait.
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    let proxy = event_loop.create_proxy();
    let mut app = App::new(app, proxy);
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Runs an application on **iOS**. Same shape as [`run`] — winit takes care of
/// `UIApplicationMain` itself and `wgpu` renders through Metal — but without the
/// three desktop services that have no UIKit backend: no `env_logger` (stderr is
/// not readable on a device), no `arboard` clipboard, no AccessKit.
///
/// The [`main!`] macro generates the same `run()` as on desktop: it is the `.app`
/// bundle's binary that calls it from its `main`.
///
/// **Status: bootstrapping.** Lifecycle, safe-area insets, IME and the soft
/// keyboard are not wired yet — see the ROADMAP.
#[cfg(ios)]
pub fn run<A: Application>(app: A) -> anyhow::Result<()> {
    startup::begin();
    storage::__install_storage();
    let event_loop = winit::event_loop::EventLoop::<A::Message>::with_user_event().build()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    let proxy = event_loop.create_proxy();
    let mut app = App::new(app, proxy);
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Runs an application in the **browser** (wasm + WebGPU): winit creates a
/// `<canvas>` and **appends it** to the document, the renderer initialises
/// asynchronously (nothing may block on the web), and the loop is handed to the
/// browser through `spawn_app`, which never returns. Called from the application's
/// `#[wasm_bindgen(start)]` entry point.
#[cfg(web)]
pub fn run_web<A: Application + 'static>(app: A) -> anyhow::Result<()> {
    use winit::platform::web::EventLoopExtWebSys;
    startup::begin();
    storage::__install_storage();

    // Panics → the browser console, instead of an opaque "unreachable" — and the page, which
    // would otherwise stay as it was, silent (milestone 745).
    web_failure::install_panic_hook();
    // The application's `log` output, in the browser's console (milestone 604).
    logging::init::<A>();
    // The browser's operating system, before any theme is built: a web build is built for
    // no operating system, and the conventions to follow are those of the one the page is
    // read on (milestone 614).
    if let Some(navigator) = web_sys::window().map(|window| window.navigator()) {
        let platform = frus_widgets::TargetPlatform::from_browser(
            &navigator.platform().unwrap_or_default(),
            &navigator.user_agent().unwrap_or_default(),
            navigator.max_touch_points(),
        );
        frus_widgets::__set_detected_target_platform(platform);
        log::info!("target platform: {platform}, from the browser");
    }

    let event_loop = winit::event_loop::EventLoop::<A::Message>::with_user_event().build()?;
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

    let proxy = event_loop.create_proxy();
    let app = App::new(app, proxy);
    // On the web the loop is driven by the browser (requestAnimationFrame) and
    // `spawn_app` does not come back — hence returning `Ok` immediately.
    event_loop.spawn_app(app);
    Ok(())
}

/// Runs an application on **Android**: the native activity supplies the
/// [`AndroidApp`], which is handed to the winit loop. Entry point called from
/// `android_main`.
#[cfg(android)]
pub fn run_android<A: Application>(app: A, android_app: AndroidApp) -> anyhow::Result<()> {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    // A second activity of this process (milestone 763): winit builds one loop per process,
    // and the first activity's still runs. This one hands over to it and finishes, before
    // touching anything the running one uses (see `android_lifecycle`).
    if !android_lifecycle::first_in_process(&android_app) {
        logging::init::<A>();
        android_lifecycle::hand_over(&android_app);
        return Ok(());
    }
    startup::begin();
    // The application's folder, and its preferences, for an application started without
    // `main!` (which records them before the application is made).
    storage::__android_started(&android_app);
    // The renderer's compiled pipelines, kept in the application's cache folder between
    // launches: the next start reads them instead of compiling them again (milestones 685 and
    // 697, #217). The folder milestone 685 used inside `files` is removed.
    if let Some(files) = android_app.internal_data_path() {
        // Where `app_data_dir` points the application (milestone 779).
        data_dir::set_android_files(files.clone());
        let (now, before) = (
            cache_dir::android(&files),
            cache_dir::android_before_697(&files),
        );
        // Not when they are the same folder: the cache would be wiped at every start.
        if before != now {
            let _ = std::fs::remove_dir_all(before);
        }
        frus_gpu::set_pipeline_cache_dir(now);
    }

    // The application's `log` output, and its panics, in logcat under its crate's name
    // (milestone 604).
    logging::init::<A>();

    let ran = (|| -> anyhow::Result<()> {
        let event_loop = winit::event_loop::EventLoop::<A::Message>::with_user_event()
            .with_android_app(android_app.clone())
            .build()?;
        event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);

        let proxy = event_loop.create_proxy();
        let mut app = App::new(app, proxy);
        // Input bridge (a real InputConnection): composition, swipe, CJK. On failure the
        // shell falls back to key mode (TYPE_NULL).
        android_ime::install(&android_app);
        // Keep the activity handle around to query the system insets.
        app.set_android_app(android_app);
        event_loop.run_app(&mut app)?;
        Ok(())
    })();

    // The loop ends when the activity is destroyed (milestone 755: see `android_lifecycle`).
    // Android would keep the **process** cached for the next launch, but winit allows only
    // one `EventLoop` per process: a new activity in this process could not run. So the
    // process ends here, while `onDestroy` still waits for this function: the system keeps
    // what the activity saved, and the next launch starts a new process from it. A loop that
    // failed ends it too (milestone 763): a process left behind would take the next activity
    // and could not run it.
    let code = match ran {
        Ok(()) => 0,
        Err(err) => {
            log::error!("frus (android) stopped: {err:#}");
            1
        }
    };
    log::info!("the activity is gone: the process ends");
    std::process::exit(code);
}
