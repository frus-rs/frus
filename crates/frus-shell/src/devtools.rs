// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The debugging service of a running application** (milestone 788), in debug builds only.
//!
//! `frus run` serves DevTools — a page in the browser, and the same protocol for any other
//! client — and relays each request to the application it runs. The application answers here:
//! the widget tree and a widget's details, what is under a point, a tap, a scroll, typing, a
//! screenshot, the frames' costs, and the switches a developer flips while looking (debug paint,
//! the animations' speed, the brightness…). The protocol is written down in
//! `docs/guide/devtools.md`.
//!
//! A request is one line of JSON, `{"id":7,"method":"tree","params":{}}`, and so is its answer,
//! `{"id":7,"result":{…}}` or `{"id":7,"error":"…"}`. How the lines travel depends on the
//! platform:
//!
//! - a desktop application reads `devtools <json>` on the socket it reports to `frus run` on
//!   ([`crate::dev`]) and writes its answer back the same way;
//! - a page gets them through the script `frus run` adds to it, and posts its answer back;
//! - an Android application listens on a local socket in the abstract namespace,
//!   `frus_devtools_<package>`, which `frus run` reaches with `adb forward`. A socket of that
//!   kind needs no permission, so the application does not have to declare `INTERNET`.
//!
//! Each request is queued, the window is woken, and the next frame answers it once its layout
//! is done ([`take`]): the tree it describes is the one on screen.

use std::sync::Mutex;

use frus_widgets::__serde_json::{self as json, json, Map, Value};
use frus_widgets::{InspectorNode, Rect, SemanticsProperties, WidgetId};

/// The protocol's version: a client checks it in the answer to `hello`.
pub(crate) const PROTOCOL: u32 = 1;

/// One request, and where its answer goes.
pub(crate) struct Request {
    /// The request, one JSON object.
    pub body: String,
    reply: Box<dyn FnOnce(String) + Send>,
}

impl Request {
    /// A request whose answer is handed to `reply`, on the interface's thread.
    pub(crate) fn new(body: String, reply: impl FnOnce(String) + Send + 'static) -> Self {
        Self {
            body,
            reply: Box::new(reply),
        }
    }

    /// Sends the answer, one line of JSON.
    pub(crate) fn answer(self, answer: String) {
        (self.reply)(answer);
    }
}

/// The requests not answered yet.
static QUEUE: Mutex<Vec<Request>> = Mutex::new(Vec::new());

/// Queues a request for the next frame, and asks for that frame.
pub(crate) fn push(request: Request) {
    if let Ok(mut queue) = QUEUE.lock() {
        queue.push(request);
    }
    crate::dev_commands::wake();
}

/// The requests that arrived since the last frame, oldest first.
pub(crate) fn take() -> Vec<Request> {
    QUEUE
        .lock()
        .map(|mut queue| std::mem::take(&mut *queue))
        .unwrap_or_default()
}

/// A request read: its id (echoed in the answer), its method and its parameters.
pub(crate) struct Call {
    pub id: Value,
    pub method: String,
    pub params: Value,
}

/// Reads a request; an error names what is wrong with it.
pub(crate) fn parse(body: &str) -> Result<Call, String> {
    let value: Value = json::from_str(body).map_err(|e| format!("not JSON: {e}"))?;
    let method = value
        .get("method")
        .and_then(Value::as_str)
        .ok_or("a request names its method")?
        .to_string();
    Ok(Call {
        id: value.get("id").cloned().unwrap_or(Value::Null),
        method,
        params: value.get("params").cloned().unwrap_or(json!({})),
    })
}

/// The answer to request `id`: its result, or why there is none.
pub(crate) fn answer(id: &Value, outcome: Result<Value, String>) -> String {
    match outcome {
        Ok(result) => json!({ "id": id, "result": result }).to_string(),
        Err(error) => json!({ "id": id, "error": error }).to_string(),
    }
}

/// A widget's id as the protocol writes it: 16 hexadecimal digits, since a JavaScript number
/// cannot hold every 64-bit value.
pub(crate) fn id_text(id: WidgetId) -> String {
    format!("{:016x}", id.as_u64())
}

/// An id read back from the protocol.
pub(crate) fn parse_id(text: &str) -> Option<WidgetId> {
    let text = text.trim().trim_start_matches('#').trim_start_matches("0x");
    u64::from_str_radix(text, 16).ok().map(WidgetId::from_u64)
}

fn rect(r: Rect) -> Value {
    json!({ "x": r.x, "y": r.y, "width": r.width, "height": r.height })
}

/// What the frame knows of each widget besides its box: the words it painted, and what it
/// tells assistive technologies.
#[derive(Default)]
pub(crate) struct Extras<'a> {
    /// The words each widget painted, by id: a label's text, a button's.
    pub texts: std::collections::HashMap<u64, String>,
    /// What each widget tells assistive technologies, by id.
    pub semantics: std::collections::HashMap<u64, &'a SemanticsProperties>,
}

impl<'a> Extras<'a> {
    /// The words of `primitives`, by the widget that painted them, layers included, and the
    /// semantics of the frame.
    pub(crate) fn of(
        primitives: &[frus_widgets::Primitive],
        semantics: &'a [(WidgetId, Rect, SemanticsProperties)],
    ) -> Self {
        fn collect(
            primitives: &[frus_widgets::Primitive],
            out: &mut std::collections::HashMap<u64, String>,
        ) {
            for p in primitives {
                let words = match p {
                    frus_widgets::Primitive::Text { text, .. } => text.clone(),
                    frus_widgets::Primitive::RichText { runs, .. } => {
                        runs.iter().map(|run| run.text.as_str()).collect()
                    }
                    frus_widgets::Primitive::Layer {
                        primitives,
                        opacity,
                        ..
                    } => {
                        if *opacity > 0.0 {
                            collect(primitives, out);
                        }
                        continue;
                    }
                    _ => continue,
                };
                let entry = out.entry(p.owner()).or_default();
                if !entry.is_empty() {
                    entry.push(' ');
                }
                entry.push_str(&words);
            }
        }
        let mut texts = std::collections::HashMap::new();
        collect(primitives, &mut texts);
        Self {
            texts,
            semantics: semantics
                .iter()
                .map(|(id, _, sem)| (id.as_u64(), sem))
                .collect(),
        }
    }
}

/// What a client is told of a widget's semantics.
fn semantics_json(sem: &SemanticsProperties) -> Value {
    let mut out = Map::new();
    out.insert("role".into(), format!("{:?}", sem.role).into());
    if let Some(label) = &sem.label {
        out.insert("label".into(), label.clone().into());
    }
    if let Some(value) = &sem.value {
        // A password's characters are not handed out, here either.
        let value = if sem.obscured {
            "•".repeat(value.chars().count())
        } else {
            value.clone()
        };
        out.insert("value".into(), value.into());
    }
    let toggled = format!("{:?}", sem.toggled);
    if toggled != "None" && toggled != "NotApplicable" {
        out.insert("toggled".into(), toggled.into());
    }
    if let Some(expanded) = sem.expanded {
        out.insert("expanded".into(), expanded.into());
    }
    if let Some((min, value, max)) = sem.range {
        out.insert("range".into(), json!([min, value, max]));
    }
    let mut actions = Vec::new();
    if sem.clickable && !sem.disabled {
        actions.push("tap");
    }
    if sem.range.is_some() && !sem.disabled {
        actions.push("increase");
        actions.push("decrease");
    }
    out.insert("actions".into(), json!(actions));
    if sem.disabled {
        out.insert("disabled".into(), true.into());
    }
    if sem.obscured {
        out.insert("obscured".into(), true.into());
    }
    Value::Object(out)
}

/// One widget as `tree` lists it: id, name, depth, parent, box, and when it has them its key,
/// its words and its semantics.
fn node_json(node: &InspectorNode, parent: Option<usize>, extras: &Extras) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), id_text(node.id).into());
    out.insert("name".into(), node.name.into());
    out.insert("depth".into(), node.depth.into());
    out.insert("parent".into(), parent.map_or(Value::Null, Value::from));
    out.insert("rect".into(), rect(node.rect));
    if let Some(key) = node.key {
        out.insert("key".into(), format!("{key:x}").into());
    }
    if let Some(text) = extras.texts.get(&node.id.as_u64()) {
        out.insert("text".into(), text.clone().into());
    }
    if let Some(sem) = extras.semantics.get(&node.id.as_u64()) {
        out.insert("semantics".into(), semantics_json(sem));
    }
    Value::Object(out)
}

/// The parent of each node of a tree listed in paint order with depths: the last node before
/// it one level up.
pub(crate) fn parents(nodes: &[InspectorNode]) -> Vec<Option<usize>> {
    let mut stack: Vec<usize> = Vec::new();
    nodes
        .iter()
        .enumerate()
        .map(|(i, node)| {
            while stack
                .last()
                .is_some_and(|&top| nodes[top].depth >= node.depth)
            {
                stack.pop();
            }
            let parent = stack.last().copied();
            stack.push(i);
            parent
        })
        .collect()
}

/// The whole tree, in paint order: what `tree` answers.
pub(crate) fn tree_json(nodes: &[InspectorNode], extras: &Extras) -> Value {
    let parents = parents(nodes);
    Value::Array(
        nodes
            .iter()
            .zip(parents)
            .map(|(node, parent)| node_json(node, parent, extras))
            .collect(),
    )
}

/// One widget in detail: what `tree` says of it, its layout style, and the chain of its
/// ancestors' names.
pub(crate) fn details_json(nodes: &[InspectorNode], index: usize, extras: &Extras) -> Value {
    let parents = parents(nodes);
    let node = &nodes[index];
    let mut out = node_json(node, parents[index], extras);
    let s = &node.style;
    let insets = |i: frus_widgets::Insets| json!([i.left, i.top, i.right, i.bottom]);
    let style = json!({
        "width": format!("{:?}", s.width),
        "height": format!("{:?}", s.height),
        "minWidth": format!("{:?}", s.min_width),
        "minHeight": format!("{:?}", s.min_height),
        "maxWidth": format!("{:?}", s.max_width),
        "maxHeight": format!("{:?}", s.max_height),
        "padding": insets(s.padding),
        "margin": insets(s.margin),
        "flexGrow": s.flex_grow,
        "flexShrink": s.flex_shrink,
        "flexBasis": format!("{:?}", s.flex_basis),
        "direction": format!("{:?}", s.flex_direction),
        "justify": format!("{:?}", s.justify),
        "align": format!("{:?}", s.align),
        "alignSelf": s.align_self.map(|a| format!("{a:?}")),
        "gap": s.gap,
        "aspectRatio": s.aspect_ratio,
        "wrap": s.flex_wrap,
        "gridColumns": s.grid_columns,
    });
    let mut ancestors = Vec::new();
    let mut at = parents[index];
    while let Some(i) = at {
        ancestors.push(json!({ "id": id_text(nodes[i].id), "name": nodes[i].name }));
        at = parents[i];
    }
    if let Value::Object(map) = &mut out {
        map.insert("style".into(), style);
        map.insert("ancestors".into(), Value::Array(ancestors));
        // The children, by index, for a client that walks down.
        let children: Vec<Value> = parents
            .iter()
            .enumerate()
            .filter(|(_, p)| **p == Some(index))
            .map(|(i, _)| json!({ "id": id_text(nodes[i].id), "name": nodes[i].name }))
            .collect();
        map.insert("children".into(), Value::Array(children));
    }
    out
}

/// What a client looks a widget up by: any of its words, its semantics label, its type, its
/// key or its id. Several given: all must hold.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Finder {
    pub id: Option<WidgetId>,
    pub text: Option<String>,
    pub label: Option<String>,
    pub name: Option<String>,
    pub key: Option<String>,
    /// The words and the label are matched whole, unless this is set: then contained, with
    /// case ignored.
    pub contains: bool,
}

impl Finder {
    /// The finder in `params`: `{"text":"Save"}`, `{"label":"Title","contains":true}`,
    /// `{"type":"Button"}`, `{"key":"9e3779b97f4a7c15"}`, `{"id":"…"}`. `None` when it names nothing.
    pub(crate) fn of(params: &Value) -> Option<Self> {
        let text = |key: &str| params.get(key).and_then(Value::as_str).map(str::to_string);
        let finder = Self {
            id: params.get("id").and_then(Value::as_str).and_then(parse_id),
            text: text("text"),
            label: text("label"),
            name: text("type"),
            key: text("key"),
            contains: params
                .get("contains")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        };
        let names_something = finder.id.is_some()
            || finder.text.is_some()
            || finder.label.is_some()
            || finder.name.is_some()
            || finder.key.is_some();
        names_something.then_some(finder)
    }

    fn words_match(&self, wanted: &str, found: Option<&str>) -> bool {
        let Some(found) = found else {
            return false;
        };
        if self.contains {
            found.to_lowercase().contains(&wanted.to_lowercase())
        } else {
            found == wanted
        }
    }

    /// Whether `node` is what is looked for.
    pub(crate) fn matches(&self, node: &InspectorNode, extras: &Extras) -> bool {
        let raw = node.id.as_u64();
        self.id.is_none_or(|id| id == node.id)
            && self.name.as_deref().is_none_or(|name| name == node.name)
            && self
                .key
                .as_deref()
                .is_none_or(|key| node.key.is_some_and(|k| format!("{k:x}") == key))
            && self.text.as_deref().is_none_or(|text| {
                self.words_match(text, extras.texts.get(&raw).map(String::as_str))
            })
            && self.label.as_deref().is_none_or(|label| {
                self.words_match(
                    label,
                    extras
                        .semantics
                        .get(&raw)
                        .and_then(|sem| sem.label.as_deref()),
                )
            })
    }

    /// The indices of the nodes found, in paint order: the one on top comes last.
    pub(crate) fn find(&self, nodes: &[InspectorNode], extras: &Extras) -> Vec<usize> {
        nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| self.matches(node, extras))
            .map(|(i, _)| i)
            .collect()
    }
}

/// A key named as the protocol names it — `"Enter"`, `"Tab"`, `"Backspace"`, `"ArrowDown"`,
/// `"a"`, with `"ctrl+a"` and the like for the modifiers — as the stroke the shell presses.
pub(crate) fn key_stroke(name: &str) -> Option<frus_widgets::KeyStroke> {
    use frus_widgets::ShortcutKey as K;
    let mut stroke = frus_widgets::KeyStroke {
        key: K::Enter,
        ctrl: false,
        shift: false,
        alt: false,
        meta: false,
    };
    let mut parts: Vec<&str> = name.split('+').collect();
    let key = parts.pop()?.trim();
    for modifier in parts {
        match modifier.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => stroke.ctrl = true,
            "shift" => stroke.shift = true,
            "alt" | "option" => stroke.alt = true,
            "meta" | "cmd" | "command" | "super" => stroke.meta = true,
            _ => return None,
        }
    }
    stroke.key = match key.to_ascii_lowercase().as_str() {
        "enter" | "return" => K::Enter,
        "escape" | "esc" => K::Escape,
        "tab" => K::Tab,
        "space" | " " => K::Space,
        "backspace" => K::Backspace,
        "delete" | "del" => K::Delete,
        "arrowup" | "up" => K::Up,
        "arrowdown" | "down" => K::Down,
        "arrowleft" | "left" => K::Left,
        "arrowright" | "right" => K::Right,
        "home" => K::Home,
        "end" => K::End,
        "pageup" => K::PageUp,
        "pagedown" => K::PageDown,
        f if f.len() > 1 && f.starts_with('f') && f[1..].parse::<u8>().is_ok() => {
            K::F(f[1..].parse().ok()?)
        }
        _ => {
            let mut chars = key.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            K::Char(c)
        }
    };
    Some(stroke)
}

/// Standard base64, padded: how a screenshot travels in JSON.
#[cfg_attr(not(any(desktop, test)), allow(dead_code))]
pub(crate) fn base64(bytes: &[u8]) -> String {
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

/// One frame's costs, as `frames` lists them.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct FrameRecord {
    /// Its number, counted from the first frame recorded.
    pub number: u64,
    /// Milliseconds since the frame before it began; 0 for the first.
    pub interval: f32,
    /// Building the tree, when it was built.
    pub build: Option<f32>,
    /// Laying it out and painting it into a scene.
    pub layout_paint: f32,
    /// Rendering the scene, waiting for the GPU and the display included.
    pub render: f32,
    /// Of that, waiting for a surface image to draw into.
    pub acquire: f32,
    /// Of that, tessellating, encoding and submitting: the CPU's part.
    pub draw: f32,
    /// The GPU's own time, where the device can say.
    pub gpu: Option<f32>,
    /// What asked for the frame after it.
    pub why: crate::frame_stats::Why,
}

/// How many frames are kept: ten seconds at 60 Hz.
const KEPT_FRAMES: usize = 600;

/// The frames' costs, recorded once a client asked for them.
#[derive(Default)]
pub(crate) struct Frames {
    /// Whether frames are recorded: from the first `frames` request on.
    pub recording: bool,
    records: std::collections::VecDeque<FrameRecord>,
    next: u64,
    last_start: Option<web_time::Instant>,
}

impl Frames {
    /// Records a frame that began at `start`.
    pub(crate) fn push(
        &mut self,
        start: web_time::Instant,
        costs: &crate::frame_stats::FrameCosts,
    ) {
        let interval = self.last_start.map_or(0.0, |before| {
            start.saturating_duration_since(before).as_secs_f32() * 1000.0
        });
        self.last_start = Some(start);
        if self.records.len() == KEPT_FRAMES {
            self.records.pop_front();
        }
        self.records.push_back(FrameRecord {
            number: self.next,
            interval,
            build: costs.build,
            layout_paint: costs.layout_paint,
            render: costs.render,
            acquire: costs.acquire,
            draw: costs.draw,
            gpu: costs.gpu,
            why: costs.why,
        });
        self.next += 1;
    }

    /// The frames recorded after frame `after` (all of them for `None`), as `frames` answers.
    pub(crate) fn after(&self, after: Option<u64>) -> Value {
        let frames: Vec<Value> = self
            .records
            .iter()
            .filter(|r| after.is_none_or(|after| r.number > after))
            .map(|r| {
                json!({
                    "number": r.number,
                    "interval": r.interval,
                    "build": r.build,
                    "layoutPaint": r.layout_paint,
                    "render": r.render,
                    "acquire": r.acquire,
                    "draw": r.draw,
                    "gpu": r.gpu,
                    "why": r.why.names(),
                })
            })
            .collect();
        json!({ "frames": frames, "budget": 1000.0 / 60.0 })
    }
}

/// What the debugging tools hold between frames.
#[derive(Default)]
pub(crate) struct DevTools {
    /// The requests taken at the start of this frame, answered once it is laid out.
    pub pending: Vec<Request>,
    /// The screenshots asked for, answered once the frame is drawn.
    pub screenshots: Vec<Request>,
    /// The widgets of the last frame laid out with them collected.
    pub nodes: Vec<InspectorNode>,
    /// The widget selected, highlighted over the application.
    pub selected: Option<WidgetId>,
    /// Select mode: a tap selects the widget under it instead of reaching the application,
    /// and the widget under the pointer is highlighted.
    pub select_mode: bool,
    /// The frames' costs.
    pub frames: Frames,
}

impl DevTools {
    /// Whether this frame must collect the widgets: a request will ask about them, or one is
    /// highlighted.
    pub(crate) fn wants_nodes(&self) -> bool {
        !self.pending.is_empty() || self.selected.is_some() || self.select_mode
    }

    /// Whether the frames' costs are measured.
    pub(crate) fn measuring(&self) -> bool {
        self.frames.recording
    }
}

/// The name of the local socket an Android application under `frus run` answers on: one per
/// package, in the abstract namespace.
#[cfg_attr(not(android), allow(dead_code))]
pub(crate) fn socket_name(package: &str) -> String {
    format!("frus_devtools_{package}")
}

/// Answers on the local socket `frus run` reaches with `adb forward`, for as long as the
/// application runs: each line read is a request, each answer a line written back.
#[cfg(android)]
pub(crate) fn serve_on_device() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::android::net::SocketAddrExt;
    use std::os::unix::net::{SocketAddr, UnixListener};

    let package = std::fs::read_to_string("/proc/self/cmdline")
        .ok()
        .and_then(|line| line.split('\0').next().map(str::to_string))
        .filter(|name| !name.is_empty());
    let Some(package) = package else {
        return;
    };
    let name = socket_name(&package);
    let listener = SocketAddr::from_abstract_name(name.as_bytes())
        .and_then(|address| UnixListener::bind_addr(&address));
    let listener = match listener {
        Ok(listener) => listener,
        Err(error) => {
            log::warn!(target: "frus::dev", "DevTools cannot listen on @{name}: {error}");
            return;
        }
    };
    std::thread::spawn(move || {
        for stream in listener.incoming().map_while(Result::ok) {
            std::thread::spawn(move || {
                let Ok(writer) = stream.try_clone() else {
                    return;
                };
                let writer = std::sync::Arc::new(Mutex::new(writer));
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let writer = writer.clone();
                    push(Request::new(line, move |answer| {
                        if let Ok(mut writer) = writer.lock() {
                            let _ = writer.write_all(format!("{answer}\n").as_bytes());
                        }
                    }));
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use frus_widgets::{build_ui_inspected, Column, Container, Keyed, Runtime, Size, Text, Theme};

    fn page() -> (frus_widgets::Ui<()>, Vec<InspectorNode>) {
        let root: Column<()> = Column::new()
            .child(Text::new("Title"))
            .child(Keyed::new(7, Container::new().width(80.0).height(30.0)));
        let (runtime, theme) = (Runtime::default(), Theme::default());
        build_ui_inspected(&root, Size::new(200.0, 200.0), &runtime, &theme)
    }

    #[test]
    fn requests_are_read_and_answered() {
        let call = parse(r#"{"id":3,"method":"tree","params":{"a":1}}"#).unwrap();
        assert_eq!(call.id, json!(3));
        assert_eq!(call.method, "tree");
        assert_eq!(call.params, json!({"a":1}));
        let call = parse(r#"{"method":"hello"}"#).unwrap();
        assert_eq!(call.params, json!({}));
        assert!(parse("{}").is_err());
        assert!(parse("nope").is_err());
        let read = |text: String| json::from_str::<Value>(&text).unwrap();
        assert_eq!(
            read(answer(&json!(3), Ok(json!(true)))),
            json!({"id": 3, "result": true})
        );
        assert_eq!(
            read(answer(&json!(4), Err("no".into()))),
            json!({"id": 4, "error": "no"})
        );
    }

    #[test]
    fn ids_survive_the_trip() {
        let id = WidgetId::from_u64(0xdead_beef_0123_4567);
        assert_eq!(parse_id(&id_text(id)), Some(id));
        assert_eq!(parse_id("#00000000000000ff"), Some(WidgetId::from_u64(255)));
        assert_eq!(parse_id("zz"), None);
    }

    /// Each node names its parent: the last one before it a level up.
    #[test]
    fn the_tree_knows_each_parent() {
        let (ui, nodes) = page();
        let parents = parents(&nodes);
        assert_eq!(parents[0], None);
        for (i, parent) in parents.iter().enumerate().skip(1) {
            let p = parent.expect("every node but the root has a parent");
            assert_eq!(nodes[p].depth + 1, nodes[i].depth);
        }
        let extras = Extras::of(ui.scene().primitives(), ui.semantics());
        let tree = tree_json(&nodes, &extras);
        let tree = tree.as_array().unwrap();
        assert_eq!(tree.len(), nodes.len());
        let title = tree
            .iter()
            .find(|n| n["text"] == "Title")
            .expect("the words");
        assert_eq!(title["name"], "Text");
        // The key as frus holds it: a hash of the value given to `Keyed`, in hexadecimal.
        let keyed = tree
            .iter()
            .find(|n| n.get("key").is_some())
            .expect("the key");
        assert_eq!(keyed["rect"]["width"], 80.0);
    }

    /// A widget is found by its words, its type or its key; several criteria must all hold.
    #[test]
    fn widgets_are_found() {
        let (ui, nodes) = page();
        let extras = Extras::of(ui.scene().primitives(), ui.semantics());
        let by = |params: Value| Finder::of(&params).unwrap().find(&nodes, &extras);
        let found = by(json!({"text":"Title"}));
        assert_eq!(found.len(), 1);
        assert_eq!(nodes[found[0]].name, "Text");
        assert!(by(json!({"text":"titl"})).is_empty());
        assert_eq!(by(json!({"text":"titl","contains":true})), found);
        assert_eq!(by(json!({"type":"Text"})), found);
        assert_eq!(
            by(json!({"type":"Text","text":"other"})),
            Vec::<usize>::new()
        );
        let key = nodes
            .iter()
            .find_map(|n| n.key)
            .map(|k| format!("{k:x}"))
            .expect("a keyed widget");
        let keyed = by(json!({ "key": key }));
        assert_eq!(keyed.len(), 1);
        assert!(by(json!({"key": "7"})).is_empty());
        let id = id_text(nodes[keyed[0]].id);
        assert_eq!(by(json!({ "id": id })), keyed);
        assert!(Finder::of(&json!({})).is_none());

        let details = details_json(&nodes, keyed[0], &extras);
        assert_eq!(details["style"]["width"], "Length(80.0)");
        assert_eq!(details["ancestors"][0]["name"], nodes[0].name);
    }

    #[test]
    fn keys_are_named() {
        use frus_widgets::ShortcutKey as K;
        assert_eq!(key_stroke("Enter").unwrap().key, K::Enter);
        assert_eq!(key_stroke("ArrowDown").unwrap().key, K::Down);
        assert_eq!(key_stroke("F5").unwrap().key, K::F(5));
        let select_all = key_stroke("ctrl+a").unwrap();
        assert_eq!(select_all.key, K::Char('a'));
        assert!(select_all.ctrl && !select_all.shift);
        assert!(key_stroke("hyper+a").is_none());
        assert!(key_stroke("nonsense").is_none());
    }

    #[test]
    fn base64_is_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    /// Frames are kept up to a limit, numbered, and listed after a number.
    #[test]
    fn frames_are_listed_after_a_number() {
        let mut frames = Frames::default();
        let start = web_time::Instant::now();
        let costs = crate::frame_stats::FrameCosts {
            layout_paint: 2.0,
            ..Default::default()
        };
        for i in 0..(KEPT_FRAMES as u64 + 5) {
            frames.push(start + std::time::Duration::from_millis(16 * i), &costs);
        }
        let all = frames.after(None);
        let list = all["frames"].as_array().unwrap();
        assert_eq!(list.len(), KEPT_FRAMES);
        assert_eq!(list[0]["number"], 5);
        assert!((list[1]["interval"].as_f64().unwrap() - 16.0).abs() < 0.01);
        let newer = frames.after(Some(KEPT_FRAMES as u64 + 2));
        assert_eq!(newer["frames"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn requests_wait_in_order() {
        let _ = take();
        let (send, receive) = std::sync::mpsc::channel();
        for n in 0..3 {
            let send = send.clone();
            push(Request::new(format!("{n}"), move |a| {
                let _ = send.send(a);
            }));
        }
        let taken = take();
        assert_eq!(
            taken.iter().map(|r| r.body.as_str()).collect::<Vec<_>>(),
            ["0", "1", "2"]
        );
        for request in taken {
            let body = request.body.clone();
            request.answer(body);
        }
        assert_eq!(receive.try_iter().collect::<Vec<_>>(), ["0", "1", "2"]);
    }
}
