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
    /// Where the frame drew it.
    at: PixelRect,
    /// Where the native side was last told it is: behind `at` while it moves (milestone 796).
    placed: PixelRect,
    controller: Option<PlatformViewController>,
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
                    // **Moved once it stops** (milestone 796). frus draws the view's image
                    // where the frame has it, so its native place only matters to the screen
                    // reader and the keyboard's popups. Moved on every frame of a scroll, the
                    // native view was drawn again every frame — Android redraws a view that
                    // moves in its window — and its image copied and uploaded again: on the
                    // test phone the page's GPU time went from a few milliseconds to forty.
                    // A new size is placed at once: the image itself changes.
                    let resized = (at.w, at.h) != (live.placed.w, live.placed.h);
                    let settled = at == live.at && at != live.placed;
                    live.at = at;
                    if resized || settled {
                        live.placed = at;
                        commands.push(Command::Place {
                            view: live.view,
                            at,
                        });
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
                    self.live.insert(
                        want.id,
                        Live {
                            view,
                            view_type: want.view_type.to_owned(),
                            at,
                            placed: at,
                            controller: want.controller.cloned(),
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
                    controller.attach(None);
                }
                commands.push(Command::Dispose { view: live.view });
            }
        }
        commands
    }

    /// Whether a view moved and has not been placed there yet: one more frame places it once
    /// it has stopped (milestone 796).
    pub(crate) fn placing(&self) -> bool {
        self.live.values().any(|live| live.at != live.placed)
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
}

struct Gesture {
    view: i32,
    /// The view's corner, in the window's pixels, when the gesture began.
    origin: (f32, f32),
    /// The pointers down, in the order they came: winit's id, the id Android knows it by
    /// (the lowest free one when it came, kept until it lifts) and the window position.
    pointers: Vec<(u64, i32, f32, f32)>,
}

/// Decides, touch by touch, whether frus or a native view gets it.
#[derive(Default)]
pub(crate) struct TouchRouter {
    /// The pointers of a gesture frus has.
    frus: HashSet<u64>,
    gesture: Option<Gesture>,
}

impl TouchRouter {
    /// Routes touch `id` at `at` (the window's physical pixels). `hit` is asked only for a
    /// press that would start a gesture: the native view under it, if no frus target drawn
    /// over the view takes it, and where that view is.
    pub(crate) fn route(
        &mut self,
        id: u64,
        phase: Phase,
        at: (f32, f32),
        hit: impl FnOnce() -> Option<(i32, PixelRect)>,
    ) -> Routed {
        match phase {
            Phase::Started => {
                if let Some(gesture) = self.gesture.as_mut() {
                    // Another finger on a gesture a view has: the view's too.
                    let free = (0..)
                        .find(|n| !gesture.pointers.iter().any(|p| p.1 == *n))
                        .unwrap_or(0);
                    gesture.pointers.push((id, free, at.0, at.1));
                    let index = gesture.pointers.len() - 1;
                    return Routed::View(Self::touch(gesture, action::POINTER_DOWN, index));
                }
                if self.frus.is_empty() {
                    if let Some((view, rect)) = hit() {
                        let gesture = Gesture {
                            view,
                            origin: (rect.x as f32, rect.y as f32),
                            pointers: vec![(id, 0, at.0, at.1)],
                        };
                        let touch = Self::touch(&gesture, action::DOWN, 0);
                        self.gesture = Some(gesture);
                        return Routed::View(touch);
                    }
                }
                self.frus.insert(id);
                Routed::Frus
            }
            Phase::Moved => match self.gesture.as_mut() {
                Some(gesture) if gesture.pointers.iter().any(|p| p.0 == id) => {
                    for pointer in gesture.pointers.iter_mut().filter(|p| p.0 == id) {
                        pointer.2 = at.0;
                        pointer.3 = at.1;
                    }
                    Routed::View(Self::touch(gesture, action::MOVE, 0))
                }
                _ => Routed::Frus,
            },
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
        // Moved: placed on the first frame it stays there (milestone 796).
        assert!(views
            .sync(&[want(1, rect.translate(0.0, -15.5))], 2.0)
            .is_empty());
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
        views.sync(&[want(1, Rect::new(0.0, 11.2, 363.3, 259.6))], 2.75);
        let moved = views.sync(&[want(1, Rect::new(0.0, 11.2, 363.3, 259.6))], 2.75);
        assert!(
            matches!(&moved[..], [Command::Place { at, .. }] if (at.w, at.h) == first),
            "a move, at the same size"
        );
    }

    /// **A view that moves is placed once it stops** (milestone 796): a scroll moves it every
    /// frame, and each move made Android draw it again and frus copy its image again. While
    /// it moves nothing is sent, the views say one is still to be placed, and the first frame
    /// it stays put places it; a new size is sent at once.
    #[test]
    fn a_moving_view_is_placed_once_it_stops() {
        let mut views = PlatformViews::default();
        views.sync(&[want(1, Rect::new(0.0, 100.0, 200.0, 100.0))], 2.0);
        for y in [90.0, 80.0, 70.0, 60.0] {
            let commands = views.sync(&[want(1, Rect::new(0.0, y, 200.0, 100.0))], 2.0);
            assert!(commands.is_empty(), "nothing while it moves: {commands:?}");
            assert!(views.placing());
        }
        let settled = views.sync(&[want(1, Rect::new(0.0, 60.0, 200.0, 100.0))], 2.0);
        assert!(
            matches!(&settled[..], [Command::Place { at, .. }] if at.y == 120),
            "{settled:?}"
        );
        assert!(!views.placing());
        assert!(views
            .sync(&[want(1, Rect::new(0.0, 60.0, 200.0, 100.0))], 2.0)
            .is_empty());
        let resized = views.sync(&[want(1, Rect::new(0.0, 60.0, 200.0, 150.0))], 2.0);
        assert!(matches!(&resized[..], [Command::Place { at, .. }] if at.h == 300));
        // The view's own rectangle follows the frame all along: touches are mapped through it.
        views.sync(&[want(1, Rect::new(0.0, 30.0, 200.0, 150.0))], 2.0);
        assert_eq!(views.rect_of(1).map(|r| r.y), Some(60));
    }

    const VIEW: PixelRect = PixelRect {
        x: 100,
        y: 200,
        w: 300,
        h: 300,
    };

    fn on_view() -> Option<(i32, PixelRect)> {
        Some((9, VIEW))
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
