// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Native views inside a frus page** (milestone 794): [`AndroidView`] embeds a real
//! `android.view.View` — a web view, a map, an ad, a native text field — sized and placed by
//! frus's layout, drawn with the rest of the frame, and given the touches that land on it.
//!
//! The widget only says *what* it wants and *where* it was laid out: the walk records that
//! ([`PlatformViewPlacement`]), and the shell makes the view the first time it appears, moves
//! or resizes it on the frames it changes, and drops it when the widget leaves. The view's
//! images come back through [`set_platform_view_frame`] and are drawn where the widget is, so
//! anything frus draws over it — a dialog, a menu — covers it, and a page scrolling under it
//! moves it in the same frame.
//!
//! [`PlatformView`] is the same widget for every platform (milestone 802): on Windows the view
//! is a child window, on the web an element of the page, both over frus's surface and moved
//! and clipped by the shell with the frame.
//!
//! On platforms without native views the widget shows its [fallback](AndroidView::fallback):
//! by default an [`UnsupportedView`], a labelled box in debug builds and nothing in release.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use frus_core::{
    Color, FilterQuality, ImageHandle, Point, Rect, Scene, SemanticsProperties, TextStyle,
};
use frus_layout::{Dimension, Style};

use crate::interaction::{Status, WidgetId};
use crate::theme::Theme;
use crate::widget::Widget;

/// The height an [`AndroidView`] takes when it is given none.
pub const ANDROID_VIEW_DEFAULT_HEIGHT: f32 = 300.0;

/// Whether this build embeds [`AndroidView`]s: Android only. [`PlatformView`] is hosted on
/// other platforms too: see [`platform_view_type_available`].
pub const PLATFORM_VIEWS_SUPPORTED: bool = cfg!(target_os = "android");

/// What an [`AndroidView`] asks the shell for: the kind of view and what to make it with.
///
/// ```
/// use frus_widgets::{AndroidView, Widget};
///
/// let view: AndroidView<()> = AndroidView::new("frus/webview", "https://example.com");
/// assert_eq!(view.spec().view_type(), "frus/webview");
/// assert_eq!(view.spec().params(), "https://example.com");
/// ```
#[derive(Clone, Debug)]
pub struct PlatformViewSpec {
    view_type: String,
    params: String,
    controller: Option<PlatformViewController>,
}

impl PlatformViewSpec {
    /// What a widget of its own asks for, to embed a native view the way [`AndroidView`]
    /// does: return it from [`Widget::platform_view`].
    pub fn new(view_type: impl Into<String>, params: impl Into<String>) -> Self {
        Self {
            view_type: view_type.into(),
            params: params.into(),
            controller: None,
        }
    }

    /// The same, driven through `controller`.
    pub fn with_controller(mut self, controller: &PlatformViewController) -> Self {
        self.controller = Some(controller.clone());
        self
    }

    /// The view type a factory was registered under (`"com.example/map"`).
    pub fn view_type(&self) -> &str {
        &self.view_type
    }

    /// The creation parameters handed to the factory.
    pub fn params(&self) -> &str {
        &self.params
    }

    /// The controller the application talks to the view through, if it gave one.
    pub fn controller(&self) -> Option<&PlatformViewController> {
        self.controller.as_ref()
    }
}

/// Something a native view sent its widget: a name and its data, both plain text.
///
/// Two names come from frus itself: [`FOCUS`](Self::FOCUS) (`"true"` when the view took the
/// input focus, `"false"` when it let it go) and [`ERROR`](Self::ERROR) (the view could not be
/// made, and why).
///
/// ```
/// use frus_widgets::PlatformViewEvent;
///
/// let event = PlatformViewEvent::new("pageFinished", "https://example.com/");
/// assert_eq!(event.name, "pageFinished");
/// assert!(!event.is_error());
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformViewEvent {
    /// What happened (`"pageFinished"`, `"changed"`, …), as the view's factory names it.
    pub name: String,
    /// What it carries.
    pub data: String,
}

impl PlatformViewEvent {
    /// The view took (`"true"`) or let go of (`"false"`) the input focus.
    pub const FOCUS: &'static str = "frus.focus";
    /// The view could not be made: no factory for its type, or the factory failed.
    pub const ERROR: &'static str = "frus.error";

    /// An event named `name` carrying `data`.
    pub fn new(name: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            data: data.into(),
        }
    }

    /// Whether the view could not be made.
    pub fn is_error(&self) -> bool {
        self.name == Self::ERROR
    }
}

/// One call queued for a native view: the method's name and its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformViewCall {
    /// The method, as the view's factory names it (`"loadUrl"`).
    pub method: String,
    /// Its arguments, as plain text.
    pub arguments: String,
}

#[derive(Default, Debug)]
struct ControllerState {
    view: Option<i32>,
    calls: Vec<PlatformViewCall>,
}

/// How an application talks to the native view of an [`AndroidView`]: keep one in the
/// application's state, hand it to the widget with [`AndroidView::controller`], and
/// [`invoke`](Self::invoke) the view's methods. Calls made before the view exists wait for it,
/// in order. The view answers through events ([`AndroidView::on_event`]).
///
/// ```
/// use frus_widgets::PlatformViewController;
///
/// let web = PlatformViewController::new();
/// web.invoke("loadUrl", "https://example.com");
/// assert!(!web.is_created(), "the view is made once its widget is laid out");
/// ```
#[derive(Clone, Default, Debug)]
pub struct PlatformViewController {
    state: Arc<Mutex<ControllerState>>,
}

impl PlatformViewController {
    /// A controller not bound to any view yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Calls `method` on the view with `arguments`, on the next frame (or once the view
    /// exists). Nothing comes back from the call itself: a view that answers sends an event.
    pub fn invoke(&self, method: impl Into<String>, arguments: impl Into<String>) {
        self.lock().calls.push(PlatformViewCall {
            method: method.into(),
            arguments: arguments.into(),
        });
    }

    /// Whether the view this controller drives exists.
    pub fn is_created(&self) -> bool {
        self.lock().view.is_some()
    }

    /// The native view's identity while it exists — for the shell.
    pub fn view_id(&self) -> Option<i32> {
        self.lock().view
    }

    /// Binds the controller to native view `view` (`None`: the view went) — for the shell.
    pub fn attach(&self, view: Option<i32>) {
        self.lock().view = view;
    }

    /// The calls queued since the last time — for the shell, which sends them to the view.
    pub fn take_calls(&self) -> Vec<PlatformViewCall> {
        std::mem::take(&mut self.lock().calls)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ControllerState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl PartialEq for PlatformViewController {
    /// Two controllers are equal when they are the same one.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

/// Where the walk found a native view, this frame: what the shell makes, moves and drops
/// views from.
#[derive(Clone, Debug, PartialEq)]
pub struct PlatformViewPlacement {
    /// The widget's identity: the view stays the same one while it does.
    pub id: WidgetId,
    /// What the widget asked for.
    pub view_type: String,
    /// The creation parameters (read only when the view is made).
    pub params: String,
    /// The whole box the widget was laid out in, in the window's logical pixels.
    pub rect: Rect,
    /// The part of it that shows, inside every clip around it.
    pub visible: Rect,
    /// The application's controller, if it gave one.
    pub controller: Option<PlatformViewController>,
    /// How many of each kind of press target had been registered when the widget was:
    /// whatever comes after was drawn over it and takes the presses it covers.
    pub(crate) above: TargetCounts,
}

/// The lengths of the press-target registries at one point of the walk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TargetCounts {
    pub hits: usize,
    pub long_presses: usize,
    pub pans: usize,
    pub pointer_listeners: usize,
    pub overlay_cover: usize,
    pub overlay_paint: usize,
    pub scrollables: usize,
}

thread_local! {
    /// The latest image of each native view, by widget, set by the shell.
    static FRAMES: RefCell<HashMap<u64, ImageHandle>> = RefCell::new(HashMap::new());
}

/// Sets the image the native view of `widget` last drew (`None` forgets it) — for the shell,
/// which copies the images the views produce. The widget draws it on the next frame.
// Compiled once, here: a copy inlined into an application would reach another thread-local
// (the hot-reload rule of tests/thread_locals.rs).
#[inline(never)]
pub fn set_platform_view_frame(widget: WidgetId, frame: Option<ImageHandle>) {
    FRAMES.with(|frames| {
        let mut frames = frames.borrow_mut();
        match frame {
            Some(frame) => {
                frames.insert(widget.as_u64(), frame);
            }
            None => {
                frames.remove(&widget.as_u64());
            }
        }
    });
}

/// The image the native view of the widget whose identity is `owner` last drew.
// Compiled once, here: a copy inlined into an application would reach another thread-local
// (the hot-reload rule of tests/thread_locals.rs).
#[inline(never)]
pub fn platform_view_frame(owner: u64) -> Option<ImageHandle> {
    FRAMES.with(|frames| frames.borrow().get(&owner).cloned())
}

type EventHandler<Msg> = Box<dyn Fn(&PlatformViewEvent) -> Option<Msg>>;

/// The view types the shell hosts on this platform besides Android's (milestone 802): a
/// Windows child window, a page element. Filled by the shell at start-up and by plugins.
static HOSTED_TYPES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Says that the shell can make native views of `view_type` on this platform — for the shell
/// and for plugins that register a factory (milestone 802). A [`PlatformView`] of a type
/// nobody registered shows its fallback.
///
/// ```
/// use frus_widgets::{platform_view_type_available, register_platform_view_type};
///
/// register_platform_view_type("com.example/chart");
/// assert!(platform_view_type_available("com.example/chart"));
/// ```
pub fn register_platform_view_type(view_type: &str) {
    let mut types = HOSTED_TYPES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !types.iter().any(|t| t == view_type) {
        types.push(view_type.to_owned());
    }
}

/// Whether a [`PlatformView`] of `view_type` gets a real native view here: on Android always
/// (its factories are Java classes, and a type without one sends
/// [`PlatformViewEvent::ERROR`]); elsewhere when the shell or a plugin
/// [registered](register_platform_view_type) it.
///
/// ```
/// use frus_widgets::platform_view_type_available;
///
/// if !cfg!(target_os = "android") {
///     assert!(!platform_view_type_available("com.example/never-registered"));
/// }
/// ```
pub fn platform_view_type_available(view_type: &str) -> bool {
    cfg!(target_os = "android")
        || HOSTED_TYPES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .any(|t| t == view_type)
}

/// What [`AndroidView`] and [`PlatformView`] share: the view asked for, its size, its events
/// and what stands in for it.
struct Embedded<Msg> {
    spec: PlatformViewSpec,
    width: Option<f32>,
    height: Option<f32>,
    on_event: Option<EventHandler<Msg>>,
    label: Option<String>,
    /// Whether a native view is made here; the fallback stands in otherwise.
    hosted: bool,
    /// The fallback, shown when `hosted` is false.
    fallback: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: 'static> Embedded<Msg> {
    fn new(widget: &str, view_type: String, params: String, hosted: bool) -> Self {
        let fallback: Vec<Box<dyn Widget<Msg>>> = if hosted {
            Vec::new()
        } else {
            vec![Box::new(UnsupportedView::new(format!(
                "{widget} \"{view_type}\"\nnot available on this platform"
            )))]
        };
        Self {
            spec: PlatformViewSpec::new(view_type, params),
            width: None,
            height: None,
            on_event: None,
            label: None,
            hosted,
            fallback,
        }
    }

    fn style(&self) -> Style {
        Style {
            width: self
                .width
                .map_or(Dimension::Percent(1.0), Dimension::Length),
            height: Dimension::Length(self.height.unwrap_or(ANDROID_VIEW_DEFAULT_HEIGHT)),
            min_width: Dimension::Length(0.0),
            min_height: Dimension::Length(0.0),
            flex_shrink: 0.0,
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        if self.hosted {
            &[]
        } else {
            &self.fallback
        }
    }

    fn paint(&self, bounds: Rect, status: Status, scene: &mut Scene) {
        if !self.hosted {
            return;
        }
        // The view's latest image: on Android every image it draws; on a platform whose
        // views are windows or page elements over frus's surface, the picture taken while
        // something frus draws covers it whole. Until one arrives there is nothing to show,
        // which is what a native view shows before it has drawn.
        if let Some(frame) = platform_view_frame(scene.current_owner()) {
            scene.draw_image_with(
                &frame,
                bounds,
                Rect::new(0.0, 0.0, 1.0, 1.0),
                Color::WHITE.fade(status.opacity),
                false,
                // The image is the view's own pixels, one for one: no chain to make.
                FilterQuality::Low,
            );
        }
    }

    fn semantics(&self) -> Option<SemanticsProperties> {
        self.label
            .as_ref()
            .map(|label| SemanticsProperties::new(frus_core::Role::None).label(label.clone()))
    }
}

/// The builder methods and the `Widget` impl of a widget wrapping an [`Embedded`].
macro_rules! embedded_view {
    ($name:ident, $where:literal) => {
        impl<Msg: 'static> $name<Msg> {
            /// The width, in logical pixels. By default the view takes the whole width it is
            /// given.
            pub fn width(mut self, width: f32) -> Self {
                self.0.width = Some(width.max(0.0));
                self
            }

            /// The height, in logical pixels ([`ANDROID_VIEW_DEFAULT_HEIGHT`] by default).
            pub fn height(mut self, height: f32) -> Self {
                self.0.height = Some(height.max(0.0));
                self
            }

            /// The controller the application calls the view's methods through.
            pub fn controller(mut self, controller: &PlatformViewController) -> Self {
                self.0.spec.controller = Some(controller.clone());
                self
            }

            /// The message for each event the view sends (`None` sends nothing).
            pub fn on_event(
                mut self,
                handler: impl Fn(&PlatformViewEvent) -> Option<Msg> + 'static,
            ) -> Self {
                self.0.on_event = Some(Box::new(handler));
                self
            }

            /// What a screen reader says the view is, before it reads the view's own content.
            pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
                self.0.label = Some(label.into());
                self
            }

            #[doc = concat!(
                                "What stands in for the view where it cannot be made (",
                                $where,
                                "). The default is an [`UnsupportedView`] naming the view type."
                            )]
            pub fn fallback(mut self, fallback: impl Widget<Msg> + 'static) -> Self {
                self.0.fallback = vec![Box::new(fallback)];
                self
            }

            /// What the widget asks the shell for.
            pub fn spec(&self) -> &PlatformViewSpec {
                &self.0.spec
            }

            /// Whether a native view is made here, rather than the fallback shown.
            pub fn is_hosted(&self) -> bool {
                self.0.hosted
            }
        }

        impl<Msg: 'static> Widget<Msg> for $name<Msg> {
            fn style(&self) -> Style {
                self.0.style()
            }

            fn children(&self) -> &[Box<dyn Widget<Msg>>] {
                self.0.children()
            }

            fn paint(&self, bounds: Rect, status: Status, _theme: &Theme, scene: &mut Scene) {
                self.0.paint(bounds, status, scene);
            }

            fn on_click(&self) -> Option<Msg> {
                None
            }

            fn debug_name(&self) -> &'static str {
                stringify!($name)
            }

            fn semantics(&self) -> Option<SemanticsProperties> {
                self.0.semantics()
            }

            fn platform_view(&self) -> Option<&PlatformViewSpec> {
                self.0.hosted.then_some(&self.0.spec)
            }

            fn on_platform_view_event(&self, event: &PlatformViewEvent) -> Option<Msg> {
                self.0.on_event.as_ref().and_then(|handler| handler(event))
            }
        }
    };
}

/// A **native Android view** inside the page: `view_type` names the factory that makes it
/// (registered by a plugin, or one of frus's own, `"frus/webview"` and `"frus/edittext"`),
/// and `params` is handed to it.
///
/// The view is made the first time the widget is laid out, moved or resized on the frames
/// its box changes, and dropped when the widget leaves. It is drawn where the widget is,
/// under anything frus draws over it, and the touches that land on it are its own from the
/// press to the lift. It takes the focus and the keyboard as a native view does, and a
/// screen reader reads its own content in its place.
///
/// Elsewhere than on Android it shows its [fallback](Self::fallback). [`PlatformView`] is the
/// same widget for every platform that hosts native views.
///
/// ```
/// use frus_widgets::{AndroidView, PlatformViewController, PlatformViewEvent};
///
/// #[derive(Clone)]
/// enum Msg {
///     Loaded(String),
/// }
///
/// let web = PlatformViewController::new();
/// let page: AndroidView<Msg> = AndroidView::new("frus/webview", "<h1>Hello</h1>")
///     .height(240.0)
///     .controller(&web)
///     .on_event(|event: &PlatformViewEvent| {
///         (event.name == "pageFinished").then(|| Msg::Loaded(event.data.clone()))
///     });
/// # let _ = page;
/// ```
pub struct AndroidView<Msg>(Embedded<Msg>);

impl<Msg: 'static> AndroidView<Msg> {
    /// The view `view_type` makes, given `params`.
    pub fn new(view_type: impl Into<String>, params: impl Into<String>) -> Self {
        Self(Embedded::new(
            "AndroidView",
            view_type.into(),
            params.into(),
            PLATFORM_VIEWS_SUPPORTED,
        ))
    }
}

embedded_view!(AndroidView, "every platform but Android");

/// A **native view** inside the page, on every platform that hosts one (milestone 802): an
/// Android `View`, a Windows child window, an element of the web page. `view_type` names the
/// factory that makes it and `params` is handed to it.
///
/// frus's own types are made on each platform by what that platform has:
///
/// | View type | Android | Windows | Web |
/// | --- | --- | --- | --- |
/// | `"frus/webview"` | `WebView` | WebView2 (feature `webview2`) | `<iframe>` |
/// | `"frus/edittext"` | `EditText` | `EDIT` control | `<input>` |
/// | `"frus/video"` | | | `<video>` |
///
/// They understand the same calls and send the same events, so one
/// [`PlatformViewController`] drives each of them the same way. A plugin adds types of its
/// own. A type with no factory on the platform shows the [fallback](Self::fallback): an
/// [`UnsupportedView`] by default (on macOS, Linux and iOS, for now, every type does).
///
/// On Android the view is drawn by frus into the frame, so anything frus draws over it
/// covers it. On Windows and the web the view is a window or an element over frus's surface:
/// frus moves and clips it on every frame its box changes, leaves out of it the parts a frus
/// overlay or a button covers, and while a dialog covers it whole, shows a picture of it
/// (Windows) or nothing (the web) under the dialog.
///
/// ```
/// use frus_widgets::{PlatformView, PlatformViewController, PlatformViewEvent};
///
/// #[derive(Clone)]
/// enum Msg {
///     Typed(String),
/// }
///
/// let field = PlatformViewController::new();
/// let name: PlatformView<Msg> = PlatformView::new("frus/edittext", "Your name")
///     .height(40.0)
///     .controller(&field)
///     .on_event(|event: &PlatformViewEvent| {
///         (event.name == "changed").then(|| Msg::Typed(event.data.clone()))
///     });
/// field.invoke("setText", "Ada");
/// # let _ = name;
/// ```
pub struct PlatformView<Msg>(Embedded<Msg>);

impl<Msg: 'static> PlatformView<Msg> {
    /// The view `view_type` makes, given `params`.
    pub fn new(view_type: impl Into<String>, params: impl Into<String>) -> Self {
        let view_type = view_type.into();
        let hosted = platform_view_type_available(&view_type);
        Self(Embedded::new(
            "PlatformView",
            view_type,
            params.into(),
            hosted,
        ))
    }
}

embedded_view!(PlatformView, "a platform without a factory for its type");

/// What stands in for a native view where there are none: in a debug build an outlined box
/// with a label saying what is missing; in a release build nothing at all,
/// so a shipped application never shows a developer's note.
///
/// Its colours and text style default to the theme's and can each be set.
///
/// ```
/// use frus_widgets::UnsupportedView;
///
/// let stand_in = UnsupportedView::new("Map: Android only").shown_in_release(true);
/// # let _ = stand_in;
/// ```
pub struct UnsupportedView {
    label: String,
    background: Option<Color>,
    outline: Option<Color>,
    text_color: Option<Color>,
    text_style: Option<TextStyle>,
    in_release: bool,
}

impl UnsupportedView {
    /// A stand-in saying `label`.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            background: None,
            outline: None,
            text_color: None,
            text_style: None,
            in_release: false,
        }
    }

    /// The fill (the theme's `surface_container_highest` by default).
    pub fn background(mut self, color: Color) -> Self {
        self.background = Some(color);
        self
    }

    /// The outline (the theme's `outline` by default).
    pub fn outline(mut self, color: Color) -> Self {
        self.outline = Some(color);
        self
    }

    /// The label's colour (the theme's `on_surface_variant` by default).
    pub fn text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }

    /// The label's style (the theme's `label_medium` by default).
    pub fn text_style(mut self, style: TextStyle) -> Self {
        self.text_style = Some(style);
        self
    }

    /// Whether a release build draws it too (`false` by default: it is a developer's note).
    pub fn shown_in_release(mut self, shown: bool) -> Self {
        self.in_release = shown;
        self
    }

    fn drawn(&self) -> bool {
        cfg!(debug_assertions) || self.in_release
    }
}

impl<Msg> Widget<Msg> for UnsupportedView {
    fn style(&self) -> Style {
        Style {
            width: Dimension::Percent(1.0),
            height: Dimension::Percent(1.0),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &[]
    }

    fn paint(&self, bounds: Rect, status: Status, theme: &Theme, scene: &mut Scene) {
        if !self.drawn() {
            return;
        }
        let o = status.opacity;
        let scheme = &theme.scheme;
        scene.draw_rect(
            bounds,
            self.background
                .unwrap_or(scheme.surface_container_highest)
                .fade(o),
            4.0,
            1.0,
            self.outline.unwrap_or(scheme.outline).fade(o),
        );
        let style = self
            .text_style
            .unwrap_or_else(|| crate::theme::type_scale(Some(theme)).label_medium)
            .resolved();
        let pad = 8.0;
        scene.text_wrapped(
            Point::new(bounds.x + pad, bounds.y + pad),
            self.label.clone(),
            &style,
            self.text_color.unwrap_or(scheme.on_surface_variant).fade(o),
            (bounds.width - 2.0 * pad).max(0.0),
        );
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "UnsupportedView"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::Runtime;
    use crate::ui::build_ui;
    use crate::{Container, Flex, SingleChildScrollView, Stack};
    use frus_core::Size;

    /// A native view on every host, as an `AndroidView` is on Android.
    struct Native(PlatformViewSpec);

    impl Widget<u8> for Native {
        fn style(&self) -> Style {
            Style {
                width: Dimension::Length(100.0),
                height: Dimension::Length(100.0),
                flex_shrink: 0.0,
                ..Default::default()
            }
        }
        fn children(&self) -> &[Box<dyn Widget<u8>>] {
            &[]
        }
        fn paint(&self, _: Rect, _: Status, _: &Theme, _: &mut Scene) {}
        fn on_click(&self) -> Option<u8> {
            None
        }
        fn platform_view(&self) -> Option<&PlatformViewSpec> {
            Some(&self.0)
        }
    }

    fn native() -> Native {
        Native(PlatformViewSpec::new("frus/webview", "<p>hi</p>"))
    }

    fn frame(tree: &dyn Widget<u8>, rt: &Runtime) -> crate::ui::Ui<u8> {
        build_ui(tree, Size::new(400.0, 300.0), rt, &Theme::default())
    }

    #[test]
    fn the_walk_records_where_a_native_view_is_and_presses_on_it_are_its() {
        let tree = Flex::column()
            .child(Container::new().height(50.0))
            .child(native());
        let rt = Runtime::default();
        let ui = frame(&tree, &rt);
        let views = ui.platform_views();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].view_type, "frus/webview");
        assert_eq!(views[0].rect, Rect::new(0.0, 50.0, 100.0, 100.0));
        assert_eq!(
            ui.platform_view_at(Point::new(50.0, 100.0)).map(|v| v.id),
            Some(views[0].id)
        );
        assert!(ui.platform_view_at(Point::new(50.0, 20.0)).is_none());
        assert!(
            ui.semantics().iter().any(|(id, ..)| *id == views[0].id),
            "a node for the screen reader to read the view's content in"
        );
    }

    #[test]
    fn something_drawn_over_the_view_takes_the_presses_it_covers() {
        let tree = Stack::new()
            .layer(native())
            .layer(Container::new().width(40.0).height(40.0).on_click(1u8));
        let rt = Runtime::default();
        let ui = frame(&tree, &rt);
        assert!(
            ui.platform_view_at(Point::new(20.0, 20.0)).is_none(),
            "the button over the view"
        );
        assert!(ui.platform_view_at(Point::new(80.0, 80.0)).is_some());
        // And a parent that answers presses, drawn first, does not.
        let under = Container::new().on_click(2u8).child(native());
        let ui = frame(&under, &rt);
        assert!(ui.platform_view_at(Point::new(20.0, 20.0)).is_some());
    }

    #[test]
    fn a_dialog_over_the_view_takes_every_press() {
        let tree = crate::AlertDialog::new(true)
            .title("Over it")
            .on_dismiss(3u8)
            .body(native());
        let rt = Runtime::default();
        let ui = frame(&tree, &rt);
        assert_eq!(
            ui.platform_views().len(),
            1,
            "the view is still there, under it"
        );
        assert!(ui.platform_view_at(Point::new(50.0, 50.0)).is_none());
    }

    #[test]
    fn a_view_scrolled_out_of_sight_is_kept_and_a_cached_page_still_reports_it() {
        let tree = SingleChildScrollView::new().height(100.0).child(
            Flex::column()
                .child(Container::new().height(300.0))
                .child(native()),
        );
        let rt = Runtime::default();
        let ui = frame(&tree, &rt);
        let views = ui.platform_views();
        assert_eq!(views.len(), 1, "out of sight, still wanted");
        assert!(views[0].visible.width <= 0.0 || views[0].visible.height <= 0.0);
        assert!(ui.platform_view_at(Point::new(50.0, 50.0)).is_none());
        // In sight inside the scrolling area, the area around it does not take its presses.
        let shown = SingleChildScrollView::new().height(100.0).child(
            Flex::column()
                .child(native())
                .child(Container::new().height(300.0)),
        );
        let ui = frame(&shown, &rt);
        assert!(ui.platform_view_at(Point::new(50.0, 50.0)).is_some());

        let boundary = Container::<u8>::new().repaint_boundary().child(native());
        for _ in 0..3 {
            let ui = frame(&boundary, &rt);
            assert_eq!(
                ui.platform_views().len(),
                1,
                "never replayed from the cache"
            );
        }
    }

    #[test]
    fn calls_wait_for_the_view_in_order() {
        let controller = PlatformViewController::new();
        controller.invoke("loadUrl", "a");
        controller.invoke("evaluateJavascript", "b");
        assert!(!controller.is_created());
        controller.attach(Some(7));
        assert_eq!(controller.view_id(), Some(7));
        let calls = controller.take_calls();
        assert_eq!(
            calls.iter().map(|c| c.method.as_str()).collect::<Vec<_>>(),
            ["loadUrl", "evaluateJavascript"]
        );
        assert!(controller.take_calls().is_empty(), "each call goes once");
        assert_eq!(
            controller.clone(),
            controller,
            "a clone is the same controller"
        );
        assert_ne!(PlatformViewController::new(), controller);
    }

    #[test]
    fn frames_are_kept_by_widget_and_forgotten() {
        let id = WidgetId::from_u64(0xfeed);
        let frame = frus_core::ImageData::from_rgba(1, 1, vec![1, 2, 3, 4]).into_handle();
        set_platform_view_frame(id, Some(frame.clone()));
        assert_eq!(
            platform_view_frame(0xfeed).map(|f| f.id()),
            Some(frame.id())
        );
        set_platform_view_frame(id, None);
        assert!(platform_view_frame(0xfeed).is_none());
    }

    #[test]
    fn events_become_the_applications_messages() {
        let view: AndroidView<String> = AndroidView::new("frus/edittext", "Name")
            .on_event(|event| (event.name == "changed").then(|| event.data.clone()));
        assert_eq!(
            view.on_platform_view_event(&PlatformViewEvent::new("changed", "Ada")),
            Some("Ada".to_string())
        );
        assert_eq!(
            view.on_platform_view_event(&PlatformViewEvent::new("submitted", "Ada")),
            None
        );
    }

    #[test]
    fn sizing_takes_the_width_and_a_default_height() {
        let view: AndroidView<()> = AndroidView::new("frus/webview", "");
        let style = Widget::<()>::style(&view);
        assert_eq!(style.width, Dimension::Percent(1.0));
        assert_eq!(style.height, Dimension::Length(ANDROID_VIEW_DEFAULT_HEIGHT));
        let sized: AndroidView<()> = AndroidView::new("frus/webview", "")
            .width(120.0)
            .height(80.0);
        let style = Widget::<()>::style(&sized);
        assert_eq!(style.width, Dimension::Length(120.0));
        assert_eq!(style.height, Dimension::Length(80.0));
    }

    #[cfg(not(target_os = "android"))]
    #[test]
    fn elsewhere_the_fallback_stands_in_and_no_view_is_asked_for() {
        let view: AndroidView<()> = AndroidView::new("com.example/map", "");
        assert!(view.platform_view().is_none());
        assert_eq!(view.children().len(), 1);
        assert_eq!(view.children()[0].debug_name(), "UnsupportedView");
        let custom: AndroidView<()> =
            AndroidView::new("com.example/map", "").fallback(crate::Placeholder::new());
        assert_eq!(custom.children()[0].debug_name(), "Placeholder");
    }

    #[test]
    fn a_platform_view_is_hosted_where_its_type_is_registered() {
        let missing: PlatformView<()> = PlatformView::new("test802/missing", "");
        assert!(!missing.is_hosted());
        assert!(missing.platform_view().is_none() || cfg!(target_os = "android"));
        if !cfg!(target_os = "android") {
            assert_eq!(missing.children()[0].debug_name(), "UnsupportedView");
        }
        register_platform_view_type("test802/hosted");
        register_platform_view_type("test802/hosted");
        let hosted: PlatformView<()> = PlatformView::new("test802/hosted", "p")
            .fallback(crate::Placeholder::new())
            .height(40.0);
        assert!(hosted.is_hosted());
        assert!(
            hosted.children().is_empty(),
            "no fallback where it is hosted"
        );
        assert_eq!(hosted.platform_view().map(|spec| spec.params()), Some("p"));
        assert_eq!(Widget::<()>::debug_name(&hosted), "PlatformView");
        // An Android view stays an Android view: hosted on Android alone.
        let android: AndroidView<()> = AndroidView::new("test802/hosted", "");
        assert_eq!(android.is_hosted(), cfg!(target_os = "android"));
    }

    #[test]
    fn what_frus_paints_over_a_view_is_left_out_of_it() {
        // A button laid over the view.
        let tree = Stack::new()
            .layer(native())
            .layer(Container::new().width(40.0).height(40.0).on_click(1u8));
        let rt = Runtime::default();
        let ui = frame(&tree, &rt);
        let view = &ui.platform_views()[0];
        assert_eq!(
            ui.platform_view_covers(view),
            [Rect::new(0.0, 0.0, 40.0, 40.0)]
        );
        // A parent that answers presses, drawn first, covers nothing.
        let under = Container::new().on_click(2u8).child(native());
        let ui = frame(&under, &rt);
        assert!(ui.platform_view_covers(&ui.platform_views()[0]).is_empty());
        // A dialog: its scrim covers the whole window.
        let dialog = crate::AlertDialog::new(true)
            .title("Over it")
            .on_dismiss(3u8)
            .body(native());
        let ui = frame(&dialog, &rt);
        let view = &ui.platform_views()[0];
        assert!(ui
            .platform_view_covers(view)
            .iter()
            .any(|cover| cover.contains(Point::new(0.0, 0.0))
                && cover.contains(Point::new(99.0, 99.0))));
        // A menu below an anchor: its box, not the barrier around it.
        let menu = Flex::column()
            .child(
                crate::OverlayPortal::new(Container::new().width(10.0).height(10.0))
                    .overlay(
                        Container::new().width(30.0).height(30.0),
                        crate::Placement::Below,
                    )
                    .dismiss(4u8),
            )
            .child(native());
        let ui = frame(&menu, &rt);
        let view = &ui.platform_views()[0];
        let covers = ui.platform_view_covers(view);
        assert_eq!(covers.len(), 1, "{covers:?}");
        assert!(covers[0].width <= 30.0 && covers[0].height <= 30.0);
    }

    #[test]
    fn the_stand_in_is_a_debug_builds_note() {
        let theme = Theme::default();
        let mut scene = Scene::new();
        let stand_in = UnsupportedView::new("Map");
        Widget::<()>::paint(
            &stand_in,
            Rect::new(0.0, 0.0, 200.0, 100.0),
            Status::default(),
            &theme,
            &mut scene,
        );
        assert_eq!(scene.is_empty(), !cfg!(debug_assertions));
        let mut scene = Scene::new();
        Widget::<()>::paint(
            &UnsupportedView::new("Map").shown_in_release(true),
            Rect::new(0.0, 0.0, 200.0, 100.0),
            Status::default(),
            &theme,
            &mut scene,
        );
        assert!(!scene.is_empty());
    }
}
