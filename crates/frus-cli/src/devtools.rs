// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **DevTools** (milestone 788): a page `frus run` serves on `localhost`, and the protocol behind
//! it, which any other client can speak too.
//!
//! The page looks into the application `frus run` started: its widget tree, a widget's box,
//! style and semantics, the frames' costs, the log, and the switches a developer flips while
//! looking (debug paint, slow animations, the brightness…). Every request goes to
//! `POST /api` as `{"method":"tree","params":{}}`. A few are answered here — the log, a reload,
//! the device's own screenshot on Android — and the rest are relayed to the application, which
//! answers them against its next frame (`frus-shell`'s `devtools`):
//!
//! - a desktop application, on the socket it reports to this run on (`devlink`);
//! - a page, through the script this run adds to it (`serve`);
//! - an Android application, on a local socket in the abstract namespace, reached with
//!   `adb forward`.
//!
//! Only a debug build answers: nothing of DevTools is compiled into a release.
//!
//! Every request carries the run's token, in the `X-Frus-DevTools` header or as `?token=` in the
//! address: a page from anywhere else, open in the developer's browser, cannot drive the
//! application — it does not know the token, and a custom header is not something a page may
//! send to another site without asking it first.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::json::{self, object, Value};
use crate::report;

/// The protocol's version, the one `frus-shell` answers `hello` with.
pub(crate) const PROTOCOL: u64 = 1;

/// The page.
const PAGE: &str = include_str!("devtools.html");

/// The port DevTools is served on when it is free; another one otherwise.
const PORT: u16 = 9100;

/// How long the application has to answer.
const TIMEOUT: Duration = Duration::from_secs(10);

/// How many lines of the log are kept for the page.
const KEPT_LINES: usize = 5000;

/// The application's answers, waited for by the requests they answer: what the desktop's socket
/// and the web page hand over, one JSON line each.
#[derive(Default)]
pub(crate) struct Answers {
    waiting: Mutex<HashMap<u64, Sender<String>>>,
}

impl Answers {
    /// An answer arrived: the request with its id gets it. One nobody waits for any more (it
    /// came too late) is dropped.
    pub(crate) fn deliver(&self, answer: &str) {
        let Some(id) = json::parse(answer)
            .ok()
            .and_then(|value| value.get("id").and_then(Value::as_f64))
        else {
            return;
        };
        let waiting = self
            .waiting
            .lock()
            .ok()
            .and_then(|mut w| w.remove(&(id as u64)));
        if let Some(waiting) = waiting {
            let _ = waiting.send(answer.to_string());
        }
    }

    /// Sends request `id` with `send`, and waits for its answer.
    fn ask(&self, id: u64, send: impl FnOnce() -> bool) -> Result<String, String> {
        let (tell, told) = mpsc::channel();
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.insert(id, tell);
        }
        let outcome = if send() {
            told.recv_timeout(TIMEOUT).map_err(|_| {
                "the application did not answer: is it running, and is it a debug build?"
                    .to_string()
            })
        } else {
            Err("the application is not connected to frus run".to_string())
        };
        if let Ok(mut waiting) = self.waiting.lock() {
            waiting.remove(&id);
        }
        outcome
    }
}

/// A line sent to the application: `true` when it could be.
type SendLine = Box<dyn Fn(&str) -> bool + Send + Sync>;

/// How requests reach the application, and its answers come back.
pub(crate) enum Relay {
    /// A line `devtools <json>` sent with the function, the answer delivered to `answers`:
    /// the desktop's socket, a page's queue of commands.
    Lines {
        send: SendLine,
        answers: Arc<Answers>,
    },
    /// An Android application's local socket, through `adb forward`.
    Adb {
        adb: PathBuf,
        serial: String,
        socket: String,
        /// The local port forwarded to it, once it is.
        port: Mutex<Option<u16>>,
    },
}

impl Relay {
    /// An Android application with `package` on device `serial`.
    pub(crate) fn adb(adb: PathBuf, serial: &str, package: &str) -> Self {
        Relay::Adb {
            adb,
            serial: serial.to_string(),
            socket: format!("frus_devtools_{package}"),
            port: Mutex::new(None),
        }
    }

    /// Request `id`, one JSON object, to the application; its answer.
    fn call(&self, id: u64, request: &str) -> Result<String, String> {
        match self {
            Relay::Lines { send, answers } => {
                answers.ask(id, || send(&format!("devtools {request}")))
            }
            Relay::Adb { .. } => {
                // A restarted application listens anew: once more, through a new forward.
                self.call_device(request).or_else(|_| {
                    self.forget_forward();
                    self.call_device(request)
                })
            }
        }
    }

    /// The local port forwarded to the device's socket, forwarded now if it is not yet.
    fn forwarded(&self) -> Result<u16, String> {
        let Relay::Adb {
            adb,
            serial,
            socket,
            port,
        } = self
        else {
            return Err("not a device".into());
        };
        let mut port = port.lock().map_err(|e| e.to_string())?;
        if let Some(port) = *port {
            return Ok(port);
        }
        let out = Command::new(adb)
            .args(["-s", serial, "forward", "tcp:0"])
            .arg(format!("localabstract:{socket}"))
            .output()
            .map_err(|e| format!("adb forward: {e}"))?;
        let given: u16 = String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .map_err(|_| {
                format!(
                    "adb could not forward a port to the application: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                )
            })?;
        *port = Some(given);
        Ok(given)
    }

    fn forget_forward(&self) {
        if let Relay::Adb {
            adb, serial, port, ..
        } = self
        {
            if let Some(old) = port.lock().ok().and_then(|mut p| p.take()) {
                let _ = Command::new(adb)
                    .args(["-s", serial, "forward", "--remove"])
                    .arg(format!("tcp:{old}"))
                    .output();
            }
        }
    }

    fn call_device(&self, request: &str) -> Result<String, String> {
        let port = self.forwarded()?;
        let stream = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), TIMEOUT)
            .map_err(|e| format!("the application's socket: {e}"))?;
        let _ = stream.set_read_timeout(Some(TIMEOUT));
        (&stream)
            .write_all(format!("{request}\n").as_bytes())
            .map_err(|e| format!("the application's socket: {e}"))?;
        let mut answer = String::new();
        BufReader::new(&stream)
            .read_line(&mut answer)
            .map_err(|e| format!("the application did not answer: {e}"))?;
        if answer.trim().is_empty() {
            return Err(
                "the application did not answer: is it running, and is it a debug build?".into(),
            );
        }
        Ok(answer.trim().to_string())
    }

    /// The device's own screenshot, PNG bytes, where the platform takes it rather than the
    /// application: `None` elsewhere.
    fn device_screenshot(&self) -> Option<Result<Vec<u8>, String>> {
        let Relay::Adb { adb, serial, .. } = self else {
            return None;
        };
        let out = Command::new(adb)
            .args(["-s", serial, "exec-out", "screencap", "-p"])
            .output();
        Some(match out {
            Ok(out) if out.status.success() && out.stdout.starts_with(b"\x89PNG") => Ok(out.stdout),
            _ => Err("the device took no screenshot".into()),
        })
    }
}

/// What the page is told about the run.
#[derive(Clone, Debug, Default)]
pub(crate) struct Info {
    pub app: String,
    pub device: String,
    pub platform: String,
}

/// The lines of the application's log, numbered, for the page.
#[derive(Default)]
struct Log {
    lines: std::collections::VecDeque<(u64, String, bool)>,
    next: u64,
}

static LOG: Mutex<Log> = Mutex::new(Log {
    lines: std::collections::VecDeque::new(),
    next: 0,
});

/// Keeps a line of the application's log for DevTools' log view.
pub(crate) fn remember(line: &str, error: bool) {
    if let Ok(mut log) = LOG.lock() {
        if log.lines.len() == KEPT_LINES {
            log.lines.pop_front();
        }
        let n = log.next;
        log.lines.push_back((n, line.to_string(), error));
        log.next += 1;
    }
}

/// The lines after line `after` (all of them for `None`), and the number to ask after next.
fn log_after(after: Option<u64>) -> Value {
    let Ok(log) = LOG.lock() else {
        return object([]);
    };
    let lines: Vec<Value> = log
        .lines
        .iter()
        .filter(|(n, _, _)| after.is_none_or(|after| *n > after))
        .map(|(n, text, error)| {
            object([
                ("number", (*n).into()),
                ("text", text.as_str().into()),
                ("error", (*error).into()),
            ])
        })
        .collect();
    object([("lines", Value::Array(lines))])
}

/// The address of the DevTools of this run, once it is served.
static ADDRESS: OnceLock<String> = OnceLock::new();
/// The run's DevTools, for [`stop`].
static HUB: Mutex<Option<Arc<Hub>>> = Mutex::new(None);

/// A run's DevTools: served on a thread of its own until the program ends.
struct Hub {
    relay: Relay,
    info: Info,
    token: String,
    next_id: AtomicU64,
}

/// A token nobody can guess from outside: the standard library's random hash keys, mixed with
/// the time and the process.
fn token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut text = String::new();
    for round in 0..2u64 {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(round);
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        hasher.write_u32(std::process::id());
        text.push_str(&format!("{:016x}", hasher.finish()));
    }
    text
}

/// Serves DevTools for this run's application, reached through `relay`. The address is said
/// when the keys are (see [`announce`]). A port that cannot be opened leaves the run without
/// DevTools, said once.
pub(crate) fn start(relay: Relay, info: Info) {
    let listener =
        TcpListener::bind(("127.0.0.1", PORT)).or_else(|_| TcpListener::bind(("127.0.0.1", 0)));
    let listener = match listener {
        Ok(listener) => listener,
        Err(error) => {
            report::warn(&format!("DevTools could not be served: {error}"));
            return;
        }
    };
    let Ok(address) = listener.local_addr() else {
        return;
    };
    let hub = Arc::new(Hub {
        relay,
        info,
        token: token(),
        next_id: AtomicU64::new(1),
    });
    let _ = ADDRESS.set(format!(
        "http://127.0.0.1:{}/?token={}",
        address.port(),
        hub.token
    ));
    if let Ok(mut slot) = HUB.lock() {
        *slot = Some(hub.clone());
    }
    std::thread::spawn(move || {
        for stream in listener.incoming().map_while(Result::ok) {
            let hub = hub.clone();
            std::thread::spawn(move || {
                let _ = handle(stream, &hub);
            });
        }
    });
}

/// The run ends: the port forwarded to a device is given back.
pub(crate) fn stop() {
    if let Some(hub) = HUB.lock().ok().and_then(|mut slot| slot.take()) {
        hub.relay.forget_forward();
    }
}

/// The address of this run's DevTools, if it is served.
pub(crate) fn address() -> Option<&'static str> {
    ADDRESS.get().map(String::as_str)
}

/// Says where DevTools is, when the application is up: a line for a person, the `app.devTools`
/// event for an editor.
pub(crate) fn announce() {
    let Some(address) = address() else {
        return;
    };
    if report::is_machine() {
        report::event("app.devTools", object([("uri", address.into())]));
    } else {
        report::say(&format!("DevTools: {address} (press v to open it)"));
    }
}

/// Opens DevTools in the default browser (`v`).
pub(crate) fn open() {
    let Some(address) = address() else {
        report::say("DevTools runs with a debug build: run without --release to have it");
        return;
    };
    let opened = if cfg!(windows) {
        // `start` takes its first quoted argument as a title; the address goes after one.
        Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(address.replace('&', "^&"))
            .spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(address).spawn()
    } else {
        Command::new("xdg-open").arg(address).spawn()
    };
    match opened {
        Ok(_) => report::say(&format!("Opening DevTools: {address}")),
        Err(_) => report::say(&format!("Open DevTools at {address}")),
    }
}

/// The value of `key` in a query string.
fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
}

/// One request to the server: the page, or a call.
fn handle(mut stream: TcpStream, hub: &Hub) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
    let mut length = 0usize;
    let mut token = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            } else if name.eq_ignore_ascii_case("x-frus-devtools") {
                token = Some(value.trim().to_string());
            }
        }
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let token = token.or_else(|| query_value(query, "token").map(str::to_string));
    if token.as_deref() != Some(hub.token.as_str()) {
        return respond(
            &mut stream,
            "403 Forbidden",
            "text/plain",
            b"DevTools needs the address frus run printed, token included.",
        );
    }
    match (method, path) {
        ("GET", "/") => respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            PAGE.as_bytes(),
        ),
        ("POST", "/api") => {
            let mut body = vec![0; length.min(1 << 20)];
            reader.read_exact(&mut body)?;
            let answer = hub.call(&String::from_utf8_lossy(&body));
            respond(&mut stream, "200 OK", "application/json", answer.as_bytes())
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", b"not found"),
    }
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

/// An answer of the hub's own: `{"result":…}` or `{"error":"…"}`.
fn answer(outcome: Result<Value, String>) -> String {
    match outcome {
        Ok(result) => object([("result", result)]).to_json(),
        Err(error) => object([("error", error.into())]).to_json(),
    }
}

impl Hub {
    /// A call, `{"method":…,"params":…}`: answered here, or relayed to the application.
    fn call(&self, body: &str) -> String {
        let Ok(request) = json::parse(body) else {
            return answer(Err("a call is a JSON object".into()));
        };
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            return answer(Err("a call names its method".into()));
        };
        let params = request.get("params").cloned().unwrap_or(object([]));
        match method {
            "status" => answer(Ok(object([
                ("protocol", PROTOCOL.into()),
                ("app", self.info.app.as_str().into()),
                ("device", self.info.device.as_str().into()),
                ("platform", self.info.platform.as_str().into()),
            ]))),
            "logs" => {
                let after = params
                    .get("after")
                    .and_then(Value::as_f64)
                    .map(|n| n as u64);
                answer(Ok(log_after(after)))
            }
            "reload" | "restart" => {
                crate::keys::press(if method == "reload" { 'r' } else { 'R' });
                answer(Ok(object([])))
            }
            "screenshot" if self.relay.device_screenshot_here() => answer(
                self.relay
                    .device_screenshot()
                    .unwrap_or(Err(String::new()))
                    .map(|png| object([("format", "png".into()), ("data", base64(&png).into())])),
            ),
            _ => {
                let id = self.next_id.fetch_add(1, Ordering::SeqCst);
                let request = object([
                    ("id", id.into()),
                    ("method", method.into()),
                    ("params", params),
                ]);
                match self.relay.call(id, &request.to_json()) {
                    // The application's answer, its id dropped: the page did not choose it.
                    Ok(line) => match json::parse(&line) {
                        Ok(value) => match (value.get("result"), value.get("error")) {
                            (Some(result), _) => answer(Ok(result.clone())),
                            (None, Some(error)) => answer(Err(error
                                .as_str()
                                .map_or_else(|| error.to_json(), str::to_string))),
                            _ => answer(Err("the application's answer is empty".into())),
                        },
                        Err(_) => answer(Err("the application's answer is not JSON".into())),
                    },
                    Err(why) => answer(Err(why)),
                }
            }
        }
    }
}

impl Relay {
    fn device_screenshot_here(&self) -> bool {
        matches!(self, Relay::Adb { .. })
    }
}

/// Standard base64, padded.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_relay() -> (Arc<Answers>, Arc<Mutex<Vec<String>>>, Relay) {
        let answers = Arc::new(Answers::default());
        let sent = Arc::new(Mutex::new(Vec::new()));
        let (into, back) = (sent.clone(), answers.clone());
        // An application that answers every request at once, with its method echoed.
        let relay = Relay::Lines {
            send: Box::new(move |line: &str| {
                into.lock().unwrap().push(line.to_string());
                let request = json::parse(line.strip_prefix("devtools ").unwrap()).unwrap();
                let id = request.get("id").and_then(Value::as_f64).unwrap();
                let method = request.get("method").and_then(Value::as_str).unwrap();
                let reply = if method == "boom" {
                    format!(r#"{{"id":{id},"error":"no such thing"}}"#)
                } else {
                    format!(r#"{{"id":{id},"result":{{"method":"{method}"}}}}"#)
                };
                let back = back.clone();
                std::thread::spawn(move || back.deliver(&reply));
                true
            }),
            answers: answers.clone(),
        };
        (answers, sent, relay)
    }

    fn hub(relay: Relay) -> Hub {
        Hub {
            relay,
            info: Info {
                app: "counter".into(),
                device: "Windows".into(),
                platform: "windows".into(),
            },
            token: "t".into(),
            next_id: AtomicU64::new(1),
        }
    }

    /// A call the hub does not answer goes to the application, with an id of the hub's; the
    /// answer comes back without it.
    #[test]
    fn calls_are_relayed_and_answered() {
        let (_, sent, relay) = lines_relay();
        let hub = hub(relay);
        let answer = hub.call(r#"{"method":"tree","params":{"a":1}}"#);
        assert_eq!(answer, r#"{"result":{"method":"tree"}}"#);
        assert_eq!(
            sent.lock().unwrap()[0],
            r#"devtools {"id":1,"method":"tree","params":{"a":1}}"#
        );
        assert_eq!(
            hub.call(r#"{"method":"boom"}"#),
            r#"{"error":"no such thing"}"#
        );
        assert!(hub.call("nope").contains("error"));
        let status = hub.call(r#"{"method":"status"}"#);
        assert!(
            status.contains(r#""protocol":1"#) && status.contains("counter"),
            "{status}"
        );
    }

    /// An application that never answers is waited for, then given up on, and a late answer
    /// is dropped.
    #[test]
    fn an_answer_that_never_comes_is_an_error() {
        let answers = Answers::default();
        let started = std::time::Instant::now();
        let refused = answers.ask(9, || false);
        assert!(refused.unwrap_err().contains("not connected"));
        assert!(started.elapsed() < Duration::from_secs(1));
        answers.deliver(r#"{"id":9,"result":1}"#);
        assert!(answers.waiting.lock().unwrap().is_empty());
    }

    #[test]
    fn the_log_is_kept_and_listed_after_a_number() {
        let before = LOG.lock().unwrap().next;
        remember("one", false);
        remember("two", true);
        let listed = log_after(Some(before)).to_json();
        assert!(listed.contains(r#""text":"two","error":true"#), "{listed}");
        assert!(!listed.contains(r#""text":"one""#), "{listed}");
    }

    /// The page and the API refuse a request without the run's token.
    #[test]
    fn the_server_wants_the_token() {
        let (_, _, relay) = lines_relay();
        let hub = Arc::new(hub(relay));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = hub.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().map_while(Result::ok) {
                let _ = handle(stream, &served);
            }
        });
        let ask = |request: &str| {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
            stream.write_all(request.as_bytes()).unwrap();
            let mut answer = String::new();
            stream.read_to_string(&mut answer).unwrap();
            answer
        };
        assert!(ask("GET / HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 403"));
        assert!(ask("GET /?token=x HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 403"));
        let page = ask("GET /?token=t HTTP/1.1\r\n\r\n");
        assert!(page.starts_with("HTTP/1.1 200") && page.contains("<html"));
        let body = r#"{"method":"hello"}"#;
        let call = ask(&format!(
            "POST /api HTTP/1.1\r\nX-Frus-DevTools: t\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ));
        assert!(call.ends_with(r#"{"result":{"method":"hello"}}"#), "{call}");
    }

    #[test]
    fn tokens_differ() {
        assert_ne!(token(), token());
        assert_eq!(token().len(), 32);
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"fo"), "Zm8=");
    }
}
