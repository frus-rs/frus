// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Native views hosted over frus's surface** (milestone 802): what the Windows and the web
//! hosts share, and what a plugin writes a view type with.
//!
//! On Android frus draws a native view itself, into its frame (`android_platform_view.rs`).
//! On Windows a native view is a child window, and on the web an element of the page: both
//! are drawn by the system **above** every pixel frus draws. frus therefore does not draw
//! them. It puts a *container* where the widget was laid out — a child window of its own, a
//! `<div>` — moves it on every frame the box moves, cuts out of it what frus draws over it
//! ([`Shape`]) and hides it while it is out of sight. The view itself fills its container.
//!
//! A host is the platform's half: [`ViewHost`]. Windows' is `windows_platform_view.rs`, the
//! web's `web_platform_view.rs`. Another platform adds its own the same way — an `NSView`
//! inside the content view on macOS, a `GtkWidget` in an overlay on Linux, a `UIView` on iOS
//! — and registers its view types; until then a `PlatformView` shows its fallback there.
//!
//! Nothing here runs until a view is asked for: no host is made, no thread started.

use std::sync::{Arc, Mutex};

use crate::platform_view::{Command, PixelRect, PlatformViewError, Shape};

/// The events the views sent since the loop last looked: the view, the name, the data.
static EVENTS: Mutex<Vec<(i32, String, String)>> = Mutex::new(Vec::new());
/// What wakes the loop for a frame.
#[allow(clippy::type_complexity)]
static WAKE: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

/// What wakes the loop when a view sends an event.
pub(crate) fn set_wake(wake: Arc<dyn Fn() + Send + Sync>) {
    *WAKE.lock().unwrap_or_else(|p| p.into_inner()) = Some(wake);
}

/// View `view` sent `name` with `data`: queued for its widget, and the loop woken.
pub(crate) fn send(view: i32, name: &str, data: &str) {
    EVENTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push((view, name.to_owned(), data.to_owned()));
    let wake = WAKE.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if let Some(wake) = wake {
        wake();
    }
}

/// The events the views sent since the last call.
pub(crate) fn take_events() -> Vec<(i32, String, String)> {
    std::mem::take(&mut *EVENTS.lock().unwrap_or_else(|p| p.into_inner()))
}

/// How a native view talks to its widget: [`send`](Self::send) an event, which becomes the
/// application's message through the widget's `on_event`. It may be cloned, kept, and used
/// from any thread.
///
/// ```no_run
/// fn on_click(channel: &frus_shell::PlatformViewChannel) {
///     channel.send("markerTapped", "Paris");
/// }
/// # let _ = on_click;
/// ```
#[derive(Clone, Debug)]
pub struct PlatformViewChannel {
    view: i32,
}

impl PlatformViewChannel {
    pub(crate) fn new(view: i32) -> Self {
        Self { view }
    }

    /// The view's number, unique while it lives.
    pub fn view(&self) -> i32 {
        self.view
    }

    /// Sends the widget an event named `name`, carrying `data`.
    pub fn send(&self, name: &str, data: &str) {
        send(self.view, name, data);
    }
}

/// A native view a plugin made, as frus drives it (milestone 802). Dropping it is how it is
/// disposed of: the widget left.
///
/// Every method has an empty default.
pub trait NativeView {
    /// The application called `method` with `arguments`
    /// ([`PlatformViewController::invoke`](frus_widgets::PlatformViewController::invoke)).
    fn invoke(&mut self, method: &str, arguments: &str) {
        let _ = (method, arguments);
    }

    /// The container was resized: the view fills `width` × `height` physical pixels.
    fn resized(&mut self, width: i32, height: i32) {
        let _ = (width, height);
    }

    /// The view came into sight (`true`) or went out of it: a view may stop its work while
    /// nobody sees it.
    fn visibility_changed(&mut self, visible: bool) {
        let _ = visible;
    }
}

/// What a factory makes a view with.
#[derive(Debug)]
pub struct PlatformViewContext<'a> {
    /// The creation parameters the widget gave.
    pub params: &'a str,
    /// The channel the view sends its events through.
    pub channel: PlatformViewChannel,
    /// The container's size, in physical pixels: the view fills it.
    pub width: i32,
    /// See [`width`](Self::width).
    pub height: i32,
    /// The container on Windows: a child window (`HWND`) of frus's window, positioned and
    /// clipped by frus. The view makes its own window a child of it, at `(0, 0)`.
    #[cfg(windows)]
    pub parent: isize,
    /// The container on the web: a `<div>` over the canvas, positioned and clipped by frus.
    /// The view appends its element to it, at 100% of its width and height.
    #[cfg(target_arch = "wasm32")]
    pub container: &'a web_sys::HtmlElement,
}

/// A factory of native views: called on the interface's thread each time a widget of its type
/// appears.
pub(crate) type Factory =
    dyn Fn(&PlatformViewContext<'_>) -> Result<Box<dyn NativeView>, String> + Send + Sync;

#[allow(clippy::type_complexity)]
static FACTORIES: Mutex<Vec<(String, Arc<Factory>)>> = Mutex::new(Vec::new());

/// The factory registered for `view_type`, if one was.
#[cfg_attr(not(any(windows, target_arch = "wasm32")), allow(dead_code))]
pub(crate) fn factory(view_type: &str) -> Option<Arc<Factory>> {
    FACTORIES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .find(|(t, _)| t == view_type)
        .map(|(_, f)| f.clone())
}

/// Registers the factory that makes the native views of `view_type` on **Windows** and on
/// **the web** (milestone 802), for [`PlatformView::new`](frus_widgets::PlatformView::new).
/// frus makes a container where the widget is laid out (a child window, a `<div>`), keeps it
/// in place, and asks `factory` for the view to put in it (see [`PlatformViewContext`]).
/// A factory registered under one of frus's own types replaces frus's.
///
/// Call it before the first view of that type is built. On Android a factory is a Java class
/// ([`register_android_view_factory`](crate::register_android_view_factory)); on macOS, Linux
/// and iOS it returns [`PlatformViewError::Unsupported`] and the widget shows its fallback.
///
/// ```no_run
/// struct Chart;
///
/// impl frus_shell::NativeView for Chart {
///     fn invoke(&mut self, method: &str, arguments: &str) {
///         println!("{method}({arguments})");
///     }
/// }
///
/// let registered = frus_shell::register_platform_view_factory("com.example/chart", |context| {
///     // Windows: make a child window of `context.parent`; the web: append an element to
///     // `context.container`.
///     let _ = context.params;
///     Ok(Box::new(Chart) as Box<dyn frus_shell::NativeView>)
/// });
/// if let Err(frus_shell::PlatformViewError::Unsupported) = registered {
///     // macOS, Linux, iOS: the widget shows its fallback.
/// }
/// ```
pub fn register_platform_view_factory(
    view_type: &str,
    factory: impl Fn(&PlatformViewContext<'_>) -> Result<Box<dyn NativeView>, String>
        + Send
        + Sync
        + 'static,
) -> Result<(), PlatformViewError> {
    if !cfg!(any(windows, target_arch = "wasm32")) {
        return Err(PlatformViewError::Unsupported);
    }
    let mut factories = FACTORIES.lock().unwrap_or_else(|p| p.into_inner());
    factories.retain(|(t, _)| t != view_type);
    factories.push((view_type.to_owned(), Arc::new(factory)));
    frus_widgets::register_platform_view_type(view_type);
    Ok(())
}

/// A platform's half of the native views hosted over frus's surface: what carries out the
/// commands [`PlatformViews::sync`](crate::platform_view::PlatformViews::sync) decides.
pub(crate) trait ViewHost {
    /// Make view `view` of `view_type` at `at` (the window's physical pixels), hidden until
    /// it is shaped. A type it cannot make sends [`frus_widgets::PlatformViewEvent::ERROR`].
    fn create(&mut self, view: i32, view_type: &str, params: &str, at: PixelRect);
    /// Move or resize it.
    fn place(&mut self, view: i32, at: PixelRect);
    /// Show only `shape` of it.
    fn shape(&mut self, view: i32, shape: &Shape);
    /// Drop it.
    fn dispose(&mut self, view: i32);
    /// Call `method` on it.
    fn invoke(&mut self, view: i32, method: &str, arguments: &str);
    /// A picture of it as it shows now, for frus to draw while a dialog covers it; `None`
    /// where none can be taken.
    fn snapshot(&mut self, view: i32) -> Option<frus_widgets::ImageHandle>;
    /// A press went to frus: a view holding the keyboard focus lets it go.
    fn clear_focus(&mut self);
}

/// Carries out `commands` on `host`, in order — except that a view newly covered whole is
/// not hidden yet: its picture is returned, to be drawn by the next frame, and the view is
/// hidden once that frame is on the screen ([`Covers`]).
pub(crate) fn run(host: &mut dyn ViewHost, commands: &[Command], covers: &mut Covers) {
    for command in commands {
        match command {
            Command::Create {
                view,
                view_type,
                params,
                at,
            } => host.create(*view, view_type, params, *at),
            Command::Place { view, at } => host.place(*view, *at),
            #[cfg(not(android))]
            Command::Clip { view, shape } => {
                if *shape == Shape::Covered {
                    match host.snapshot(*view) {
                        Some(picture) => covers.pictured(*view, picture),
                        None => host.shape(*view, shape),
                    }
                } else {
                    covers.shown(*view);
                    host.shape(*view, shape);
                }
            }
            Command::Dispose { view } => {
                covers.shown(*view);
                host.dispose(*view);
            }
            Command::Invoke {
                view,
                method,
                arguments,
            } => host.invoke(*view, method, arguments),
        }
    }
}

/// The views a dialog covers whole, pictured for frus to draw under it (milestone 802).
///
/// A window or an element over frus's surface cannot be put *under* a frus dialog: it is
/// hidden instead, and a picture of it is drawn in its place. The picture must be on the
/// screen before the view goes, or the place would be empty for a frame: so the view is
/// hidden one frame after its picture is handed over.
#[derive(Default)]
pub(crate) struct Covers {
    /// Pictures taken this frame, for the widgets to draw.
    fresh: Vec<(i32, frus_widgets::ImageHandle)>,
    /// Views whose picture a frame has drawn: to hide.
    due: Vec<i32>,
    /// Views shown as a picture now.
    pictured: Vec<i32>,
}

impl Covers {
    fn pictured(&mut self, view: i32, picture: frus_widgets::ImageHandle) {
        self.fresh.push((view, picture));
    }

    fn shown(&mut self, view: i32) {
        self.fresh.retain(|(v, _)| *v != view);
        self.due.retain(|v| *v != view);
    }

    /// The pictures taken since the last call: each goes to its view's widget, and the view
    /// is due to be hidden after the frame that draws it.
    pub(crate) fn take_fresh(&mut self) -> Vec<(i32, frus_widgets::ImageHandle)> {
        let fresh = std::mem::take(&mut self.fresh);
        for (view, _) in &fresh {
            self.due.push(*view);
            if !self.pictured.contains(view) {
                self.pictured.push(*view);
            }
        }
        fresh
    }

    /// The views to hide now that a frame has drawn their picture.
    pub(crate) fn take_due(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.due)
    }

    /// Whether `view` is shown as a picture; a view shown again stops being one, and its
    /// picture is to be forgotten.
    pub(crate) fn release(&mut self, view: i32) -> bool {
        let was = self.pictured.contains(&view);
        self.pictured.retain(|v| *v != view);
        was
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder {
        calls: Vec<String>,
        pictures: bool,
    }

    impl ViewHost for Recorder {
        fn create(&mut self, view: i32, view_type: &str, _: &str, _: PixelRect) {
            self.calls.push(format!("create {view} {view_type}"));
        }
        fn place(&mut self, view: i32, at: PixelRect) {
            self.calls.push(format!("place {view} {}", at.y));
        }
        fn shape(&mut self, view: i32, shape: &Shape) {
            self.calls.push(format!("shape {view} {shape:?}"));
        }
        fn dispose(&mut self, view: i32) {
            self.calls.push(format!("dispose {view}"));
        }
        fn invoke(&mut self, view: i32, method: &str, arguments: &str) {
            self.calls
                .push(format!("invoke {view} {method} {arguments}"));
        }
        fn snapshot(&mut self, _: i32) -> Option<frus_widgets::ImageHandle> {
            self.pictures
                .then(|| frus_widgets::ImageData::from_rgba(1, 1, vec![0; 4]).into_handle())
        }
        fn clear_focus(&mut self) {}
    }

    #[test]
    fn a_covered_view_is_pictured_then_hidden_a_frame_later() {
        let mut host = Recorder {
            pictures: true,
            ..Default::default()
        };
        let mut covers = Covers::default();
        run(
            &mut host,
            &[Command::Clip {
                view: 3,
                shape: Shape::Covered,
            }],
            &mut covers,
        );
        assert!(
            host.calls.is_empty(),
            "not hidden before its picture is drawn"
        );
        let fresh = covers.take_fresh();
        assert_eq!(fresh.len(), 1);
        assert_eq!(
            covers.take_due(),
            [3],
            "hidden after the frame that drew it"
        );
        assert!(covers.take_due().is_empty());
        // Shown again: the picture goes.
        run(
            &mut host,
            &[Command::Clip {
                view: 3,
                shape: Shape::Whole,
            }],
            &mut covers,
        );
        assert_eq!(host.calls, ["shape 3 Whole"]);
        assert!(covers.release(3));
        assert!(!covers.release(3));
    }

    #[test]
    fn a_host_that_cannot_picture_hides_at_once_and_commands_keep_their_order() {
        let mut host = Recorder::default();
        let mut covers = Covers::default();
        run(
            &mut host,
            &[
                Command::Create {
                    view: 1,
                    view_type: "frus/webview".into(),
                    params: String::new(),
                    at: PixelRect::default(),
                },
                Command::Invoke {
                    view: 1,
                    method: "loadUrl".into(),
                    arguments: "https://example.com".into(),
                },
                Command::Clip {
                    view: 1,
                    shape: Shape::Covered,
                },
                Command::Dispose { view: 1 },
            ],
            &mut covers,
        );
        assert_eq!(
            host.calls,
            [
                "create 1 frus/webview",
                "invoke 1 loadUrl https://example.com",
                "shape 1 Covered",
                "dispose 1"
            ]
        );
        assert!(covers.take_fresh().is_empty());
    }

    #[test]
    fn events_queue_until_taken() {
        let channel = PlatformViewChannel::new(42);
        channel.send("changed", "Ada");
        let events = take_events();
        assert!(events.contains(&(42, "changed".to_owned(), "Ada".to_owned())));
    }

    #[cfg(any(windows, target_arch = "wasm32"))]
    #[test]
    fn a_registered_factory_makes_its_type_available() {
        struct Nothing;
        impl NativeView for Nothing {}
        register_platform_view_factory("test802/factory", |_| {
            Ok(Box::new(Nothing) as Box<dyn NativeView>)
        })
        .unwrap();
        assert!(factory("test802/factory").is_some());
        assert!(frus_widgets::platform_view_type_available(
            "test802/factory"
        ));
    }

    #[cfg(not(any(windows, target_arch = "wasm32")))]
    #[test]
    fn elsewhere_a_factory_is_unsupported() {
        assert_eq!(
            register_platform_view_factory("test802/factory", |_| Err("no".into())),
            Err(PlatformViewError::Unsupported)
        );
    }
}
