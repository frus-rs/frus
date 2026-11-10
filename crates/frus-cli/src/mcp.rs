// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! `frus mcp`: a Model Context Protocol server, so that an AI coding agent can run a frus
//! application, look at it and drive it (milestone 789).
//!
//! The agent's client starts `frus mcp` and speaks JSON-RPC 2.0 on its standard input and
//! output, one message a line: `initialize`, `tools/list`, `tools/call`. Everything the tools do
//! goes through what exists already:
//!
//! - `launch_app` starts `frus run --machine` and reads its events: the build, the log, the
//!   DevTools address, reloads and restarts. `hot_reload` and `hot_restart` are its commands.
//! - Every look into the application and every action on it is a call of the DevTools protocol
//!   (milestone 788) on the address of that run: `tree`, `find`, `tap`, `type`, `screenshot`…
//! - `attach_app` reaches a `frus run` the developer started: by its address, or by the note
//!   each run leaves in the build's directory (`devtools::record`).
//!
//! Standard output carries the protocol and nothing else: what the tool would say goes to
//! standard error, and every program it starts has its output captured.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitCode, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

use crate::json::{self, object, Value};
use crate::project::Project;
use crate::report;

/// The versions of the protocol this server speaks, the newest first. What it uses — tools, and
/// text and image results — is the same in each.
const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// How many lines of a launched run's output are kept.
const KEPT_LINES: usize = 5000;

/// How long a tool waits for an application that is still building or restarting, by default.
const READY_WAIT: Duration = Duration::from_secs(20);

/// How long the application has to say it applied a patch that was built.
const PATCH_APPLIED_WAIT: Duration = Duration::from_secs(15);

/// How long a failed patch's located compile errors are waited for.
const LOCATE_WAIT: Duration = Duration::from_secs(240);

/// The longest text a tool returns: more is cut, its middle dropped.
const MAX_TEXT: usize = 40_000;

/// What the server tells the agent once, at the start.
const INSTRUCTIONS: &str = "\
Drives frus applications (a Rust UI toolkit) for you: run one, read its widget tree and log, \
tap, scroll, type, take screenshots, hot reload after an edit.
Typical session: launch_app (or attach_app when the developer already has `frus run` going), \
get_widget_tree or find_widgets to locate widgets, tap / enter_text / scroll to act, then \
screenshot or find_widgets to check the result. After editing the code, call hot_reload: it \
waits for the change to be in and reports compile errors. get_logs shows the application's \
log and the build's errors. Coordinates are logical pixels from the top-left corner of the \
application's window. stop_app when done.";

/// The tools, as `tools/list` lists them. `FINDER` stands for the properties that find a
/// widget, `APP` for the one that picks the application.
const TOOLS: &str = r#"[
{"name": "list_devices",
 "description": "List the devices a frus app can run on now: this computer, Android phones and emulators adb sees, installed browsers, a web server. Pass an id to launch_app's `device`.",
 "inputSchema": {"type": "object", "properties": {}},
 "annotations": {"readOnlyHint": true}},
{"name": "launch_app",
 "description": "Build and start a frus app (`frus run --machine`) on a device, in debug mode with hot reload, and wait until it is up. Returns the app id and its DevTools address. The first build can take minutes; if it is not up within wait_seconds, call app_status to keep waiting. Build errors are returned. Several apps can run at once; the other tools act on the last one unless given `app_id`.",
 "inputSchema": {"type": "object", "properties": {
   "device": {"type": "string", "description": "Where to run: an id from list_devices, or `desktop`, `android`, `web` (a browser), `web-server`, or `all` (this computer and every connected phone, each its own app). Default: this computer."},
   "devices": {"type": "array", "items": {"type": "string"}, "description": "Several devices at once, each its own app with its own app_id."},
   "project_dir": {"type": "string", "description": "The app's directory (its Cargo.toml). Default: the server's working directory."},
   "flags": {"type": "array", "items": {"type": "string"}, "description": "More `frus run` options, e.g. [\"--no-hot\"] to restart instead of patching, [\"--release\"] (no DevTools then), [\"--\", \"app-arg\"]."},
   "wait_seconds": {"type": "number", "description": "How long to wait for the app to be up. Default 300."}}}},
{"name": "attach_app",
 "description": "Attach to a `frus run` that is already running (started by the developer, in debug mode), instead of launching one. Without arguments, finds the run of the app in project_dir. The other tools also attach by themselves when no app is launched.",
 "inputSchema": {"type": "object", "properties": {
   "uri": {"type": "string", "description": "The DevTools address `frus run` printed: http://127.0.0.1:PORT/?token=..."},
   "project_dir": {"type": "string", "description": "The app's directory, to find its running `frus run`. Default: the server's working directory."}}}},
{"name": "app_status",
 "description": "The apps this server knows: id, device, whether it is building, running or stopped, its DevTools address, the last thing frus said. With wait_seconds, first waits for the app to be up (e.g. after launch_app timed out).",
 "inputSchema": {"type": "object", "properties": {APP,
   "wait_seconds": {"type": "number", "description": "Wait up to this long for the app to be up."}}},
 "annotations": {"readOnlyHint": true}},
{"name": "stop_app",
 "description": "Stop the app and its `frus run`.",
 "inputSchema": {"type": "object", "properties": {APP}},
 "annotations": {"destructiveHint": true}},
{"name": "hot_reload",
 "description": "Bring code edits into the running app, keeping its state. With hot reload (the default) a saved file is patched in on its own: this waits until the last save is in and says how it went, compile errors included. With --no-hot, it rebuilds and restarts the app with its state saved and restored. Call it after editing the app's sources.",
 "inputSchema": {"type": "object", "properties": {APP,
   "wait_seconds": {"type": "number", "description": "How long to wait for the build. Default 300."}}}},
{"name": "hot_restart",
 "description": "Rebuild and restart the app from scratch, its state lost. For changes hot reload cannot bring in (e.g. new dependencies, the app's startup code).",
 "inputSchema": {"type": "object", "properties": {APP,
   "wait_seconds": {"type": "number", "description": "How long to wait for the build. Default 300."}}}},
{"name": "get_logs",
 "description": "The app's log and frus's own messages (build output, compile errors, panics). Each line has a number; pass the last one seen as `since` to get only newer lines.",
 "inputSchema": {"type": "object", "properties": {APP,
   "since": {"type": "number", "description": "Only lines after this number."},
   "limit": {"type": "number", "description": "At most this many lines, the newest. Default 100."},
   "errors_only": {"type": "boolean", "description": "Only errors: compile errors, panics, the app's error and warning lines."}}},
 "annotations": {"readOnlyHint": true}},
{"name": "get_widget_tree",
 "description": "The widget tree on screen now, as indented text: type, id, box [x,y widthxheight] in logical pixels, key, the words it paints, its semantics (role, label, value, actions). Ids work with get_widget, tap, etc. Large trees are cut: use depth, limit, or root to look at a part.",
 "inputSchema": {"type": "object", "properties": {APP,
   "root": {"type": "string", "description": "Only the subtree of this widget id."},
   "depth": {"type": "number", "description": "Only this many levels below the root."},
   "limit": {"type": "number", "description": "At most this many widgets. Default 250."}}},
 "annotations": {"readOnlyHint": true}},
{"name": "find_widgets",
 "description": "Find widgets on screen by the words they paint, their semantics label, their type name or key. All criteria given must hold. Returns each match with its id, box and center. Cheaper than get_widget_tree when you know what you look for.",
 "inputSchema": {"type": "object", "properties": {APP, FINDER}},
 "annotations": {"readOnlyHint": true}},
{"name": "get_widget",
 "description": "One widget in detail: box, layout style (sizes, padding, margin, flex), semantics, ancestors and children.",
 "inputSchema": {"type": "object", "properties": {APP,
   "id": {"type": "string", "description": "The widget's id (16 hex digits) from get_widget_tree or find_widgets."}},
  "required": ["id"]},
 "annotations": {"readOnlyHint": true}},
{"name": "widget_at",
 "description": "The deepest widget at a point, in logical pixels.",
 "inputSchema": {"type": "object", "properties": {APP,
   "x": {"type": "number"}, "y": {"type": "number"}}, "required": ["x", "y"]},
 "annotations": {"readOnlyHint": true}},
{"name": "tap",
 "description": "Tap a widget found by its words, label, type, key or id (the one on top when several match), or a point in logical pixels (x, y). A real pointer down and up: buttons, list items, fields (which then take the focus).",
 "inputSchema": {"type": "object", "properties": {APP, FINDER}}},
{"name": "scroll",
 "description": "Scroll the scrollable area under a widget or a point by dx, dy logical pixels, as a mouse wheel does. dy > 0 brings what is further down into view.",
 "inputSchema": {"type": "object", "properties": {APP, FINDER,
   "dx": {"type": "number", "description": "Default 0."},
   "dy": {"type": "number", "description": "Default 0."}}}},
{"name": "enter_text",
 "description": "Type text into a text field: into `target` (tapped first to focus it) or the field that has the focus. Typed key by key, so the field reacts as to a keyboard; with replace: true, the field's whole value is set at once instead (target required).",
 "inputSchema": {"type": "object", "properties": {APP,
   "text": {"type": "string", "description": "The text to type."},
   "target": {"type": "object", "description": "The field: a finder (text, label, type, key, id, contains) or a point (x, y).", "properties": {FINDER}},
   "replace": {"type": "boolean", "description": "Set the field's whole value instead of typing."}},
  "required": ["text"]}},
{"name": "press_key",
 "description": "Press a key in the app: Enter, Tab, Escape, Backspace, Delete, ArrowUp/Down/Left/Right, Home, End, PageUp, PageDown, F1-F12, a character, or a combination like ctrl+a, shift+Tab.",
 "inputSchema": {"type": "object", "properties": {APP,
   "key": {"type": "string"}}, "required": ["key"]}},
{"name": "go_back",
 "description": "The system's Back (Android's back button, Escape-like on desktop): pops a route or closes a dialog, never leaves the app. Says whether it went back.",
 "inputSchema": {"type": "object", "properties": {APP}}},
{"name": "screenshot",
 "description": "A screenshot of the app as an image (PNG). On Android it is the device's whole screen. Downscaled so its longest side is at most max_size pixels (default 1024); the text says how image pixels map to the logical pixels tap takes.",
 "inputSchema": {"type": "object", "properties": {APP,
   "max_size": {"type": "number", "description": "Longest side in pixels; 0 keeps the full size. Default 1024."},
   "save_to": {"type": "string", "description": "Also save the full-size PNG to this file."}}},
 "annotations": {"readOnlyHint": true}},
{"name": "set_animation_speed",
 "description": "Slow animations down (or speed them up) to watch or capture them: 1 is normal, 0.1 is ten times slower.",
 "inputSchema": {"type": "object", "properties": {APP,
   "speed": {"type": "number"}}, "required": ["speed"]}},
{"name": "set_debug_options",
 "description": "Debug switches of the running app. Returns all of them after the change.",
 "inputSchema": {"type": "object", "properties": {APP,
   "debug_paint": {"type": "boolean", "description": "Outline every widget's box."},
   "performance_overlay": {"type": "boolean", "description": "Show frame timings over the app."},
   "brightness": {"type": "string", "enum": ["light", "dark", "system"]},
   "platform": {"type": "string", "enum": ["android", "ios", "windows", "macos", "linux", "fuchsia", "default"], "description": "The platform the widgets behave and look as."},
   "text_scale": {"type": "number", "description": "Text size factor; 0 for the system's."}}}},
{"name": "get_frame_stats",
 "description": "Frame timings: build, layout and paint, render, in milliseconds against the 16.7 ms budget of 60 Hz, and the slow (janky) frames. Recording starts with the first call: call once, interact, call again.",
 "inputSchema": {"type": "object", "properties": {APP,
   "since": {"type": "number", "description": "Only frames after this number."}}},
 "annotations": {"readOnlyHint": true}},
{"name": "analyze",
 "description": "Run `frus analyze` in the app's directory: clippy and rustfmt, every problem listed with its place.",
 "inputSchema": {"type": "object", "properties": {
   "project_dir": {"type": "string"},
   "fix": {"type": "boolean", "description": "Apply formatting and clippy's suggestions."}}}},
{"name": "run_tests",
 "description": "Run `frus test` in the app's directory: its tests, a summary, each failure with its place and message.",
 "inputSchema": {"type": "object", "properties": {
   "project_dir": {"type": "string"},
   "filter": {"type": "string", "description": "Only tests whose name contains this."}}}}
]"#;

/// The properties that find a widget.
const FINDER: &str = r#""text": {"type": "string", "description": "The words the widget paints (whole words unless contains)."},
   "label": {"type": "string", "description": "Its semantics label (what a screen reader says)."},
   "type": {"type": "string", "description": "Its type name, e.g. Button, TextField."},
   "key": {"type": "string", "description": "Its key, as get_widget_tree shows it."},
   "id": {"type": "string", "description": "Its id."},
   "contains": {"type": "boolean", "description": "Match text and label as a part, ignoring case."},
   "x": {"type": "number", "description": "Or a point: x in logical pixels."},
   "y": {"type": "number", "description": "And y."}"#;

/// The property that picks the application.
const APP: &str = r#""app_id": {"type": "string", "description": "Which app, when several run (from launch_app or app_status). Default: the last one."}"#;

/// The tools' definitions.
fn tools() -> Value {
    json::parse(&TOOLS.replace("FINDER", FINDER).replace("APP", APP))
        .unwrap_or(Value::Array(Vec::new()))
}

// --- Talking to a run's DevTools ---

/// The DevTools of a run: where, and its token.
#[derive(Clone, Debug, PartialEq)]
struct Hub {
    host: String,
    token: String,
}

impl Hub {
    /// From the address `frus run` prints: `http://127.0.0.1:9100/?token=…`.
    fn from_uri(uri: &str) -> Result<Hub, String> {
        let rest = uri
            .trim()
            .strip_prefix("http://")
            .ok_or_else(|| format!("`{uri}` is not a DevTools address (http://…/?token=…)"))?;
        let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
        let token = path
            .split_once('?')
            .map_or("", |(_, query)| query)
            .split('&')
            .find_map(|pair| pair.strip_prefix("token="))
            .filter(|token| !token.is_empty())
            .ok_or_else(|| {
                format!("`{uri}` has no token: use the whole address frus run printed")
            })?;
        Ok(Hub {
            host: host.to_string(),
            token: token.to_string(),
        })
    }

    fn uri(&self) -> String {
        format!("http://{}/?token={}", self.host, self.token)
    }

    /// A call of the protocol: its result, or the error it answered.
    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let body = object([("method", method.into()), ("params", params)]).to_json();
        let mut stream = TcpStream::connect(&self.host).map_err(|e| {
            format!(
                "DevTools at {} cannot be reached ({e}): has the run ended?",
                self.host
            )
        })?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
        write!(
            stream,
            "POST /api HTTP/1.1\r\nHost: {}\r\nX-Frus-DevTools: {}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.host,
            self.token,
            body.len()
        )
        .map_err(|e| e.to_string())?;
        let mut answer = Vec::new();
        stream
            .read_to_end(&mut answer)
            .map_err(|e| format!("DevTools did not answer: {e}"))?;
        let answer = String::from_utf8_lossy(&answer);
        let (head, body) = answer.split_once("\r\n\r\n").unwrap_or((&answer, ""));
        if !head.starts_with("HTTP/1.1 200") {
            return Err(format!(
                "DevTools refused the call: {}",
                head.lines().next().unwrap_or("").trim()
            ));
        }
        let value = json::parse(body).map_err(|_| "DevTools' answer is not JSON".to_string())?;
        match (value.get("result"), value.get("error")) {
            (Some(result), _) => Ok(result.clone()),
            (None, Some(error)) => Err(error
                .as_str()
                .map_or_else(|| error.to_json(), str::to_string)),
            _ => Err("DevTools' answer is empty".into()),
        }
    }
}

// --- A run ---

/// A line of a launched run's output.
#[derive(Clone, Debug)]
struct Line {
    number: u64,
    /// `app` (the application's log), `frus` (the tool's messages), `build` (cargo's output).
    source: &'static str,
    text: String,
    error: bool,
    at: SystemTime,
}

/// How a reload or a restart went.
#[derive(Clone, Debug, PartialEq)]
enum Outcome {
    Done { full: bool, seconds: f64 },
    Failed(String),
}

/// What is known of a run, kept up to date by the threads reading its output.
#[derive(Default)]
struct State {
    hub: Option<Hub>,
    device: String,
    started: bool,
    /// Why the run ended, once it has.
    stopped: Option<String>,
    /// Whether a saved file is patched in by itself.
    hot: bool,
    lines: VecDeque<Line>,
    next_line: u64,
    outcomes: Vec<(SystemTime, Outcome)>,
    /// The last thing the tool said.
    progress: String,
}

impl State {
    fn log(&mut self, source: &'static str, text: &str, error: bool) {
        if self.lines.len() == KEPT_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(Line {
            number: self.next_line,
            source,
            text: text.to_string(),
            error,
            at: SystemTime::now(),
        });
        self.next_line += 1;
    }

    /// Takes in one line of `frus run --machine`'s standard output.
    fn event(&mut self, line: &str) {
        let Ok(value) = json::parse(line) else {
            if !line.trim().is_empty() {
                self.log("frus", line, false);
            }
            return;
        };
        let Some(name) = value.get("event").and_then(Value::as_str) else {
            return;
        };
        let params = value.get("params").cloned().unwrap_or(object([]));
        let text = |key: &str| params.get(key).and_then(Value::as_str).unwrap_or("");
        let error = params.get("error").and_then(Value::as_bool) == Some(true);
        match name {
            "app.start" => self.device = text("deviceName").to_string(),
            "app.progress" => {
                let message = text("message").to_string();
                self.log("frus", &message, error);
                if message.contains("hot reload is off") {
                    self.hot = false;
                }
                if error {
                    self.outcomes
                        .push((SystemTime::now(), Outcome::Failed(message.clone())));
                }
                self.progress = message;
            }
            "app.log" => self.log("app", text("log"), error),
            "app.started" => self.started = true,
            "app.devTools" => self.hub = Hub::from_uri(text("uri")).ok(),
            "app.restarted" => {
                let full = params.get("fullRestart").and_then(Value::as_bool) == Some(true);
                let seconds = params.get("seconds").and_then(Value::as_f64).unwrap_or(0.0);
                self.outcomes
                    .push((SystemTime::now(), Outcome::Done { full, seconds }));
            }
            "app.stop" => {
                let why = params.get("error").and_then(Value::as_str);
                self.stopped = Some(why.map_or_else(|| "it stopped".to_string(), str::to_string));
            }
            _ => {}
        }
    }

    /// The first outcome at or after `since`.
    fn outcome_since(&self, since: SystemTime) -> Option<Outcome> {
        self.outcomes
            .iter()
            .find(|(at, _)| *at >= since)
            .map(|(_, outcome)| outcome.clone())
    }

    /// Whether the application said it applied a patch since `since`.
    fn patch_applied_since(&self, since: SystemTime) -> bool {
        self.lines.iter().any(|l| {
            l.at >= since && l.source == "app" && l.text.contains("hot reload: patch applied")
        })
    }

    /// Whether a compile error with its place (`src/x.rs:40:28: error…`) was said since `since`.
    fn located_error_since(&self, since: SystemTime) -> bool {
        self.outcomes.iter().any(|(at, outcome)| {
            *at >= since && matches!(outcome, Outcome::Failed(why) if why.contains(": error"))
        })
    }

    /// The last few lines, for a message that needs them: the errors only, and only those since
    /// a time, when asked.
    fn tail(&self, n: usize, errors_since: Option<SystemTime>) -> String {
        let lines: Vec<&Line> = self
            .lines
            .iter()
            .filter(|l| errors_since.is_none_or(|since| l.error && l.at >= since))
            .collect();
        lines[lines.len().saturating_sub(n)..]
            .iter()
            .map(|l| format!("[{}] {}", l.source, l.text))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Waits until `done` holds, or `wait` has passed: whether it holds.
    fn wait_until(&self, wait: Duration, mut done: impl FnMut(&State) -> bool) -> bool {
        let deadline = Instant::now() + wait;
        let mut state = self.lock();
        loop {
            if done(&state) {
                return true;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, left.min(Duration::from_millis(500)))
                .map(|(state, _)| state)
                .unwrap_or_else(|e| e.into_inner().0);
        }
    }
}

/// An application the server drives: one it launched, or one it attached to.
struct Session {
    id: String,
    /// The `frus run` it started, and its input: none when attached.
    child: Option<Child>,
    input: Option<ChildStdin>,
    root: Option<PathBuf>,
    started_at: SystemTime,
    shared: Arc<Shared>,
    commands: u64,
}

impl Session {
    fn hub(&self) -> Option<Hub> {
        self.shared.lock().hub.clone()
    }

    /// A command to the launched `frus run` (`app.restart`, `app.stop`).
    fn command(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.commands += 1;
        let line = object([
            ("id", self.commands.into()),
            ("method", method.into()),
            ("params", params),
        ])
        .to_json();
        let input = self
            .input
            .as_mut()
            .ok_or("this app was attached to, not launched")?;
        writeln!(input, "{line}")
            .and_then(|_| input.flush())
            .map_err(|_| "frus run has ended".to_string())
    }

    /// A call of the application's DevTools, once it is up: a run still building is waited for
    /// (`wait`), and an application restarting is asked again until it is back.
    fn call(&self, method: &str, params: Value, wait: Duration) -> Result<Value, String> {
        let deadline = Instant::now() + wait;
        if self.child.is_some() {
            let up = self
                .shared
                .wait_until(wait, |s| s.hub.is_some() || s.stopped.is_some());
            let state = self.shared.lock();
            if let Some(why) = &state.stopped {
                return Err(format!(
                    "the app is not running: {why}\nLast lines:\n{}",
                    state.tail(15, None)
                ));
            }
            if !up {
                return Err(format!(
                    "the app is not up yet (last: {}). Call app_status with wait_seconds to wait \
                     for it.",
                    state.progress
                ));
            }
        }
        let hub = self
            .hub()
            .ok_or("this run has no DevTools: it is a release build")?;
        loop {
            match hub.call(method, params.clone()) {
                Err(why)
                    if (why.contains("not connected") || why.contains("did not answer"))
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(300));
                }
                outcome => return outcome,
            }
        }
    }

    /// Ends the run: asked to stop, then made to.
    fn stop(&mut self) -> String {
        let Some(child) = self.child.as_mut() else {
            return match self.hub().map(|hub| hub.call("stop", object([]))) {
                Some(Ok(_)) => format!("Asked the `frus run` of {} to stop.", self.id),
                _ => format!("Detached from {}.", self.id),
            };
        };
        let pid = child.id();
        let _ = self.command("app.stop", object([]));
        let deadline = Instant::now() + Duration::from_secs(20);
        let child = self.child.as_mut().expect("launched");
        while Instant::now() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                return format!("Stopped {}.", self.id);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        kill_tree(child, pid);
        format!("Stopped {} (it had to be killed).", self.id)
    }
}

/// Ends a process and the ones it started.
fn kill_tree(child: &mut Child, pid: u32) {
    if cfg!(windows) {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Starts `frus run --machine` in `root`, its output read into a session.
///
/// `slot` is set when this run is one of several the agent launched together: each then keeps
/// its own copy of the sources and its own build, as `frus run -d all`'s runs do.
fn launch(
    root: &Path,
    device: Option<&str>,
    flags: &[String],
    slot: Option<&str>,
) -> Result<Session, String> {
    let program = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = Command::new(program);
    command.args(["run", "--machine"]);
    if let Some(device) = device {
        command.args(["-d", device]);
    }
    if let Some(slot) = slot {
        command.env(crate::sessions::SESSION_ENV, slot);
    }
    command
        .args(flags)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start frus run: {e}"))?;
    let has = |flag: &str| flags.iter().any(|f| f == flag);
    let shared = Arc::new(Shared::default());
    shared.lock().hot = has("--hot") || !(has("--no-hot") || has("--release"));
    if let Some(out) = child.stdout.take() {
        let shared = shared.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                shared.lock().event(&line);
                shared.changed.notify_all();
            }
            let mut state = shared.lock();
            if state.stopped.is_none() {
                state.stopped = Some("frus run ended".into());
            }
            drop(state);
            shared.changed.notify_all();
        });
    }
    if let Some(err) = child.stderr.take() {
        let shared = shared.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let trimmed = line.trim_start();
                let error = trimmed.starts_with("error");
                shared.lock().log("build", &line, error);
                shared.changed.notify_all();
            }
        });
    }
    Ok(Session {
        id: format!("frus-run-{}", child.id()),
        input: child.stdin.take(),
        child: Some(child),
        root: Some(root.to_path_buf()),
        started_at: SystemTime::now(),
        shared,
        commands: 0,
    })
}

/// A session for a run that is already going, reached through `hub`.
fn attached(hub: Hub, root: Option<PathBuf>) -> Result<Session, String> {
    let status = hub.call("status", object([]))?;
    let text = |key: &str| {
        status
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let shared = Arc::new(Shared::default());
    {
        let mut state = shared.lock();
        state.device = text("device");
        state.started = true;
        state.hub = Some(hub.clone());
    }
    Ok(Session {
        id: format!("{}@{}", text("app"), hub.host),
        child: None,
        input: None,
        root,
        started_at: SystemTime::now(),
        shared,
        commands: 0,
    })
}

/// The DevTools of the runs noted in `dir` (see `devtools::record`) for the application at
/// `root`, the newest first. Notes of runs that no longer answer are removed.
fn noted_runs(dir: &Path, root: &Path) -> Vec<Hub> {
    let same = |a: &Path, b: &Path| {
        std::fs::canonicalize(a)
            .ok()
            .zip(std::fs::canonicalize(b).ok())
            .is_some_and(|(a, b)| a == b)
    };
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        let Ok(note) = std::fs::read_to_string(&path).map(|text| json::parse(&text)) else {
            continue;
        };
        let Ok(note) = note else { continue };
        let noted_root = note.get("root").and_then(Value::as_str).unwrap_or("");
        let Some(hub) = note
            .get("uri")
            .and_then(Value::as_str)
            .and_then(|uri| Hub::from_uri(uri).ok())
        else {
            continue;
        };
        if hub.call("status", object([])).is_err() {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        if same(Path::new(noted_root), root) {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            found.push((modified, hub));
        }
    }
    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    found.into_iter().map(|(_, hub)| hub).collect()
}

/// The newest time a file of the application's sources was written.
fn newest_source(root: &Path) -> Option<SystemTime> {
    fn walk(dir: &Path, newest: &mut Option<SystemTime>) {
        for entry in std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, newest);
            } else if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
                *newest = Some(newest.map_or(modified, |n: SystemTime| n.max(modified)));
            }
        }
    }
    let mut newest = None;
    walk(&root.join("src"), &mut newest);
    newest
}

// --- The server ---

struct Server {
    sessions: Vec<Session>,
    /// Where the server was started: the default project.
    here: PathBuf,
}

/// A tool's result: its content, or an error said to the agent.
type ToolResult = Result<Vec<Value>, String>;

fn text(content: impl Into<String>) -> Value {
    object([
        ("type", "text".into()),
        ("text", clip(content.into()).into()),
    ])
}

/// A text kept under [`MAX_TEXT`]: its start and its end.
fn clip(text: String) -> String {
    if text.len() <= MAX_TEXT {
        return text;
    }
    let mut head = MAX_TEXT / 4;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len() - MAX_TEXT * 3 / 4;
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    format!(
        "{}\n… ({} bytes cut) …\n{}",
        &text[..head],
        tail - head,
        &text[tail..]
    )
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn arg_f64(args: &Value, key: &str) -> Option<f64> {
    args.get(key).and_then(Value::as_f64)
}

fn arg_bool(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool) == Some(true)
}

fn seconds(args: &Value, key: &str, default: u64) -> Duration {
    Duration::from_secs_f64(
        arg_f64(args, key)
            .unwrap_or(default as f64)
            .clamp(0.0, 3600.0),
    )
}

/// The finder of a call: `text`, `label`, `type`, `key`, `id`, `contains`, or a point.
fn finder(args: &Value) -> Value {
    let mut members = Vec::new();
    for key in ["text", "label", "type", "key", "id", "contains", "x", "y"] {
        if let Some(value) = args.get(key) {
            if *value != Value::Null {
                members.push((key.to_string(), value.clone()));
            }
        }
    }
    Value::Object(members)
}

fn is_empty(value: &Value) -> bool {
    matches!(value, Value::Object(members) if members.is_empty())
}

fn number(value: Option<&Value>) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(0.0)
}

/// `[x,y widthxheight]`, rounded.
fn rect_text(rect: Option<&Value>) -> String {
    let n = |key: &str| number(rect.and_then(|r| r.get(key))).round();
    format!("[{},{} {}x{}]", n("x"), n("y"), n("width"), n("height"))
}

/// Text quoted on one line, cut at 80 characters.
fn quoted(text: &str) -> String {
    let one_line = text.replace('\n', "⏎");
    let cut: String = one_line.chars().take(80).collect();
    if cut.len() < one_line.len() {
        format!("{cut:?}…")
    } else {
        format!("{cut:?}")
    }
}

/// The widget tree as indented text: one line a widget.
fn tree_text(tree: &Value, root: Option<&str>, depth: Option<usize>, limit: usize) -> String {
    let nodes = tree.get("nodes").and_then(Value::as_array).unwrap_or(&[]);
    let parent = |i: usize| {
        nodes[i]
            .get("parent")
            .and_then(Value::as_f64)
            .map(|p| p as usize)
            .filter(|&p| p < nodes.len())
    };
    let root_index = root.map(|id| {
        nodes
            .iter()
            .position(|n| n.get("id").and_then(Value::as_str) == Some(id))
    });
    if let Some(None) = root_index {
        return format!("No widget {} on screen now.", root.unwrap_or(""));
    }
    let root_index = root_index.flatten();
    // Each node's level below the root, if it is in the root's subtree.
    let mut level: Vec<Option<usize>> = vec![None; nodes.len()];
    for i in 0..nodes.len() {
        level[i] = match (root_index, parent(i)) {
            (Some(r), _) if r == i => Some(0),
            (Some(_), Some(p)) if p < i => level[p].map(|l| l + 1),
            (Some(_), _) => None,
            (None, Some(p)) if p < i => level[p].map(|l| l + 1),
            (None, _) => Some(0),
        };
    }
    let size = tree.get("size");
    let mut out = format!(
        "{} widgets on screen, window {}x{} logical pixels. Line: type #id [x,y widthxheight] \
         key text semantics.\n",
        nodes.len(),
        number(size.and_then(|s| s.get("width"))).round(),
        number(size.and_then(|s| s.get("height"))).round(),
    );
    let (mut shown, mut skipped) = (0, 0);
    for (i, node) in nodes.iter().enumerate() {
        let Some(l) = level[i] else { continue };
        if depth.is_some_and(|d| l > d) {
            continue;
        }
        if shown == limit {
            skipped += 1;
            continue;
        }
        shown += 1;
        out.push_str(&"  ".repeat(l.min(40)));
        out.push_str(&node_line(node));
        out.push('\n');
    }
    if skipped > 0 {
        out.push_str(&format!(
            "… {skipped} more widgets not shown: pass root=<id> to see a part, a smaller depth, \
             or a bigger limit.\n"
        ));
    }
    out
}

/// One widget of the tree, on a line.
fn node_line(node: &Value) -> String {
    let s = |key: &str| node.get(key).and_then(Value::as_str);
    let mut line = format!(
        "{} #{} {}",
        s("name").unwrap_or("?"),
        s("id").unwrap_or("?"),
        rect_text(node.get("rect"))
    );
    if let Some(key) = s("key") {
        line.push_str(&format!(" key={key}"));
    }
    if let Some(text) = s("text") {
        line.push_str(&format!(" text={}", quoted(text)));
    }
    if let Some(sem) = node.get("semantics") {
        let ss = |key: &str| sem.get(key).and_then(Value::as_str);
        let mut parts = Vec::new();
        if let Some(role) = ss("role").filter(|r| *r != "None" && *r != "Generic") {
            parts.push(role.to_string());
        }
        if let Some(label) = ss("label").filter(|l| Some(*l) != s("text")) {
            parts.push(format!("label={}", quoted(label)));
        }
        if let Some(value) = ss("value") {
            parts.push(format!("value={}", quoted(value)));
        }
        for flag in ["toggled", "expanded"] {
            if let Some(v) = ss(flag) {
                parts.push(format!("{flag}={v}"));
            }
        }
        if sem.get("disabled").is_some() {
            parts.push("disabled".into());
        }
        if let Some(actions) = sem.get("actions").and_then(Value::as_array) {
            let names: Vec<&str> = actions.iter().filter_map(Value::as_str).collect();
            if !names.is_empty() {
                parts.push(format!("actions={}", names.join("/")));
            }
        }
        if !parts.is_empty() {
            line.push_str(&format!(" ({})", parts.join(", ")));
        }
    }
    line
}

/// A widget `find` or `at` answered, on a line.
fn summary_line(node: &Value) -> String {
    let s = |key: &str| node.get(key).and_then(Value::as_str);
    let center = node.get("center");
    let mut line = format!(
        "{} #{} center ({}, {}) {}",
        s("name").unwrap_or("?"),
        s("id").unwrap_or("?"),
        number(center.and_then(|c| c.get("x"))).round(),
        number(center.and_then(|c| c.get("y"))).round(),
        rect_text(node.get("rect"))
    );
    if let Some(text) = s("text") {
        line.push_str(&format!(" text={}", quoted(text)));
    }
    if let Some(label) = s("label") {
        line.push_str(&format!(" label={}", quoted(label)));
    }
    line
}

/// The frames `frames` answered, summed up.
fn frames_text(frames: &Value) -> String {
    let budget = number(frames.get("budget")).max(1.0);
    let list = frames
        .get("frames")
        .and_then(Value::as_array)
        .unwrap_or(&[]);
    if list.is_empty() {
        return "No frames recorded yet: recording starts with this call. Interact with the app \
                (or let it animate), then call get_frame_stats again."
            .into();
    }
    let f = |frame: &Value, key: &str| number(frame.get(key));
    let own = |frame: &Value| f(frame, "build") + f(frame, "layoutPaint") + f(frame, "draw");
    let n = list.len() as f64;
    let avg = |key: &str| list.iter().map(|fr| f(fr, key)).sum::<f64>() / n;
    let janky: Vec<&Value> = list.iter().filter(|fr| own(fr) > budget).collect();
    let worst = list.iter().map(own).fold(0.0, f64::max);
    let mut out = format!(
        "{} frames (#{} to #{}), budget {budget:.1} ms. Average: build {:.2} ms, layout+paint \
         {:.2} ms, render {:.2} ms (display wait included). Worst own work {worst:.2} ms. Janky \
         (own work over budget): {}.\n",
        list.len(),
        f(&list[0], "number"),
        f(&list[list.len() - 1], "number"),
        avg("build"),
        avg("layoutPaint"),
        avg("render"),
        janky.len(),
    );
    out.push_str("Last frames (number: build / layout+paint / render ms, why):\n");
    for frame in &list[list.len().saturating_sub(12)..] {
        let why = frame
            .get("why")
            .and_then(Value::as_array)
            .map(|w| {
                w.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "#{}: {:.2} / {:.2} / {:.2}{} {why}\n",
            f(frame, "number"),
            f(frame, "build"),
            f(frame, "layoutPaint"),
            f(frame, "render"),
            if own(frame) > budget { " JANK" } else { "" }
        ));
    }
    out.push_str(&format!(
        "Call again with since={} for newer frames only.",
        f(&list[list.len() - 1], "number")
    ));
    out
}

/// Standard base64, read.
fn unbase64(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut count) = (0u32, 0);
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return Err("the screenshot is not base64".into()),
        };
        bits = (bits << 6) | u32::from(v);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    Ok(out)
}

/// A frus command run in the application's directory, its output captured.
fn frus_command(root: &Path, args: &[&str]) -> ToolResult {
    let program = std::env::current_exe().map_err(|e| e.to_string())?;
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("could not run frus {}: {e}", args.join(" ")))?;
    let mut said = String::from_utf8_lossy(&output.stdout).into_owned();
    let errors = String::from_utf8_lossy(&output.stderr);
    if !errors.trim().is_empty() {
        said.push_str("\n--- standard error ---\n");
        said.push_str(&errors);
    }
    let verdict = if output.status.success() {
        "succeeded"
    } else {
        "failed"
    };
    let report = format!("`frus {}` {verdict}.\n{said}", args.join(" "));
    if output.status.success() {
        Ok(vec![text(report)])
    } else {
        Err(clip(report))
    }
}

impl Server {
    fn project_root(&self, args: &Value) -> Result<PathBuf, String> {
        let dir = arg_str(args, "project_dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.here.clone());
        Ok(Project::find(&dir)?.root)
    }

    /// The session a tool acts on: `app_id`'s, or the last one; with none, a run of the
    /// project's found and attached to.
    fn session(&mut self, args: &Value) -> Result<&mut Session, String> {
        if let Some(id) = arg_str(args, "app_id") {
            return self
                .sessions
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or_else(|| format!("no app `{id}`: app_status lists them"));
        }
        if self.sessions.is_empty() {
            let root = self.project_root(args).ok();
            if let Some(hub) = root.as_ref().and_then(|root| self.discover(root).ok()) {
                let session = attached(hub, root)?;
                self.sessions.push(session);
            }
        }
        self.sessions.last_mut().ok_or_else(|| {
            "no app: start one with launch_app, or attach_app to a `frus run` that is going".into()
        })
    }

    fn discover(&self, root: &Path) -> Result<Hub, String> {
        let project = Project::find(root)?;
        let target = project.target_dir()?;
        noted_runs(&crate::devtools::runs_dir(&target), &project.root)
            .into_iter()
            .next()
            .ok_or_else(|| {
                format!(
                    "no `frus run` of {} is going (in debug): start one with launch_app, or pass \
                     the DevTools address it printed as uri",
                    project.root.display()
                )
            })
    }

    /// An application call through the session `args` picks.
    fn app(&mut self, args: &Value, method: &str, params: Value) -> Result<Value, String> {
        self.session(args)?.call(method, params, READY_WAIT)
    }

    fn call_tool(&mut self, name: &str, args: &Value) -> ToolResult {
        match name {
            "list_devices" => {
                let found = crate::devices::discover();
                Ok(vec![text(format!(
                    "{}\nPass the id (second column) as launch_app's device.",
                    crate::devices::table(&found)
                ))])
            }
            "launch_app" => self.launch_app(args),
            "attach_app" => {
                let session = match arg_str(args, "uri") {
                    Some(uri) => attached(Hub::from_uri(uri)?, self.project_root(args).ok())?,
                    None => {
                        let root = self.project_root(args)?;
                        attached(self.discover(&root)?, Some(root))?
                    }
                };
                // One lock at a time: a guard held through the statement would deadlock.
                let device = session.shared.lock().device.clone();
                let uri = session.hub().map(|h| h.uri()).unwrap_or_default();
                let says = format!("Attached to {} on {device}. DevTools: {uri}", session.id);
                self.sessions.retain(|s| s.id != session.id);
                self.sessions.push(session);
                Ok(vec![text(says)])
            }
            "app_status" => self.app_status(args),
            "stop_app" => {
                let id = self.session(args)?.id.clone();
                let index = self
                    .sessions
                    .iter()
                    .position(|s| s.id == id)
                    .ok_or("no app")?;
                let mut session = self.sessions.remove(index);
                Ok(vec![text(session.stop())])
            }
            "hot_reload" => self.reload(args, false),
            "hot_restart" => self.reload(args, true),
            "get_logs" => self.logs(args),
            "get_widget_tree" => {
                let tree = self.app(args, "tree", object([]))?;
                let depth = arg_f64(args, "depth").map(|d| d.max(0.0) as usize);
                let limit = arg_f64(args, "limit").map_or(250, |l| l.max(1.0) as usize);
                Ok(vec![text(tree_text(
                    &tree,
                    arg_str(args, "root"),
                    depth,
                    limit,
                ))])
            }
            "find_widgets" => {
                let find = finder(args);
                if is_empty(&find) {
                    return Err("say what to find: text, label, type, key or id".into());
                }
                let found = self.app(args, "find", find)?;
                let nodes = found.get("nodes").and_then(Value::as_array).unwrap_or(&[]);
                if nodes.is_empty() {
                    return Ok(vec![text(
                        "No widget matches. text and label match whole words: try contains: \
                         true, or get_widget_tree to see what is there.",
                    )]);
                }
                let mut out = format!(
                    "{} found, in paint order (tap acts on the last, on top):\n",
                    nodes.len()
                );
                for node in nodes {
                    out.push_str(&summary_line(node));
                    out.push('\n');
                }
                Ok(vec![text(out)])
            }
            "get_widget" => {
                let id = arg_str(args, "id").ok_or("id is missing")?;
                let node = self.app(args, "node", object([("id", id.into())]))?;
                Ok(vec![text(node.to_json())])
            }
            "widget_at" => {
                let point = object([
                    ("x", number(args.get("x")).into()),
                    ("y", number(args.get("y")).into()),
                ]);
                let node = self.app(args, "at", point)?;
                Ok(vec![text(if node == Value::Null {
                    "No widget there.".to_string()
                } else {
                    summary_line(&node)
                })])
            }
            "tap" => {
                let find = finder(args);
                if is_empty(&find) {
                    return Err("say what to tap: text, label, type, key, id, or x and y".into());
                }
                let done = self.app(args, "tap", find)?;
                Ok(vec![text(format!(
                    "Tapped at ({}, {}){}.",
                    number(done.get("x")).round(),
                    number(done.get("y")).round(),
                    done.get("id")
                        .and_then(Value::as_str)
                        .map(|id| format!(" on #{id}"))
                        .unwrap_or_default()
                ))])
            }
            "scroll" => {
                let mut params = finder(args);
                if is_empty(&params) {
                    return Err("say where to scroll: a finder, or x and y".into());
                }
                if let Value::Object(members) = &mut params {
                    members.push(("dx".into(), number(args.get("dx")).into()));
                    members.push(("dy".into(), number(args.get("dy")).into()));
                }
                let done = self.app(args, "scroll", params)?;
                Ok(vec![text(format!(
                    "Scrolled at ({}, {}).",
                    number(done.get("x")).round(),
                    number(done.get("y")).round()
                ))])
            }
            "enter_text" => {
                let typed = arg_str(args, "text").ok_or("text is missing")?;
                let target = args.get("target").map(finder).filter(|t| !is_empty(t));
                if arg_bool(args, "replace") {
                    let target = target.ok_or("replace needs a target")?;
                    let done = self.app(
                        args,
                        "setText",
                        object([("target", target), ("text", typed.into())]),
                    )?;
                    return Ok(vec![text(format!(
                        "Set the value of #{}.",
                        done.get("id").and_then(Value::as_str).unwrap_or("?")
                    ))]);
                }
                // An Android application's fields take their text from the input method, not
                // from key strokes: there the field's value is extended instead.
                let android = self
                    .app(args, "hello", object([]))
                    .ok()
                    .and_then(|hello| {
                        hello
                            .get("platform")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .is_some_and(|platform| platform == "android");
                if android {
                    let target = target.ok_or(
                        "on Android, give the field as target: its text is set rather than typed",
                    )?;
                    let found = self.app(args, "find", target)?;
                    let id = found
                        .get("nodes")
                        .and_then(Value::as_array)
                        .and_then(|nodes| nodes.last())
                        .and_then(|node| node.get("id"))
                        .and_then(Value::as_str)
                        .ok_or("no such field on screen")?
                        .to_string();
                    let node = self.app(args, "node", object([("id", id.as_str().into())]))?;
                    let value = node
                        .get("semantics")
                        .and_then(|s| s.get("value"))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    self.app(
                        args,
                        "setText",
                        object([
                            ("target", object([("id", id.as_str().into())])),
                            ("text", format!("{value}{typed}").into()),
                        ]),
                    )?;
                    return Ok(vec![text(format!(
                        "Entered {} characters at the end of #{id} (on Android the field's value is \
                         set, as the input method does).",
                        typed.chars().count()
                    ))]);
                }
                let mut params = vec![("text".to_string(), Value::from(typed))];
                if let Some(target) = target {
                    params.push(("target".into(), target));
                }
                let done = self.app(args, "type", Value::Object(params))?;
                let focused = done
                    .get("focused")
                    .filter(|f| **f != Value::Null && **f != Value::Bool(false));
                Ok(vec![text(match focused {
                    Some(_) => format!("Typed {} characters.", number(done.get("typed"))),
                    None => format!(
                        "Typed {} characters, but no widget had the focus: tap the field first, \
                         or give target.",
                        number(done.get("typed"))
                    ),
                })])
            }
            "press_key" => {
                let key = arg_str(args, "key").ok_or("key is missing")?;
                self.app(args, "key", object([("key", key.into())]))?;
                Ok(vec![text(format!("Pressed {key}."))])
            }
            "go_back" => {
                let done = self.app(args, "back", object([]))?;
                let went = done.get("wentBack").and_then(Value::as_bool) == Some(true);
                Ok(vec![text(if went {
                    "Went back."
                } else {
                    "Nothing to go back from: the app is at its first page."
                })])
            }
            "screenshot" => self.screenshot(args),
            "set_animation_speed" => {
                let speed = arg_f64(args, "speed").ok_or("speed is missing")?;
                if speed <= 0.0 {
                    return Err(
                        "speed is a positive number: 1 is normal, 0.1 ten times slower".into(),
                    );
                }
                let state = self.app(args, "set", object([("animationSpeed", speed.into())]))?;
                Ok(vec![text(format!(
                    "Animation speed {speed}. Switches now: {}",
                    state.to_json()
                ))])
            }
            "set_debug_options" => {
                let mut params = Vec::new();
                for (from, to) in [
                    ("debug_paint", "debugPaint"),
                    ("performance_overlay", "performanceOverlay"),
                ] {
                    if let Some(on) = args.get(from).and_then(Value::as_bool) {
                        params.push((to.to_string(), Value::Bool(on)));
                    }
                }
                if let Some(brightness) = arg_str(args, "brightness") {
                    let value = match brightness {
                        "system" => Value::Null,
                        other => other.into(),
                    };
                    params.push(("brightness".into(), value));
                }
                if let Some(platform) = arg_str(args, "platform") {
                    let value = match platform {
                        "default" => Value::Null,
                        other => other.into(),
                    };
                    params.push(("platform".into(), value));
                }
                if let Some(scale) = arg_f64(args, "text_scale") {
                    let value = if scale > 0.0 {
                        scale.into()
                    } else {
                        Value::Null
                    };
                    params.push(("textScale".into(), value));
                }
                let state = self.app(args, "set", Value::Object(params))?;
                Ok(vec![text(format!("Switches now: {}", state.to_json()))])
            }
            "get_frame_stats" => {
                let mut params = Vec::new();
                if let Some(since) = arg_f64(args, "since") {
                    params.push(("after".to_string(), since.into()));
                }
                let frames = self.app(args, "frames", Value::Object(params))?;
                Ok(vec![text(frames_text(&frames))])
            }
            "analyze" => {
                let root = self.project_root(args)?;
                if arg_bool(args, "fix") {
                    frus_command(&root, &["analyze", "--fix"])
                } else {
                    frus_command(&root, &["analyze"])
                }
            }
            "run_tests" => {
                let root = self.project_root(args)?;
                match arg_str(args, "filter") {
                    Some(filter) => frus_command(&root, &["test", filter]),
                    None => frus_command(&root, &["test"]),
                }
            }
            other => Err(format!("no tool `{other}`")),
        }
    }

    fn launch_app(&mut self, args: &Value) -> ToolResult {
        let root = self.project_root(args)?;
        let flags: Vec<String> = args
            .get("flags")
            .and_then(Value::as_array)
            .unwrap_or(&[])
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        let release = flags.iter().any(|f| f == "--release");
        // Several devices: one run each, as `frus run -d all` makes them, each its own app.
        let mut wanted: Vec<String> = arg_str(args, "device")
            .map(str::to_string)
            .into_iter()
            .collect();
        if let Some(more) = args.get("devices").and_then(Value::as_array) {
            wanted.extend(more.iter().filter_map(Value::as_str).map(str::to_string));
        }
        let targets: Vec<(Option<String>, Option<String>)> = if crate::sessions::is_several(&wanted)
        {
            let project = Project::find(&root)?;
            let found = crate::devices::discover();
            let made_for = project.app_platforms();
            crate::sessions::resolve(&found, &wanted, made_for.as_deref())?
                .iter()
                .map(|d| (Some(d.id.clone()), Some(crate::sessions::slot(d))))
                .collect()
        } else {
            vec![(wanted.first().cloned(), None)]
        };
        let several = targets.len() > 1;
        let mut ids = Vec::new();
        for (device, slot) in &targets {
            let session = launch(
                &root,
                device.as_deref(),
                &flags,
                slot.as_deref().filter(|_| several),
            )?;
            ids.push(session.id.clone());
            self.sessions.push(session);
        }
        let wait = seconds(args, "wait_seconds", 300);
        let deadline = Instant::now() + wait;
        let mut said = Vec::new();
        let mut failed = Vec::new();
        for id in &ids {
            let Some(session) = self.sessions.iter().find(|s| &s.id == id) else {
                continue;
            };
            let left = deadline.saturating_duration_since(Instant::now());
            let up = session.shared.wait_until(left, |s| {
                s.stopped.is_some() || if release { s.started } else { s.hub.is_some() }
            });
            let (stopped, progress, device) = {
                let state = session.shared.lock();
                (
                    state.stopped.clone(),
                    state.progress.clone(),
                    state.device.clone(),
                )
            };
            if let Some(why) = stopped {
                let tail = session.shared.lock().tail(40, None);
                failed.push(id.clone());
                said.push(format!(
                    "{id} on {device} did not start: {why}\nLast lines:\n{tail}"
                ));
            } else if !up {
                said.push(format!(
                    "{id} is still building or starting (last: {progress}). Call app_status with \
                     wait_seconds to wait for it; get_logs shows the build."
                ));
            } else if release {
                said.push(format!(
                    "{id} is running on {device}, a release build: no DevTools, so only \
                     get_logs, hot_restart and stop_app work."
                ));
            } else {
                said.push(self.describe(id, Duration::from_secs(30)));
            }
        }
        self.sessions.retain(|s| !failed.contains(&s.id));
        if several {
            said.push("Each device is its own app: pass app_id to act on one.".into());
        }
        if failed.len() == ids.len() {
            return Err(said.join("\n"));
        }
        Ok(vec![text(said.join("\n"))])
    }

    /// A session's state, with its application's `hello` once it answers.
    fn describe(&self, id: &str, wait: Duration) -> String {
        let Some(session) = self.sessions.iter().find(|s| s.id == id) else {
            return format!("no app `{id}`");
        };
        let (device, stopped, hub, progress, started, hot) = {
            let s = session.shared.lock();
            (
                s.device.clone(),
                s.stopped.clone(),
                s.hub.clone(),
                s.progress.clone(),
                s.started,
                s.hot,
            )
        };
        let how = if session.child.is_some() {
            if hot {
                "launched, hot reload on"
            } else {
                "launched, reload restarts"
            }
        } else {
            "attached"
        };
        if let Some(why) = stopped {
            return format!("{id} ({how}) on {device}: stopped, {why}.");
        }
        let Some(hub) = hub else {
            let what = if started {
                "running without DevTools"
            } else {
                "building or starting"
            };
            return format!("{id} ({how}) on {device}: {what}. Last: {progress}");
        };
        match session.call("hello", object([]), wait) {
            Ok(hello) => {
                let size = hello.get("size");
                format!(
                    "{id} ({how}) is running on {device}: platform {}, window {}x{} logical \
                     pixels, {} physical pixels each. DevTools: {}",
                    hello.get("platform").and_then(Value::as_str).unwrap_or("?"),
                    number(size.and_then(|s| s.get("width"))).round(),
                    number(size.and_then(|s| s.get("height"))).round(),
                    number(hello.get("devicePixelRatio")),
                    hub.uri()
                )
            }
            Err(why) => format!(
                "{id} ({how}) on {device}: its DevTools are at {} but the app does not answer \
                 ({why}).",
                hub.uri()
            ),
        }
    }

    fn app_status(&mut self, args: &Value) -> ToolResult {
        if self.sessions.is_empty() {
            let _ = self.session(args);
        }
        if self.sessions.is_empty() {
            return Ok(vec![text(
                "No app. launch_app starts one; attach_app reaches a `frus run` already going.",
            )]);
        }
        if let Some(wait) = arg_f64(args, "wait_seconds") {
            let session = self.session(args)?;
            let wait = Duration::from_secs_f64(wait.clamp(0.0, 3600.0));
            session
                .shared
                .wait_until(wait, |s| s.hub.is_some() || s.stopped.is_some());
        }
        let ids: Vec<String> = self.sessions.iter().map(|s| s.id.clone()).collect();
        let lines: Vec<String> = ids
            .iter()
            .map(|id| self.describe(id, Duration::from_secs(5)))
            .collect();
        Ok(vec![text(format!(
            "{}\nTools act on the last one unless given app_id.",
            lines.join("\n")
        ))])
    }

    fn reload(&mut self, args: &Value, full: bool) -> ToolResult {
        let wait = seconds(args, "wait_seconds", 300);
        let session = self.session(args)?;
        let asked = SystemTime::now();
        if session.child.is_none() {
            let hub = session.hub().ok_or("no DevTools")?;
            hub.call(if full { "restart" } else { "reload" }, object([]))?;
            std::thread::sleep(Duration::from_secs(1));
            session.call("hello", object([]), wait)?;
            return Ok(vec![text(if full {
                "Restart asked of the `frus run` attached to; the app answers again."
            } else {
                "Reload asked of the `frus run` attached to (with hot reload, a saved file is \
                 patched in by itself: check get_logs for errors)."
            })]);
        }
        let hot = session.shared.lock().hot;
        // With hot reload a save is patched in by itself: what is waited for is the outcome of
        // the last save, which may be in already. Otherwise, the rebuild asked for now.
        let since = if hot && !full {
            let newest = session.root.as_deref().and_then(newest_source);
            match newest {
                Some(newest) if newest < session.started_at => {
                    return Ok(vec![text(
                        "No source file changed since the app started: nothing to reload. Save \
                         the edit, then call hot_reload.",
                    )]);
                }
                Some(newest) => newest,
                None => asked,
            }
        } else {
            asked
        };
        session.command("app.restart", object([("fullRestart", full.into())]))?;
        let shared = session.shared.clone();
        let done = shared.wait_until(wait, |s| {
            s.outcome_since(since).is_some() || s.stopped.is_some()
        });
        let state = shared.lock();
        if let Some(why) = &state.stopped {
            return Err(format!(
                "the app stopped: {why}\nLast lines:\n{}",
                state.tail(30, None)
            ));
        }
        if !done {
            return Err(format!(
                "nothing came of the {} in {} s (last: {}). get_logs shows the build.",
                if full { "restart" } else { "reload" },
                wait.as_secs(),
                state.progress
            ));
        }
        let outcome = state.outcome_since(since);
        drop(state);
        match outcome {
            Some(Outcome::Done { seconds, .. }) if full || !hot => {
                let _ = session.call("hello", object([]), Duration::from_secs(30));
                Ok(vec![text(format!(
                    "{} in {seconds:.1} s{}.",
                    if full {
                        "Restarted"
                    } else {
                        "Reloaded: rebuilt and restarted"
                    },
                    if full {
                        ", its state reset"
                    } else {
                        ", with the state the app saves (State::save) restored"
                    }
                ))])
            }
            Some(Outcome::Done { seconds, .. }) => {
                // Built is not yet applied: the application says when the patch is in, and the
                // next frame has it.
                let applied =
                    shared.wait_until(PATCH_APPLIED_WAIT, |s| s.patch_applied_since(since));
                Ok(vec![text(if applied {
                    format!(
                        "Patched in {seconds:.1} s: the running app has the change, its state \
                         kept."
                    )
                } else {
                    format!(
                        "Patched in {seconds:.1} s, but the app has not said it applied the \
                         patch yet: check the screen, or hot_restart."
                    )
                })])
            }
            Some(Outcome::Failed(why)) => {
                // With hot reload, the engine says only that the build failed; frus run then
                // asks the compiler where, which takes a while: its located errors are waited
                // for.
                if hot && !full {
                    let left = wait.saturating_sub(asked.elapsed().unwrap_or_default());
                    shared.wait_until(left.min(LOCATE_WAIT), |s| {
                        s.located_error_since(since) || s.stopped.is_some()
                    });
                }
                Err(format!(
                    "the change could not be brought in: {why}
Errors:
{}",
                    shared.lock().tail(40, Some(since))
                ))
            }
            None => Err("no outcome".into()),
        }
    }

    fn logs(&mut self, args: &Value) -> ToolResult {
        let since = arg_f64(args, "since").map(|n| n as u64);
        let limit = arg_f64(args, "limit").map_or(100, |l| l.max(1.0) as usize);
        let errors_only = arg_bool(args, "errors_only");
        let session = self.session(args)?;
        let lines: Vec<Line> = if session.child.is_some() {
            let state = session.shared.lock();
            state
                .lines
                .iter()
                .filter(|l| since.is_none_or(|s| l.number > s))
                .cloned()
                .collect()
        } else {
            let mut params = Vec::new();
            if let Some(since) = since {
                params.push(("after".to_string(), since.into()));
            }
            let hub = session.hub().ok_or("no DevTools")?;
            let answer = hub.call("logs", Value::Object(params))?;
            answer
                .get("lines")
                .and_then(Value::as_array)
                .unwrap_or(&[])
                .iter()
                .map(|l| Line {
                    number: number(l.get("number")) as u64,
                    source: "app",
                    text: l
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    error: l.get("error").and_then(Value::as_bool) == Some(true),
                    at: SystemTime::now(),
                })
                .collect()
        };
        let lines: Vec<&Line> = lines.iter().filter(|l| !errors_only || l.error).collect();
        let shown = &lines[lines.len().saturating_sub(limit)..];
        let mut out = String::new();
        if shown.len() < lines.len() {
            out.push_str(&format!(
                "({} earlier lines not shown: raise limit, or pass since)\n",
                lines.len() - shown.len()
            ));
        }
        for line in shown {
            out.push_str(&format!(
                "{} [{}{}] {}\n",
                line.number,
                line.source,
                if line.error { " error" } else { "" },
                line.text
            ));
        }
        match lines.last() {
            Some(last) => out.push_str(&format!("Next: since={}", last.number)),
            None => out.push_str("No lines."),
        }
        Ok(vec![text(out)])
    }

    fn screenshot(&mut self, args: &Value) -> ToolResult {
        let max = arg_f64(args, "max_size").map_or(1024, |m| m.max(0.0) as u32);
        let shot = self.app(args, "screenshot", object([]))?;
        let data = shot
            .get("data")
            .and_then(Value::as_str)
            .ok_or("no image came")?;
        let png = unbase64(data)?;
        let mut said = Vec::new();
        if let Some(path) = arg_str(args, "save_to") {
            std::fs::write(path, &png).map_err(|e| format!("could not save {path}: {e}"))?;
            said.push(format!("Saved the full-size PNG to {path}."));
        }
        let (image, size) = match crate::png::decode(&png) {
            Ok(full) => {
                let small = crate::png::shrink(&full, max);
                let size = (small.width, small.height);
                if small.width != full.width {
                    said.push(format!(
                        "Downscaled from {}x{} pixels.",
                        full.width, full.height
                    ));
                }
                // Encoded again, whatever the size: the application writes its frame fast
                // rather than small.
                let encoded = crate::png::encode(&small);
                let bytes = if encoded.len() < png.len() || small.width != full.width {
                    encoded
                } else {
                    png
                };
                (bytes, Some(size))
            }
            Err(_) => (png, None),
        };
        if let Ok(hello) = self.app(args, "hello", object([])) {
            let size_of = hello.get("size");
            let (w, h) = (
                number(size_of.and_then(|s| s.get("width"))),
                number(size_of.and_then(|s| s.get("height"))),
            );
            if let Some((iw, ih)) = size {
                let platform = hello.get("platform").and_then(Value::as_str).unwrap_or("");
                said.push(format!(
                    "Image {iw}x{ih} pixels; the app's window is {}x{} logical pixels{}.",
                    w.round(),
                    h.round(),
                    if platform == "android" {
                        " (the image is the whole screen, status bar included: find_widgets \
                         gives exact coordinates)"
                            .to_string()
                    } else if w > 0.0 {
                        format!(": logical = image pixels x {:.3}", w / f64::from(iw))
                    } else {
                        String::new()
                    }
                ));
            }
        }
        Ok(vec![
            object([
                ("type", "image".into()),
                ("data", crate::devtools::base64(&image).into()),
                ("mimeType", "image/png".into()),
            ]),
            text(said.join(" ")),
        ])
    }

    /// One JSON-RPC message from the client: the answer to send, if it is a request.
    fn handle(&mut self, message: &Value) -> Option<Value> {
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str);
        let (Some(id), Some(method)) = (id, method) else {
            // A notification (`notifications/initialized`, `notifications/cancelled`) or an
            // answer: nothing to say.
            return None;
        };
        let params = message.get("params").cloned().unwrap_or(object([]));
        let result = match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str);
                let version = PROTOCOL_VERSIONS
                    .iter()
                    .find(|v| Some(**v) == asked)
                    .unwrap_or(&PROTOCOL_VERSIONS[0]);
                Ok(object([
                    ("protocolVersion", (*version).into()),
                    (
                        "capabilities",
                        object([("tools", object([("listChanged", false.into())]))]),
                    ),
                    (
                        "serverInfo",
                        object([
                            ("name", "frus".into()),
                            ("title", "frus".into()),
                            ("version", env!("CARGO_PKG_VERSION").into()),
                        ]),
                    ),
                    ("instructions", INSTRUCTIONS.into()),
                ]))
            }
            "ping" => Ok(object([])),
            "tools/list" => Ok(object([("tools", tools())])),
            "resources/list" => Ok(object([("resources", Value::Array(Vec::new()))])),
            "prompts/list" => Ok(object([("prompts", Value::Array(Vec::new()))])),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let known = tools()
                    .as_array()
                    .unwrap_or(&[])
                    .iter()
                    .any(|t| t.get("name").and_then(Value::as_str) == Some(name));
                if known {
                    let args = params.get("arguments").cloned().unwrap_or(object([]));
                    let outcome = self.call_tool(name, &args);
                    Ok(match outcome {
                        Ok(content) => object([
                            ("content", Value::Array(content)),
                            ("isError", false.into()),
                        ]),
                        Err(why) => object([
                            ("content", Value::Array(vec![text(why)])),
                            ("isError", true.into()),
                        ]),
                    })
                } else {
                    Err((-32602, format!("no tool `{name}`")))
                }
            }
            other => Err((-32601, format!("no method `{other}`"))),
        };
        Some(match result {
            Ok(result) => object([("jsonrpc", "2.0".into()), ("id", id), ("result", result)]),
            Err((code, message)) => error(id, code, &message),
        })
    }

    /// The client is gone: every run launched is stopped.
    fn shut_down(&mut self) {
        for mut session in self.sessions.drain(..) {
            if session.child.is_some() {
                let _ = session.stop();
            }
        }
    }
}

fn error(id: Value, code: i32, message: &str) -> Value {
    object([
        ("jsonrpc", "2.0".into()),
        ("id", id),
        (
            "error",
            object([
                ("code", f64::from(code).into()),
                ("message", message.into()),
            ]),
        ),
    ])
}

/// `frus mcp`: serves until the client closes standard input.
pub(crate) fn mcp() -> Result<ExitCode, String> {
    report::to_stderr();
    let mut server = Server {
        sessions: Vec::new(),
        here: std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let stdin = std::io::stdin();
    for line in stdin.lock().lines().map_while(Result::ok) {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        let answer = match json::parse(line) {
            Ok(message @ Value::Object(_)) => server.handle(&message),
            Ok(_) => Some(error(Value::Null, -32600, "a message is one JSON object")),
            Err(why) => Some(error(Value::Null, -32700, &why)),
        };
        if let Some(answer) = answer {
            report::line(&answer);
        }
    }
    server.shut_down();
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn server() -> Server {
        Server {
            sessions: Vec::new(),
            here: std::env::temp_dir(),
        }
    }

    fn request(server: &mut Server, method: &str, params: Value) -> Value {
        server
            .handle(&object([
                ("jsonrpc", "2.0".into()),
                ("id", 1u64.into()),
                ("method", method.into()),
                ("params", params),
            ]))
            .unwrap()
    }

    #[test]
    fn the_handshake_agrees_on_a_version() {
        let mut s = server();
        let answer = request(
            &mut s,
            "initialize",
            object([("protocolVersion", "2025-06-18".into())]),
        );
        let result = answer.get("result").unwrap();
        assert_eq!(
            result.get("protocolVersion").and_then(Value::as_str),
            Some("2025-06-18")
        );
        assert!(result.get("capabilities").unwrap().get("tools").is_some());
        let answer = request(
            &mut s,
            "initialize",
            object([("protocolVersion", "1999-01-01".into())]),
        );
        assert_eq!(
            answer
                .get("result")
                .and_then(|r| r.get("protocolVersion"))
                .and_then(Value::as_str),
            Some(PROTOCOL_VERSIONS[0])
        );
        // A notification has no answer; an unknown method is an error.
        assert!(s
            .handle(&object([("method", "notifications/initialized".into())]))
            .is_none());
        let answer = request(&mut s, "nope", object([]));
        assert_eq!(
            answer.get("error").and_then(|e| e.get("code")),
            Some(&Value::Number(-32601.0))
        );
        assert_eq!(answer.get("id"), Some(&Value::Number(1.0)));
    }

    #[test]
    fn every_tool_is_described_and_handled() {
        let tools = tools();
        let tools = tools.as_array().unwrap();
        assert!(tools.len() >= 20, "{}", tools.len());
        let mut names = Vec::new();
        for tool in tools {
            let name = tool.get("name").and_then(Value::as_str).unwrap();
            assert!(name.len() <= 20 && name.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
            assert!(
                tool.get("description")
                    .and_then(Value::as_str)
                    .unwrap()
                    .len()
                    > 30,
                "{name}"
            );
            let schema = tool.get("inputSchema").unwrap();
            assert_eq!(schema.get("type").and_then(Value::as_str), Some("object"));
            assert!(schema.get("properties").is_some(), "{name}");
            names.push(name.to_string());
        }
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count);
        // Each is handled: none falls through to "no tool".
        let mut s = server();
        for name in &names {
            if [
                "launch_app",
                "analyze",
                "run_tests",
                "list_devices",
                "attach_app",
            ]
            .contains(&name.as_str())
            {
                continue;
            }
            let outcome = s.call_tool(name, &object([("app_id", "none".into())]));
            assert!(
                !matches!(&outcome, Err(why) if why.starts_with("no tool")),
                "{name}"
            );
        }
        let answer = request(
            &mut s,
            "tools/call",
            object([("name", "no_such_tool".into())]),
        );
        assert_eq!(
            answer.get("error").and_then(|e| e.get("code")),
            Some(&Value::Number(-32602.0))
        );
    }

    #[test]
    fn a_devtools_address_is_read() {
        let hub = Hub::from_uri("http://127.0.0.1:9100/?token=6c1f").unwrap();
        assert_eq!(hub.host, "127.0.0.1:9100");
        assert_eq!(hub.token, "6c1f");
        assert_eq!(hub.uri(), "http://127.0.0.1:9100/?token=6c1f");
        assert!(Hub::from_uri("http://127.0.0.1:9100/").is_err());
        assert!(Hub::from_uri("127.0.0.1:9100").is_err());
    }

    #[test]
    fn a_run_is_followed_through_its_events() {
        let mut state = State {
            hot: true,
            ..State::default()
        };
        let before = SystemTime::now();
        for line in [
            r#"{"event":"app.start","params":{"appId":"a","deviceName":"Windows"}}"#,
            r#"{"event":"app.progress","params":{"appId":"a","message":"Building…"}}"#,
            r#"{"event":"app.progress","params":{"appId":"a","message":"hot reload is off for this run: no engine","error":true}}"#,
            r#"{"event":"app.log","params":{"appId":"a","log":"hello","error":false}}"#,
            r#"{"event":"app.started","params":{"appId":"a"}}"#,
            r#"{"event":"app.devTools","params":{"appId":"a","uri":"http://127.0.0.1:9101/?token=t"}}"#,
            r#"{"event":"app.restarted","params":{"appId":"a","fullRestart":false,"seconds":1.5}}"#,
            r#"{"id":1,"result":true}"#,
            "not json",
        ] {
            state.event(line);
        }
        assert_eq!(state.device, "Windows");
        assert!(state.started && !state.hot);
        assert_eq!(state.hub.as_ref().unwrap().host, "127.0.0.1:9101");
        assert!(matches!(
            state.outcome_since(before),
            Some(Outcome::Failed(why)) if why.contains("hot reload is off")
        ));
        assert_eq!(state.lines.len(), 4);
        assert_eq!(state.lines[2].source, "app");
        assert!(state.tail(10, Some(before)).contains("hot reload is off"));
        assert!(!state.tail(10, Some(before)).contains("hello"));
        assert_eq!(
            state.tail(10, Some(SystemTime::now() + Duration::from_secs(9))),
            ""
        );
        assert!(!state.patch_applied_since(before));
        state.event(r#"{"event":"app.log","params":{"appId":"a","log":"[x INFO frus_shell::hot] hot reload: patch applied","error":false}}"#);
        assert!(state.patch_applied_since(before));
        assert!(!state.located_error_since(before));
        state.event(r#"{"event":"app.progress","params":{"appId":"a","message":"D:/app/src/x.rs:4:9: error: expected `,`","error":true}}"#);
        assert!(state.located_error_since(before));
        state.event(r#"{"event":"app.stop","params":{"appId":"a","error":"boom"}}"#);
        assert_eq!(state.stopped.as_deref(), Some("boom"));
    }

    fn sample_tree() -> Value {
        json::parse(
            r#"{"size":{"width":400,"height":300},"nodes":[
            {"id":"0000000000000001","name":"Column","depth":0,"parent":null,"rect":{"x":0,"y":0,"width":400,"height":300}},
            {"id":"0000000000000002","name":"Text","depth":1,"parent":0,"rect":{"x":10,"y":10,"width":80,"height":20},"text":"Count: 0"},
            {"id":"0000000000000003","name":"Button","depth":1,"parent":0,"rect":{"x":10,"y":40,"width":60,"height":30.4},"key":"7","semantics":{"role":"Button","label":"Add","actions":["tap"]}},
            {"id":"0000000000000004","name":"Text","depth":2,"parent":2,"rect":{"x":20,"y":45,"width":20,"height":20},"text":"+"}
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn the_tree_is_written_as_indented_lines() {
        let tree = sample_tree();
        let all = tree_text(&tree, None, None, 100);
        assert!(
            all.starts_with("4 widgets on screen, window 400x300"),
            "{all}"
        );
        assert!(
            all.contains("\nColumn #0000000000000001 [0,0 400x300]\n"),
            "{all}"
        );
        assert!(all.contains("\n  Text #0000000000000002 [10,10 80x20] text=\"Count: 0\"\n"));
        assert!(all.contains(
            "\n  Button #0000000000000003 [10,40 60x30] key=7 (Button, label=\"Add\", actions=tap)\n"
        ), "{all}");
        assert!(all.contains("\n    Text #0000000000000004"));
        let shallow = tree_text(&tree, None, Some(1), 100);
        assert!(!shallow.contains("#0000000000000004"));
        let part = tree_text(&tree, Some("0000000000000003"), None, 100);
        assert!(
            part.contains("\nButton #0000000000000003")
                && part.contains("\n  Text #0000000000000004")
        );
        assert!(!part.contains("#0000000000000002"));
        let cut = tree_text(&tree, None, None, 2);
        assert!(cut.contains("2 more widgets not shown"), "{cut}");
        assert!(tree_text(&tree, Some("ffff"), None, 9).starts_with("No widget"));
    }

    #[test]
    fn frames_are_summed_up() {
        assert!(frames_text(&object([])).contains("No frames"));
        let frames = json::parse(
            r#"{"budget":16.7,"frames":[
              {"number":1,"build":1,"layoutPaint":2,"render":5,"draw":1,"why":["animation"]},
              {"number":2,"build":10,"layoutPaint":8,"render":20,"draw":3,"why":[]}]}"#,
        )
        .unwrap();
        let said = frames_text(&frames);
        assert!(said.contains("2 frames (#1 to #2)"), "{said}");
        assert!(said.contains("Janky (own work over budget): 1"), "{said}");
        assert!(said.contains("#2: 10.00 / 8.00 / 20.00 JANK"), "{said}");
        assert!(said.contains("since=2"));
    }

    #[test]
    fn base64_is_read_back() {
        for bytes in [&b""[..], b"f", b"fo", b"foo", b"foobar", &[0, 255, 128, 7]] {
            assert_eq!(unbase64(&crate::devtools::base64(bytes)).unwrap(), bytes);
        }
        assert!(unbase64("a*b").is_err());
    }

    #[test]
    fn long_text_is_clipped_in_the_middle() {
        let long = "é".repeat(MAX_TEXT);
        let clipped = clip(long.clone());
        assert!(clipped.len() < long.len() && clipped.contains("bytes cut"));
        assert_eq!(clip("short".into()), "short");
    }

    /// A DevTools hub that answers every call with its method and params.
    fn fake_hub(token: &'static str) -> Hub {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().map_while(Result::ok) {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let (mut length, mut allowed) = (0, false);
                let mut line = String::new();
                while reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(n) = line.strip_prefix("Content-Length: ") {
                        length = n.trim().parse().unwrap();
                    }
                    if line.trim() == format!("X-Frus-DevTools: {token}") {
                        allowed = true;
                    }
                    line.clear();
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let call = json::parse(&String::from_utf8_lossy(&body)).unwrap();
                let method = call.get("method").and_then(Value::as_str).unwrap();
                let reply = match method {
                    _ if !allowed => {
                        let _ = stream.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\nno");
                        continue;
                    }
                    "status" => object([
                        ("app", "counter".into()),
                        ("device", "Windows".into()),
                    ]),
                    "find" => json::parse(
                        r#"{"nodes":[{"id":"00000000000000aa","name":"Button","rect":{"x":0,"y":0,"width":10,"height":10},"center":{"x":5,"y":5},"text":"Add"}]}"#,
                    )
                    .unwrap(),
                    "tap" => object([("x", 5.0.into()), ("y", 5.0.into()), ("id", "00000000000000aa".into())]),
                    "boom" => {
                        let body = r#"{"error":"no such thing"}"#;
                        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
                        continue;
                    }
                    _ => object([("method", method.into()), ("params", call.get("params").cloned().unwrap())]),
                };
                let body = object([("result", reply)]).to_json();
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Hub {
            host: format!("127.0.0.1:{port}"),
            token: token.to_string(),
        }
    }

    #[test]
    fn an_attached_run_is_driven_through_its_devtools() {
        let hub = fake_hub("secret");
        assert_eq!(hub.call("boom", object([])).unwrap_err(), "no such thing");
        let wrong = Hub {
            token: "guess".into(),
            ..hub.clone()
        };
        assert!(wrong
            .call("status", object([]))
            .unwrap_err()
            .contains("refused"));
        let mut s = server();
        let session = attached(hub.clone(), None).unwrap();
        assert!(session.id.starts_with("counter@127.0.0.1:"));
        s.sessions.push(session);
        let found = s
            .call_tool("find_widgets", &object([("text", "Add".into())]))
            .unwrap();
        let said = found[0].get("text").and_then(Value::as_str).unwrap();
        assert!(
            said.contains("Button #00000000000000aa center (5, 5)"),
            "{said}"
        );
        let tapped = s
            .call_tool(
                "tap",
                &object([("text", "Add".into()), ("label", Value::Null)]),
            )
            .unwrap();
        assert_eq!(
            tapped[0].get("text").and_then(Value::as_str),
            Some("Tapped at (5, 5) on #00000000000000aa.")
        );
        // The finder leaves out what is not given; the switches are named as the protocol names
        // them.
        let set = s
            .call_tool(
                "set_debug_options",
                &object([
                    ("debug_paint", true.into()),
                    ("brightness", "system".into()),
                ]),
            )
            .unwrap();
        let said = set[0].get("text").and_then(Value::as_str).unwrap();
        assert!(
            said.contains(r#""params":{"debugPaint":true,"brightness":null}"#),
            "{said}"
        );
        let typed = s
            .call_tool(
                "enter_text",
                &object([
                    ("text", "hi".into()),
                    ("target", object([("label", "Name".into())])),
                ]),
            )
            .unwrap();
        assert!(typed[0].get("text").is_some());
        assert!(s
            .call_tool("tap", &object([]))
            .unwrap_err()
            .contains("say what"));
    }

    #[test]
    fn noted_runs_are_found_and_dead_ones_removed() {
        let dir = std::env::temp_dir().join(format!("frus-mcp-runs-{}", std::process::id()));
        let root = std::env::temp_dir();
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let hub = fake_hub("t");
        let note = |uri: &str, root: &Path| {
            object([
                ("uri", uri.into()),
                ("root", root.display().to_string().into()),
            ])
            .to_json()
        };
        std::fs::write(dir.join("run-1.json"), note(&hub.uri(), &root)).unwrap();
        // A run that ended without removing its note: nothing listens on port 9.
        std::fs::write(
            dir.join("run-2.json"),
            note("http://127.0.0.1:9/?token=x", &root),
        )
        .unwrap();
        // Another application's run.
        std::fs::write(dir.join("run-3.json"), note(&hub.uri(), &dir)).unwrap();
        let found = noted_runs(&dir, &root);
        assert_eq!(found, vec![hub]);
        assert!(!dir.join("run-2.json").exists());
        assert!(dir.join("run-3.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
