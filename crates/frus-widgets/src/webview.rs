// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **Web views** (milestones 799 and 804): a web page inside a frus page, driven from Rust,
//! with one API on Android, Windows and the web.
//!
//! - [`WebViewController`] loads pages ([`load_request`](WebViewController::load_request),
//!   [`load_html_string`](WebViewController::load_html_string), files and assets), moves in
//!   the history, runs JavaScript (with or without its result), opens JavaScript channels the
//!   page can post messages to, and holds what the page last said: its address, its title,
//!   its progress, whether it can go back.
//! - [`NavigationDelegate`] hears the page's life: started, progress, finished, errors, a
//!   change of address, and decides each navigation the page asks for.
//! - [`WebViewWidget`] shows the controller's page where it is laid out.
//! - [`WebViewCookieManager`] sets and clears the cookies every web view shares.
//!
//! The controller owns the page, not the widget: a widget rebuilt somewhere else — a page
//! pushed over it and popped, a tab switched away and back — shows the same page, with its
//! history and its scroll. The page goes when the last copy of the controller is dropped.
//!
//! Underneath it is a native view ([`PlatformViewSpec`] of type [`WEB_VIEW_TYPE`]): Android's
//! `WebView`, drawn into the frame; WebView2 on Windows; an `<iframe>` on the web, which can
//! do only what a browser lets a page do with a frame (see [`WebViewError::Unsupported`]).
//! On macOS, Linux and iOS the widget shows its fallback, and the calls that need an answer
//! answer [`WebViewError::Unsupported`] ([`web_views_available`]).
//!
//! The controller talks to the view only through [`PlatformViewController::invoke`] and the
//! view's events, with the names listed in [`web_view_wire`]: any host that makes a
//! [`WEB_VIEW_TYPE`] view and speaks them drives the same controller.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll, Waker};

use frus_core::{Color, FilterQuality, Rect, Scene, SemanticsProperties};
use frus_layout::{Dimension, Style};

use crate::interaction::Status;
use crate::notifier::{ChangeNotifier, Listenable, Subscription};
use crate::platformview::{
    platform_view_frame, PlatformViewController, PlatformViewEvent, PlatformViewSpec,
    UnsupportedView,
};
use crate::theme::Theme;
use crate::widget::Widget;

/// The height a [`WebViewWidget`] given none starts from, before it takes the room it is given.
pub const WEB_VIEW_DEFAULT_HEIGHT: f32 = 300.0;

/// The native view type every web view is made as.
pub const WEB_VIEW_TYPE: &str = "frus/webview";

#[cfg(test)]
thread_local! {
    /// A test's stand-in for the platform: whether it has web views, and whether they are a
    /// browser's frames.
    static TEST_PLATFORM: Cell<Option<(bool, bool)>> = const { Cell::new(None) };
}

/// Whether web views are shown here: on Android, on Windows (the shell registers WebView2 at
/// start-up) and on the web. On macOS, Linux and iOS a [`WebViewWidget`] shows its fallback.
///
/// ```
/// use frus_widgets::web_views_available;
///
/// if !web_views_available() {
///     // A link to open in the browser instead, say.
/// }
/// ```
pub fn web_views_available() -> bool {
    #[cfg(test)]
    if let Some((available, _)) = TEST_PLATFORM.with(Cell::get) {
        return available;
    }
    crate::platformview::platform_view_type_available(WEB_VIEW_TYPE)
}

/// Whether the web views are a browser's frames (the web), which cannot do everything.
fn in_a_frame() -> bool {
    #[cfg(test)]
    if let Some((_, frame)) = TEST_PLATFORM.with(Cell::get) {
        return frame;
    }
    cfg!(target_arch = "wasm32")
}

/// `Ok` where the platform's web views can do what was asked, [`WebViewError::Unsupported`]
/// where they cannot (`frame_can`: whether a browser's frame can).
fn can(frame_can: bool) -> Result<(), WebViewError> {
    if web_views_available() && (frame_can || !in_a_frame()) {
        Ok(())
    } else {
        Err(WebViewError::Unsupported)
    }
}

static NEXT_CONTROLLER: AtomicU64 = AtomicU64::new(1);
static NEXT_RESULT: AtomicU64 = AtomicU64::new(1);

/// Why a web view could not do what it was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WebViewError {
    /// The platform's web views cannot do it. On macOS, Linux and iOS there are none (see
    /// [`web_views_available`]). On the web a web view is a browser's frame, which has no
    /// user agent, zoom setting, cache, storage or cookies of its own, and runs no script
    /// for the page around it in a page of another origin.
    Unsupported,
    /// The script threw, or the page would not run it (on the web, a page from another
    /// origin keeps its scripts to itself).
    JavaScript(String),
    /// A JavaScript channel's name must be a JavaScript identifier.
    InvalidChannelName(String),
    /// A channel of that name is open already.
    ChannelExists(String),
}

impl std::fmt::Display for WebViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebViewError::Unsupported => f.write_str("the platform's web views cannot do this"),
            WebViewError::JavaScript(why) => write!(f, "the script failed: {why}"),
            WebViewError::InvalidChannelName(name) => {
                write!(f, "{name:?} is not a JavaScript identifier")
            }
            WebViewError::ChannelExists(name) => write!(f, "a channel named {name:?} is open"),
        }
    }
}

impl std::error::Error for WebViewError {}

/// Whether the page may run JavaScript ([`WebViewController::set_javascript_mode`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JavaScriptMode {
    /// Scripts run (the default).
    #[default]
    Unrestricted,
    /// No script runs. Channels and [`run_javascript`](WebViewController::run_javascript)
    /// need scripts.
    Disabled,
}

/// The HTTP method of a [`WebRequest`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WebMethod {
    /// `GET` (the default).
    #[default]
    Get,
    /// `POST`, with the request's body.
    Post,
}

/// A page to load: an address, and optionally a method, headers and a body.
///
/// ```
/// use frus_widgets::WebRequest;
///
/// let plain: WebRequest = "https://example.com".into();
/// let signed = WebRequest::new("https://example.com/me").header("Authorization", "Bearer t");
/// let form = WebRequest::post("https://example.com/search", b"q=frus".to_vec());
/// assert_eq!(plain.url(), "https://example.com");
/// assert_eq!(signed.headers().len(), 1);
/// assert_eq!(form.body(), b"q=frus");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WebRequest {
    url: String,
    method: WebMethod,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl WebRequest {
    /// A `GET` of `url`.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    /// A `POST` of `body` to `url`.
    pub fn post(url: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            url: url.into(),
            method: WebMethod::Post,
            body: body.into(),
            ..Self::default()
        }
    }

    /// One more header. Android sends the headers of a `GET` only (its `postUrl` takes none).
    /// On the web, a request with headers or a body is fetched by the page and its HTML
    /// shown in the frame, which the server must allow (CORS).
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// The address.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The method.
    pub fn method(&self) -> WebMethod {
        self.method
    }

    /// The headers, in order.
    pub fn headers(&self) -> &[(String, String)] {
        &self.headers
    }

    /// The body (empty for a `GET`).
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

impl From<&str> for WebRequest {
    fn from(url: &str) -> Self {
        WebRequest::new(url)
    }
}

impl From<String> for WebRequest {
    fn from(url: String) -> Self {
        WebRequest::new(url)
    }
}

/// A message a page posted to a [JavaScript channel](WebViewController::add_javascript_channel):
/// `Name.postMessage("…")` in the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JavaScriptMessage {
    /// The channel's name.
    pub channel: String,
    /// What the page posted.
    pub message: String,
}

/// A navigation the page asked for, for [`NavigationDelegate::on_navigation_request`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationRequest {
    /// Where it goes.
    pub url: String,
    /// Whether the whole page goes there, rather than a frame inside it.
    pub is_main_frame: bool,
}

/// What [`NavigationDelegate::on_navigation_request`] decides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NavigationDecision {
    /// The page goes there.
    #[default]
    Navigate,
    /// The page stays where it is.
    Prevent,
}

/// What kind of failure a [`WebResourceError`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WebResourceErrorType {
    /// The server's name could not be found.
    HostLookup,
    /// The server could not be reached.
    Connect,
    /// The server took too long.
    Timeout,
    /// The address could not be understood or its scheme is not supported.
    BadUrl,
    /// A secure connection could not be made.
    FailedSslHandshake,
    /// The file could not be found or read.
    File,
    /// Too many redirects.
    RedirectLoop,
    /// The load was cancelled.
    Cancelled,
    /// Anything else.
    Unknown,
}

impl WebResourceErrorType {
    /// The type of an Android `WebViewClient.ERROR_*` code.
    fn from_android(code: i64) -> Self {
        match code {
            -2 => Self::HostLookup,
            -6 => Self::Connect,
            -8 => Self::Timeout,
            -12 | -10 => Self::BadUrl,
            -11 => Self::FailedSslHandshake,
            -13 | -14 => Self::File,
            -9 => Self::RedirectLoop,
            _ => Self::Unknown,
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "hostLookup" => Self::HostLookup,
            "connect" => Self::Connect,
            "timeout" => Self::Timeout,
            "badUrl" => Self::BadUrl,
            "ssl" => Self::FailedSslHandshake,
            "file" => Self::File,
            "redirectLoop" => Self::RedirectLoop,
            "cancelled" => Self::Cancelled,
            "unknown" => Self::Unknown,
            _ => return None,
        })
    }
}

/// A page or a resource of it that could not be loaded, for
/// [`NavigationDelegate::on_web_resource_error`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebResourceError {
    /// The platform's own code.
    pub error_code: i64,
    /// What the platform said.
    pub description: String,
    /// What kind of failure.
    pub error_type: WebResourceErrorType,
    /// Whether it was the page itself, rather than an image or a script of it.
    pub is_for_main_frame: bool,
    /// The address that failed, when the platform says.
    pub url: Option<String>,
}

/// An HTTP error status the server answered, for [`NavigationDelegate::on_http_error`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponseError {
    /// The status (`404`, `500`…).
    pub status: u16,
    /// The address, when the platform says.
    pub url: Option<String>,
}

/// The page's address changed, for [`NavigationDelegate::on_url_change`] — a navigation, or a
/// script's `history.pushState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UrlChange {
    /// The new address.
    pub url: Option<String>,
}

type Hook<T> = Option<Rc<T>>;

/// What a web view tells the application about its page: each hook is optional.
///
/// Set it with [`WebViewController::set_navigation_delegate`]. The hooks run on the
/// interface's thread, between frames: a component changes its state from them through its
/// [`StateHandle`](crate::StateHandle).
///
/// ```
/// use frus_widgets::{NavigationDecision, NavigationDelegate};
///
/// let delegate = NavigationDelegate::new()
///     .on_page_started(|url| println!("loading {url}"))
///     .on_progress(|percent| println!("{percent}%"))
///     .on_page_finished(|url| println!("loaded {url}"))
///     .on_navigation_request(|request| {
///         if request.url.starts_with("https://ads.") {
///             NavigationDecision::Prevent
///         } else {
///             NavigationDecision::Navigate
///         }
///     });
/// assert!(delegate.decides_navigation());
/// ```
#[derive(Clone, Default)]
pub struct NavigationDelegate {
    navigation_request: Hook<dyn Fn(&NavigationRequest) -> NavigationDecision>,
    page_started: Hook<dyn Fn(&str)>,
    page_finished: Hook<dyn Fn(&str)>,
    progress: Hook<dyn Fn(u8)>,
    resource_error: Hook<dyn Fn(&WebResourceError)>,
    http_error: Hook<dyn Fn(&HttpResponseError)>,
    url_change: Hook<dyn Fn(&UrlChange)>,
}

impl NavigationDelegate {
    /// A delegate that hears nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Decides each navigation of the page itself (not of a frame inside it), including a
    /// link the user follows. Without it, every navigation goes ahead.
    ///
    /// The decision is made between frames, so the view first holds the navigation back and
    /// then makes it if it goes ahead: a navigation that goes ahead is a new load of its
    /// address (a `POST`'s body is not sent again).
    pub fn on_navigation_request(
        mut self,
        decide: impl Fn(&NavigationRequest) -> NavigationDecision + 'static,
    ) -> Self {
        self.navigation_request = Some(Rc::new(decide));
        self
    }

    /// A page started loading, with its address.
    pub fn on_page_started(mut self, hear: impl Fn(&str) + 'static) -> Self {
        self.page_started = Some(Rc::new(hear));
        self
    }

    /// A page finished loading, with its address.
    pub fn on_page_finished(mut self, hear: impl Fn(&str) + 'static) -> Self {
        self.page_finished = Some(Rc::new(hear));
        self
    }

    /// How far the page has loaded, from 0 to 100.
    pub fn on_progress(mut self, hear: impl Fn(u8) + 'static) -> Self {
        self.progress = Some(Rc::new(hear));
        self
    }

    /// The page, or a resource of it, could not be loaded.
    pub fn on_web_resource_error(mut self, hear: impl Fn(&WebResourceError) + 'static) -> Self {
        self.resource_error = Some(Rc::new(hear));
        self
    }

    /// The server answered the page itself with an HTTP error status.
    pub fn on_http_error(mut self, hear: impl Fn(&HttpResponseError) + 'static) -> Self {
        self.http_error = Some(Rc::new(hear));
        self
    }

    /// The page's address changed.
    pub fn on_url_change(mut self, hear: impl Fn(&UrlChange) + 'static) -> Self {
        self.url_change = Some(Rc::new(hear));
        self
    }

    /// Whether the delegate decides navigations (the view then asks it about each one).
    pub fn decides_navigation(&self) -> bool {
        self.navigation_request.is_some()
    }
}

impl std::fmt::Debug for NavigationDelegate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavigationDelegate")
            .field("decides_navigation", &self.decides_navigation())
            .finish_non_exhaustive()
    }
}

/// The names a web view host speaks: the methods the controller
/// [invokes](PlatformViewController::invoke) and the events it hears. Arguments and event
/// data are JSON (a string, a bool, or an object with the fields named here).
///
/// A host makes a view of type [`WEB_VIEW_TYPE`] from creation parameters that are a JSON
/// object: `controller` (a number: the same number is the same page, which a host keeps
/// while the controller lives and hands to the next widget that shows it), `javascript`
/// (bool), `userAgent` (string or null), `background` (`"#rrggbbaa"` or null), `zoom` (bool),
/// `channels` (an array of names) and `decidesNavigation` (bool).
pub mod web_view_wire {
    /// Load a request: `{"url", "method": "GET"|"POST", "headers": {name: value}, "body":
    /// base64}`.
    pub const LOAD_REQUEST: &str = "web.loadRequest";
    /// Load HTML: `{"html", "baseUrl": string or null}`.
    pub const LOAD_HTML: &str = "web.loadHtml";
    /// Load a local file: the path, a string.
    pub const LOAD_FILE: &str = "web.loadFile";
    /// Load a file packaged with the application: its path under `assets/`, a string.
    pub const LOAD_ASSET: &str = "web.loadAsset";
    /// Load the page again.
    pub const RELOAD: &str = "web.reload";
    /// Go back in the history.
    pub const GO_BACK: &str = "web.goBack";
    /// Go forward in the history.
    pub const GO_FORWARD: &str = "web.goForward";
    /// Run a script: the script, a string.
    pub const RUN_JAVASCRIPT: &str = "web.runJavascript";
    /// Run a script and answer [`JAVASCRIPT_RESULT`]: `{"id", "script"}`.
    pub const RUN_JAVASCRIPT_FOR_RESULT: &str = "web.runJavascriptForResult";
    /// Open a channel: its name, a string.
    pub const ADD_CHANNEL: &str = "web.addChannel";
    /// Close a channel: its name, a string.
    pub const REMOVE_CHANNEL: &str = "web.removeChannel";
    /// Scripts on (`true`) or off: a bool.
    pub const SET_JAVASCRIPT: &str = "web.setJavascript";
    /// The user agent: a string, or null for the platform's own.
    pub const SET_USER_AGENT: &str = "web.setUserAgent";
    /// The colour behind the page: `"#rrggbbaa"`.
    pub const SET_BACKGROUND: &str = "web.setBackground";
    /// Pinch zoom on or off: a bool.
    pub const SET_ZOOM: &str = "web.setZoom";
    /// Whether the controller decides navigations: a bool.
    pub const SET_DECIDES_NAVIGATION: &str = "web.setDecidesNavigation";
    /// Clear the cache (all web views').
    pub const CLEAR_CACHE: &str = "web.clearCache";
    /// Clear the local storage (all web views').
    pub const CLEAR_LOCAL_STORAGE: &str = "web.clearLocalStorage";

    /// A page started: `{"url"}`.
    pub const PAGE_STARTED: &str = "web.pageStarted";
    /// A page finished: `{"url"}`.
    pub const PAGE_FINISHED: &str = "web.pageFinished";
    /// Progress: `{"progress": 0..=100}`.
    pub const PROGRESS: &str = "web.progress";
    /// The history and the page's names: `{"url", "title", "canGoBack", "canGoForward"}`.
    pub const STATE: &str = "web.state";
    /// The address changed: `{"url"}`.
    pub const URL_CHANGE: &str = "web.urlChange";
    /// A load failed: `{"code", "type", "description", "mainFrame", "url"}`; `type` is one of
    /// `hostLookup`, `connect`, `timeout`, `badUrl`, `ssl`, `file`, `redirectLoop`,
    /// `cancelled`, `unknown` (absent: read from Android's code).
    pub const RESOURCE_ERROR: &str = "web.resourceError";
    /// An HTTP error status: `{"status", "url"}`.
    pub const HTTP_ERROR: &str = "web.httpError";
    /// A navigation to decide, held back by the view: `{"url", "mainFrame"}`.
    pub const NAVIGATION_REQUEST: &str = "web.navigationRequest";
    /// A message on a channel: `{"channel", "message"}`.
    pub const CHANNEL_MESSAGE: &str = "web.channelMessage";
    /// A script's result: `{"id", "ok", "value"}` (`value` is the result as JSON, or why it
    /// failed).
    pub const JAVASCRIPT_RESULT: &str = "web.javascriptResult";

    /// Set a cookie (for every view): `{"url", "name", "value", "domain", "path"}`.
    pub const SET_COOKIE: &str = "web.setCookie";
    /// Clear every cookie, answered by [`COOKIES_CLEARED`]: `{"id"}`.
    pub const CLEAR_COOKIES: &str = "web.clearCookies";
    /// The answer to [`CLEAR_COOKIES`]: `{"id", "had": bool}`.
    pub const COOKIES_CLEARED: &str = "web.cookiesCleared";
    /// The controller of a page was dropped: the page goes. `{"controller"}`.
    pub const RELEASE: &str = "web.release";
}

/// A web view's settings, kept so that a view made later starts with them.
#[derive(Clone, Debug)]
struct Settings {
    javascript: JavaScriptMode,
    user_agent: Option<String>,
    background: Option<Color>,
    zoom: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            javascript: JavaScriptMode::Unrestricted,
            user_agent: None,
            background: None,
            zoom: true,
        }
    }
}

/// What the page last said.
#[derive(Clone, Debug, Default)]
struct PageState {
    url: Option<String>,
    title: Option<String>,
    progress: u8,
    loading: bool,
    can_go_back: bool,
    can_go_forward: bool,
}

/// One answer to wait for.
struct Slot<T> {
    value: RefCell<Option<T>>,
    waker: RefCell<Option<Waker>>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            value: RefCell::new(None),
            waker: RefCell::new(None),
        }
    }
}

impl<T> Slot<T> {
    fn ready(value: T) -> Rc<Self> {
        Rc::new(Slot {
            value: RefCell::new(Some(value)),
            waker: RefCell::new(None),
        })
    }

    fn fill(&self, value: T) {
        *self.value.borrow_mut() = Some(value);
        if let Some(waker) = self.waker.borrow_mut().take() {
            waker.wake();
        }
    }

    fn poll(&self, cx: &mut Context<'_>) -> Poll<T> {
        match self.value.borrow_mut().take() {
            Some(value) => Poll::Ready(value),
            None => {
                *self.waker.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// The result of [`WebViewController::run_javascript_returning_result`]: the script's value
/// as JSON (a string comes back quoted: `"\"Title\""`), once the page has run it.
///
/// Await it in a task ([`spawn_local`](crate::spawn_local), a state's
/// [`spawn`](crate::StateHandle::spawn)).
#[must_use = "the result arrives only when awaited"]
pub struct JavaScriptResult {
    slot: Rc<Slot<Result<String, WebViewError>>>,
}

impl Future for JavaScriptResult {
    type Output = Result<String, WebViewError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.slot.poll(cx)
    }
}

type Channel = (String, Rc<dyn Fn(JavaScriptMessage)>);
/// Where a script's result waits for its future.
type ResultSlot = Rc<Slot<Result<String, WebViewError>>>;

struct Inner {
    id: u64,
    view: PlatformViewController,
    settings: RefCell<Settings>,
    page: RefCell<PageState>,
    delegate: RefCell<NavigationDelegate>,
    channels: RefCell<Vec<Channel>>,
    results: RefCell<HashMap<u64, ResultSlot>>,
    changed: ChangeNotifier,
}

impl Drop for Inner {
    fn drop(&mut self) {
        // The page lives as long as its controller: the host lets it go.
        if web_views_available() {
            push_global(
                web_view_wire::RELEASE,
                format!("{{\"controller\":{}}}", self.id),
            );
        }
    }
}

/// Drives a web view: what it loads, its history, its scripts, its channels, its settings, and
/// what its page last said. Keep one in the state of the page that shows it, hand it to a
/// [`WebViewWidget`], and call it from anywhere on the interface's thread: calls made before
/// the view exists wait for it, in order.
///
/// It is [`Listenable`]: it notifies (and asks for a rebuild) whenever the page's address,
/// title, progress or history changes, so a tree that reads
/// [`current_url`](Self::current_url) or [`progress`](Self::progress) while it is built
/// shows the change.
///
/// ```
/// use frus_widgets::{JavaScriptMode, NavigationDelegate, WebRequest, WebViewController};
///
/// let web = WebViewController::new();
/// web.set_javascript_mode(JavaScriptMode::Unrestricted);
/// web.set_navigation_delegate(NavigationDelegate::new().on_progress(|p| println!("{p}%")));
/// web.add_javascript_channel("Toaster", |message| println!("{}", message.message))
///     .unwrap();
/// web.load_request(WebRequest::new("https://example.com").header("Accept-Language", "en"));
/// assert_eq!(web.current_url(), None, "nothing has loaded yet");
/// ```
#[derive(Clone)]
pub struct WebViewController {
    inner: Rc<Inner>,
}

/// A [`WebViewController`] that does not keep its page alive
/// ([`WebViewController::downgrade`]).
#[derive(Clone, Debug, Default)]
pub struct WeakWebViewController {
    inner: std::rc::Weak<Inner>,
}

impl WeakWebViewController {
    /// The controller, while some copy of it lives.
    pub fn upgrade(&self) -> Option<WebViewController> {
        self.inner
            .upgrade()
            .map(|inner| WebViewController { inner })
    }
}

impl Default for WebViewController {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for WebViewController {
    /// Two controllers are equal when they are the same one.
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl std::fmt::Debug for WebViewController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebViewController")
            .field("id", &self.inner.id)
            .field("url", &self.inner.page.borrow().url)
            .finish_non_exhaustive()
    }
}

impl Listenable for WebViewController {
    fn add_listener(&self, listener: impl Fn() + 'static) -> Subscription {
        self.inner.changed.add_listener(listener)
    }
}

fn json(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

fn hex(color: Color) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        byte(color.r),
        byte(color.g),
        byte(color.b),
        byte(color.a)
    )
}

/// The standard base64 of `bytes` (a request's body travels as text).
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

impl WebViewController {
    /// A controller with no page yet.
    pub fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                id: NEXT_CONTROLLER.fetch_add(1, Ordering::Relaxed),
                view: PlatformViewController::new(),
                settings: RefCell::default(),
                page: RefCell::default(),
                delegate: RefCell::default(),
                channels: RefCell::default(),
                results: RefCell::default(),
                changed: ChangeNotifier::new(),
            }),
        }
    }

    fn call(&self, method: &str, arguments: String) {
        self.inner.view.invoke(method, arguments);
    }

    /// A handle that does not keep the page alive: what a channel's or a delegate's hook
    /// that calls the controller back holds, since the controller holds the hook.
    ///
    /// ```
    /// use frus_widgets::WebViewController;
    ///
    /// let web = WebViewController::new();
    /// let weak = web.downgrade();
    /// web.add_javascript_channel("Ping", move |_| {
    ///     if let Some(web) = weak.upgrade() {
    ///         web.run_javascript("pong()");
    ///     }
    /// })
    /// .unwrap();
    /// ```
    pub fn downgrade(&self) -> WeakWebViewController {
        WeakWebViewController {
            inner: Rc::downgrade(&self.inner),
        }
    }

    /// Loads `request`: an address (`"https://…"`), or a [`WebRequest`] with headers, or a
    /// `POST`.
    pub fn load_request(&self, request: impl Into<WebRequest>) {
        let request = request.into();
        let headers: serde_json::Map<String, serde_json::Value> = request
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        let method = match request.method {
            WebMethod::Get => "GET",
            WebMethod::Post => "POST",
        };
        let value = serde_json::json!({
            "url": request.url,
            "method": method,
            "headers": headers,
            "body": base64(&request.body),
        });
        self.call(web_view_wire::LOAD_REQUEST, value.to_string());
    }

    /// Shows `html`. Relative addresses in it resolve against `base_url`, which is also the
    /// page's origin for its scripts.
    pub fn load_html_string(&self, html: impl Into<String>, base_url: Option<&str>) {
        let value = serde_json::json!({ "html": html.into(), "baseUrl": base_url });
        self.call(web_view_wire::LOAD_HTML, value.to_string());
    }

    /// Loads a file of the device: an absolute path.
    pub fn load_file(&self, path: impl AsRef<std::path::Path>) {
        let path = path.as_ref().to_string_lossy().into_owned();
        self.call(web_view_wire::LOAD_FILE, json(&path));
    }

    /// Loads a file packaged with the application, by its path under the application's
    /// `assets/` folder (`"help/index.html"`). On Android these are the APK's assets.
    pub fn load_asset(&self, key: impl Into<String>) {
        self.call(web_view_wire::LOAD_ASSET, json(&key.into()));
    }

    /// Loads the page again.
    pub fn reload(&self) {
        self.call(web_view_wire::RELOAD, String::new());
    }

    /// Goes back in the history, if it can.
    pub fn go_back(&self) {
        self.call(web_view_wire::GO_BACK, String::new());
    }

    /// Goes forward in the history, if it can.
    pub fn go_forward(&self) {
        self.call(web_view_wire::GO_FORWARD, String::new());
    }

    /// Whether there is a page to go back to.
    pub fn can_go_back(&self) -> bool {
        self.inner.page.borrow().can_go_back
    }

    /// Whether there is a page to go forward to.
    pub fn can_go_forward(&self) -> bool {
        self.inner.page.borrow().can_go_forward
    }

    /// The page's address, once one has started loading.
    pub fn current_url(&self) -> Option<String> {
        self.inner.page.borrow().url.clone()
    }

    /// The page's title, once it has one.
    pub fn title(&self) -> Option<String> {
        self.inner
            .page
            .borrow()
            .title
            .clone()
            .filter(|t| !t.is_empty())
    }

    /// How far the page has loaded, from 0 to 100.
    pub fn progress(&self) -> u8 {
        self.inner.page.borrow().progress
    }

    /// Whether a page is loading.
    pub fn is_loading(&self) -> bool {
        self.inner.page.borrow().loading
    }

    /// Runs `script` in the page, for its effect.
    pub fn run_javascript(&self, script: impl Into<String>) {
        self.call(web_view_wire::RUN_JAVASCRIPT, json(&script.into()));
    }

    /// Runs `script` in the page and gives back its value, as JSON.
    ///
    /// ```no_run
    /// use frus_widgets::{spawn_local, WebViewController};
    ///
    /// let web = WebViewController::new();
    /// let asked = web.run_javascript_returning_result("document.title");
    /// let _task = spawn_local(async move {
    ///     if let Ok(title) = asked.await {
    ///         println!("the page is called {title}");
    ///     }
    /// });
    /// ```
    ///
    /// On the web it answers [`WebViewError::Unsupported`] for a page of another origin, whose
    /// scripts the browser keeps from the page around it.
    pub fn run_javascript_returning_result(&self, script: impl Into<String>) -> JavaScriptResult {
        if !web_views_available() {
            return JavaScriptResult {
                slot: Slot::ready(Err(WebViewError::Unsupported)),
            };
        }
        let id = NEXT_RESULT.fetch_add(1, Ordering::Relaxed);
        let slot: Rc<Slot<Result<String, WebViewError>>> = Rc::default();
        self.inner.results.borrow_mut().insert(id, slot.clone());
        let value = serde_json::json!({ "id": id, "script": script.into() });
        self.call(web_view_wire::RUN_JAVASCRIPT_FOR_RESULT, value.to_string());
        JavaScriptResult { slot }
    }

    /// Opens a channel the page posts messages to: `name.postMessage("…")` in the page calls
    /// `on_message`. The page sees the channel from its next load.
    pub fn add_javascript_channel(
        &self,
        name: impl Into<String>,
        on_message: impl Fn(JavaScriptMessage) + 'static,
    ) -> Result<(), WebViewError> {
        let name = name.into();
        if !is_identifier(&name) {
            return Err(WebViewError::InvalidChannelName(name));
        }
        {
            let mut channels = self.inner.channels.borrow_mut();
            if channels.iter().any(|(n, _)| *n == name) {
                return Err(WebViewError::ChannelExists(name));
            }
            channels.push((name.clone(), Rc::new(on_message)));
        }
        self.call(web_view_wire::ADD_CHANNEL, json(&name));
        Ok(())
    }

    /// Closes the channel `name` (from the page's next load).
    pub fn remove_javascript_channel(&self, name: &str) {
        let removed = {
            let mut channels = self.inner.channels.borrow_mut();
            let before = channels.len();
            channels.retain(|(n, _)| n != name);
            channels.len() != before
        };
        if removed {
            self.call(web_view_wire::REMOVE_CHANNEL, json(&name));
        }
    }

    /// Whether the page may run scripts.
    pub fn set_javascript_mode(&self, mode: JavaScriptMode) {
        self.inner.settings.borrow_mut().javascript = mode;
        self.call(
            web_view_wire::SET_JAVASCRIPT,
            json(&(mode == JavaScriptMode::Unrestricted)),
        );
    }

    /// The user agent the view sends; `None` for the platform's own.
    ///
    /// [`WebViewError::Unsupported`] on the web for one of its own: a frame sends the
    /// browser's.
    pub fn set_user_agent(&self, user_agent: Option<&str>) -> Result<(), WebViewError> {
        can(user_agent.is_none())?;
        self.inner.settings.borrow_mut().user_agent = user_agent.map(str::to_owned);
        self.call(web_view_wire::SET_USER_AGENT, json(&user_agent));
        Ok(())
    }

    /// The user agent set with [`set_user_agent`](Self::set_user_agent).
    pub fn user_agent(&self) -> Option<String> {
        self.inner.settings.borrow().user_agent.clone()
    }

    /// The colour behind the page, seen where the page draws none (and before it has
    /// drawn).
    pub fn set_background_color(&self, color: Color) {
        self.inner.settings.borrow_mut().background = Some(color);
        self.call(web_view_wire::SET_BACKGROUND, json(&hex(color)));
    }

    /// Whether the page can be zoomed with two fingers, and on Windows with Ctrl and the wheel
    /// (on by default).
    ///
    /// [`WebViewError::Unsupported`] on the web for turning it off: a frame zooms with the
    /// browser.
    pub fn enable_zoom(&self, enabled: bool) -> Result<(), WebViewError> {
        can(enabled)?;
        self.inner.settings.borrow_mut().zoom = enabled;
        self.call(web_view_wire::SET_ZOOM, json(&enabled));
        Ok(())
    }

    /// Clears the cache of every web view of the application.
    ///
    /// [`WebViewError::Unsupported`] on the web: the cache is the browser's.
    pub fn clear_cache(&self) -> Result<(), WebViewError> {
        can(false)?;
        self.call(web_view_wire::CLEAR_CACHE, String::new());
        Ok(())
    }

    /// Clears the local storage of every web view of the application.
    ///
    /// [`WebViewError::Unsupported`] on the web: a page of the application's origin shares
    /// the application's own storage, and another origin's is the browser's.
    pub fn clear_local_storage(&self) -> Result<(), WebViewError> {
        can(false)?;
        self.call(web_view_wire::CLEAR_LOCAL_STORAGE, String::new());
        Ok(())
    }

    /// What the view tells the application about its page, replacing the delegate set
    /// before.
    pub fn set_navigation_delegate(&self, delegate: NavigationDelegate) {
        let decides = delegate.decides_navigation();
        let before = self.inner.delegate.borrow().decides_navigation();
        *self.inner.delegate.borrow_mut() = delegate;
        if decides != before {
            self.call(web_view_wire::SET_DECIDES_NAVIGATION, json(&decides));
        }
    }

    /// The native view's controller underneath, for a widget of an application's own that
    /// shows this page ([`PlatformViewSpec::with_controller`]).
    pub fn platform_view_controller(&self) -> &PlatformViewController {
        &self.inner.view
    }

    /// What a view is made with: the page's identity and the settings so far (see
    /// [`web_view_wire`]).
    pub fn creation_params(&self) -> String {
        let settings = self.inner.settings.borrow();
        let channels: Vec<String> = self
            .inner
            .channels
            .borrow()
            .iter()
            .map(|(n, _)| n.clone())
            .collect();
        serde_json::json!({
            "controller": self.inner.id,
            "javascript": settings.javascript == JavaScriptMode::Unrestricted,
            "userAgent": settings.user_agent,
            "background": settings.background.map(hex),
            "zoom": settings.zoom,
            "channels": channels,
            "decidesNavigation": self.inner.delegate.borrow().decides_navigation(),
        })
        .to_string()
    }

    fn changed(&self) {
        // The shell takes the system Back to the page while it has somewhere to go back to.
        self.inner.view.set_handles_back(self.can_go_back());
        self.inner.changed.notify_listeners();
    }

    /// Takes in an event of the view: the page's state, the delegate's hooks, a channel's
    /// message, a script's result. [`WebViewWidget`] calls it; a widget of an application's
    /// own that shows this page calls it from its
    /// [`on_platform_view_event`](Widget::on_platform_view_event).
    pub fn handle_event(&self, event: &PlatformViewEvent) {
        let name = event.name.as_str();
        if !name.starts_with("web.") {
            return;
        }
        let data: serde_json::Value =
            serde_json::from_str(&event.data).unwrap_or(serde_json::Value::Null);
        let text = |key: &str| data.get(key).and_then(|v| v.as_str()).map(str::to_owned);
        let flag = |key: &str| data.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
        let number = |key: &str| data.get(key).and_then(|v| v.as_i64());
        // Cloned out first: a hook may set another delegate.
        let delegate = self.inner.delegate.borrow().clone();
        match name {
            web_view_wire::PAGE_STARTED | web_view_wire::PAGE_FINISHED => {
                let started = name == web_view_wire::PAGE_STARTED;
                let url = text("url").unwrap_or_default();
                {
                    let mut page = self.inner.page.borrow_mut();
                    page.loading = started;
                    page.progress = if started { 0 } else { 100 };
                    page.url = Some(url.clone());
                }
                self.changed();
                let hook = if started {
                    &delegate.page_started
                } else {
                    &delegate.page_finished
                };
                if let Some(hook) = hook {
                    hook(&url);
                }
            }
            web_view_wire::PROGRESS => {
                let progress = number("progress").unwrap_or(0).clamp(0, 100) as u8;
                self.inner.page.borrow_mut().progress = progress;
                self.changed();
                if let Some(hook) = &delegate.progress {
                    hook(progress);
                }
            }
            web_view_wire::STATE => {
                {
                    let mut page = self.inner.page.borrow_mut();
                    if let Some(url) = text("url") {
                        page.url = Some(url);
                    }
                    if let Some(title) = text("title") {
                        page.title = Some(title);
                    }
                    page.can_go_back = flag("canGoBack");
                    page.can_go_forward = flag("canGoForward");
                }
                self.changed();
            }
            web_view_wire::URL_CHANGE => {
                let url = text("url");
                if let Some(url) = &url {
                    self.inner.page.borrow_mut().url = Some(url.clone());
                }
                self.changed();
                if let Some(hook) = &delegate.url_change {
                    hook(&UrlChange { url });
                }
            }
            web_view_wire::RESOURCE_ERROR => {
                let code = number("code").unwrap_or(0);
                let error = WebResourceError {
                    error_code: code,
                    description: text("description").unwrap_or_default(),
                    error_type: text("type")
                        .and_then(|t| WebResourceErrorType::from_name(&t))
                        .unwrap_or_else(|| WebResourceErrorType::from_android(code)),
                    is_for_main_frame: flag("mainFrame"),
                    url: text("url"),
                };
                if error.is_for_main_frame {
                    self.inner.page.borrow_mut().loading = false;
                    self.changed();
                }
                if let Some(hook) = &delegate.resource_error {
                    hook(&error);
                }
            }
            web_view_wire::HTTP_ERROR => {
                if let Some(hook) = &delegate.http_error {
                    hook(&HttpResponseError {
                        status: number("status").unwrap_or(0).clamp(0, 999) as u16,
                        url: text("url"),
                    });
                }
            }
            web_view_wire::NAVIGATION_REQUEST => {
                let request = NavigationRequest {
                    url: text("url").unwrap_or_default(),
                    is_main_frame: data.get("mainFrame").and_then(|v| v.as_bool()) != Some(false),
                };
                let decision = delegate
                    .navigation_request
                    .as_ref()
                    .map_or(NavigationDecision::Navigate, |decide| decide(&request));
                // The view held the navigation back; going ahead is loading its address.
                if decision == NavigationDecision::Navigate {
                    self.load_request(request.url);
                }
            }
            web_view_wire::CHANNEL_MESSAGE => {
                let channel = text("channel").unwrap_or_default();
                let hook = self
                    .inner
                    .channels
                    .borrow()
                    .iter()
                    .find(|(n, _)| *n == channel)
                    .map(|(_, hook)| hook.clone());
                if let Some(hook) = hook {
                    hook(JavaScriptMessage {
                        channel,
                        message: text("message").unwrap_or_default(),
                    });
                }
            }
            web_view_wire::JAVASCRIPT_RESULT => {
                let Some(id) = data.get("id").and_then(|v| v.as_u64()) else {
                    return;
                };
                let slot = self.inner.results.borrow_mut().remove(&id);
                if let Some(slot) = slot {
                    let value = text("value").unwrap_or_else(|| "null".to_owned());
                    slot.fill(if flag("ok") {
                        Ok(value)
                    } else if flag("unsupported") {
                        Err(WebViewError::Unsupported)
                    } else {
                        Err(WebViewError::JavaScript(value))
                    });
                }
            }
            _ => {}
        }
    }
}

// --- What every web view shares: the cookies, the pages let go -----------------------------

thread_local! {
    /// Calls for every web view rather than one.
    static GLOBAL: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// The answers to wait for, by call.
    static CLEARED: RefCell<HashMap<u64, Rc<Slot<bool>>>> = RefCell::new(HashMap::new());
    static NEXT_CLEAR: Cell<u64> = const { Cell::new(1) };
}

#[inline(never)]
fn push_global(method: &str, arguments: String) {
    // A controller dropped while its thread goes away has nobody to tell.
    let _ = GLOBAL.try_with(|calls| calls.borrow_mut().push((method.to_owned(), arguments)));
    crate::local_task::request_frame();
}

/// The calls for every web view made since the last time — for the shell, which sends them
/// to the platform: `(method, arguments)` with the names of [`web_view_wire`].
// Compiled once, here: a copy inlined into an application would reach another thread-local
// (the hot-reload rule of tests/thread_locals.rs).
#[inline(never)]
pub fn take_web_view_calls() -> Vec<(String, String)> {
    GLOBAL.with(|calls| std::mem::take(&mut *calls.borrow_mut()))
}

/// The platform's answer to a [`web_view_wire::CLEAR_COOKIES`] call — for the shell.
// Compiled once, here (tests/thread_locals.rs).
#[inline(never)]
pub fn web_view_cookies_cleared(id: u64, had: bool) {
    let slot = CLEARED.with(|slots| slots.borrow_mut().remove(&id));
    if let Some(slot) = slot {
        slot.fill(had);
    }
}

/// An event of the platform for no web view in particular — for the shell. Says whether it
/// was one (then something may have changed for the next frame).
// Compiled once, here (tests/thread_locals.rs).
#[inline(never)]
pub fn handle_web_view_global_event(name: &str, data: &str) -> bool {
    if name != web_view_wire::COOKIES_CLEARED {
        return false;
    }
    let value: serde_json::Value = serde_json::from_str(data).unwrap_or(serde_json::Value::Null);
    let Some(id) = value.get("id").and_then(|v| v.as_u64()) else {
        return false;
    };
    let had = value.get("had").and_then(|v| v.as_bool()).unwrap_or(false);
    web_view_cookies_cleared(id, had);
    true
}

/// A cookie for [`WebViewCookieManager::set_cookie`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebViewCookie {
    /// Its name.
    pub name: String,
    /// Its value.
    pub value: String,
    /// The domain it is for (`"example.com"`).
    pub domain: String,
    /// The path it is for (`"/"` by default).
    pub path: String,
}

impl WebViewCookie {
    /// Cookie `name` = `value` for `domain`, on every path.
    pub fn new(
        name: impl Into<String>,
        value: impl Into<String>,
        domain: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            domain: domain.into(),
            path: "/".to_owned(),
        }
    }

    /// The path it is for.
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }
}

/// Whether [`WebViewCookieManager::clear_cookies`] found cookies to clear.
#[must_use = "the answer arrives only when awaited"]
pub struct CookiesCleared {
    slot: Rc<Slot<bool>>,
    unsupported: bool,
}

impl Future for CookiesCleared {
    type Output = Result<bool, WebViewError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.unsupported {
            return Poll::Ready(Err(WebViewError::Unsupported));
        }
        self.slot.poll(cx).map(Ok)
    }
}

/// The cookies every web view of the application shares.
///
/// On the web the cookies are the browser's: both calls answer [`WebViewError::Unsupported`].
/// On Windows they go through the first web view made; calls made before one exists wait for
/// it.
///
/// ```
/// use frus_widgets::{WebViewCookie, WebViewCookieManager};
///
/// let cookies = WebViewCookieManager::new();
/// if cookies
///     .set_cookie(WebViewCookie::new("session", "abc", "example.com"))
///     .is_err()
/// {
///     // No cookies of its own here (the web, or no web views).
/// }
/// let _cleared = cookies.clear_cookies(); // await it for whether there were any
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct WebViewCookieManager;

impl WebViewCookieManager {
    /// The cookie manager.
    pub fn new() -> Self {
        Self
    }

    /// Sets `cookie`, for every web view, from the next request on.
    #[inline(never)]
    pub fn set_cookie(&self, cookie: WebViewCookie) -> Result<(), WebViewError> {
        can(false)?;
        let value = serde_json::json!({
            "url": format!("https://{}{}", cookie.domain.trim_start_matches('.'), cookie.path),
            "name": cookie.name,
            "value": cookie.value,
            "domain": cookie.domain,
            "path": cookie.path,
        });
        push_global(web_view_wire::SET_COOKIE, value.to_string());
        Ok(())
    }

    /// Clears every cookie; the answer says whether there were any.
    #[inline(never)]
    pub fn clear_cookies(&self) -> CookiesCleared {
        if can(false).is_err() {
            return CookiesCleared {
                slot: Rc::default(),
                unsupported: true,
            };
        }
        let id = NEXT_CLEAR.with(|next| next.replace(next.get() + 1));
        let slot: Rc<Slot<bool>> = Rc::default();
        CLEARED.with(|slots| slots.borrow_mut().insert(id, slot.clone()));
        push_global(web_view_wire::CLEAR_COOKIES, format!("{{\"id\":{id}}}"));
        CookiesCleared {
            slot,
            unsupported: false,
        }
    }
}

// --- The widget ----------------------------------------------------------------------------

/// Shows the page of a [`WebViewController`] where it is laid out.
///
/// By default it takes the room it is given: the whole width, and the height a column or a
/// flexible parent has left ([`WEB_VIEW_DEFAULT_HEIGHT`] where there is none to take, as in a
/// scrolling list). [`height`](Self::height) fixes it.
///
/// A drag that starts on the page scrolls the page while it can, and the list around it once
/// the page is at its edge. The system's Back goes back in the page's history while it can.
///
/// On Windows and the web the page is a window or an element over frus's surface (see
/// [`PlatformView`](crate::PlatformView)): frus moves and clips it with the frame. On macOS,
/// Linux and iOS it shows its [fallback](Self::fallback).
///
/// ```
/// use frus_widgets::{WebViewController, WebViewWidget};
///
/// let web = WebViewController::new();
/// web.load_request("https://example.com");
/// let page: WebViewWidget<()> = WebViewWidget::new(&web).semantic_label("Help");
/// # let _ = page;
/// ```
pub struct WebViewWidget<Msg> {
    /// Whether a web view is made here, rather than the fallback shown.
    available: bool,
    controller: WebViewController,
    spec: PlatformViewSpec,
    width: Option<f32>,
    height: Option<f32>,
    label: Option<String>,
    fallback: Vec<Box<dyn Widget<Msg>>>,
}

impl<Msg: 'static> WebViewWidget<Msg> {
    /// The page of `controller`.
    pub fn new(controller: &WebViewController) -> Self {
        let available = web_views_available();
        let fallback: Vec<Box<dyn Widget<Msg>>> = if available {
            Vec::new()
        } else {
            vec![Box::new(UnsupportedView::new(
                "WebViewWidget\nnot available on this platform",
            ))]
        };
        Self {
            available,
            spec: PlatformViewSpec::new(WEB_VIEW_TYPE, controller.creation_params())
                .with_controller(controller.platform_view_controller()),
            controller: controller.clone(),
            width: None,
            height: None,
            label: None,
            fallback,
        }
    }

    /// The width, in logical pixels (the whole width by default).
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width.max(0.0));
        self
    }

    /// The height, in logical pixels (the room it is given by default).
    pub fn height(mut self, height: f32) -> Self {
        self.height = Some(height.max(0.0));
        self
    }

    /// What a screen reader says the page is, before it reads the page itself.
    pub fn semantic_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// What stands in for the page where there are no web views. The default is an
    /// [`UnsupportedView`].
    pub fn fallback(mut self, fallback: impl Widget<Msg> + 'static) -> Self {
        if !self.available {
            self.fallback = vec![Box::new(fallback)];
        }
        self
    }

    /// The controller it shows.
    pub fn controller(&self) -> &WebViewController {
        &self.controller
    }
}

impl<Msg: 'static> Widget<Msg> for WebViewWidget<Msg> {
    fn style(&self) -> Style {
        let free = self.height.is_none();
        Style {
            width: self
                .width
                .map_or(Dimension::Percent(1.0), Dimension::Length),
            height: self.height.map_or(Dimension::Auto, Dimension::Length),
            // Free, it starts from the default height and takes the room a column has, or
            // gives it back: a column with no height of its own still shows the page.
            flex_basis: if free {
                Dimension::Length(WEB_VIEW_DEFAULT_HEIGHT)
            } else {
                Dimension::Auto
            },
            flex_grow: if free { 1.0 } else { 0.0 },
            flex_shrink: if free { 1.0 } else { 0.0 },
            min_width: Dimension::Length(0.0),
            min_height: Dimension::Length(0.0),
            ..Default::default()
        }
    }

    fn children(&self) -> &[Box<dyn Widget<Msg>>] {
        &self.fallback
    }

    fn paint(&self, bounds: Rect, status: Status, _theme: &Theme, scene: &mut Scene) {
        if !self.available {
            return;
        }
        // The page's latest image, drawn where the widget is; nothing before its first one.
        // On Windows, the picture of the page taken while a dialog covers it.
        if let Some(frame) = platform_view_frame(scene.current_owner()) {
            scene.draw_image_with(
                &frame,
                bounds,
                Rect::new(0.0, 0.0, 1.0, 1.0),
                Color::WHITE.fade(status.opacity),
                false,
                FilterQuality::Low,
            );
        }
    }

    fn on_click(&self) -> Option<Msg> {
        None
    }

    fn debug_name(&self) -> &'static str {
        "WebViewWidget"
    }

    fn semantics(&self) -> Option<SemanticsProperties> {
        self.label
            .as_ref()
            .map(|label| SemanticsProperties::new(frus_core::Role::None).label(label.clone()))
    }

    fn platform_view(&self) -> Option<&PlatformViewSpec> {
        self.available.then_some(&self.spec)
    }

    fn on_platform_view_event(&self, event: &PlatformViewEvent) -> Option<Msg> {
        self.controller.handle_event(event);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(name: &str, data: serde_json::Value) -> PlatformViewEvent {
        PlatformViewEvent::new(name, data.to_string())
    }

    fn calls(web: &WebViewController) -> Vec<(String, serde_json::Value)> {
        web.platform_view_controller()
            .take_calls()
            .into_iter()
            .map(|call| {
                let value =
                    serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
                (call.method, value)
            })
            .collect()
    }

    fn poll_now<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
        Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
    }

    #[test]
    fn a_request_goes_out_with_its_method_headers_and_body() {
        let web = WebViewController::new();
        web.load_request(WebRequest::new("https://a.test/").header("X-A", "1"));
        web.load_request(WebRequest::post("https://a.test/f", b"q=frus".to_vec()));
        let sent = calls(&web);
        assert_eq!(sent[0].0, web_view_wire::LOAD_REQUEST);
        assert_eq!(sent[0].1["url"], "https://a.test/");
        assert_eq!(sent[0].1["method"], "GET");
        assert_eq!(sent[0].1["headers"]["X-A"], "1");
        assert_eq!(sent[1].1["method"], "POST");
        assert_eq!(sent[1].1["body"], "cT1mcnVz", "base64 of q=frus");
    }

    #[test]
    fn a_hook_holding_a_weak_handle_lets_the_page_go() {
        let web = WebViewController::new();
        let weak = web.downgrade();
        let inner = weak.clone();
        web.add_javascript_channel("Ping", move |_| {
            if let Some(web) = inner.upgrade() {
                web.run_javascript("pong()");
            }
        })
        .unwrap();
        assert!(weak.upgrade().is_some());
        drop(web);
        assert!(weak.upgrade().is_none(), "no cycle keeps it");
    }

    #[test]
    fn base64_pads_like_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
    }

    #[test]
    fn html_assets_history_and_scripts_carry_their_names() {
        let web = WebViewController::new();
        web.load_html_string("<p>hi</p>", Some("https://base.test/"));
        web.load_asset("help/index.html");
        web.reload();
        web.go_back();
        web.go_forward();
        web.run_javascript("x()");
        let sent = calls(&web);
        let names: Vec<&str> = sent.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                web_view_wire::LOAD_HTML,
                web_view_wire::LOAD_ASSET,
                web_view_wire::RELOAD,
                web_view_wire::GO_BACK,
                web_view_wire::GO_FORWARD,
                web_view_wire::RUN_JAVASCRIPT
            ]
        );
        assert_eq!(sent[0].1["baseUrl"], "https://base.test/");
        assert_eq!(sent[1].1, "help/index.html");
        assert_eq!(sent[5].1, "x()");
    }

    #[test]
    fn the_page_state_follows_the_events_and_notifies() {
        let web = WebViewController::new();
        let heard = Rc::new(Cell::new(0));
        let seen = heard.clone();
        let _listening = web.add_listener(move || seen.set(seen.get() + 1));
        let log = Rc::new(RefCell::new(Vec::<String>::new()));
        let (a, b, c) = (log.clone(), log.clone(), log.clone());
        web.set_navigation_delegate(
            NavigationDelegate::new()
                .on_page_started(move |url| a.borrow_mut().push(format!("start {url}")))
                .on_progress(move |p| b.borrow_mut().push(format!("{p}%")))
                .on_page_finished(move |url| c.borrow_mut().push(format!("done {url}"))),
        );
        let url = serde_json::json!({"url": "https://a.test/"});
        web.handle_event(&event(web_view_wire::PAGE_STARTED, url.clone()));
        assert!(web.is_loading());
        web.handle_event(&event(
            web_view_wire::PROGRESS,
            serde_json::json!({"progress": 40}),
        ));
        assert_eq!(web.progress(), 40);
        web.handle_event(&event(web_view_wire::PAGE_FINISHED, url));
        web.handle_event(&event(
            web_view_wire::STATE,
            serde_json::json!({
                "url": "https://a.test/", "title": "A", "canGoBack": true, "canGoForward": false
            }),
        ));
        assert_eq!(web.current_url().as_deref(), Some("https://a.test/"));
        assert_eq!(web.title().as_deref(), Some("A"));
        assert!(web.can_go_back() && !web.can_go_forward());
        assert!(!web.is_loading());
        assert!(web.platform_view_controller().handles_back());
        assert_eq!(heard.get(), 4);
        assert_eq!(
            *log.borrow(),
            ["start https://a.test/", "40%", "done https://a.test/"]
        );
        // An event of another kind of view is not the page's.
        web.handle_event(&PlatformViewEvent::new("pageFinished", "x"));
        assert_eq!(heard.get(), 4);
    }

    #[test]
    fn a_navigation_is_decided_by_the_delegate() {
        let web = WebViewController::new();
        web.set_navigation_delegate(NavigationDelegate::new().on_navigation_request(|request| {
            if request.url.contains("blocked") {
                NavigationDecision::Prevent
            } else {
                NavigationDecision::Navigate
            }
        }));
        assert_eq!(
            calls(&web),
            [(
                web_view_wire::SET_DECIDES_NAVIGATION.to_owned(),
                serde_json::json!(true)
            )]
        );
        web.handle_event(&event(
            web_view_wire::NAVIGATION_REQUEST,
            serde_json::json!({"url": "https://blocked.test/", "mainFrame": true}),
        ));
        assert!(calls(&web).is_empty(), "prevented: nothing loads");
        web.handle_event(&event(
            web_view_wire::NAVIGATION_REQUEST,
            serde_json::json!({"url": "https://ok.test/", "mainFrame": true}),
        ));
        let sent = calls(&web);
        assert_eq!(sent[0].0, web_view_wire::LOAD_REQUEST);
        assert_eq!(sent[0].1["url"], "https://ok.test/");
        // A delegate without the hook tells the view to stop asking.
        web.set_navigation_delegate(NavigationDelegate::new());
        assert_eq!(calls(&web)[0].1, serde_json::json!(false));
    }

    #[test]
    fn channels_are_identifiers_heard_by_name() {
        let web = WebViewController::new();
        assert_eq!(
            web.add_javascript_channel("not ok", |_| {}),
            Err(WebViewError::InvalidChannelName("not ok".into()))
        );
        let got = Rc::new(RefCell::new(None));
        let into = got.clone();
        web.add_javascript_channel("Toaster", move |m| *into.borrow_mut() = Some(m))
            .unwrap();
        assert_eq!(
            web.add_javascript_channel("Toaster", |_| {}),
            Err(WebViewError::ChannelExists("Toaster".into()))
        );
        assert!(web.creation_params().contains("\"channels\":[\"Toaster\"]"));
        web.handle_event(&event(
            web_view_wire::CHANNEL_MESSAGE,
            serde_json::json!({"channel": "Toaster", "message": "hi"}),
        ));
        assert_eq!(
            got.borrow().as_ref().map(|m| m.message.as_str()),
            Some("hi")
        );
        web.remove_javascript_channel("Toaster");
        assert_eq!(calls(&web).last().unwrap().0, web_view_wire::REMOVE_CHANNEL);
    }

    #[test]
    fn errors_carry_their_type() {
        let web = WebViewController::new();
        let got = Rc::new(RefCell::new(Vec::new()));
        let into = got.clone();
        web.set_navigation_delegate(
            NavigationDelegate::new()
                .on_web_resource_error(move |e| into.borrow_mut().push(e.clone())),
        );
        web.handle_event(&event(
            web_view_wire::RESOURCE_ERROR,
            serde_json::json!({
                "code": -2, "description": "no host", "mainFrame": true, "url": "https://x.test/"
            }),
        ));
        web.handle_event(&event(
            web_view_wire::RESOURCE_ERROR,
            serde_json::json!({"code": 7, "type": "timeout", "mainFrame": false}),
        ));
        let got = got.borrow();
        assert_eq!(got[0].error_type, WebResourceErrorType::HostLookup);
        assert!(got[0].is_for_main_frame);
        assert_eq!(got[1].error_type, WebResourceErrorType::Timeout);
    }

    #[test]
    fn settings_are_kept_for_a_view_made_later() {
        let web = WebViewController::new();
        web.set_javascript_mode(JavaScriptMode::Disabled);
        on_platform(true, false, || {
            web.set_user_agent(Some("frus-test")).unwrap();
            web.enable_zoom(false).unwrap();
        });
        web.set_background_color(Color::rgb(1.0, 0.0, 0.0));
        let params: serde_json::Value = serde_json::from_str(&web.creation_params()).unwrap();
        assert_eq!(params["javascript"], false);
        assert_eq!(params["userAgent"], "frus-test");
        assert_eq!(params["background"], "#ff0000ff");
        assert_eq!(params["zoom"], false);
        assert_eq!(web.user_agent().as_deref(), Some("frus-test"));
        let other: serde_json::Value =
            serde_json::from_str(&WebViewController::new().creation_params()).unwrap();
        assert_ne!(
            params["controller"], other["controller"],
            "each page has its own identity"
        );
    }

    /// Runs `test` on a platform with web views or none (`available`), browser frames or not
    /// (`frame`): a thread-local, so that tests running side by side do not see it.
    fn on_platform(available: bool, frame: bool, test: impl FnOnce()) {
        TEST_PLATFORM.with(|platform| platform.set(Some((available, frame))));
        test();
        TEST_PLATFORM.with(|platform| platform.set(None));
    }

    #[test]
    fn without_web_views_a_result_answers_unsupported_at_once() {
        on_platform(false, false, || {
            let web = WebViewController::new();
            let mut asked = web.run_javascript_returning_result("1 + 1");
            assert_eq!(
                poll_now(&mut asked),
                Poll::Ready(Err(WebViewError::Unsupported))
            );
            let mut cleared = WebViewCookieManager::new().clear_cookies();
            assert_eq!(
                poll_now(&mut cleared),
                Poll::Ready(Err(WebViewError::Unsupported))
            );
            assert_eq!(
                web.set_user_agent(Some("a")),
                Err(WebViewError::Unsupported)
            );
            let widget: WebViewWidget<()> = WebViewWidget::new(&web);
            assert!(widget.platform_view().is_none());
            assert_eq!(widget.children().len(), 1, "the stand-in");
        });
    }

    #[test]
    fn where_web_views_are_hosted_the_widget_asks_for_one() {
        for frame in [false, true] {
            on_platform(true, frame, || {
                let web = WebViewController::new();
                let widget: WebViewWidget<()> = WebViewWidget::new(&web);
                let spec = widget.platform_view().expect("a native view");
                assert_eq!(spec.view_type(), WEB_VIEW_TYPE);
                assert!(spec.params().contains("\"controller\""));
                assert!(widget.children().is_empty(), "no stand-in");
                // A fallback given anyway is not shown where the page is.
                let widget = widget.fallback(UnsupportedView::new("x"));
                assert!(widget.children().is_empty());
            });
        }
    }

    #[test]
    fn a_frame_refuses_what_a_browser_keeps_and_sends_nothing() {
        on_platform(true, true, || {
            let web = WebViewController::new();
            assert_eq!(
                web.set_user_agent(Some("MyApp")),
                Err(WebViewError::Unsupported)
            );
            assert_eq!(web.user_agent(), None, "not kept either");
            assert_eq!(web.set_user_agent(None), Ok(()), "the browser's own");
            assert_eq!(web.enable_zoom(false), Err(WebViewError::Unsupported));
            assert_eq!(
                web.enable_zoom(true),
                Ok(()),
                "a frame zooms with the browser"
            );
            assert_eq!(web.clear_cache(), Err(WebViewError::Unsupported));
            assert_eq!(web.clear_local_storage(), Err(WebViewError::Unsupported));
            let cookies = WebViewCookieManager::new();
            assert_eq!(
                cookies.set_cookie(WebViewCookie::new("a", "b", "c.test")),
                Err(WebViewError::Unsupported)
            );
            let mut cleared = cookies.clear_cookies();
            assert_eq!(
                poll_now(&mut cleared),
                Poll::Ready(Err(WebViewError::Unsupported))
            );
            assert!(take_web_view_calls().is_empty(), "no cookie call went out");
            let names: Vec<String> = calls(&web).into_iter().map(|(n, _)| n).collect();
            assert_eq!(
                names,
                [web_view_wire::SET_USER_AGENT, web_view_wire::SET_ZOOM],
                "only what was allowed"
            );
        });
    }

    #[test]
    fn android_and_windows_take_every_setting() {
        on_platform(true, false, || {
            let web = WebViewController::new();
            assert_eq!(web.set_user_agent(Some("MyApp")), Ok(()));
            assert_eq!(web.enable_zoom(false), Ok(()));
            assert_eq!(web.clear_cache(), Ok(()));
            assert_eq!(web.clear_local_storage(), Ok(()));
            assert_eq!(calls(&web).len(), 4);
            let _ = take_web_view_calls();
            assert_eq!(
                WebViewCookieManager::new().set_cookie(WebViewCookie::new("a", "b", "c.test")),
                Ok(())
            );
            let mut cleared = WebViewCookieManager::new().clear_cookies();
            assert_eq!(
                poll_now(&mut cleared),
                Poll::Pending,
                "waits for the platform"
            );
            let sent: Vec<String> = take_web_view_calls().into_iter().map(|(n, _)| n).collect();
            assert_eq!(
                sent,
                [web_view_wire::SET_COOKIE, web_view_wire::CLEAR_COOKIES]
            );
        });
    }

    #[test]
    fn a_script_a_frame_cannot_run_answers_unsupported() {
        let web = WebViewController::new();
        let id = 9_999_101;
        let slot: Rc<Slot<Result<String, WebViewError>>> = Rc::default();
        web.inner.results.borrow_mut().insert(id, slot.clone());
        let mut asked = JavaScriptResult { slot };
        web.handle_event(&event(
            web_view_wire::JAVASCRIPT_RESULT,
            serde_json::json!({"id": id, "ok": false, "value": "another origin", "unsupported": true}),
        ));
        assert_eq!(
            poll_now(&mut asked),
            Poll::Ready(Err(WebViewError::Unsupported))
        );
    }

    #[test]
    fn a_result_waits_for_its_answer() {
        let web = WebViewController::new();
        let id = 9_999_001;
        let slot: Rc<Slot<Result<String, WebViewError>>> = Rc::default();
        web.inner.results.borrow_mut().insert(id, slot.clone());
        let mut asked = JavaScriptResult { slot };
        assert_eq!(poll_now(&mut asked), Poll::Pending);
        web.handle_event(&event(
            web_view_wire::JAVASCRIPT_RESULT,
            serde_json::json!({"id": id, "ok": true, "value": "\"A\""}),
        ));
        assert_eq!(poll_now(&mut asked), Poll::Ready(Ok("\"A\"".to_owned())));
        let slot: Rc<Slot<Result<String, WebViewError>>> = Rc::default();
        web.inner.results.borrow_mut().insert(id + 1, slot.clone());
        let mut failed = JavaScriptResult { slot };
        web.handle_event(&event(
            web_view_wire::JAVASCRIPT_RESULT,
            serde_json::json!({"id": id + 1, "ok": false, "value": "ReferenceError"}),
        ));
        assert_eq!(
            poll_now(&mut failed),
            Poll::Ready(Err(WebViewError::JavaScript("ReferenceError".into())))
        );
    }

    #[test]
    fn cookies_cleared_answers_its_own_call() {
        let slot: Rc<Slot<bool>> = Rc::default();
        CLEARED.with(|slots| slots.borrow_mut().insert(77_001, slot.clone()));
        let mut cleared = CookiesCleared {
            slot,
            unsupported: false,
        };
        assert_eq!(poll_now(&mut cleared), Poll::Pending);
        assert!(!handle_web_view_global_event("web.other", "{}"));
        assert!(handle_web_view_global_event(
            web_view_wire::COOKIES_CLEARED,
            r#"{"id":77001,"had":true}"#
        ));
        assert_eq!(poll_now(&mut cleared), Poll::Ready(Ok(true)));
    }

    #[test]
    fn a_page_with_no_height_takes_the_room_and_one_with_a_height_keeps_it() {
        let web = WebViewController::new();
        let free: WebViewWidget<()> = WebViewWidget::new(&web);
        assert_eq!(free.style().flex_grow, 1.0);
        assert_eq!(
            free.style().flex_basis,
            Dimension::Length(WEB_VIEW_DEFAULT_HEIGHT)
        );
        let fixed: WebViewWidget<()> = WebViewWidget::new(&web).height(200.0);
        assert_eq!(fixed.style().height, Dimension::Length(200.0));
        assert_eq!(fixed.style().flex_grow, 0.0);
    }
}
