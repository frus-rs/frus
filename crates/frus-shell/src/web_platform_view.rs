// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The web host of **native views** (milestone 802): each view is an element of the page in a
//! `<div>` — its *container* — laid over the canvas where the widget is, as the element a text
//! field is edited through is (milestone 711), and moved with the frame.
//!
//! The page draws its elements **above** the canvas, whatever frus draws: the parts frus draws
//! over a view (a menu, a button laid on it) are cut out of its container with a `clip-path`,
//! and a view covered whole (a dialog's scrim) or out of sight is hidden. A page cannot take a
//! picture of a frame from another site, so nothing stands in for it under a dialog.
//!
//! The pointer and the keyboard go to the element by themselves: it is the page's. A view
//! taking the focus says so (`frus.focus`); a press on frus gives the focus back to the canvas.
//!
//! Three view types are built in: `frus/webview` (an `<iframe>`), `frus/edittext` (an
//! `<input>`) and `frus/video` (a `<video>`).

use std::collections::HashMap;
use std::sync::Arc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    Document, Event, HtmlCanvasElement, HtmlElement, HtmlIFrameElement, HtmlInputElement,
    HtmlVideoElement, KeyboardEvent,
};
use winit::platform::web::WindowExtWebSys;
use winit::window::Window;

use crate::platform_view::{PixelRect, Shape};
use crate::view_host::{self, NativeView, PlatformViewChannel, PlatformViewContext, ViewHost};
use frus_widgets::PlatformViewEvent;

/// The view types this host makes with no plugin.
pub(crate) fn register_builtins() {
    for view_type in ["frus/webview", "frus/edittext", "frus/video"] {
        frus_widgets::register_platform_view_type(view_type);
    }
}

type Listener = Closure<dyn FnMut(Event)>;

enum Content {
    Frame(HtmlIFrameElement),
    Input(HtmlInputElement),
    Video(HtmlVideoElement),
    Plugin(Box<dyn NativeView>),
    Nothing,
}

struct Hosted {
    container: HtmlElement,
    content: Content,
    at: PixelRect,
    shape: Shape,
    _listeners: Vec<Listener>,
}

/// The views of the page.
pub(crate) struct Host {
    document: Document,
    canvas: HtmlCanvasElement,
    views: HashMap<i32, Hosted>,
}

impl Host {
    /// The host of `window`'s canvas; `None` with no canvas or no document.
    pub(crate) fn new(window: &Arc<Window>) -> Option<Self> {
        let canvas = window.canvas()?;
        let document = canvas.owner_document()?;
        let wake = window.clone();
        view_host::set_wake(Arc::new(move || wake.request_redraw()));
        Some(Self {
            document,
            canvas,
            views: HashMap::new(),
        })
    }

    fn element<T: JsCast>(&self, tag: &str) -> Option<T> {
        self.document.create_element(tag).ok()?.dyn_into::<T>().ok()
    }

    /// The container's place on the page: over the canvas, `at` from its corner (CSS pixels).
    fn style(&self, at: PixelRect, shape: &Shape) -> String {
        let bounds = self.canvas.get_bounding_client_rect();
        let clip = match shape {
            Shape::Whole => String::from("none"),
            Shape::Parts(parts) => {
                let path: String = parts
                    .iter()
                    .map(|p| format!("M{} {}h{}v{}h{}Z", p.x, p.y, p.w, p.h, -p.w))
                    .collect();
                format!("path('{path}')")
            }
            Shape::Hidden | Shape::Covered => String::from("none"),
        };
        format!(
            "position:fixed;margin:0;padding:0;border:0;overflow:hidden;z-index:1;\
             left:{}px;top:{}px;width:{}px;height:{}px;clip-path:{clip};visibility:{};",
            bounds.left() + f64::from(at.x),
            bounds.top() + f64::from(at.y),
            at.w.max(0),
            at.h.max(0),
            if shape.shows() { "visible" } else { "hidden" },
        )
    }

    fn restyle(&self, view: i32) {
        if let Some(hosted) = self.views.get(&view) {
            let style = self.style(hosted.at, &hosted.shape);
            hosted.container.set_attribute("style", &style).ok();
        }
    }

    fn make(
        &self,
        view: i32,
        view_type: &str,
        params: &str,
        container: &HtmlElement,
        at: PixelRect,
    ) -> (Content, Vec<Listener>) {
        let mut listeners = Vec::new();
        if let Some(factory) = view_host::factory(view_type) {
            let context = PlatformViewContext {
                params,
                channel: PlatformViewChannel::new(view),
                width: at.w,
                height: at.h,
                container,
            };
            return match factory(&context) {
                Ok(made) => (Content::Plugin(made), listeners),
                Err(why) => {
                    view_host::send(view, PlatformViewEvent::ERROR, &why);
                    (Content::Nothing, listeners)
                }
            };
        }
        let mut listen = |target: &web_sys::EventTarget, kind: &str, listener: Listener| {
            let _ =
                target.add_event_listener_with_callback(kind, listener.as_ref().unchecked_ref());
            listeners.push(listener);
        };
        let focus =
            |el: &web_sys::EventTarget,
             listen: &mut dyn FnMut(&web_sys::EventTarget, &str, Listener)| {
                listen(
                    el,
                    "focus",
                    Closure::new(move |_: Event| {
                        view_host::send(view, PlatformViewEvent::FOCUS, "true")
                    }),
                );
                listen(
                    el,
                    "blur",
                    Closure::new(move |_: Event| {
                        view_host::send(view, PlatformViewEvent::FOCUS, "false")
                    }),
                );
            };
        const FILL: &str = "display:block;box-sizing:border-box;width:100%;height:100%;margin:0;";
        let content = match view_type {
            "frus/webview" => {
                let Some(frame) = self.element::<HtmlIFrameElement>("iframe") else {
                    return (Content::Nothing, listeners);
                };
                frame
                    .set_attribute("style", &format!("{FILL}border:0;background:#fff;"))
                    .ok();
                load(&frame, params);
                let source = frame.clone();
                listen(
                    &frame,
                    "load",
                    Closure::new(move |_: Event| {
                        view_host::send(view, "pageFinished", &address(&source));
                    }),
                );
                // A press in a frame takes the focus from the page: the page's window hears it.
                let watched = frame.clone();
                let document = self.document.clone();
                if let Some(window) = web_sys::window() {
                    listen(
                        &window,
                        "blur",
                        Closure::new(move |_: Event| {
                            if document.active_element().is_some_and(|active| {
                                active == *AsRef::<web_sys::Element>::as_ref(&watched)
                            }) {
                                view_host::send(view, PlatformViewEvent::FOCUS, "true");
                            }
                        }),
                    );
                }
                container.append_child(&frame).ok();
                Content::Frame(frame)
            }
            "frus/edittext" => {
                let Some(input) = self.element::<HtmlInputElement>("input") else {
                    return (Content::Nothing, listeners);
                };
                input.set_type("text");
                input.set_placeholder(params);
                input
                    .set_attribute(
                        "style",
                        &format!(
                            "{FILL}padding:0 10px;border:1px solid #767676;background:#fff;\
                             color:#000;font:15px system-ui,sans-serif;outline-offset:-2px;"
                        ),
                    )
                    .ok();
                let typed = input.clone();
                listen(
                    &input,
                    "input",
                    Closure::new(move |_: Event| view_host::send(view, "changed", &typed.value())),
                );
                let submitted = input.clone();
                listen(
                    &input,
                    "keydown",
                    Closure::new(move |event: Event| {
                        if event
                            .dyn_ref::<KeyboardEvent>()
                            .is_some_and(|key| key.key() == "Enter")
                        {
                            view_host::send(view, "submitted", &submitted.value());
                        }
                    }),
                );
                focus(&input, &mut listen);
                container.append_child(&input).ok();
                Content::Input(input)
            }
            "frus/video" => {
                let Some(video) = self.element::<HtmlVideoElement>("video") else {
                    return (Content::Nothing, listeners);
                };
                video
                    .set_attribute(
                        "style",
                        &format!("{FILL}background:#000;object-fit:contain;"),
                    )
                    .ok();
                video.set_controls(true);
                video.set_attribute("playsinline", "").ok();
                video.set_src(params);
                for (kind, name) in [
                    ("playing", "playing"),
                    ("pause", "paused"),
                    ("ended", "ended"),
                    ("error", "error"),
                    ("loadedmetadata", "loaded"),
                ] {
                    let watched = video.clone();
                    listen(
                        &video,
                        kind,
                        Closure::new(move |_: Event| {
                            let data = match name {
                                "loaded" => format!("{}", watched.duration()),
                                "error" => watched.src(),
                                _ => format!("{}", watched.current_time()),
                            };
                            view_host::send(view, name, &data);
                        }),
                    );
                }
                focus(&video, &mut listen);
                container.append_child(&video).ok();
                Content::Video(video)
            }
            other => {
                view_host::send(
                    view,
                    PlatformViewEvent::ERROR,
                    &format!("no native view type \"{other}\" on the web"),
                );
                Content::Nothing
            }
        };
        (content, listeners)
    }
}

/// Shows `params` in `frame`: an address, or HTML when it starts with `<`.
fn load(frame: &HtmlIFrameElement, params: &str) {
    if params.trim_start().starts_with('<') {
        frame.remove_attribute("src").ok();
        frame.set_srcdoc(params);
    } else if !params.trim().is_empty() {
        frame.remove_attribute("srcdoc").ok();
        frame.set_src(params);
    }
}

/// The address `frame` shows: its document's own when the page may read it (a page of the
/// same site, or HTML given), its `src` otherwise.
fn address(frame: &HtmlIFrameElement) -> String {
    frame
        .content_window()
        .and_then(|window| window.location().href().ok())
        .unwrap_or_else(|| frame.src())
}

/// Calls `name` on what `path` reaches from `frame`'s window (`["history"]`, `"back"`): a frame
/// from another site answers nothing, and that is said in the log.
fn frame_call(frame: &HtmlIFrameElement, path: &[&str], name: &str) -> Result<JsValue, JsValue> {
    let mut target: JsValue = frame
        .content_window()
        .ok_or_else(|| JsValue::from_str("the frame has no window"))?
        .into();
    for step in path {
        target = js_sys::Reflect::get(&target, &JsValue::from_str(step))?;
    }
    let function: js_sys::Function = js_sys::Reflect::get(&target, &JsValue::from_str(name))?
        .dyn_into()
        .map_err(|_| JsValue::from_str("not a function"))?;
    function.call0(&target)
}

impl ViewHost for Host {
    fn create(&mut self, view: i32, view_type: &str, params: &str, at: PixelRect) {
        let Some(container) = self.element::<HtmlElement>("div") else {
            return;
        };
        container
            .set_attribute("data-frus-view", &view.to_string())
            .ok();
        let shape = Shape::Hidden;
        container
            .set_attribute("style", &self.style(at, &shape))
            .ok();
        match self.canvas.parent_node() {
            Some(parent) => parent.append_child(&container).ok(),
            None => self
                .document
                .body()
                .and_then(|body| body.append_child(&container).ok()),
        };
        let (content, listeners) = self.make(view, view_type, params, &container, at);
        self.views.insert(
            view,
            Hosted {
                container,
                content,
                at,
                shape,
                _listeners: listeners,
            },
        );
    }

    fn place(&mut self, view: i32, at: PixelRect) {
        let Some(hosted) = self.views.get_mut(&view) else {
            return;
        };
        let resized = (hosted.at.w, hosted.at.h) != (at.w, at.h);
        hosted.at = at;
        if resized {
            if let Content::Plugin(plugin) = &mut hosted.content {
                plugin.resized(at.w, at.h);
            }
        }
        self.restyle(view);
    }

    fn shape(&mut self, view: i32, shape: &Shape) {
        let Some(hosted) = self.views.get_mut(&view) else {
            return;
        };
        let shown = hosted.shape.shows();
        hosted.shape = shape.clone();
        if shown != shape.shows() {
            if let Content::Plugin(plugin) = &mut hosted.content {
                plugin.visibility_changed(shape.shows());
            }
        }
        self.restyle(view);
    }

    fn dispose(&mut self, view: i32) {
        if let Some(hosted) = self.views.remove(&view) {
            let had_focus = self
                .document
                .active_element()
                .is_some_and(|active| hosted.container.contains(Some(&active)));
            drop(hosted.content);
            hosted.container.remove();
            if had_focus {
                let _ = self.canvas.focus();
            }
        }
    }

    fn invoke(&mut self, view: i32, method: &str, arguments: &str) {
        let Some(hosted) = self.views.get_mut(&view) else {
            return;
        };
        let result: Result<(), JsValue> = match &mut hosted.content {
            Content::Frame(frame) => match method {
                "loadUrl" => {
                    frame.remove_attribute("srcdoc").ok();
                    frame.set_src(arguments);
                    Ok(())
                }
                "loadHtml" => {
                    frame.remove_attribute("src").ok();
                    frame.set_srcdoc(arguments);
                    Ok(())
                }
                "evaluateJavascript" => {
                    let answer = frame
                        .content_window()
                        .ok_or_else(|| JsValue::from_str("the frame has no window"))
                        .and_then(|window| {
                            let eval: js_sys::Function =
                                js_sys::Reflect::get(&window, &JsValue::from_str("eval"))?
                                    .dyn_into()
                                    .map_err(|_| JsValue::from_str("no eval"))?;
                            eval.call1(&window, &JsValue::from_str(arguments))
                        })
                        .and_then(|value| {
                            if value.is_undefined() {
                                return Ok(String::from("null"));
                            }
                            js_sys::JSON::stringify(&value)
                                .map(|json| json.as_string().unwrap_or_default())
                        });
                    match answer {
                        Ok(json) => view_host::send(view, "javascriptResult", &json),
                        Err(err) => {
                            // A page from another site keeps its scripts to itself.
                            view_host::send(view, "javascriptResult", "null");
                            log::warn!("native view {view}: evaluateJavascript failed ({err:?})");
                        }
                    }
                    Ok(())
                }
                "goBack" => frame_call(frame, &["history"], "back").map(drop),
                "goForward" => frame_call(frame, &["history"], "forward").map(drop),
                "reload" => frame_call(frame, &["location"], "reload")
                    .map(drop)
                    .or_else(|_| {
                        // Another site's page: loaded again from its address.
                        let src = frame.src();
                        frame.set_src(&src);
                        Ok(())
                    }),
                "stop" => frame_call(frame, &[], "stop").map(drop),
                _ => Ok(()),
            },
            Content::Input(input) => {
                match method {
                    "setText" => {
                        input.set_value(arguments);
                        view_host::send(view, "changed", arguments);
                    }
                    "setHint" => input.set_placeholder(arguments),
                    _ => {}
                }
                Ok(())
            }
            Content::Video(video) => match method {
                "play" => video.play().map(drop),
                "pause" => video.pause(),
                "setSource" => {
                    video.set_src(arguments);
                    Ok(())
                }
                "seekTo" => {
                    if let Ok(seconds) = arguments.trim().parse::<f64>() {
                        video.set_current_time(seconds);
                    }
                    Ok(())
                }
                "setMuted" => {
                    video.set_muted(arguments.trim() == "true");
                    Ok(())
                }
                "setLooping" => {
                    video.set_loop(arguments.trim() == "true");
                    Ok(())
                }
                _ => Ok(()),
            },
            Content::Plugin(plugin) => {
                plugin.invoke(method, arguments);
                Ok(())
            }
            Content::Nothing => Ok(()),
        };
        if let Err(err) = result {
            log::warn!("native view {view}: {method} failed ({err:?})");
        }
    }

    fn snapshot(&mut self, _view: i32) -> Option<frus_widgets::ImageHandle> {
        None
    }

    fn clear_focus(&mut self) {
        let inside = self.document.active_element().is_some_and(|active| {
            self.views
                .values()
                .any(|hosted| hosted.container.contains(Some(&active)))
        });
        if inside {
            let _ = self.canvas.focus();
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        for view in self.views.keys().copied().collect::<Vec<_>>() {
            self.dispose(view);
        }
    }
}
