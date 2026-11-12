// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Native views**, the shell's half (milestone 794), as plain data: built and tested on
//! every host, carried out on Android by `android_platform_view.rs`.
//!
//! - [`PlatformViews::sync`] turns the frame's [placements](frus_widgets::PlatformViewPlacement)
//!   into what the Java side must do: make a view the first time its widget is laid out, move
//!   or resize it on the frames its box changes (and only then), drop it when the widget
//!   leaves, and send the calls the application queued on its controller.
//! - [`TouchRouter`] decides whose each touch is. A press that lands on a native view, with
//!   nothing frus drew over it there, makes the whole gesture the view's, from the press to the
//!   last lift; a gesture that starts in frus stays in frus, wherever its fingers go.
//! - [`packed_straight`] turns the view's image, premultiplied as Android draws it, into the
//!   straight alpha frus's images hold.

use std::collections::{HashMap, HashSet};

use frus_widgets::Rect;
use frus_widgets::{PlatformViewController, WidgetId};

/// Why a native view factory could not be registered.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlatformViewError {
    /// This platform has no native views (every one but Android, for now).
    Unsupported,
}

impl std::fmt::Display for PlatformViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlatformViewError::Unsupported => {
                f.write_str("native views are not available on this platform")
            }
        }
    }
}

impl std::error::Error for PlatformViewError {}

/// Registers the factory that makes the native views of `view_type`, for
/// [`AndroidView::new`](frus_widgets::AndroidView::new): `class_name` names a Java class that
/// implements `dev.frus.platform.PlatformViewFactory` and has a public constructor taking no
/// arguments, loaded from `dex` — the plugin's own compiled Java, which the plugin carries in
/// its library — or from frus's own dex for `None`.
///
/// Call it before the first view of that type is laid out, from the plugin's set-up (it may be
/// called before the application starts). The factory is made when the first native view is,
/// and a class that cannot be loaded is said in the log; a view of a type without a factory
/// sends its widget a [`PlatformViewEvent::ERROR`](frus_widgets::PlatformViewEvent::ERROR)
/// event. Elsewhere than on Android it returns [`PlatformViewError::Unsupported`].
///
/// ```no_run
/// // In a plugin: its Java, compiled by its build script into a dex it carries.
/// static MAP_DEX: &[u8] = &[]; // include_bytes!(concat!(env!("OUT_DIR"), "/map.dex"))
///
/// match frus_shell::register_android_view_factory("com.example/map", "com.example.MapFactory", Some(MAP_DEX)) {
///     Ok(()) => {}
///     Err(frus_shell::PlatformViewError::Unsupported) => { /* a desktop or the web */ }
///     Err(other) => eprintln!("{other}"),
/// }
/// ```
pub fn register_android_view_factory(
    view_type: &str,
    class_name: &str,
    dex: Option<&'static [u8]>,
) -> Result<(), PlatformViewError> {
    #[cfg(android)]
    {
        crate::android_platform_view::register(view_type, class_name, dex);
        Ok(())
    }
    #[cfg(not(android))]
    {
        let _ = (view_type, class_name, dex);
        Err(PlatformViewError::Unsupported)
    }
}

/// A box in the window's physical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl PixelRect {
    /// `rect`, in logical pixels, in the window's physical ones. The size is rounded on its
    /// own, not as the difference of two rounded edges: a box scrolled by a fraction of a
    /// pixel keeps its size, so its view is moved and never resized (a resize draws it again).
    pub(crate) fn from_logical(rect: Rect, scale: f32) -> Self {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        Self {
            x: (rect.x * scale).round() as i32,
            y: (rect.y * scale).round() as i32,
            w: (rect.width * scale).round().max(0.0) as i32,
            h: (rect.height * scale).round().max(0.0) as i32,
        }
    }

    /// `rect`, in physical pixels already, with each edge rounded on its own: two boxes that
    /// touch still touch.
    #[cfg_attr(android, allow(dead_code))]
    pub(crate) fn edges(rect: Rect) -> Self {
        let (left, top) = (rect.x.round() as i32, rect.y.round() as i32);
        let right = (rect.x + rect.width).round() as i32;
        let bottom = (rect.y + rect.height).round() as i32;
        Self {
            x: left,
            y: top,
            w: (right - left).max(0),
            h: (bottom - top).max(0),
        }
    }

    #[cfg_attr(android, allow(dead_code))]
    /// Whether it holds no pixel.
    pub(crate) fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// The pixels both hold.
    #[cfg_attr(android, allow(dead_code))]
    pub(crate) fn intersect(self, other: PixelRect) -> PixelRect {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.w).min(other.x + other.w);
        let bottom = (self.y + self.h).min(other.y + other.h);
        PixelRect {
            x: left,
            y: top,
            w: (right - left).max(0),
            h: (bottom - top).max(0),
        }
    }

    /// What is left of it once `cut` is taken out: up to four boxes, none overlapping — the
    /// band above the cut, the band below, and the two sides between.
    #[cfg_attr(android, allow(dead_code))]
    pub(crate) fn minus(self, cut: PixelRect) -> Vec<PixelRect> {
        let common = self.intersect(cut);
        if common.is_empty() {
            return vec![self];
        }
        let (right, bottom) = (self.x + self.w, self.y + self.h);
        let pieces = [
            PixelRect {
                x: self.x,
                y: self.y,
                w: self.w,
                h: common.y - self.y,
            },
            PixelRect {
                x: self.x,
                y: common.y + common.h,
                w: self.w,
                h: bottom - (common.y + common.h),
            },
            PixelRect {
                x: self.x,
                y: common.y,
                w: common.x - self.x,
                h: common.h,
            },
            PixelRect {
                x: common.x + common.w,
                y: common.y,
                w: right - (common.x + common.w),
                h: common.h,
            },
        ];
        pieces.into_iter().filter(|p| !p.is_empty()).collect()
    }
}

/// How much of a native view hosted **over** frus's surface shows (milestone 802): a Windows
/// child window or a page element is drawn above every pixel frus draws, so the parts frus
/// draws over it — a menu, a button laid on it, a dialog's scrim — are cut out of it, and the
/// parts a scrolling area clips away too.
#[cfg_attr(android, allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    /// All of it shows.
    Whole,
    /// These parts of it show, in its own pixels (from its top-left corner), none overlapping.
    Parts(Vec<PixelRect>),
    /// None of it: it is scrolled or clipped out of sight.
    Hidden,
    /// None of it: frus draws over all of it — a dialog's scrim. A host that can take a
    /// picture of the view hands it to frus to draw under the dialog.
    Covered,
}

impl Shape {
    /// The shape of the view whose box is `rect`, of which `visible` is inside every clip
    /// around it and `covers` are what frus drew over it: all in the window's physical pixels.
    #[cfg_attr(android, allow(dead_code))]
    pub(crate) fn of(rect: Rect, visible: Rect, covers: &[Rect]) -> Self {
        // Where the view is placed, rounded as it is placed; and the part of it in sight. A
        // view in sight whole is its whole box, whatever the rounding of the clip's edges:
        // a page scrolling by fractions of a pixel must not reshape it on every frame.
        let at = PixelRect::from_logical(rect, 1.0);
        let whole = visible.x <= rect.x + 0.5
            && visible.y <= rect.y + 0.5
            && visible.x + visible.width >= rect.x + rect.width - 0.5
            && visible.y + visible.height >= rect.y + rect.height - 0.5;
        let seen = if whole {
            at
        } else {
            PixelRect::edges(visible).intersect(at)
        };
        if seen.is_empty() || at.is_empty() {
            return Shape::Hidden;
        }
        let mut parts = vec![seen];
        for cover in covers {
            let cover = PixelRect::edges(*cover);
            if cover.is_empty() {
                continue;
            }
            parts = parts
                .into_iter()
                .flat_map(|part| part.minus(cover))
                .collect();
            if parts.is_empty() {
                return Shape::Covered;
            }
        }
        if parts == [at] {
            return Shape::Whole;
        }
        Shape::Parts(
            parts
                .into_iter()
                .map(|part| PixelRect {
                    x: part.x - at.x,
                    y: part.y - at.y,
                    ..part
                })
                .collect(),
        )
    }

    /// Whether any of the view shows.
    #[cfg_attr(android, allow(dead_code))]
    pub(crate) fn shows(&self) -> bool {
        matches!(self, Shape::Whole | Shape::Parts(_))
    }
}

/// One native view the frame wants.
#[derive(Clone, Debug)]
pub(crate) struct Wanted<'a> {
    pub id: WidgetId,
    pub view_type: &'a str,
    pub params: &'a str,
    /// The widget's whole box, in logical pixels.
    pub rect: Rect,
    pub controller: Option<&'a PlatformViewController>,
    /// How much of it shows, for a view hosted over frus's surface; `None` for one frus draws
    /// itself (Android's).
    #[cfg(not(android))]
    pub shape: Option<Shape>,
}

/// What the Java side must do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// Make view `view` of `view_type`, at `at`.
    Create {
        view: i32,
        view_type: String,
        params: String,
        at: PixelRect,
    },
    /// Move or resize view `view` to `at`.
    Place { view: i32, at: PixelRect },
    /// Drop view `view`.
    Dispose { view: i32 },
    /// Show only `shape` of view `view` (a view hosted over frus's surface).
    #[cfg(not(android))]
    Clip { view: i32, shape: Shape },
    /// Call `method` on view `view`.
    Invoke {
        view: i32,
        method: String,
        arguments: String,
    },
}

struct Live {
    view: i32,
    view_type: String,
    at: PixelRect,
    controller: Option<PlatformViewController>,
    #[cfg(not(android))]
    shape: Option<Shape>,
}

/// The native views that exist, by widget.
#[derive(Default)]
pub(crate) struct PlatformViews {
    live: HashMap<WidgetId, Live>,
    next: i32,
}

impl PlatformViews {
    /// What it takes to go from the views that exist to the ones `wanted` this frame, at
    /// `scale` physical pixels per logical one. Nothing, on a frame where nothing moved.
    pub(crate) fn sync(&mut self, wanted: &[Wanted<'_>], scale: f32) -> Vec<Command> {
        let mut commands = Vec::new();
        let mut seen: HashSet<WidgetId> = HashSet::new();
        for want in wanted {
            // A widget painted twice in a frame (a hero's flight, say) is one view, where it
            // was first painted.
            if !seen.insert(want.id) {
                continue;
            }
            let at = PixelRect::from_logical(want.rect, scale);
            match self.live.get_mut(&want.id) {
                Some(live) if live.view_type == want.view_type => {
                    if live.at != at {
                        live.at = at;
                        commands.push(Command::Place {
                            view: live.view,
                            at,
                        });
                    }
                    // Only a view hosted over the surface has a shape: none on Android, which
                    // pays nothing for it.
                    #[cfg(not(android))]
                    if want.shape.is_some() && live.shape != want.shape {
                        live.shape = want.shape.clone();
                        if let Some(shape) = &want.shape {
                            commands.push(Command::Clip {
                                view: live.view,
                                shape: shape.clone(),
                            });
                        }
                    }
                    if live.controller.as_ref() != want.controller {
                        if let Some(old) = live.controller.take() {
                            old.attach(None);
                        }
                        live.controller = want.controller.cloned();
                    }
                }
                existing => {
                    // A different view type in the same place is a different view.
                    if let Some(old) = existing {
                        commands.push(Command::Dispose { view: old.view });
                        if let Some(controller) = old.controller.take() {
                            controller.attach(None);
                        }
                    }
                    self.next = self.next.checked_add(1).unwrap_or(1);
                    let view = self.next;
                    commands.push(Command::Create {
                        view,
                        view_type: want.view_type.to_owned(),
                        params: want.params.to_owned(),
                        at,
                    });
                    #[cfg(not(android))]
                    if let Some(shape) = &want.shape {
                        commands.push(Command::Clip {
                            view,
                            shape: shape.clone(),
                        });
                    }
                    self.live.insert(
                        want.id,
                        Live {
                            view,
                            view_type: want.view_type.to_owned(),
                            at,
                            controller: want.controller.cloned(),
                            #[cfg(not(android))]
                            shape: want.shape.clone(),
                        },
                    );
                }
            }
            let live = &self.live[&want.id];
            if let Some(controller) = &live.controller {
                controller.attach(Some(live.view));
                for call in controller.take_calls() {
                    commands.push(Command::Invoke {
                        view: live.view,
                        method: call.method,
                        arguments: call.arguments,
                    });
                }
            }
        }
        let gone: Vec<WidgetId> = self
            .live
            .keys()
            .filter(|id| !seen.contains(id))
            .copied()
            .collect();
        for id in gone {
            if let Some(live) = self.live.remove(&id) {
                if let Some(controller) = live.controller {
                    // Unless another widget showing the same controller (a web page shown
                    // somewhere else) has it already: only web views' controllers are shown by
                    // two widgets (milestone 799).
                    if cfg!(not(feature = "webview")) || controller.view_id() == Some(live.view) {
                        controller.attach(None);
                    }
                }
                commands.push(Command::Dispose { view: live.view });
            }
        }
        commands
    }

    /// Whether no native view exists: then nothing of this is done at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// The native view of `widget`.
    pub(crate) fn view_of(&self, widget: WidgetId) -> Option<i32> {
        self.live.get(&widget).map(|live| live.view)
    }

    /// The widget of native view `view`.
    pub(crate) fn widget_of(&self, view: i32) -> Option<WidgetId> {
        self.live
            .iter()
            .find(|(_, live)| live.view == view)
            .map(|(id, _)| *id)
    }

    /// Where native view `view` is, in physical pixels.
    pub(crate) fn rect_of(&self, view: i32) -> Option<PixelRect> {
        self.live
            .values()
            .find(|live| live.view == view)
            .map(|live| live.at)
    }

    /// Every view that exists and its widget.
    pub(crate) fn views(&self) -> impl Iterator<Item = (WidgetId, i32)> + '_ {
        self.live.iter().map(|(id, live)| (*id, live.view))
    }
}

/// `MotionEvent`'s actions.
pub(crate) mod action {
    pub const DOWN: i32 = 0;
    pub const UP: i32 = 1;
    pub const MOVE: i32 = 2;
    pub const CANCEL: i32 = 3;
    pub const POINTER_DOWN: i32 = 5;
    pub const POINTER_UP: i32 = 6;
}

/// Where a touch is in its life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

/// One touch for a native view: what `MotionEvent.obtain` is made from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViewTouch {
    pub view: i32,
    /// [`action`]: `DOWN`, `MOVE`, `POINTER_DOWN`…
    pub action: i32,
    /// The pointer a `POINTER_DOWN` or `POINTER_UP` is about, as an index into `ids`.
    pub index: usize,
    /// Every pointer down, the one going up included.
    pub ids: Vec<i32>,
    /// Their positions, `x, y` each, in the view's own physical pixels.
    pub xy: Vec<f32>,
}

/// Whose a touch is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Routed {
    /// frus's: handled as any other touch.
    Frus,
    /// A native view's.
    View(ViewTouch),
    #[cfg(feature = "webview")]
    /// A drag the view had and frus takes from here (milestone 799): the view is sent
    /// `cancel`, and frus gets a press at `from` (the window's physical pixels), then this
    /// touch. The rest of the gesture is frus's.
    Handover { cancel: ViewTouch, from: (f32, f32) },
}

/// The ways frus can scroll under a press: what a drag on a native view may be handed to.
#[cfg(feature = "webview")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Axes {
    pub vertical: bool,
    pub horizontal: bool,
}

/// A press on a native view: which, where it is, and what frus can scroll around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ViewHit {
    pub view: i32,
    pub rect: PixelRect,
    /// What frus can scroll around it (milestone 799).
    #[cfg(feature = "webview")]
    pub frus: Axes,
}

/// The event a web view sends with the directions its content can still scroll ([`scroll`]).
#[cfg(feature = "webview")]
pub(crate) const SCROLL_EVENT: &str = "frus.scroll";

/// The directions a view's content can still scroll, as a web view says them.
#[cfg(feature = "webview")]
pub(crate) mod scroll {
    pub const UP: u8 = 1;
    pub const DOWN: u8 = 2;
    pub const LEFT: u8 = 4;
    pub const RIGHT: u8 = 8;
}

struct Gesture {
    view: i32,
    /// The view's corner, in the window's pixels, when the gesture began.
    origin: (f32, f32),
    /// The pointers down, in the order they came: winit's id, the id Android knows it by
    /// (the lowest free one when it came, kept until it lifts) and the window position.
    pointers: Vec<(u64, i32, f32, f32)>,
    /// Where the first finger went down.
    #[cfg(feature = "webview")]
    start: (f32, f32),
    /// What frus can scroll around the view.
    #[cfg(feature = "webview")]
    frus: Axes,
    /// The drag's axis once it moved past the slop (`Some(true)`: vertical); `None` before,
    /// and for a gesture that stays the view's whatever it does (a pinch).
    #[cfg(feature = "webview")]
    axis: Option<bool>,
    /// Whether the slop was passed, or a second finger came.
    #[cfg(feature = "webview")]
    decided: bool,
}

/// Decides, touch by touch, whether frus or a native view gets it.
#[cfg_attr(not(feature = "webview"), derive(Default))]
pub(crate) struct TouchRouter {
    /// The pointers of a gesture frus has.
    frus: HashSet<u64>,
    gesture: Option<Gesture>,
    /// How far a finger moves, in physical pixels, before it is a drag.
    #[cfg(feature = "webview")]
    pub slop: f32,
    /// Which ways each view's content can still scroll ([`scroll`]).
    #[cfg(feature = "webview")]
    scrolls: HashMap<i32, u8>,
}

#[cfg(feature = "webview")]
impl Default for TouchRouter {
    fn default() -> Self {
        Self {
            frus: HashSet::new(),
            gesture: None,
            #[cfg(feature = "webview")]
            slop: 18.0,
            #[cfg(feature = "webview")]
            scrolls: HashMap::new(),
        }
    }
}

impl TouchRouter {
    /// Which ways view `view`'s content can still scroll now ([`scroll`]).
    #[cfg(feature = "webview")]
    pub(crate) fn set_scroll(&mut self, view: i32, flags: u8) {
        self.scrolls.insert(view, flags);
    }

    #[cfg(feature = "webview")]
    /// Whether a drag of `delta` along an axis is frus's: frus scrolls that way around the
    /// view, and the view's content cannot move that way any further.
    fn frus_takes(&self, gesture: &Gesture, vertical: bool, delta: f32) -> bool {
        if delta == 0.0 {
            return false;
        }
        let frus = if vertical {
            gesture.frus.vertical
        } else {
            gesture.frus.horizontal
        };
        // A finger moving down shows what is above: the content must be able to scroll up.
        let needs = match (vertical, delta > 0.0) {
            (true, true) => scroll::UP,
            (true, false) => scroll::DOWN,
            (false, true) => scroll::LEFT,
            (false, false) => scroll::RIGHT,
        };
        // A view that has not drawn yet has said nothing: it cannot scroll.
        let flags = self.scrolls.get(&gesture.view).copied().unwrap_or(0);
        frus && flags & needs == 0
    }

    /// Routes touch `id` at `at` (the window's physical pixels). `hit` is asked only for a
    /// press that would start a gesture: the native view under it, if no frus target drawn
    /// over the view takes it.
    ///
    /// A one-finger drag on a view inside a frus scrolling area is the view's while its
    /// content can scroll that way. Past the slop in a direction the content cannot go, or
    /// once the content reaches its edge, the drag is handed to frus ([`Routed::Handover`]).
    pub(crate) fn route(
        &mut self,
        id: u64,
        phase: Phase,
        at: (f32, f32),
        hit: impl FnOnce() -> Option<ViewHit>,
    ) -> Routed {
        match phase {
            Phase::Started => {
                if let Some(gesture) = self.gesture.as_mut() {
                    // Another finger on a gesture a view has: the view's too, for good (a
                    // pinch is never handed over).
                    let free = (0..)
                        .find(|n| !gesture.pointers.iter().any(|p| p.1 == *n))
                        .unwrap_or(0);
                    gesture.pointers.push((id, free, at.0, at.1));
                    #[cfg(feature = "webview")]
                    {
                        gesture.decided = true;
                        gesture.axis = None;
                    }
                    let index = gesture.pointers.len() - 1;
                    return Routed::View(Self::touch(gesture, action::POINTER_DOWN, index));
                }
                if self.frus.is_empty() {
                    if let Some(hit) = hit() {
                        let gesture = Gesture {
                            view: hit.view,
                            origin: (hit.rect.x as f32, hit.rect.y as f32),
                            pointers: vec![(id, 0, at.0, at.1)],
                            #[cfg(feature = "webview")]
                            start: at,
                            #[cfg(feature = "webview")]
                            frus: hit.frus,
                            #[cfg(feature = "webview")]
                            axis: None,
                            #[cfg(feature = "webview")]
                            decided: false,
                        };
                        let touch = Self::touch(&gesture, action::DOWN, 0);
                        self.gesture = Some(gesture);
                        return Routed::View(touch);
                    }
                }
                self.frus.insert(id);
                Routed::Frus
            }
            Phase::Moved => {
                // A drag the view cannot follow goes to frus (milestone 799).
                #[cfg(feature = "webview")]
                if let Some(routed) = self.hand_over(id, at) {
                    return routed;
                }
                match self.gesture.as_mut() {
                    Some(gesture) if gesture.pointers.iter().any(|p| p.0 == id) => {
                        for pointer in gesture.pointers.iter_mut().filter(|p| p.0 == id) {
                            pointer.2 = at.0;
                            pointer.3 = at.1;
                        }
                        Routed::View(Self::touch(gesture, action::MOVE, 0))
                    }
                    _ => Routed::Frus,
                }
            }
            Phase::Ended | Phase::Cancelled => {
                let Some(gesture) = self.gesture.as_mut() else {
                    self.frus.remove(&id);
                    return Routed::Frus;
                };
                let Some(index) = gesture.pointers.iter().position(|p| p.0 == id) else {
                    self.frus.remove(&id);
                    return Routed::Frus;
                };
                gesture.pointers[index].2 = at.0;
                gesture.pointers[index].3 = at.1;
                if phase == Phase::Cancelled {
                    let touch = Self::touch(gesture, action::CANCEL, 0);
                    self.gesture = None;
                    return Routed::View(touch);
                }
                if gesture.pointers.len() == 1 {
                    let touch = Self::touch(gesture, action::UP, 0);
                    self.gesture = None;
                    return Routed::View(touch);
                }
                let touch = Self::touch(gesture, action::POINTER_UP, index);
                gesture.pointers.remove(index);
                Routed::View(touch)
            }
        }
    }

    /// Whether this movement of pointer `id` hands the view's drag to frus: past the slop in a
    /// direction the view's content cannot go, or on, once it reached its edge. Records the
    /// drag's axis on the way. `None`: the view keeps it.
    #[cfg(feature = "webview")]
    fn hand_over(&mut self, id: u64, at: (f32, f32)) -> Option<Routed> {
        let gesture = self.gesture.as_ref()?;
        let index = gesture.pointers.iter().position(|p| p.0 == id)?;
        if gesture.pointers.len() != 1 {
            return None;
        }
        let previous = (gesture.pointers[index].2, gesture.pointers[index].3);
        let mut decided = gesture.decided;
        let mut axis = gesture.axis;
        let mut from = None;
        if !decided {
            let total = (at.0 - gesture.start.0, at.1 - gesture.start.1);
            if total.0.abs().max(total.1.abs()) >= self.slop {
                decided = true;
                let vertical = total.1.abs() >= total.0.abs();
                axis = Some(vertical);
                let along = if vertical { total.1 } else { total.0 };
                if self.frus_takes(gesture, vertical, along) {
                    from = Some(gesture.start);
                }
            }
        } else if let Some(vertical) = axis {
            let step = if vertical {
                at.1 - previous.1
            } else {
                at.0 - previous.0
            };
            if self.frus_takes(gesture, vertical, step) {
                from = Some(previous);
            }
        }
        let gesture = self.gesture.as_mut()?;
        gesture.decided = decided;
        gesture.axis = axis;
        let from = from?;
        let cancel = Self::touch(gesture, action::CANCEL, 0);
        self.gesture = None;
        self.frus.insert(id);
        Some(Routed::Handover { cancel, from })
    }

    /// The view a gesture is under way on, if any.
    #[cfg(test)]
    pub(crate) fn active_view(&self) -> Option<i32> {
        self.gesture.as_ref().map(|g| g.view)
    }

    /// The view went mid-gesture: the rest of the gesture is nobody's.
    pub(crate) fn forget_view(&mut self, view: i32) {
        if self.gesture.as_ref().is_some_and(|g| g.view == view) {
            self.gesture = None;
        }
        #[cfg(feature = "webview")]
        self.scrolls.remove(&view);
    }

    fn touch(gesture: &Gesture, action: i32, index: usize) -> ViewTouch {
        ViewTouch {
            view: gesture.view,
            action,
            index,
            ids: gesture.pointers.iter().map(|p| p.1).collect(),
            xy: gesture
                .pointers
                .iter()
                .flat_map(|p| [p.2 - gesture.origin.0, p.3 - gesture.origin.1])
                .collect(),
        }
    }
}

/// `rgba`, premultiplied as Android draws, turned into straight alpha in place — what frus's
/// images hold. Opaque pixels, nearly all of a page, are left as they are.
pub(crate) fn packed_straight(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let a = pixel[3];
        if a == 255 {
            continue;
        }
        if a == 0 {
            pixel[0] = 0;
            pixel[1] = 0;
            pixel[2] = 0;
            continue;
        }
        let a = u32::from(a);
        for channel in &mut pixel[..3] {
            *channel = ((u32::from(*channel) * 255 + a / 2) / a).min(255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(android))]
    #[test]
    fn elsewhere_registering_a_factory_says_it_is_unsupported() {
        assert_eq!(
            register_android_view_factory("com.example/map", "com.example.MapFactory", None),
            Err(PlatformViewError::Unsupported)
        );
    }

    fn want(id: u64, rect: Rect) -> Wanted<'static> {
        Wanted {
            id: WidgetId::from_u64(id),
            view_type: "frus/webview",
            params: "<p>hi</p>",
            rect,
            controller: None,
            shape: None,
        }
    }

    #[test]
    fn a_view_is_made_once_moved_when_it_moves_and_dropped_when_it_leaves() {
        let mut views = PlatformViews::default();
        let rect = Rect::new(10.0, 20.0, 100.0, 50.0);
        let first = views.sync(&[want(1, rect)], 2.0);
        assert_eq!(
            first,
            [Command::Create {
                view: 1,
                view_type: "frus/webview".into(),
                params: "<p>hi</p>".into(),
                at: PixelRect {
                    x: 20,
                    y: 40,
                    w: 200,
                    h: 100
                },
            }]
        );
        assert!(
            views.sync(&[want(1, rect)], 2.0).is_empty(),
            "a frame where nothing moved sends nothing"
        );
        let moved = views.sync(&[want(1, rect.translate(0.0, -15.5))], 2.0);
        assert_eq!(
            moved,
            [Command::Place {
                view: 1,
                at: PixelRect {
                    x: 20,
                    y: 9,
                    w: 200,
                    h: 100
                }
            }]
        );
        assert_eq!(views.widget_of(1), Some(WidgetId::from_u64(1)));
        assert_eq!(views.view_of(WidgetId::from_u64(1)), Some(1));
        assert_eq!(views.sync(&[], 2.0), [Command::Dispose { view: 1 }]);
        assert!(views.is_empty());
        assert!(views.sync(&[], 2.0).is_empty());
    }

    #[test]
    fn a_widget_painted_twice_is_one_view_and_a_new_type_is_a_new_view() {
        let mut views = PlatformViews::default();
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let made = views.sync(&[want(1, rect), want(1, rect.translate(5.0, 5.0))], 1.0);
        assert_eq!(made.len(), 1);
        let mut other = want(1, rect);
        other.view_type = "frus/edittext";
        let changed = views.sync(&[other], 1.0);
        assert!(matches!(changed[0], Command::Dispose { view: 1 }));
        assert!(matches!(changed[1], Command::Create { view: 2, .. }));
    }

    #[test]
    fn queued_calls_follow_the_creation_and_the_controller_is_bound() {
        let controller = PlatformViewController::new();
        controller.invoke("loadUrl", "https://example.com");
        let mut views = PlatformViews::default();
        let mut wanted = want(4, Rect::new(0.0, 0.0, 10.0, 10.0));
        wanted.controller = Some(&controller);
        let commands = views.sync(std::slice::from_ref(&wanted), 1.0);
        assert!(matches!(commands[0], Command::Create { view: 1, .. }));
        assert_eq!(
            commands[1],
            Command::Invoke {
                view: 1,
                method: "loadUrl".into(),
                arguments: "https://example.com".into()
            }
        );
        assert_eq!(controller.view_id(), Some(1));
        controller.invoke("evaluateJavascript", "1+1");
        let later = views.sync(std::slice::from_ref(&wanted), 1.0);
        assert!(matches!(&later[..], [Command::Invoke { view: 1, .. }]));
        views.sync(&[], 1.0);
        assert!(!controller.is_created(), "the view went with its widget");
    }

    #[test]
    fn a_box_scrolled_by_a_fraction_keeps_its_size() {
        // On the phone a page scrolled by fractions of a pixel gave 714 and 715 rows in turn,
        // and every change was a resize, which draws the view again.
        let size = |y: f32| {
            let at = PixelRect::from_logical(Rect::new(12.0, y, 363.3, 259.6), 2.75);
            (at.w, at.h)
        };
        let first = size(0.0);
        for step in 0..200 {
            assert_eq!(size(step as f32 * 0.37), first);
        }
        let mut views = PlatformViews::default();
        views.sync(&[want(1, Rect::new(0.0, 10.0, 363.3, 259.6))], 2.75);
        let moved = views.sync(&[want(1, Rect::new(0.0, 11.2, 363.3, 259.6))], 2.75);
        assert!(
            matches!(&moved[..], [Command::Place { at, .. }] if (at.w, at.h) == first),
            "a move, at the same size"
        );
    }

    const VIEW: PixelRect = PixelRect {
        x: 100,
        y: 200,
        w: 300,
        h: 300,
    };

    fn on_view() -> Option<ViewHit> {
        Some(ViewHit {
            view: 9,
            rect: VIEW,
            #[cfg(feature = "webview")]
            frus: Axes::default(),
        })
    }

    /// A view inside a frus list that scrolls vertically.
    #[cfg(feature = "webview")]
    fn in_list() -> Option<ViewHit> {
        Some(ViewHit {
            view: 9,
            rect: VIEW,
            frus: Axes {
                vertical: true,
                horizontal: false,
            },
        })
    }

    #[cfg(feature = "webview")]
    #[test]
    fn a_drag_the_page_cannot_follow_is_handed_to_the_list() {
        let mut router = TouchRouter::default();
        // The page is at its top: it can only scroll down.
        router.set_scroll(9, scroll::DOWN);
        assert!(matches!(
            router.route(1, Phase::Started, (150.0, 250.0), in_list),
            Routed::View(_)
        ));
        // Within the slop: still the view's.
        assert!(matches!(
            router.route(1, Phase::Moved, (150.0, 260.0), || None),
            Routed::View(_)
        ));
        // Past it, pulling down (showing what is above): the page cannot, the list can.
        let Routed::Handover { cancel, from } =
            router.route(1, Phase::Moved, (152.0, 300.0), || None)
        else {
            panic!("the list takes the drag");
        };
        assert_eq!((cancel.view, cancel.action), (9, action::CANCEL));
        assert_eq!(
            from,
            (150.0, 250.0),
            "frus is pressed where the finger went down"
        );
        // From then on everything is frus's, and the view is out of it.
        assert_eq!(
            router.route(1, Phase::Moved, (152.0, 340.0), || None),
            Routed::Frus
        );
        assert_eq!(
            router.route(1, Phase::Ended, (152.0, 340.0), || None),
            Routed::Frus
        );
        assert_eq!(router.active_view(), None);
    }

    #[cfg(feature = "webview")]
    #[test]
    fn a_drag_the_page_can_follow_stays_its_until_its_edge() {
        let mut router = TouchRouter::default();
        router.set_scroll(9, scroll::UP | scroll::DOWN);
        router.route(1, Phase::Started, (150.0, 450.0), in_list);
        // Pushing up (scrolling the page down): the page can.
        assert!(matches!(
            router.route(1, Phase::Moved, (150.0, 400.0), || None),
            Routed::View(_)
        ));
        assert!(matches!(
            router.route(1, Phase::Moved, (150.0, 380.0), || None),
            Routed::View(_)
        ));
        // The page reached its bottom while the finger keeps pushing: the list goes on.
        router.set_scroll(9, scroll::UP);
        let Routed::Handover { from, .. } = router.route(1, Phase::Moved, (150.0, 360.0), || None)
        else {
            panic!("the list takes the rest of the drag");
        };
        assert_eq!(from, (150.0, 380.0), "from where the page stopped");
    }

    #[cfg(feature = "webview")]
    #[test]
    fn a_sideways_drag_a_pinch_or_a_view_outside_a_list_are_never_handed_over() {
        let mut router = TouchRouter::default();
        router.set_scroll(9, 0);
        // Sideways, in a list that only scrolls vertically.
        router.route(1, Phase::Started, (150.0, 250.0), in_list);
        assert!(matches!(
            router.route(1, Phase::Moved, (220.0, 255.0), || None),
            Routed::View(_)
        ));
        router.route(1, Phase::Ended, (220.0, 255.0), || None);
        // A pinch.
        router.route(1, Phase::Started, (150.0, 250.0), in_list);
        router.route(2, Phase::Started, (200.0, 300.0), || None);
        assert!(matches!(
            router.route(1, Phase::Moved, (150.0, 350.0), || None),
            Routed::View(_)
        ));
        router.route(1, Phase::Ended, (150.0, 350.0), || None);
        router.route(2, Phase::Ended, (200.0, 300.0), || None);
        // No list around the view.
        router.route(1, Phase::Started, (150.0, 250.0), on_view);
        assert!(matches!(
            router.route(1, Phase::Moved, (150.0, 350.0), || None),
            Routed::View(_)
        ));
    }

    #[test]
    fn a_press_on_a_view_makes_the_whole_gesture_its() {
        let mut router = TouchRouter::default();
        let Routed::View(down) = router.route(1, Phase::Started, (150.0, 250.0), on_view) else {
            panic!("the press lands on the view");
        };
        assert_eq!((down.view, down.action), (9, action::DOWN));
        assert_eq!(down.xy, [50.0, 50.0], "in the view's own pixels");
        // Dragged right out of the view: still the view's.
        let Routed::View(moved) = router.route(1, Phase::Moved, (900.0, 1200.0), || None) else {
            panic!("the drag stays the view's");
        };
        assert_eq!(moved.action, action::MOVE);
        assert_eq!(moved.xy, [800.0, 1000.0]);
        // A second finger, wherever it lands, joins it.
        let Routed::View(second) = router.route(2, Phase::Started, (10.0, 10.0), || None) else {
            panic!("a second finger joins the view's gesture");
        };
        assert_eq!((second.action, second.index), (action::POINTER_DOWN, 1));
        assert_eq!(second.ids, [0, 1]);
        let Routed::View(first_up) = router.route(1, Phase::Ended, (900.0, 1200.0), || None) else {
            panic!();
        };
        assert_eq!((first_up.action, first_up.index), (action::POINTER_UP, 0));
        let Routed::View(last_up) = router.route(2, Phase::Ended, (10.0, 10.0), || None) else {
            panic!();
        };
        assert_eq!(last_up.action, action::UP);
        assert_eq!(last_up.ids, [1], "a pointer keeps its id until it lifts");
        assert_eq!(router.active_view(), None);
        // The next press, off the view, is frus's.
        assert_eq!(
            router.route(3, Phase::Started, (0.0, 0.0), || None),
            Routed::Frus
        );
    }

    #[test]
    fn a_gesture_that_starts_in_frus_stays_in_frus() {
        let mut router = TouchRouter::default();
        assert_eq!(
            router.route(1, Phase::Started, (0.0, 0.0), || None),
            Routed::Frus
        );
        // Dragged over the view: still frus's.
        assert_eq!(
            router.route(1, Phase::Moved, (150.0, 250.0), on_view),
            Routed::Frus
        );
        // A second finger landing on the view while frus has a gesture: frus's too, and the
        // view is not even asked.
        let mut asked = false;
        let second = router.route(2, Phase::Started, (150.0, 250.0), || {
            asked = true;
            on_view()
        });
        assert_eq!(second, Routed::Frus);
        assert!(!asked);
        assert_eq!(
            router.route(1, Phase::Ended, (0.0, 0.0), || None),
            Routed::Frus
        );
        assert_eq!(
            router.route(2, Phase::Ended, (0.0, 0.0), || None),
            Routed::Frus
        );
        // Every finger up: the next press on the view is the view's.
        assert!(matches!(
            router.route(3, Phase::Started, (150.0, 250.0), on_view),
            Routed::View(_)
        ));
    }

    #[test]
    fn a_cancel_ends_the_views_gesture_and_a_dropped_view_forgets_it() {
        let mut router = TouchRouter::default();
        router.route(1, Phase::Started, (150.0, 250.0), on_view);
        let Routed::View(cancel) = router.route(1, Phase::Cancelled, (150.0, 250.0), || None)
        else {
            panic!();
        };
        assert_eq!(cancel.action, action::CANCEL);
        router.route(2, Phase::Started, (150.0, 250.0), on_view);
        router.forget_view(9);
        assert_eq!(
            router.route(2, Phase::Moved, (160.0, 250.0), || None),
            Routed::Frus
        );
    }

    #[test]
    fn a_view_in_sight_is_whole_and_what_covers_it_is_cut_out() {
        let rect = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(Shape::of(rect, rect, &[]), Shape::Whole);
        // A page scrolled by fractions of a pixel: whole on every frame, never reshaped.
        for step in 0..60 {
            let moved = rect.translate(0.3, step as f32 * 0.37);
            assert_eq!(Shape::of(moved, moved, &[]), Shape::Whole);
        }
        // Half under the top of the scrolling area.
        let area = Rect::new(0.0, 45.0, 400.0, 400.0);
        assert_eq!(
            Shape::of(rect, area.intersect(rect), &[]),
            Shape::Parts(vec![PixelRect {
                x: 0,
                y: 25,
                w: 100,
                h: 25
            }])
        );
        assert_eq!(
            Shape::of(rect, Rect::new(0.0, 0.0, 0.0, 0.0), &[]),
            Shape::Hidden
        );
        // A menu over its top-left corner: the band under the menu and the part beside it.
        assert_eq!(
            Shape::of(rect, rect, &[Rect::new(0.0, 0.0, 40.0, 40.0)]),
            Shape::Parts(vec![
                PixelRect {
                    x: 0,
                    y: 20,
                    w: 100,
                    h: 30
                },
                PixelRect {
                    x: 30,
                    y: 0,
                    w: 70,
                    h: 20
                },
            ])
        );
        // A dialog's scrim: none of it shows, and a picture can stand in.
        let scrim = Rect::new(0.0, 0.0, 1000.0, 1000.0);
        assert_eq!(Shape::of(rect, rect, &[scrim]), Shape::Covered);
        assert!(!Shape::Covered.shows() && !Shape::Hidden.shows() && Shape::Whole.shows());
        // Something drawn beside it covers nothing of it.
        assert_eq!(
            Shape::of(rect, rect, &[Rect::new(200.0, 0.0, 50.0, 50.0)]),
            Shape::Whole
        );
    }

    #[test]
    fn what_is_left_after_a_cut_is_every_pixel_but_the_cut_once() {
        let whole = PixelRect {
            x: 0,
            y: 0,
            w: 12,
            h: 9,
        };
        for (x, y, w, h) in [
            (-3, -3, 5, 5),
            (4, 2, 3, 3),
            (10, 7, 9, 9),
            (0, 0, 12, 9),
            (5, -1, 2, 20),
        ] {
            let cut = PixelRect { x, y, w, h };
            let pieces = whole.minus(cut);
            for py in 0..whole.h {
                for px in 0..whole.w {
                    let inside =
                        |r: &PixelRect| px >= r.x && px < r.x + r.w && py >= r.y && py < r.y + r.h;
                    let count = pieces.iter().filter(|p| inside(p)).count();
                    assert_eq!(
                        count,
                        usize::from(!inside(&cut)),
                        "({px}, {py}) after {cut:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_hosted_view_is_reshaped_only_when_its_shape_changes() {
        let mut views = PlatformViews::default();
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let mut wanted = want(1, rect);
        wanted.shape = Some(Shape::Whole);
        let made = views.sync(std::slice::from_ref(&wanted), 1.0);
        assert!(matches!(made[0], Command::Create { view: 1, .. }));
        assert_eq!(
            made[1],
            Command::Clip {
                view: 1,
                shape: Shape::Whole
            }
        );
        assert!(views.sync(std::slice::from_ref(&wanted), 1.0).is_empty());
        wanted.shape = Some(Shape::Covered);
        assert_eq!(
            views.sync(std::slice::from_ref(&wanted), 1.0),
            [Command::Clip {
                view: 1,
                shape: Shape::Covered
            }]
        );
        // A view frus draws itself is never clipped.
        let mut drawn = PlatformViews::default();
        let plain = want(2, rect);
        let made = drawn.sync(std::slice::from_ref(&plain), 1.0);
        assert_eq!(made.len(), 1);
    }

    #[test]
    fn premultiplied_pixels_become_straight() {
        let mut rgba = vec![
            255, 0, 0, 255, // opaque red: untouched
            64, 32, 0, 128, // half-covered orange
            9, 9, 9, 0, // nothing at all
        ];
        packed_straight(&mut rgba);
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[128, 64, 0, 128]);
        assert_eq!(&rgba[8..12], &[0, 0, 0, 0]);
    }
}
