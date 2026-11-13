// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus/webview` on Windows (milestone 802, feature `webview2`): the system's WebView2 in a
//! view's container.
//!
//! WebView2 is made asynchronously: the environment, then the controller, each answered on the
//! window's thread from its message loop — which winit runs. Until the controller exists, the
//! calls the application makes wait, in order, as do the size and the visibility.
//!
//! It speaks two protocols, chosen by the creation parameters:
//!
//! - **A controller's page** (milestone 804), parameters that are a JSON object with a
//!   `controller` number: the `web.*` calls and events of `frus::web_view_wire`, exactly as
//!   Android's web view speaks them (`web_view_protocol.rs`). The page belongs to the
//!   controller: the host keeps it while its widget is away and hands it to the next widget
//!   of the same controller.
//! - **A plain web view** (milestone 802), parameters that are an address or HTML: the calls
//!   `loadUrl`, `loadHtml`, `evaluateJavascript` (answered by `javascriptResult`, the value as
//!   JSON), `goBack`, `goForward`, `reload` and `stop`; the events `pageStarted` and
//!   `pageFinished` (the address).
//!
//! Both send `frus.focus`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2Controller,
    ICoreWebView2Controller2, ICoreWebView2CookieList, ICoreWebView2Environment,
    ICoreWebView2Environment2, ICoreWebView2EnvironmentOptions,
    ICoreWebView2NavigationCompletedEventArgs2, ICoreWebView2Profile2, ICoreWebView2Settings2,
    ICoreWebView2Settings5, ICoreWebView2_13, ICoreWebView2_2, COREWEBVIEW2_BROWSING_DATA_KINDS,
    COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_DOM_STORAGE, COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE,
    COREWEBVIEW2_COLOR, COREWEBVIEW2_WEB_ERROR_STATUS, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
};
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, ClearBrowsingDataCompletedHandler,
    ContentLoadingEventHandler, CreateCoreWebView2ControllerCompletedHandler,
    CreateCoreWebView2EnvironmentCompletedHandler, DOMContentLoadedEventHandler,
    DocumentTitleChangedEventHandler, ExecuteScriptCompletedHandler, FocusChangedEventHandler,
    GetCookiesCompletedHandler, HistoryChangedEventHandler, NavigationCompletedEventHandler,
    NavigationStartingEventHandler, SourceChangedEventHandler, WebMessageReceivedEventHandler,
    WebResourceRequestedEventHandler,
};
use windows::core::{Interface, BOOL, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, RECT};

use crate::view_host;
use crate::web_view_protocol::{self as protocol, Call, Gate, Global, NavigationGate, Params};
use frus_widgets::PlatformViewEvent;

struct Ready {
    controller: ICoreWebView2Controller,
    webview: ICoreWebView2,
    environment: ICoreWebView2Environment,
}

#[derive(Default)]
struct State {
    ready: Option<Ready>,
    /// Calls made before the controller existed, in order.
    pending: Vec<(String, String)>,
    size: (i32, i32),
    visible: bool,
    /// The widget left before the controller came: it is closed as it comes.
    gone: bool,
    /// A controller's page: its settings at creation. `None`: a plain web view.
    params: Option<Params>,
    gate: NavigationGate,
    /// The open channels, with the identifier of their script once WebView2 gave it.
    channels: Vec<(String, Rc<RefCell<Option<String>>>)>,
    /// The platform's own user agent, for `setUserAgent(null)`.
    default_agent: Option<String>,
    /// HTML given with a base address: served when the page asks for that address.
    html_for: Option<(String, String)>,
}

thread_local! {
    /// Cookie calls made before any web view was ready: carried out by the first one.
    static WAITING: RefCell<Vec<Global>> = const { RefCell::new(Vec::new()) };
}

/// One web view.
pub(super) struct Web {
    /// The view it is shown by: its events are that view's. Changes when another widget of
    /// the same controller takes the page.
    view: Rc<Cell<i32>>,
    state: Rc<RefCell<State>>,
}

impl Web {
    /// A web view in `container`, `w` × `h`, made with `params`: a controller's parameters, or
    /// for a plain web view an address, or HTML when it starts with `<`.
    pub(super) fn new(
        container: windows_sys::Win32::Foundation::HWND,
        view: i32,
        params: &str,
        w: i32,
        h: i32,
    ) -> Self {
        let mut pending = Vec::new();
        let page = Params::parse(params);
        if page.is_none() {
            let first = params.trim_start();
            if first.starts_with('<') {
                pending.push(("loadHtml".to_owned(), params.to_owned()));
            } else if !first.is_empty() {
                pending.push(("loadUrl".to_owned(), params.to_owned()));
            }
        }
        let gate = NavigationGate::new(page.as_ref().is_some_and(|p| p.decides_navigation));
        let state = Rc::new(RefCell::new(State {
            pending,
            size: (w, h),
            visible: false,
            params: page,
            gate,
            ..Default::default()
        }));
        let view = Rc::new(Cell::new(view));
        let parent = HWND(container);
        let made = Rc::downgrade(&state);
        let at = view.clone();
        let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
            move |result, environment: Option<ICoreWebView2Environment>| {
                let environment = match (result, environment) {
                    (Ok(()), Some(environment)) => environment,
                    (result, _) => {
                        failed(at.get(), &format!("WebView2 could not start ({result:?})"));
                        return Ok(());
                    }
                };
                let made = made.clone();
                let shown = at.clone();
                let kept = environment.clone();
                let controller_made = CreateCoreWebView2ControllerCompletedHandler::create(
                    Box::new(move |result, controller: Option<ICoreWebView2Controller>| {
                        let at = shown;
                        match (result, controller) {
                            (Ok(()), Some(controller)) => ready(&at, &made, controller, kept),
                            (result, _) => failed(
                                at.get(),
                                &format!("the WebView2 controller could not be made ({result:?})"),
                            ),
                        }
                        Ok(())
                    }),
                );
                // SAFETY: the container is alive (a gone view's is closed as it comes).
                if let Err(err) =
                    unsafe { environment.CreateCoreWebView2Controller(parent, &controller_made) }
                {
                    failed(
                        at.get(),
                        &format!("the WebView2 controller could not be made ({err})"),
                    );
                }
                Ok(())
            },
        ));
        // The browser's data (cookies, storage, cache) in the application's own folder, not
        // beside the executable, which may not be writable.
        let folder = crate::data_dir::app_data_dir()
            .map(|dir| HSTRING::from(dir.join("WebView2").as_os_str()))
            .unwrap_or_default();
        let folder = if folder.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(folder.as_ptr())
        };
        // SAFETY: the strings outlive the call; the handler is reference-counted.
        let started = unsafe {
            CreateCoreWebView2EnvironmentWithOptions(
                PCWSTR::null(),
                folder,
                None::<&ICoreWebView2EnvironmentOptions>,
                &handler,
            )
        };
        if let Err(err) = started {
            failed(
                view.get(),
                &format!(
                    "WebView2 is not available: is the Edge WebView2 runtime installed? ({err})"
                ),
            );
        }
        Self { view, state }
    }

    /// The view that shows the page now (another widget of the same controller took it).
    pub(super) fn set_view(&mut self, view: i32) {
        self.view.set(view);
    }

    /// The container was resized.
    pub(super) fn resized(&mut self, w: i32, h: i32) {
        let mut state = self.state.borrow_mut();
        state.size = (w, h);
        if let Some(ready) = &state.ready {
            bounds(&ready.controller, w, h);
        }
    }

    /// The view came into sight, or went out of it: WebView2 stops drawing while hidden.
    pub(super) fn set_visible(&mut self, visible: bool) {
        let mut state = self.state.borrow_mut();
        state.visible = visible;
        if let Some(ready) = &state.ready {
            // SAFETY: a live controller.
            let _ = unsafe { ready.controller.SetIsVisible(visible) };
        }
    }

    /// The application called `method`.
    pub(super) fn invoke(&mut self, method: &str, arguments: &str) {
        let mut state = self.state.borrow_mut();
        if state.ready.is_none() {
            state
                .pending
                .push((method.to_owned(), arguments.to_owned()));
            return;
        }
        call(&self.view, &self.state, &mut state, method, arguments);
    }

    /// The page, once WebView2 made it.
    fn webview(&self) -> Option<ICoreWebView2> {
        self.state
            .try_borrow()
            .ok()?
            .ready
            .as_ref()
            .map(|ready| ready.webview.clone())
    }
}

impl Drop for Web {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        state.gone = true;
        if let Some(ready) = state.ready.take() {
            // SAFETY: a live controller, closed once.
            let _ = unsafe { ready.controller.Close() };
        }
    }
}

/// A cookie call (`web.setCookie`, `web.clearCookies`), carried out through any of `webs`
/// that is ready — the cookies are the profile's, every web view's — or by the first one to
/// be ready.
pub(super) fn cookies<'a>(global: Global, webs: impl Iterator<Item = &'a Web>) {
    let webview = webs.filter_map(Web::webview).next();
    match webview {
        Some(webview) => cookie_call(&webview, global),
        None => WAITING.with(|waiting| waiting.borrow_mut().push(global)),
    }
}

fn cookie_call(webview: &ICoreWebView2, global: Global) {
    // SAFETY: a live web view; the strings outlive each call; the handlers are
    // reference-counted.
    let done = unsafe {
        webview
            .cast::<ICoreWebView2_2>()
            .and_then(|webview| webview.CookieManager())
            .and_then(|cookies| match global {
                Global::SetCookie {
                    name,
                    value,
                    domain,
                    path,
                } => cookies
                    .CreateCookie(
                        &HSTRING::from(name),
                        &HSTRING::from(value),
                        &HSTRING::from(domain),
                        &HSTRING::from(path),
                    )
                    .and_then(|cookie| cookies.AddOrUpdateCookie(&cookie)),
                Global::ClearCookies { id } => {
                    let manager = cookies.clone();
                    cookies.GetCookies(
                        PCWSTR::null(),
                        &GetCookiesCompletedHandler::create(Box::new(
                            move |_, list: Option<ICoreWebView2CookieList>| {
                                let mut count = 0u32;
                                if let Some(list) = list {
                                    let _ = list.Count(&mut count);
                                }
                                let _ = manager.DeleteAllCookies();
                                let (name, data) = protocol::cookies_cleared(id, count > 0);
                                view_host::send(-1, name, &data);
                                Ok(())
                            },
                        )),
                    )
                }
                Global::Release { .. } => Ok(()),
            })
    };
    if let Err(err) = done {
        log::warn!("web views: the cookie call failed ({err})");
    }
}

fn failed(view: i32, why: &str) {
    log::warn!("native view {view}: {why}");
    view_host::send(view, PlatformViewEvent::ERROR, why);
}

fn emit(view: &Cell<i32>, (name, data): protocol::Event) {
    view_host::send(view.get(), name, &data);
}

fn bounds(controller: &ICoreWebView2Controller, w: i32, h: i32) {
    // SAFETY: a live controller.
    let _ = unsafe {
        controller.SetBounds(RECT {
            left: 0,
            top: 0,
            right: w.max(0),
            bottom: h.max(0),
        })
    };
}

/// Reads a string WebView2 hands out.
fn text(read: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>) -> Option<String> {
    let mut value = PWSTR::null();
    read(&mut value).ok()?;
    Some(webview2_com::take_pwstr(value))
}

fn flag(read: impl FnOnce(*mut BOOL) -> windows::core::Result<()>) -> bool {
    let mut value = BOOL(0);
    read(&mut value).is_ok() && value.as_bool()
}

/// The page's address, title and history, for the controller.
fn send_state(view: &Cell<i32>, webview: &ICoreWebView2) {
    // SAFETY: a live web view.
    unsafe {
        let url = text(|v| webview.Source(v)).unwrap_or_default();
        let title = text(|v| webview.DocumentTitle(v));
        let back = flag(|v| webview.CanGoBack(v));
        let forward = flag(|v| webview.CanGoForward(v));
        emit(view, protocol::state(&url, title.as_deref(), back, forward));
    }
}

/// The controller came: it takes the size, the visibility, the settings and the waiting
/// calls.
fn ready(
    view: &Rc<Cell<i32>>,
    state: &Weak<RefCell<State>>,
    controller: ICoreWebView2Controller,
    environment: ICoreWebView2Environment,
) {
    let Some(shared) = state.upgrade() else {
        // SAFETY: a live controller, closed once.
        let _ = unsafe { controller.Close() };
        return;
    };
    let mut state = shared.borrow_mut();
    if state.gone {
        // SAFETY: a live controller, closed once.
        let _ = unsafe { controller.Close() };
        return;
    }
    // SAFETY: a live controller and its web view.
    let webview = match unsafe { controller.CoreWebView2() } {
        Ok(webview) => webview,
        Err(err) => {
            failed(view.get(), &format!("the WebView2 has no page ({err})"));
            return;
        }
    };
    bounds(&controller, state.size.0, state.size.1);
    // SAFETY: a live controller.
    let _ = unsafe { controller.SetIsVisible(state.visible) };
    let page = state.params.is_some();
    listen(view, &Rc::downgrade(&shared), &controller, &webview, page);
    state.ready = Some(Ready {
        controller,
        webview: webview.clone(),
        environment,
    });
    if let Some(params) = state.params.clone() {
        // SAFETY: a live web view.
        state.default_agent = unsafe {
            webview
                .Settings()
                .and_then(|settings| settings.cast::<ICoreWebView2Settings2>())
                .ok()
                .and_then(|settings| text(|v| settings.UserAgent(v)))
        };
        let settings = [
            Call::SetJavascript(params.javascript),
            Call::SetZoom(params.zoom),
        ]
        .into_iter()
        .chain(params.user_agent.map(|ua| Call::SetUserAgent(Some(ua))))
        .chain(params.background.map(Call::SetBackground))
        .chain(params.channels.into_iter().map(Call::AddChannel));
        for setting in settings {
            web_call(view, &shared, &mut state, setting);
        }
        // Cookie calls made before any web view was ready.
        for global in WAITING.with(|waiting| std::mem::take(&mut *waiting.borrow_mut())) {
            cookie_call(&webview, global);
        }
    }
    for (method, arguments) in std::mem::take(&mut state.pending) {
        call(view, &shared, &mut state, &method, &arguments);
    }
}

/// The page's events: a controller's page (`page`) speaks `web.*`, a plain one the older
/// names.
fn listen(
    view: &Rc<Cell<i32>>,
    state: &Weak<RefCell<State>>,
    controller: &ICoreWebView2Controller,
    webview: &ICoreWebView2,
    page: bool,
) {
    let mut token = 0;
    // SAFETY: a live controller and its web view; the handlers are reference-counted and
    // hold the page's state weakly, so that the page does not keep itself alive.
    unsafe {
        let (at, gate) = (view.clone(), state.clone());
        let _ = webview.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let url = text(|v| args.Uri(v)).unwrap_or_default();
                if !page {
                    view_host::send(at.get(), "pageStarted", &url);
                    return Ok(());
                }
                let mut id = 0u64;
                let _ = args.NavigationId(&mut id);
                let redirect = flag(|v| args.IsRedirected(v));
                let decided = gate
                    .upgrade()
                    .and_then(|state| {
                        let mut state = state.try_borrow_mut().ok()?;
                        Some(state.gate.starting(id, &url, redirect))
                    })
                    .unwrap_or(Gate::Go);
                match decided {
                    Gate::Hold => {
                        let _ = args.SetCancel(true);
                        emit(&at, protocol::navigation_request(&url));
                    }
                    Gate::Go if !redirect => {
                        emit(&at, protocol::page_started(&url));
                        emit(&at, protocol::progress(10));
                    }
                    Gate::Go => {}
                }
                Ok(())
            })),
            &mut token,
        );
        let (at, gate) = (view.clone(), state.clone());
        let _ = webview.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |sender, args| {
                let url = sender
                    .as_ref()
                    .and_then(|webview| text(|v| webview.Source(v)))
                    .unwrap_or_default();
                if !page {
                    view_host::send(at.get(), "pageFinished", &url);
                    return Ok(());
                }
                let Some(args) = args else {
                    return Ok(());
                };
                let mut id = 0u64;
                let _ = args.NavigationId(&mut id);
                let reported = gate
                    .upgrade()
                    .and_then(|state| Some(state.try_borrow_mut().ok()?.gate.completed(id)))
                    .unwrap_or(true);
                if !reported {
                    return Ok(());
                }
                let success = flag(|v| args.IsSuccess(v));
                let mut status = COREWEBVIEW2_WEB_ERROR_STATUS(0);
                let _ = args.WebErrorStatus(&mut status);
                let mut http = 0;
                if let Ok(args) = args.cast::<ICoreWebView2NavigationCompletedEventArgs2>() {
                    let _ = args.HttpStatusCode(&mut http);
                }
                // An HTTP error status is not a failure to load: the server answered.
                let http_only = status.0 == 0 && http >= 400;
                if !success && !http_only {
                    let kind = protocol::webview2_error_type(status.0);
                    emit(
                        &at,
                        protocol::resource_error(
                            i64::from(status.0),
                            kind,
                            &format!("WebView2 error status {} ({kind})", status.0),
                            &url,
                        ),
                    );
                }
                if http >= 400 {
                    emit(&at, protocol::http_error(http, &url));
                }
                emit(&at, protocol::progress(100));
                emit(&at, protocol::page_finished(&url));
                if let Some(webview) = &sender {
                    send_state(&at, webview);
                }
                Ok(())
            })),
            &mut token,
        );
        let at = view.clone();
        let _ = controller.add_GotFocus(
            &FocusChangedEventHandler::create(Box::new(move |_, _| {
                view_host::send(at.get(), PlatformViewEvent::FOCUS, "true");
                Ok(())
            })),
            &mut token,
        );
        let at = view.clone();
        let _ = controller.add_LostFocus(
            &FocusChangedEventHandler::create(Box::new(move |_, _| {
                view_host::send(at.get(), PlatformViewEvent::FOCUS, "false");
                Ok(())
            })),
            &mut token,
        );
        if !page {
            return;
        }
        let at = view.clone();
        let _ = webview.add_ContentLoading(
            &ContentLoadingEventHandler::create(Box::new(move |_, _| {
                emit(&at, protocol::progress(50));
                Ok(())
            })),
            &mut token,
        );
        if let Ok(webview) = webview.cast::<ICoreWebView2_2>() {
            let at = view.clone();
            let _ = webview.add_DOMContentLoaded(
                &DOMContentLoadedEventHandler::create(Box::new(move |_, _| {
                    emit(&at, protocol::progress(80));
                    Ok(())
                })),
                &mut token,
            );
        }
        let at = view.clone();
        let _ = webview.add_SourceChanged(
            &SourceChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(webview) = sender {
                    let url = text(|v| webview.Source(v)).unwrap_or_default();
                    emit(&at, protocol::url_change(&url));
                    send_state(&at, &webview);
                }
                Ok(())
            })),
            &mut token,
        );
        let at = view.clone();
        let _ = webview.add_HistoryChanged(
            &HistoryChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(webview) = sender {
                    send_state(&at, &webview);
                }
                Ok(())
            })),
            &mut token,
        );
        let at = view.clone();
        let _ = webview.add_DocumentTitleChanged(
            &DocumentTitleChangedEventHandler::create(Box::new(move |sender, _| {
                if let Some(webview) = sender {
                    send_state(&at, &webview);
                }
                Ok(())
            })),
            &mut token,
        );
        let (at, open) = (view.clone(), state.clone());
        let _ = webview.add_WebMessageReceived(
            &WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else {
                    return Ok(());
                };
                let Some((channel, message)) = text(|v| args.TryGetWebMessageAsString(v))
                    .and_then(|m| protocol::channel_post(&m))
                else {
                    return Ok(());
                };
                // Only a channel the controller has open: a page cannot make one up.
                let known = open.upgrade().is_some_and(|state| {
                    state
                        .try_borrow()
                        .is_ok_and(|state| state.channels.iter().any(|(n, _)| *n == channel))
                });
                if known {
                    emit(&at, protocol::channel_message(&channel, &message));
                }
                Ok(())
            })),
            &mut token,
        );
        let served = state.clone();
        let _ = webview.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |sender, args| {
                let (Some(args), Some(state)) = (args, served.upgrade()) else {
                    return Ok(());
                };
                let Ok(mut state) = state.try_borrow_mut() else {
                    return Ok(());
                };
                let url = args
                    .Request()
                    .ok()
                    .and_then(|request| text(|v| request.Uri(v)))
                    .unwrap_or_default();
                let wanted = state.html_for.as_ref().is_some_and(|(base, _)| {
                    base.trim_end_matches('/') == url.trim_end_matches('/')
                });
                if !wanted {
                    return Ok(());
                }
                let Some((base, html)) = state.html_for.take() else {
                    return Ok(());
                };
                if let Some(ready) = &state.ready {
                    let body = windows::Win32::UI::Shell::SHCreateMemStream(Some(html.as_bytes()));
                    if let Ok(response) = ready.environment.CreateWebResourceResponse(
                        body.as_ref(),
                        200,
                        &HSTRING::from("OK"),
                        &HSTRING::from("Content-Type: text/html; charset=utf-8"),
                    ) {
                        let _ = args.SetResponse(&response);
                    }
                }
                if let Some(webview) = sender {
                    let _ = webview.RemoveWebResourceRequestedFilter(
                        &HSTRING::from(base),
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
                    );
                }
                Ok(())
            })),
            &mut token,
        );
    }
}

/// Carries out `method` on a ready page.
fn call(
    view: &Rc<Cell<i32>>,
    shared: &Rc<RefCell<State>>,
    state: &mut State,
    method: &str,
    arguments: &str,
) {
    if state.params.is_some() {
        match Call::parse(method, arguments) {
            Some(call) => web_call(view, shared, state, call),
            None => log::warn!("web view {}: no call {method}", view.get()),
        }
        return;
    }
    let Some(ready) = &state.ready else {
        return;
    };
    let webview = &ready.webview;
    let at = view.clone();
    // SAFETY: a live web view; the strings outlive each call.
    let result = unsafe {
        match method {
            "loadUrl" => webview.Navigate(&HSTRING::from(arguments)),
            "loadHtml" => webview.NavigateToString(&HSTRING::from(arguments)),
            "evaluateJavascript" => webview.ExecuteScript(
                &HSTRING::from(arguments),
                &ExecuteScriptCompletedHandler::create(Box::new(move |result, value: String| {
                    match result {
                        Ok(()) => view_host::send(at.get(), "javascriptResult", &value),
                        Err(err) => {
                            view_host::send(at.get(), "javascriptResult", &format!("{err}"))
                        }
                    }
                    Ok(())
                })),
            ),
            "goBack" => webview.GoBack(),
            "goForward" => webview.GoForward(),
            "reload" => webview.Reload(),
            "stop" => webview.Stop(),
            _ => Ok(()),
        }
    };
    if let Err(err) = result {
        log::warn!("native view {}: {method} failed ({err})", view.get());
    }
}

/// The script that opens a channel in a page of WebView2.
fn channel_script(name: &str) -> String {
    protocol::channel_script(
        name,
        "function(o){window.chrome.webview.postMessage(JSON.stringify(o));}",
    )
}

/// Carries out a controller's call on its ready page.
fn web_call(view: &Rc<Cell<i32>>, shared: &Rc<RefCell<State>>, state: &mut State, call: Call) {
    let Some(ready) = &state.ready else {
        return;
    };
    let (webview, environment) = (ready.webview.clone(), ready.environment.clone());
    let controller = ready.controller.clone();
    let noop = || ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(())));
    // SAFETY: a live web view; the strings outlive each call; the handlers are
    // reference-counted.
    let result: windows::core::Result<()> = unsafe {
        match call {
            Call::LoadRequest {
                url,
                post,
                headers,
                body,
            } => {
                state.gate.own(Some(&url));
                if !post && headers.is_empty() {
                    webview.Navigate(&HSTRING::from(url))
                } else {
                    let lines: String = headers
                        .iter()
                        .map(|(name, value)| format!("{name}: {value}\r\n"))
                        .collect();
                    let body = post
                        .then(|| windows::Win32::UI::Shell::SHCreateMemStream(Some(&body)))
                        .flatten();
                    environment
                        .cast::<ICoreWebView2Environment2>()
                        .and_then(|environment| {
                            environment.CreateWebResourceRequest(
                                &HSTRING::from(url),
                                &HSTRING::from(if post { "POST" } else { "GET" }),
                                body.as_ref(),
                                &HSTRING::from(lines),
                            )
                        })
                        .and_then(|request| {
                            webview
                                .cast::<ICoreWebView2_2>()?
                                .NavigateWithWebResourceRequest(&request)
                        })
                }
            }
            Call::LoadHtml { html, base_url } => match base_url {
                // Served for its address: its relative addresses and its origin are that
                // address's, as Android's `loadDataWithBaseURL` makes them.
                Some(base) if base.starts_with("http://") || base.starts_with("https://") => {
                    state.html_for = Some((base.clone(), html));
                    state.gate.own(Some(&base));
                    webview
                        .AddWebResourceRequestedFilter(
                            &HSTRING::from(base.as_str()),
                            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT,
                        )
                        .and_then(|()| webview.Navigate(&HSTRING::from(base)))
                }
                _ => {
                    state.gate.own(None);
                    webview.NavigateToString(&HSTRING::from(html))
                }
            },
            Call::LoadFile(path) => {
                let url = protocol::file_url(&path);
                state.gate.own(Some(&url));
                webview.Navigate(&HSTRING::from(url))
            }
            Call::LoadAsset(key) => {
                // The application's `assets` folder, beside its executable.
                let path = std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|dir| dir.join("assets").join(&key)))
                    .unwrap_or_else(|| std::path::PathBuf::from("assets").join(&key));
                let url = protocol::file_url(&path.to_string_lossy());
                state.gate.own(Some(&url));
                webview.Navigate(&HSTRING::from(url))
            }
            Call::Reload => {
                state.gate.own(None);
                webview.Reload()
            }
            Call::GoBack => {
                if flag(|v| webview.CanGoBack(v)) {
                    state.gate.own(None);
                    webview.GoBack()
                } else {
                    Ok(())
                }
            }
            Call::GoForward => {
                if flag(|v| webview.CanGoForward(v)) {
                    state.gate.own(None);
                    webview.GoForward()
                } else {
                    Ok(())
                }
            }
            Call::Run(script) => webview.ExecuteScript(&HSTRING::from(script), &noop()),
            Call::RunForResult { id, script } => {
                let at = view.clone();
                webview.ExecuteScript(
                    &HSTRING::from(script),
                    &ExecuteScriptCompletedHandler::create(Box::new(
                        move |result, value: String| {
                            let outcome = result.map(|()| value).map_err(|err| err.to_string());
                            emit(&at, protocol::javascript_result(id, outcome, false));
                            Ok(())
                        },
                    )),
                )
            }
            Call::AddChannel(name) => {
                if state.channels.iter().any(|(n, _)| *n == name) {
                    return;
                }
                let script = channel_script(&name);
                let id = Rc::new(RefCell::new(None));
                state.channels.push((name.clone(), id.clone()));
                let (kept, channel_state) = (id, Rc::downgrade(shared));
                let added = webview.AddScriptToExecuteOnDocumentCreated(
                    &HSTRING::from(script.as_str()),
                    &AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
                        move |result, script_id: String| {
                            if result.is_ok() {
                                *kept.borrow_mut() = Some(script_id.clone());
                                // Closed before WebView2 answered: the script goes now.
                                let open = channel_state.upgrade().is_some_and(|state| {
                                    state.try_borrow().is_ok_and(|state| {
                                        state.channels.iter().any(|(_, id)| Rc::ptr_eq(id, &kept))
                                    })
                                });
                                if !open {
                                    if let Some(state) = channel_state.upgrade() {
                                        if let Ok(state) = state.try_borrow() {
                                            if let Some(ready) = &state.ready {
                                                let _ = ready
                                                    .webview
                                                    .RemoveScriptToExecuteOnDocumentCreated(
                                                        &HSTRING::from(script_id),
                                                    );
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(())
                        },
                    )),
                );
                // The page already shown gets it too.
                added.and_then(|()| webview.ExecuteScript(&HSTRING::from(script), &noop()))
            }
            Call::RemoveChannel(name) => {
                let Some(at) = state.channels.iter().position(|(n, _)| *n == name) else {
                    return;
                };
                let (_, id) = state.channels.remove(at);
                let id = id.borrow().clone();
                match id {
                    Some(id) => webview.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id)),
                    None => Ok(()),
                }
            }
            Call::SetJavascript(on) => webview
                .Settings()
                .and_then(|settings| settings.SetIsScriptEnabled(on)),
            Call::SetUserAgent(agent) => {
                let agent = agent.or_else(|| state.default_agent.clone());
                match agent {
                    Some(agent) => webview
                        .Settings()
                        .and_then(|settings| settings.cast::<ICoreWebView2Settings2>())
                        .and_then(|settings| settings.SetUserAgent(&HSTRING::from(agent))),
                    None => Ok(()),
                }
            }
            Call::SetBackground([r, g, b, a]) => {
                // WebView2 is either opaque or see-through.
                let a = if a == 0 { 0 } else { 255 };
                controller
                    .cast::<ICoreWebView2Controller2>()
                    .and_then(|controller| {
                        controller.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR {
                            A: a,
                            R: r,
                            G: g,
                            B: b,
                        })
                    })
            }
            Call::SetZoom(on) => webview.Settings().and_then(|settings| {
                settings.SetIsZoomControlEnabled(on)?;
                if let Ok(settings) = settings.cast::<ICoreWebView2Settings5>() {
                    settings.SetIsPinchZoomEnabled(on)?;
                }
                Ok(())
            }),
            Call::SetDecidesNavigation(on) => {
                state.gate.decides = on;
                Ok(())
            }
            Call::ClearCache => clear(&webview, COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE),
            Call::ClearLocalStorage => {
                clear(&webview, COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_DOM_STORAGE)
            }
        }
    };
    if let Err(err) = result {
        log::warn!("web view {}: a call failed ({err})", view.get());
    }
}

/// Clears `kinds` of the profile's browsing data (every web view's, as on Android).
unsafe fn clear(
    webview: &ICoreWebView2,
    kinds: COREWEBVIEW2_BROWSING_DATA_KINDS,
) -> windows::core::Result<()> {
    webview
        .cast::<ICoreWebView2_13>()?
        .Profile()?
        .cast::<ICoreWebView2Profile2>()?
        .ClearBrowsingData(
            kinds,
            &ClearBrowsingDataCompletedHandler::create(Box::new(|_| Ok(()))),
        )
}
