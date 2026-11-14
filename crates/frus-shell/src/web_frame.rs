// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! A web view controller's page on **the web** (milestone 804, feature `webview`): an
//! `<iframe>` that speaks the `web.*` calls and events of `frus::web_view_wire`
//! (`web_view_protocol.rs`), within what a browser lets a page do with a frame.
//!
//! - **Any page**: loading an address, HTML (`srcdoc`), an asset (an address beside the
//!   page), the background, and whether scripts run (`sandbox`, from the next load). A
//!   request with headers or a body is fetched and its HTML shown, which the server must
//!   allow (CORS).
//! - **A page of the application's own origin** (HTML given, or an address of the same
//!   site): its address, title and history, back and forward, scripts with their results,
//!   JavaScript channels (`Name.postMessage` posts to the frame's parent, which hands it to
//!   the controller), and decided navigations for the links the user follows.
//! - **A page of another origin**: the browser keeps its document from the page around it.
//!   Its scripts' results answer "unsupported", and its history is only what frus loaded.
//!
//! The user agent, zoom, the cache, the storage and cookies are the browser's: the
//! controller answers `WebViewError::Unsupported` for them before anything is sent.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Event, HtmlElement, HtmlIFrameElement, MessageEvent};

use crate::view_host;
use crate::web_platform_view::Listener;
use crate::web_view_protocol::{self as protocol, Call, FrameHistory, Move, Params};

/// What a channel's script posts with: a message to the frame's parent.
const POST: &str = "function(o){window.parent.postMessage(o,'*');}";

/// The script a page of the application's origin gets on each load: the links the user
/// follows are told to the frame's parent (`frusFollow`), and while the controller decides
/// navigations (`window.__frusDecides`), held back and asked about (`frusNavigate`).
const LINKS: &str = "(function(){if(window.__frusLinks)return;window.__frusLinks=1;\
document.addEventListener('click',function(e){var a=e.target&&e.target.closest&&\
e.target.closest('a[href]');if(!a||e.defaultPrevented||(a.target&&a.target!=='_self'))return;\
if(window.__frusDecides){e.preventDefault();window.parent.postMessage({frusNavigate:a.href},'*');}\
else{window.parent.postMessage({frusFollow:a.href},'*');}},true);})();";

struct State {
    history: FrameHistory,
    channels: Vec<String>,
    decides: bool,
    javascript: bool,
    /// The address the next load shows when the frame shows HTML of its own (`srcdoc`).
    srcdoc_url: String,
    /// The address of the link the user followed, for a page whose own the browser hides.
    following: Option<String>,
    /// The address last told to the controller.
    url: String,
    /// Whether `web.pageStarted` was sent for the load under way.
    started: bool,
}

/// A controller's page in a frame.
pub(crate) struct Page {
    frame: HtmlIFrameElement,
    view: Rc<Cell<i32>>,
    state: Rc<RefCell<State>>,
    _listeners: Vec<Listener>,
}

fn emit(view: &Cell<i32>, (name, data): protocol::Event) {
    view_host::send(view.get(), name, &data);
}

/// The document of the page in `frame`, when it is of the application's origin.
fn document(frame: &HtmlIFrameElement) -> Option<web_sys::Document> {
    frame.content_document()
}

/// Runs `script` in the page of `frame`, if it is of the application's origin: its value.
fn eval(frame: &HtmlIFrameElement, script: &str) -> Result<JsValue, JsValue> {
    if document(frame).is_none() {
        return Err(JsValue::from_str(
            "the page is from another origin: the browser keeps its scripts from the page around it",
        ));
    }
    let window = frame
        .content_window()
        .ok_or_else(|| JsValue::from_str("the frame has no window"))?;
    let eval: js_sys::Function = js_sys::Reflect::get(&window, &JsValue::from_str("eval"))?
        .dyn_into()
        .map_err(|_| JsValue::from_str("no eval"))?;
    eval.call1(&window, &JsValue::from_str(script))
}

fn describe(error: &JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(error, &JsValue::from_str("message"))
                .ok()
                .and_then(|m| m.as_string())
        })
        .unwrap_or_else(|| format!("{error:?}"))
}

impl Page {
    /// The page of `params`, in a frame appended to `container`.
    pub(crate) fn new(
        document_of_page: &web_sys::Document,
        container: &HtmlElement,
        view: i32,
        params: &Params,
    ) -> Option<Self> {
        let frame: HtmlIFrameElement = document_of_page
            .create_element("iframe")
            .ok()?
            .dyn_into()
            .ok()?;
        let view = Rc::new(Cell::new(view));
        let state = Rc::new(RefCell::new(State {
            history: FrameHistory::default(),
            channels: params.channels.clone(),
            decides: params.decides_navigation,
            javascript: params.javascript,
            srcdoc_url: String::from("about:blank"),
            following: None,
            url: String::new(),
            started: false,
        }));
        let mut page = Self {
            frame: frame.clone(),
            view: view.clone(),
            state: state.clone(),
            _listeners: Vec::new(),
        };
        page.style(params.background);
        page.sandbox(params.javascript);
        let (at, watched, loaded) = (view.clone(), frame.clone(), state.clone());
        page._listeners.push(Listener::new(
            &frame,
            "load",
            Closure::new(move |_: Event| on_load(&at, &watched, &loaded)),
        ));
        if let Some(window) = web_sys::window() {
            let (at, watched, heard) = (view, frame.clone(), state);
            page._listeners.push(Listener::new(
                &window,
                "message",
                Closure::new(move |event: Event| {
                    if let Some(event) = event.dyn_ref::<MessageEvent>() {
                        on_message(&at, &watched, &heard, event);
                    }
                }),
            ));
        }
        container.append_child(&frame).ok()?;
        Some(page)
    }

    /// The view that shows the page now (another widget of the same controller took it).
    pub(crate) fn set_view(&self, view: i32) {
        self.view.set(view);
    }

    fn style(&self, background: Option<[u8; 4]>) {
        let background = background.map_or_else(
            || String::from("#fff"),
            |[r, g, b, a]| format!("rgba({r},{g},{b},{})", f32::from(a) / 255.0),
        );
        self.frame
            .set_attribute(
                "style",
                &format!(
                    "display:block;box-sizing:border-box;width:100%;height:100%;margin:0;\
                     border:0;background:{background};"
                ),
            )
            .ok();
    }

    /// Scripts on or off, from the next load: a sandbox without `allow-scripts`.
    fn sandbox(&self, javascript: bool) {
        if javascript {
            self.frame.remove_attribute("sandbox").ok();
        } else {
            self.frame
                .set_attribute(
                    "sandbox",
                    "allow-same-origin allow-forms allow-popups allow-modals allow-downloads",
                )
                .ok();
        }
    }

    /// The scripts a page given as HTML starts with: its channels, from its first line.
    fn first_scripts(&self) -> String {
        let state = self.state.borrow();
        if !state.javascript || state.channels.is_empty() {
            return String::new();
        }
        let scripts: String = state
            .channels
            .iter()
            .map(|name| protocol::channel_script(name, POST))
            .collect();
        format!("<script>{scripts}</script>")
    }

    fn start(&self, step: Move, url: &str) {
        {
            let mut state = self.state.borrow_mut();
            state.history.going(step);
            state.started = true;
        }
        emit(&self.view, protocol::page_started(url));
        emit(&self.view, protocol::progress(10));
    }

    fn show_html(&self, html: &str, url: &str) {
        self.state.borrow_mut().srcdoc_url = url.to_owned();
        self.frame.remove_attribute("src").ok();
        self.frame
            .set_srcdoc(&protocol::put_first(html, &self.first_scripts()));
    }

    fn show_address(&self, url: &str) {
        self.frame.remove_attribute("srcdoc").ok();
        self.frame.set_src(url);
    }

    fn unsupported(&self, what: &str) {
        log::warn!(
            "web view {}: {what} is not possible in a browser's frame",
            self.view.get()
        );
    }

    /// The application called `method`.
    pub(crate) fn invoke(&mut self, method: &str, arguments: &str) {
        let Some(call) = Call::parse(method, arguments) else {
            log::warn!("web view {}: no call {method}", self.view.get());
            return;
        };
        match call {
            Call::LoadRequest {
                url,
                post,
                headers,
                body,
            } => {
                self.start(Move::Load, &url);
                if !post && headers.is_empty() {
                    self.show_address(&url);
                } else {
                    self.fetch(url, post, headers, body);
                }
            }
            Call::LoadHtml { html, base_url } => {
                let url = base_url.clone().unwrap_or_else(|| "about:blank".to_owned());
                self.start(Move::Load, &url);
                let html = match &base_url {
                    Some(base) => protocol::with_base(&html, base),
                    None => html,
                };
                self.show_html(&html, &url);
            }
            Call::LoadFile(path) => {
                let url = protocol::file_url(&path);
                emit(
                    &self.view,
                    protocol::resource_error(
                        0,
                        "file",
                        "a page on the web cannot load the device's files",
                        &url,
                    ),
                );
            }
            Call::LoadAsset(key) => {
                // Served beside the application's page.
                let url = format!("assets/{}", key.trim_start_matches('/'));
                self.start(Move::Load, &url);
                self.show_address(&url);
            }
            Call::Reload => {
                let url = self.state.borrow().url.clone();
                self.start(Move::Reload, &url);
                if eval(&self.frame, "location.reload()").is_err() {
                    // Another origin's page: loaded again from its address.
                    let src = self.frame.src();
                    self.frame.set_src(&src);
                }
            }
            Call::GoBack | Call::GoForward => {
                let back = matches!(call, Call::GoBack);
                let can = {
                    let state = self.state.borrow();
                    if back {
                        state.history.can_go_back()
                    } else {
                        state.history.can_go_forward()
                    }
                };
                if !can {
                    return;
                }
                let step = if back { Move::Back } else { Move::Forward };
                self.state.borrow_mut().history.going(step);
                let script = if back {
                    "history.back()"
                } else {
                    "history.forward()"
                };
                if eval(&self.frame, script).is_err() {
                    self.state.borrow_mut().history.going(Move::Load);
                    self.unsupported("moving in the history of another origin's page");
                }
            }
            Call::Run(script) => {
                if let Err(error) = eval(&self.frame, &script) {
                    log::warn!(
                        "web view {}: the script failed ({})",
                        self.view.get(),
                        describe(&error)
                    );
                }
            }
            Call::RunForResult { id, script } => {
                let foreign = document(&self.frame).is_none();
                let outcome = eval(&self.frame, &script)
                    .and_then(|value| {
                        if value.is_undefined() {
                            return Ok(String::from("null"));
                        }
                        js_sys::JSON::stringify(&value)
                            .map(|json| json.as_string().unwrap_or_else(|| "null".to_owned()))
                    })
                    .map_err(|error| describe(&error));
                emit(
                    &self.view,
                    protocol::javascript_result(id, outcome, foreign),
                );
            }
            Call::AddChannel(name) => {
                let added = {
                    let mut state = self.state.borrow_mut();
                    let new = !state.channels.contains(&name);
                    if new {
                        state.channels.push(name.clone());
                    }
                    new && state.javascript
                };
                if added {
                    // The page shown gets it now; the next loads from their start.
                    let _ = eval(&self.frame, &protocol::channel_script(&name, POST));
                }
            }
            Call::RemoveChannel(name) => {
                self.state.borrow_mut().channels.retain(|n| *n != name);
            }
            Call::SetJavascript(on) => {
                self.state.borrow_mut().javascript = on;
                self.sandbox(on);
            }
            Call::SetBackground(color) => self.style(Some(color)),
            Call::SetDecidesNavigation(on) => {
                self.state.borrow_mut().decides = on;
                let _ = eval(
                    &self.frame,
                    &format!("window.__frusDecides={}", if on { "true" } else { "false" }),
                );
            }
            Call::SetUserAgent(_) => self.unsupported("a user agent of its own"),
            Call::SetZoom(_) => self.unsupported("turning zoom on or off"),
            Call::ClearCache => self.unsupported("clearing the cache"),
            Call::ClearLocalStorage => self.unsupported("clearing the storage"),
        }
    }

    /// A request a frame cannot make by its address (headers, a body): fetched, and its HTML
    /// shown in the frame, resolved against its address.
    fn fetch(&self, url: String, post: bool, headers: Vec<(String, String)>, body: Vec<u8>) {
        let Some(window) = web_sys::window() else {
            return;
        };
        let init = web_sys::RequestInit::new();
        init.set_method(if post { "POST" } else { "GET" });
        if let Ok(list) = web_sys::Headers::new() {
            for (name, value) in &headers {
                let _ = list.append(name, value);
            }
            init.set_headers(&list);
        }
        if post {
            init.set_body(&js_sys::Uint8Array::from(body.as_slice()));
        }
        let promise = window.fetch_with_str_and_init(&url, &init);
        let (view, frame, state) = (self.view.clone(), self.frame.clone(), self.state.clone());
        let first = self.first_scripts();
        wasm_bindgen_futures::spawn_local(async move {
            let fetched: Result<(u16, String), JsValue> = async {
                let response: web_sys::Response = wasm_bindgen_futures::JsFuture::from(promise)
                    .await?
                    .dyn_into()?;
                let status = response.status();
                let text = wasm_bindgen_futures::JsFuture::from(response.text()?).await?;
                Ok((status, text.as_string().unwrap_or_default()))
            }
            .await;
            match fetched {
                Ok((status, html)) => {
                    if status >= 400 {
                        emit(&view, protocol::http_error(i32::from(status), &url));
                    }
                    state.borrow_mut().srcdoc_url = url.clone();
                    frame.remove_attribute("src").ok();
                    let html = protocol::with_base(&html, &url);
                    frame.set_srcdoc(&protocol::put_first(&html, &first));
                }
                Err(error) => {
                    state.borrow_mut().started = false;
                    emit(
                        &view,
                        protocol::resource_error(
                            0,
                            "connect",
                            &format!(
                                "the request could not be made from the page (the server must \
                                 allow it: CORS): {}",
                                describe(&error)
                            ),
                            &url,
                        ),
                    );
                }
            }
        });
    }
}

/// The frame loaded a page: the controller hears where it is.
fn on_load(view: &Cell<i32>, frame: &HtmlIFrameElement, state: &RefCell<State>) {
    let readable = document(frame);
    let own_html = frame.has_attribute("srcdoc");
    let address = frame
        .content_window()
        .and_then(|window| window.location().href().ok());
    let mut state = state.borrow_mut();
    let url = match address.as_deref() {
        Some("about:srcdoc") if own_html => state.srcdoc_url.clone(),
        Some(address) => address.to_owned(),
        // Another origin: the link followed, or what was asked for.
        None => state.following.take().unwrap_or_else(|| frame.src()),
    };
    state.following = None;
    // The frame's first, empty page is nobody's.
    if state.history.is_blank() && url == "about:blank" {
        return;
    }
    let started = std::mem::take(&mut state.started);
    state.history.loaded(&url);
    let changed = state.url != url;
    state.url = url.clone();
    // Another origin's title is the browser's secret: its address stands for it, as a
    // web view's title does before the page names itself.
    let title = Some(readable.as_ref().map_or_else(|| url.clone(), |d| d.title()));
    let (back, forward) = (
        state.history.can_go_back() && readable.is_some(),
        state.history.can_go_forward() && readable.is_some(),
    );
    let channels = state.channels.clone();
    let (decides, javascript) = (state.decides, state.javascript);
    drop(state);
    if !started {
        // A link or a script of the page: the browser says nothing until it is loaded.
        emit(view, protocol::page_started(&url));
    }
    if changed {
        emit(view, protocol::url_change(&url));
    }
    emit(view, protocol::progress(100));
    emit(view, protocol::page_finished(&url));
    emit(
        view,
        protocol::state(
            &url,
            title.as_deref().filter(|t| !t.is_empty()),
            back,
            forward,
        ),
    );
    if readable.is_some() && javascript {
        let mut script: String = channels
            .iter()
            .map(|name| protocol::channel_script(name, POST))
            .collect();
        script.push_str(LINKS);
        script.push_str(&format!("window.__frusDecides={decides};"));
        let _ = eval(frame, &script);
    }
}

/// A message posted to the page around the frame: a channel's, or a link followed.
fn on_message(
    view: &Cell<i32>,
    frame: &HtmlIFrameElement,
    state: &RefCell<State>,
    event: &MessageEvent,
) {
    let from_frame = match (event.source(), frame.content_window()) {
        (Some(source), Some(window)) => JsValue::from(source) == JsValue::from(window),
        _ => false,
    };
    if !from_frame {
        return;
    }
    let data = event.data();
    let field = |key: &str| {
        js_sys::Reflect::get(&data, &JsValue::from_str(key))
            .ok()
            .and_then(|v| v.as_string())
    };
    if let Some(channel) = field("frusChannel") {
        // Only a channel the controller has open: a page cannot make one up.
        if state.borrow().channels.contains(&channel) {
            let message = field("message").unwrap_or_default();
            emit(view, protocol::channel_message(&channel, &message));
        }
    } else if let Some(url) = field("frusNavigate") {
        emit(view, protocol::navigation_request(&url));
    } else if let Some(url) = field("frusFollow") {
        state.borrow_mut().following = Some(url);
    }
}
