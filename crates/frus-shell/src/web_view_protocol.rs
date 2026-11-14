// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The web views' protocol as the hosts over frus's surface speak it (milestone 804): what a
//! `WebViewController` sends a `"frus/webview"` view (`web.loadRequest`, `web.goBack`, …),
//! what the view answers (`web.pageStarted`, `web.javascriptResult`, …), and the bookkeeping
//! a host needs that its platform does not do for it.
//!
//! The names are the public ones of `frus::web_view_wire` (milestone 799), the arguments and
//! the events' data JSON. The Windows host (WebView2) and the web host (an `<iframe>`) both
//! read the calls with [`Call::parse`] and write the events with the functions below, so the
//! two say exactly what Android says. Everything here is pure: it is tested without a window.

use serde_json::{json, Value};

/// What a page is made with: creation parameters that are a JSON object with a `controller`
/// number. Anything else (an address, HTML) is the plain web view of milestone 794, which
/// speaks the older names (`loadUrl`, `evaluateJavascript`, …).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Params {
    /// The controller's number: the same number is the same page.
    pub controller: u64,
    pub javascript: bool,
    pub user_agent: Option<String>,
    /// `[r, g, b, a]`.
    pub background: Option<[u8; 4]>,
    pub zoom: bool,
    pub channels: Vec<String>,
    pub decides_navigation: bool,
}

impl Params {
    /// The parameters of a controller's page, or `None` for a plain web view.
    pub fn parse(params: &str) -> Option<Self> {
        let text = params.trim_start();
        if !text.starts_with('{') {
            return None;
        }
        let value: Value = serde_json::from_str(text).ok()?;
        let controller = value.get("controller")?.as_u64()?;
        let flag =
            |key: &str, default: bool| value.get(key).and_then(Value::as_bool).unwrap_or(default);
        Some(Self {
            controller,
            javascript: flag("javascript", true),
            user_agent: value
                .get("userAgent")
                .and_then(Value::as_str)
                .map(str::to_owned),
            background: value
                .get("background")
                .and_then(Value::as_str)
                .and_then(color),
            zoom: flag("zoom", true),
            channels: value
                .get("channels")
                .and_then(Value::as_array)
                .map(|names| {
                    names
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            decides_navigation: flag("decidesNavigation", false),
        })
    }
}

/// `"#rrggbbaa"` as `[r, g, b, a]`.
pub(crate) fn color(text: &str) -> Option<[u8; 4]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 8 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 4];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// A call of a controller on its page.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Call {
    /// `post`: a `POST` of `body`; otherwise a `GET` with `headers`.
    LoadRequest {
        url: String,
        post: bool,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    LoadHtml {
        html: String,
        base_url: Option<String>,
    },
    LoadFile(String),
    LoadAsset(String),
    Reload,
    GoBack,
    GoForward,
    Run(String),
    RunForResult {
        id: u64,
        script: String,
    },
    AddChannel(String),
    RemoveChannel(String),
    SetJavascript(bool),
    SetUserAgent(Option<String>),
    SetBackground([u8; 4]),
    SetZoom(bool),
    SetDecidesNavigation(bool),
    ClearCache,
    ClearLocalStorage,
}

impl Call {
    /// The call `method` with `arguments`, or `None` when it is not one of the web views'
    /// (the plain web view's own names) or its arguments do not read.
    pub fn parse(method: &str, arguments: &str) -> Option<Self> {
        let name = method.strip_prefix("web.")?;
        let value: Value = if arguments.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(arguments).ok()?
        };
        let text = || value.as_str().map(str::to_owned);
        let flag = || value.as_bool();
        let field = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Some(match name {
            "loadRequest" => Call::LoadRequest {
                url: field("url")?,
                post: field("method").as_deref() == Some("POST"),
                headers: value
                    .get("headers")
                    .and_then(Value::as_object)
                    .map(|headers| {
                        headers
                            .iter()
                            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                            .collect()
                    })
                    .unwrap_or_default(),
                body: field("body")
                    .map(|body| base64_decode(&body))
                    .unwrap_or_default(),
            },
            "loadHtml" => Call::LoadHtml {
                html: field("html").unwrap_or_default(),
                base_url: field("baseUrl"),
            },
            "loadFile" => Call::LoadFile(text()?),
            "loadAsset" => Call::LoadAsset(text()?),
            "reload" => Call::Reload,
            "goBack" => Call::GoBack,
            "goForward" => Call::GoForward,
            "runJavascript" => Call::Run(text()?),
            "runJavascriptForResult" => Call::RunForResult {
                id: value.get("id")?.as_u64()?,
                script: field("script").unwrap_or_default(),
            },
            "addChannel" => Call::AddChannel(text()?),
            "removeChannel" => Call::RemoveChannel(text()?),
            "setJavascript" => Call::SetJavascript(flag()?),
            "setUserAgent" => Call::SetUserAgent(text()),
            "setBackground" => Call::SetBackground(color(&text()?)?),
            "setZoom" => Call::SetZoom(flag()?),
            "setDecidesNavigation" => Call::SetDecidesNavigation(flag()?),
            "clearCache" => Call::ClearCache,
            "clearLocalStorage" => Call::ClearLocalStorage,
            _ => return None,
        })
    }
}

/// A call for every web view rather than one (`frus_widgets::take_web_view_calls`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Global {
    SetCookie {
        name: String,
        value: String,
        domain: String,
        path: String,
    },
    /// Answered by [`cookies_cleared`].
    ClearCookies { id: u64 },
    /// The controller was dropped: its page goes.
    Release { controller: u64 },
}

impl Global {
    /// The global call `method` with `arguments`, if it is one.
    pub fn parse(method: &str, arguments: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(arguments).ok()?;
        let field = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Some(match method {
            "web.setCookie" => Global::SetCookie {
                name: field("name")?,
                value: field("value").unwrap_or_default(),
                domain: field("domain").unwrap_or_default(),
                path: field("path").unwrap_or_else(|| "/".to_owned()),
            },
            "web.clearCookies" => Global::ClearCookies {
                id: value.get("id")?.as_u64()?,
            },
            "web.release" => Global::Release {
                controller: value.get("controller")?.as_u64()?,
            },
            _ => return None,
        })
    }
}

// --- What the page says -------------------------------------------------------------------

/// An event: its name and its data.
pub(crate) type Event = (&'static str, String);

pub(crate) fn page_started(url: &str) -> Event {
    ("web.pageStarted", json!({ "url": url }).to_string())
}

pub(crate) fn page_finished(url: &str) -> Event {
    ("web.pageFinished", json!({ "url": url }).to_string())
}

pub(crate) fn progress(percent: u8) -> Event {
    (
        "web.progress",
        json!({ "progress": percent.min(100) }).to_string(),
    )
}

pub(crate) fn state(url: &str, title: Option<&str>, back: bool, forward: bool) -> Event {
    (
        "web.state",
        json!({ "url": url, "title": title, "canGoBack": back, "canGoForward": forward })
            .to_string(),
    )
}

pub(crate) fn url_change(url: &str) -> Event {
    ("web.urlChange", json!({ "url": url }).to_string())
}

pub(crate) fn resource_error(code: i64, kind: &str, description: &str, url: &str) -> Event {
    (
        "web.resourceError",
        json!({
            "code": code, "type": kind, "description": description, "mainFrame": true, "url": url
        })
        .to_string(),
    )
}

pub(crate) fn http_error(status: i32, url: &str) -> Event {
    (
        "web.httpError",
        json!({ "status": status, "url": url }).to_string(),
    )
}

pub(crate) fn navigation_request(url: &str) -> Event {
    (
        "web.navigationRequest",
        json!({ "url": url, "mainFrame": true }).to_string(),
    )
}

pub(crate) fn channel_message(channel: &str, message: &str) -> Event {
    (
        "web.channelMessage",
        json!({ "channel": channel, "message": message }).to_string(),
    )
}

/// A script's result: its value as JSON, or why it failed. `unsupported`: the platform
/// cannot run scripts in this page (a page of another site in an `<iframe>`).
pub(crate) fn javascript_result(
    id: u64,
    outcome: Result<String, String>,
    unsupported: bool,
) -> Event {
    let (ok, value) = match outcome {
        Ok(value) => (true, value),
        Err(why) => (false, why),
    };
    (
        "web.javascriptResult",
        json!({ "id": id, "ok": ok, "value": value, "unsupported": unsupported }).to_string(),
    )
}

pub(crate) fn cookies_cleared(id: u64, had: bool) -> Event {
    (
        "web.cookiesCleared",
        json!({ "id": id, "had": had }).to_string(),
    )
}

// --- JavaScript channels ------------------------------------------------------------------

/// The script that gives a page the channel `name`: `name.postMessage(text)` hands
/// `{"frusChannel": name, "message": text}` to `post`, a JavaScript expression of a function
/// (`window.chrome.webview.postMessage` in WebView2, a post to the parent on the web).
/// `name` is a JavaScript identifier (the controller checks it).
pub(crate) fn channel_script(name: &str, post: &str) -> String {
    format!(
        "(function(){{var p={post};window[{name:?}]={{postMessage:function(m){{\
         p({{frusChannel:{name:?},message:String(m)}});}}}};}})();"
    )
}

/// The channel and the message of what a page posted through a channel's script, or `None`
/// when it is something else.
pub(crate) fn channel_post(data: &str) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(data).ok()?;
    let channel = value.get("frusChannel")?.as_str()?.to_owned();
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    Some((channel, message))
}

// --- Decided navigations ------------------------------------------------------------------

/// Which navigations a page lets go by itself and which it holds back for the controller's
/// delegate to decide (`web.navigationRequest`), as Android's `shouldOverrideUrlLoading`
/// does: the page's own (a link, a script, a form) are held back while the delegate decides;
/// the ones the application asked for (a load, back, forward, reload) never are.
#[derive(Debug, Default)]
pub(crate) struct NavigationGate {
    pub decides: bool,
    /// The navigation the application asked for last, not started yet: its address, or
    /// `None` for one whose address is not known in advance (back, forward, reload, HTML).
    own: Option<Option<String>>,
    /// The navigation let go last: its redirects go too.
    allowed: Option<u64>,
    /// Navigations held back: their failure is not the page's.
    held: Vec<u64>,
}

/// What [`NavigationGate::starting`] decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gate {
    Go,
    /// Cancel it and ask the controller.
    Hold,
}

impl NavigationGate {
    /// A gate that holds the page's navigations back while `decides`.
    pub fn new(decides: bool) -> Self {
        Self {
            decides,
            ..Self::default()
        }
    }

    /// The application is about to navigate (to `url` when known).
    pub fn own(&mut self, url: Option<&str>) {
        self.own = Some(url.map(str::to_owned));
    }

    /// Navigation `id` to `url` is starting (`redirect`: a server's redirect of one started
    /// before).
    pub fn starting(&mut self, id: u64, url: &str, redirect: bool) -> Gate {
        if redirect && self.allowed == Some(id) {
            return Gate::Go;
        }
        let ours = match &self.own {
            Some(None) => true,
            Some(Some(expected)) => same_address(expected, url),
            None => false,
        };
        if ours {
            self.own = None;
        }
        if ours || !self.decides {
            self.allowed = Some(id);
            return Gate::Go;
        }
        self.held.push(id);
        if self.held.len() > 16 {
            self.held.remove(0);
        }
        Gate::Hold
    }

    /// Navigation `id` ended: whether its end is the page's to report (not one held back).
    pub fn completed(&mut self, id: u64) -> bool {
        match self.held.iter().position(|held| *held == id) {
            Some(at) => {
                self.held.remove(at);
                false
            }
            None => true,
        }
    }
}

/// Whether two addresses are the same page: `https://a.test` is `https://a.test/`.
fn same_address(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

// --- An `<iframe>`'s history ---------------------------------------------------------------

#[cfg_attr(not(any(web, test)), allow(dead_code))]
/// The history of a page shown in an `<iframe>`, which the browser does not tell the page
/// around it: kept from the frame's loads and the moves the application asked for.
#[derive(Debug, Default)]
pub(crate) struct FrameHistory {
    entries: Vec<String>,
    at: usize,
    /// The move asked for and not loaded yet.
    pending: Option<Move>,
}

#[cfg_attr(not(any(web, test)), allow(dead_code))]
/// A move in the history the application asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Move {
    Load,
    Back,
    Forward,
    Reload,
}

#[cfg_attr(not(any(web, test)), allow(dead_code))]
impl FrameHistory {
    /// The application asked for `step`.
    pub fn going(&mut self, step: Move) {
        self.pending = Some(step);
    }

    /// Whether nothing was loaded or asked for yet (the frame's first, empty page).
    pub fn is_blank(&self) -> bool {
        self.entries.is_empty() && self.pending.is_none()
    }

    /// The frame loaded a page, at `url`.
    pub fn loaded(&mut self, url: &str) {
        match self.pending.take() {
            Some(Move::Back) if self.at > 0 => {
                self.at -= 1;
                self.entries[self.at] = url.to_owned();
            }
            Some(Move::Forward) if self.at + 1 < self.entries.len() => {
                self.at += 1;
                self.entries[self.at] = url.to_owned();
            }
            Some(Move::Reload) if !self.entries.is_empty() => {
                self.entries[self.at] = url.to_owned();
            }
            _ => {
                if !self.entries.is_empty() {
                    self.entries.truncate(self.at + 1);
                }
                self.entries.push(url.to_owned());
                self.at = self.entries.len() - 1;
            }
        }
    }

    pub fn can_go_back(&self) -> bool {
        self.at > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.at + 1 < self.entries.len()
    }
}

// --- Platforms' details ---------------------------------------------------------------------

/// The `file:` address of an absolute path: `C:\a b\c.html` is `file:///C:/a%20b/c.html`.
pub(crate) fn file_url(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut out = String::from("file://");
    if !path.starts_with('/') {
        out.push('/');
    }
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The type of a WebView2 `COREWEBVIEW2_WEB_ERROR_STATUS`, in the names of
/// `web.resourceError`.
pub(crate) fn webview2_error_type(status: i32) -> &'static str {
    match status {
        1..=5 => "ssl",
        6 | 9 | 10 | 11 | 12 => "connect",
        7 => "timeout",
        13 => "hostLookup",
        14 => "cancelled",
        15 => "redirectLoop",
        _ => "unknown",
    }
}

/// Decodes standard base64 (a request's body), skipping what is not of its alphabet.
pub(crate) fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut count) = (0u32, 0u32);
    for c in text.bytes() {
        let six = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        bits = (bits << 6) | u32::from(six);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    out
}

#[cfg_attr(not(any(web, test)), allow(dead_code))]
/// `html` with `<base href="base">` first, so that its relative addresses resolve against
/// `base` (HTML fetched for an `<iframe>`, shown through `srcdoc`).
pub(crate) fn with_base(html: &str, base: &str) -> String {
    let escaped = base.replace('&', "&amp;").replace('"', "&quot;");
    put_first(html, &format!("<base href=\"{escaped}\">"))
}

#[cfg_attr(not(any(web, test)), allow(dead_code))]
/// `html` with `extra` (a `<script>`, a `<base>`) put first, after its `<!doctype>` if it has
/// one: before the doctype, it would put the page in quirks mode.
pub(crate) fn put_first(html: &str, extra: &str) -> String {
    let trimmed = html.trim_start();
    let starts_with_doctype = trimmed
        .get(..9)
        .is_some_and(|start| start.eq_ignore_ascii_case("<!doctype"));
    match trimmed.find('>') {
        Some(end) if starts_with_doctype => {
            let (doctype, rest) = trimmed.split_at(end + 1);
            format!("{doctype}{extra}{rest}")
        }
        _ => format!("{extra}{html}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_controllers_parameters_read_and_a_plain_view_is_not_one() {
        let params = Params::parse(
            r##"{"controller":7,"javascript":false,"userAgent":"ua","background":"#ff000080",
                "zoom":false,"channels":["A","B"],"decidesNavigation":true}"##,
        )
        .unwrap();
        assert_eq!(params.controller, 7);
        assert!(!params.javascript && !params.zoom && params.decides_navigation);
        assert_eq!(params.user_agent.as_deref(), Some("ua"));
        assert_eq!(params.background, Some([255, 0, 0, 128]));
        assert_eq!(params.channels, ["A", "B"]);
        assert_eq!(Params::parse("https://example.com"), None);
        assert_eq!(Params::parse("<h1>hi</h1>"), None);
        assert_eq!(Params::parse(r#"{"other":1}"#), None);
    }

    #[test]
    fn the_calls_read_with_their_arguments() {
        assert_eq!(
            Call::parse(
                "web.loadRequest",
                r#"{"url":"https://a.test/","method":"POST","headers":{"X":"1"},"body":"cT1mcnVz"}"#
            ),
            Some(Call::LoadRequest {
                url: "https://a.test/".into(),
                post: true,
                headers: vec![("X".into(), "1".into())],
                body: b"q=frus".to_vec(),
            })
        );
        assert_eq!(
            Call::parse("web.loadHtml", r#"{"html":"<p>","baseUrl":null}"#),
            Some(Call::LoadHtml {
                html: "<p>".into(),
                base_url: None
            })
        );
        assert_eq!(Call::parse("web.goBack", ""), Some(Call::GoBack));
        assert_eq!(
            Call::parse("web.runJavascriptForResult", r#"{"id":4,"script":"1+1"}"#),
            Some(Call::RunForResult {
                id: 4,
                script: "1+1".into()
            })
        );
        assert_eq!(
            Call::parse("web.setUserAgent", "null"),
            Some(Call::SetUserAgent(None))
        );
        assert_eq!(
            Call::parse("web.setBackground", r##""#00ff00ff""##),
            Some(Call::SetBackground([0, 255, 0, 255]))
        );
        assert_eq!(
            Call::parse("loadUrl", "https://a.test"),
            None,
            "the plain view's"
        );
        assert_eq!(Call::parse("web.unknown", ""), None);
    }

    #[cfg(feature = "webview")]
    #[test]
    fn every_name_of_the_wire_is_read() {
        use frus_widgets::web_view_wire as wire;
        for (name, args) in [
            (wire::LOAD_REQUEST, r#"{"url":"u"}"#),
            (wire::LOAD_HTML, r#"{"html":""}"#),
            (wire::LOAD_FILE, r#""/a""#),
            (wire::LOAD_ASSET, r#""a""#),
            (wire::RELOAD, ""),
            (wire::GO_BACK, ""),
            (wire::GO_FORWARD, ""),
            (wire::RUN_JAVASCRIPT, r#""x""#),
            (wire::RUN_JAVASCRIPT_FOR_RESULT, r#"{"id":1,"script":"x"}"#),
            (wire::ADD_CHANNEL, r#""A""#),
            (wire::REMOVE_CHANNEL, r#""A""#),
            (wire::SET_JAVASCRIPT, "true"),
            (wire::SET_USER_AGENT, r#""ua""#),
            (wire::SET_BACKGROUND, r##""#000000ff""##),
            (wire::SET_ZOOM, "false"),
            (wire::SET_DECIDES_NAVIGATION, "true"),
            (wire::CLEAR_CACHE, ""),
            (wire::CLEAR_LOCAL_STORAGE, ""),
        ] {
            assert!(Call::parse(name, args).is_some(), "{name}");
        }
        assert!(Global::parse(wire::CLEAR_COOKIES, r#"{"id":3}"#).is_some());
        assert!(Global::parse(wire::RELEASE, r#"{"controller":3}"#).is_some());
        assert!(Global::parse(
            wire::SET_COOKIE,
            r#"{"url":"https://a.test/","name":"n","value":"v","domain":"a.test","path":"/"}"#
        )
        .is_some());
        for (name, _) in [
            page_started(""),
            page_finished(""),
            progress(1),
            state("", None, false, false),
            url_change(""),
            resource_error(0, "unknown", "", ""),
            http_error(404, ""),
            navigation_request(""),
            channel_message("", ""),
            javascript_result(1, Ok("1".into()), false),
            cookies_cleared(1, true),
        ] {
            assert!(
                [
                    wire::PAGE_STARTED,
                    wire::PAGE_FINISHED,
                    wire::PROGRESS,
                    wire::STATE,
                    wire::URL_CHANGE,
                    wire::RESOURCE_ERROR,
                    wire::HTTP_ERROR,
                    wire::NAVIGATION_REQUEST,
                    wire::CHANNEL_MESSAGE,
                    wire::JAVASCRIPT_RESULT,
                    wire::COOKIES_CLEARED,
                ]
                .contains(&name),
                "{name}"
            );
        }
    }

    /// A page that speaks the protocol as the hosts do, with no window: what a host's page
    /// does for each call, and what it says back.
    #[cfg(feature = "webview")]
    #[derive(Default)]
    struct FakePage {
        history: FrameHistory,
        gate: NavigationGate,
        channels: Vec<String>,
        next: u64,
        said: Vec<Event>,
    }

    #[cfg(feature = "webview")]
    impl FakePage {
        fn call(&mut self, method: &str, arguments: &str) {
            match Call::parse(method, arguments).expect("a web view call") {
                Call::LoadRequest { url, .. } => {
                    self.gate.own(Some(&url));
                    self.navigate(&url, Move::Load);
                }
                Call::GoBack if self.history.can_go_back() => {
                    self.gate.own(None);
                    self.navigate("https://a.test/", Move::Back);
                }
                Call::AddChannel(name) => self.channels.push(name),
                Call::SetDecidesNavigation(on) => self.gate.decides = on,
                Call::RunForResult { id, script } => {
                    self.said
                        .push(javascript_result(id, Ok(format!("{script:?}")), false))
                }
                _ => {}
            }
        }

        /// The page navigates to `url`, by itself or as asked.
        fn navigate(&mut self, url: &str, step: Move) {
            self.next += 1;
            if self.gate.starting(self.next, url, false) == Gate::Hold {
                self.said.push(navigation_request(url));
                return;
            }
            self.said.push(page_started(url));
            self.history.going(step);
            self.history.loaded(url);
            self.said.push(url_change(url));
            self.said.push(page_finished(url));
            self.said.push(state(
                url,
                Some("T"),
                self.history.can_go_back(),
                self.history.can_go_forward(),
            ));
        }

        fn post(&mut self, channel: &str, message: &str) {
            if self.channels.iter().any(|c| c == channel) {
                self.said.push(channel_message(channel, message));
            }
        }
    }

    /// Hands the controller's calls to `page`, and what `page` says to the controller, until
    /// neither has anything left.
    #[cfg(feature = "webview")]
    fn exchange(web: &frus_widgets::WebViewController, page: &mut FakePage) {
        loop {
            let calls = web.platform_view_controller().take_calls();
            for call in &calls {
                page.call(&call.method, &call.arguments);
            }
            let said = std::mem::take(&mut page.said);
            if calls.is_empty() && said.is_empty() {
                return;
            }
            for (name, data) in said {
                web.handle_event(&frus_widgets::PlatformViewEvent::new(name, data));
            }
        }
    }

    #[cfg(feature = "webview")]
    #[test]
    fn a_controller_drives_a_page_that_speaks_the_protocol() {
        use frus_widgets::{NavigationDecision, NavigationDelegate, WebViewController};
        use std::cell::RefCell;
        use std::rc::Rc;

        let web = WebViewController::new();
        let heard = Rc::new(RefCell::new(Vec::<String>::new()));
        let into = heard.clone();
        web.add_javascript_channel("Frus", move |m| into.borrow_mut().push(m.message))
            .unwrap();
        web.set_navigation_delegate(NavigationDelegate::new().on_navigation_request(|r| {
            if r.url.contains("blocked") {
                NavigationDecision::Prevent
            } else {
                NavigationDecision::Navigate
            }
        }));
        let mut page = FakePage::default();
        web.load_request("https://a.test/");
        exchange(&web, &mut page);
        assert_eq!(web.current_url().as_deref(), Some("https://a.test/"));
        assert_eq!(web.title().as_deref(), Some("T"));
        assert!(!web.can_go_back() && !web.is_loading());

        // A link the delegate prevents: the page stays.
        page.navigate("https://blocked.test/", Move::Load);
        exchange(&web, &mut page);
        assert_eq!(web.current_url().as_deref(), Some("https://a.test/"));
        // One it lets go: loaded as the application's own, not asked about again.
        page.navigate("https://b.test/", Move::Load);
        exchange(&web, &mut page);
        assert_eq!(web.current_url().as_deref(), Some("https://b.test/"));
        assert!(web.can_go_back());

        web.go_back();
        exchange(&web, &mut page);
        assert_eq!(web.current_url().as_deref(), Some("https://a.test/"));
        assert!(!web.can_go_back() && web.can_go_forward());

        page.post("Frus", "hello");
        page.post("Unknown", "ignored");
        exchange(&web, &mut page);
        assert_eq!(*heard.borrow(), ["hello"]);
    }

    #[test]
    fn the_events_carry_what_the_controller_reads() {
        let (_, data) = state("https://a.test/", Some("A"), true, false);
        let value: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(value["title"], "A");
        assert_eq!(value["canGoBack"], true);
        let (_, data) = javascript_result(9, Err("cross-origin".into()), true);
        let value: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["unsupported"], true);
        let (_, data) = resource_error(13, webview2_error_type(13), "no host", "https://x.test/");
        let value: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(value["type"], "hostLookup");
        assert_eq!(value["mainFrame"], true);
    }

    #[test]
    fn a_channel_script_posts_what_channel_post_reads() {
        let script = channel_script("Toaster", "window.chrome.webview.postMessage");
        assert!(script.contains("window[\"Toaster\"]"));
        assert!(script.contains("frusChannel:\"Toaster\""));
        assert_eq!(
            channel_post(r#"{"frusChannel":"Toaster","message":"hi"}"#),
            Some(("Toaster".into(), "hi".into()))
        );
        assert_eq!(channel_post(r#"{"other":1}"#), None);
        assert_eq!(channel_post("not json"), None);
    }

    #[test]
    fn the_gate_holds_the_pages_navigations_and_lets_the_applications_go() {
        let mut gate = NavigationGate::default();
        assert_eq!(
            gate.starting(1, "https://a.test/", false),
            Gate::Go,
            "nobody decides"
        );
        gate.decides = true;
        gate.own(Some("https://a.test"));
        assert_eq!(
            gate.starting(2, "https://a.test/", false),
            Gate::Go,
            "the application's load"
        );
        assert_eq!(
            gate.starting(2, "https://b.test/", true),
            Gate::Go,
            "its redirect"
        );
        assert_eq!(
            gate.starting(3, "https://c.test/", false),
            Gate::Hold,
            "a link"
        );
        assert!(
            !gate.completed(3),
            "a held navigation's end is not reported"
        );
        assert!(gate.completed(2));
        gate.own(None);
        assert_eq!(gate.starting(4, "https://a.test/", false), Gate::Go, "back");
        assert_eq!(gate.starting(5, "https://d.test/", false), Gate::Hold);
        // A load whose navigation never started (a fragment) does not let a link through.
        gate.own(Some("https://a.test/#x"));
        assert_eq!(gate.starting(6, "https://e.test/", false), Gate::Hold);
    }

    #[test]
    fn a_frames_history_follows_its_loads_and_moves() {
        let mut history = FrameHistory::default();
        assert!(history.is_blank());
        history.going(Move::Load);
        assert!(!history.is_blank());
        history.loaded("a");
        history.loaded("b"); // a link in the page
        assert!(history.can_go_back() && !history.can_go_forward());
        history.going(Move::Back);
        history.loaded("a");
        assert!(!history.can_go_back() && history.can_go_forward());
        history.going(Move::Forward);
        history.loaded("b");
        assert!(history.can_go_back() && !history.can_go_forward());
        history.going(Move::Back);
        history.loaded("a");
        history.going(Move::Load);
        history.loaded("c");
        assert!(
            history.can_go_back() && !history.can_go_forward(),
            "forward is dropped"
        );
        history.going(Move::Reload);
        history.loaded("c");
        assert!(history.can_go_back() && !history.can_go_forward());
    }

    #[test]
    fn paths_become_file_addresses() {
        assert_eq!(file_url(r"C:\a b\c.html"), "file:///C:/a%20b/c.html");
        assert_eq!(file_url("/data/x.html"), "file:///data/x.html");
    }

    #[test]
    fn base64_decodes_like_the_standard() {
        assert_eq!(base64_decode(""), b"");
        assert_eq!(base64_decode("Zg=="), b"f");
        assert_eq!(base64_decode("Zm8="), b"fo");
        assert_eq!(base64_decode("Zm9vYg=="), b"foob");
    }

    #[test]
    fn webview2_statuses_have_types() {
        assert_eq!(webview2_error_type(13), "hostLookup");
        assert_eq!(webview2_error_type(7), "timeout");
        assert_eq!(webview2_error_type(2), "ssl");
        assert_eq!(webview2_error_type(0), "unknown");
    }

    #[test]
    fn a_script_goes_after_the_doctype() {
        assert_eq!(
            put_first("<!DOCTYPE html><p>", "<script></script>"),
            "<!DOCTYPE html><script></script><p>"
        );
        assert_eq!(put_first("<p>", "<base>"), "<base><p>");
    }

    #[test]
    fn fetched_html_resolves_against_its_address() {
        assert_eq!(
            with_base("<p>", "https://a.test/x?a=1&b=\"2\""),
            "<base href=\"https://a.test/x?a=1&amp;b=&quot;2&quot;\"><p>"
        );
    }
}
