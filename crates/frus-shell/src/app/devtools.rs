// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The shell's side of DevTools (milestone 788): each request of [`crate::devtools`] answered
//! against the frame just laid out, and what it asks of the window carried out — a tap, a
//! scroll, typing, a widget highlighted. Debug builds only.

use frus_widgets::__serde_json::{json, Value};

use super::*;
use crate::devtools::{self as dt, Extras, Finder};

/// What a request that acts on the application is told when nothing on screen matches.
fn nothing_matches(params: &Value) -> String {
    format!("no widget on screen matches {params}")
}

/// A point the request names: `{"x":10,"y":20}`.
fn point_of(params: &Value) -> Option<Point> {
    let x = params.get("x")?.as_f64()?;
    let y = params.get("y")?.as_f64()?;
    Some(Point::new(x as f32, y as f32))
}

fn center(rect: Rect) -> Point {
    Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

/// The platforms by the names the protocol uses.
const PLATFORMS: [(&str, frus_widgets::TargetPlatform); 6] = [
    ("android", frus_widgets::TargetPlatform::Android),
    ("ios", frus_widgets::TargetPlatform::Ios),
    ("windows", frus_widgets::TargetPlatform::Windows),
    ("macos", frus_widgets::TargetPlatform::MacOs),
    ("linux", frus_widgets::TargetPlatform::Linux),
    ("fuchsia", frus_widgets::TargetPlatform::Fuchsia),
];

/// The platform the shell runs on, as `hello` names it.
fn host() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_arch = "wasm32") {
        "web"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else {
        "linux"
    }
}

impl<A: Application> App<A> {
    /// The requests that arrived since the last frame, kept to be answered once this frame
    /// is laid out.
    pub(super) fn take_devtools(&mut self) {
        let mut arrived = dt::take();
        if !arrived.is_empty() {
            self.devtools.pending.append(&mut arrived);
        }
    }

    /// Answers the requests taken at the start of the frame, against what it laid out.
    pub(super) fn answer_devtools(&mut self) {
        let asked = !self.devtools.pending.is_empty();
        for request in std::mem::take(&mut self.devtools.pending) {
            let call = match dt::parse(&request.body) {
                Ok(call) => call,
                Err(why) => {
                    request.answer(dt::answer(&Value::Null, Err(why)));
                    continue;
                }
            };
            // A picture of what is drawn: answered once the next frame is.
            if call.method == "screenshot" {
                self.devtools.screenshots.push(request);
                self.request_redraw();
                continue;
            }
            let outcome = self.devtools_call(&call.method, &call.params);
            request.answer(dt::answer(&call.id, outcome));
        }
        // The switches as this frame leaves them, for `state` to be answered without a
        // frame — from the first request on.
        self.devtools.active |= asked;
        if self.devtools.active {
            dt::publish_state(self.devtools_state());
        }
    }

    /// The surface's logical size.
    fn devtools_size(&self) -> Value {
        let (width, height) = self.last_size.unwrap_or((0.0, 0.0));
        json!({ "width": width, "height": height })
    }

    /// One method of the protocol, answered.
    pub(super) fn devtools_call(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        match method {
            "hello" => Ok(json!({
                "protocol": dt::PROTOCOL,
                "frus": env!("CARGO_PKG_VERSION"),
                "platform": host(),
                "size": self.devtools_size(),
                "devicePixelRatio": self.total_scale(),
            })),
            "tree" => {
                let ui = self.ui.as_ref().ok_or("nothing is laid out yet")?;
                let extras = Extras::of(ui.scene().primitives(), ui.semantics());
                Ok(json!({
                    "size": self.devtools_size(),
                    "nodes": dt::tree_json(&self.devtools.nodes, &extras),
                    "selected": self.devtools.selected.map(dt::id_text),
                }))
            }
            "node" => {
                let id = params
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(dt::parse_id)
                    .ok_or("`node` takes the `id` of a widget")?;
                let ui = self.ui.as_ref().ok_or("nothing is laid out yet")?;
                let extras = Extras::of(ui.scene().primitives(), ui.semantics());
                let index = self
                    .devtools
                    .nodes
                    .iter()
                    .position(|n| n.id == id)
                    .ok_or("that widget is not on screen any more")?;
                // The axes each scroll area frees: its content is offered no bound there.
                let scrolls = |id: WidgetId| {
                    ui.scroll_regions()
                        .iter()
                        .find(|area| area.id == id)
                        .map_or((false, false), |area| match area.axis {
                            frus_widgets::Axis::Vertical => (false, true),
                            frus_widgets::Axis::Horizontal => (true, false),
                            _ => (true, true),
                        })
                };
                let surface = self.last_size.unwrap_or((0.0, 0.0));
                Ok(dt::details_json(
                    &self.devtools.nodes,
                    index,
                    &extras,
                    scrolls,
                    surface,
                ))
            }
            "find" => {
                let finder = Finder::of(params)
                    .ok_or("`find` takes `text`, `label`, `type`, `key` or `id`")?;
                let ui = self.ui.as_ref().ok_or("nothing is laid out yet")?;
                let extras = Extras::of(ui.scene().primitives(), ui.semantics());
                let found: Vec<Value> = finder
                    .find(&self.devtools.nodes, &extras)
                    .into_iter()
                    .map(|i| self.summary(i, &extras))
                    .collect();
                Ok(json!({ "nodes": found }))
            }
            "at" => {
                let at = point_of(params).ok_or("`at` takes `x` and `y`")?;
                let ui = self.ui.as_ref().ok_or("nothing is laid out yet")?;
                let extras = Extras::of(ui.scene().primitives(), ui.semantics());
                let node = frus_widgets::node_at(&self.devtools.nodes, at);
                let index =
                    node.and_then(|n| self.devtools.nodes.iter().position(|m| m.id == n.id));
                Ok(index.map_or(Value::Null, |i| self.summary(i, &extras)))
            }
            "select" => {
                self.devtools.selected = match params.get("id") {
                    Some(Value::String(id)) => Some(dt::parse_id(id).ok_or("not a widget's id")?),
                    _ => None,
                };
                self.request_redraw();
                Ok(json!({ "selected": self.devtools.selected.map(dt::id_text) }))
            }
            "tap" => {
                let (at, id) = self.devtools_target(params)?;
                self.devtools_tap(at);
                Ok(json!({ "x": at.x, "y": at.y, "id": id.map(dt::id_text) }))
            }
            "scroll" => {
                let (at, _) = self.devtools_target(params)?;
                let dx = params.get("dx").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                let dy = params.get("dy").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                let moved_from = self.cursor;
                self.cursor = at;
                // `dy > 0` brings what is further down into view, as a finger moving up does:
                // the opposite of a wheel's number.
                self.wheel_scroll(-dx, -dy);
                self.cursor = moved_from;
                Ok(json!({ "x": at.x, "y": at.y }))
            }
            "type" => {
                // The field to type into, tapped first: `target`, a finder or a point.
                if let Some(target) = params.get("target") {
                    let (at, _) = self.devtools_target(target)?;
                    self.devtools_tap(at);
                }
                let text = params
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or("`type` takes the `text` to type")?;
                // Into a field, straight through the field's editing, as an input method
                // commits text: on Android the keys' own path is closed to typing while the
                // input bridge is installed — editing goes through the input connection
                // alone — and keys sent there were dropped (milestone 788).
                let field = self.runtime.input.focused.filter(|&id| {
                    self.tree
                        .as_ref()
                        .and_then(|tree| find_widget(tree.as_ref(), id))
                        .is_some_and(wants_keyboard)
                });
                if let Some(id) = field {
                    for c in text.chars() {
                        let key = match c {
                            '\n' => Key::Enter,
                            c => Key::Text(c.to_string()),
                        };
                        self.apply_key(id, key);
                    }
                    self.request_redraw();
                    return Ok(json!({
                        "typed": text.chars().count(),
                        "focused": dt::id_text(id),
                    }));
                }
                for c in text.chars() {
                    let key = match c {
                        '\n' => frus_widgets::ShortcutKey::Enter,
                        ' ' => frus_widgets::ShortcutKey::Space,
                        c => frus_widgets::ShortcutKey::Char(c),
                    };
                    self.press_stroke(frus_widgets::KeyStroke {
                        key,
                        ctrl: false,
                        shift: false,
                        alt: false,
                        meta: false,
                    });
                }
                self.request_redraw();
                Ok(json!({
                    "typed": text.chars().count(),
                    "focused": self.runtime.input.focused.map(dt::id_text),
                }))
            }
            "setText" => {
                let target = params
                    .get("target")
                    .ok_or("`setText` takes the `target` field, as a finder")?;
                let (_, id) = self.devtools_target(target)?;
                let id = id.ok_or("`setText` names a field, not a point")?;
                let text = params
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or("`setText` takes the `text` to put in the field")?;
                self.set_value_for_assistive_tech(id, text.to_string());
                Ok(json!({ "id": dt::id_text(id) }))
            }
            "key" => {
                let name = params
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or("`key` takes the `key` to press")?;
                let stroke = dt::key_stroke(name).ok_or_else(|| format!("no key is {name:?}"))?;
                self.press_stroke(stroke);
                self.request_redraw();
                Ok(json!({}))
            }
            "back" => {
                let went = self.system_back_step();
                self.request_redraw();
                Ok(json!({ "wentBack": went }))
            }
            "state" => Ok(self.devtools_state()),
            "set" => {
                self.devtools_set(params)?;
                self.build_dirty = true;
                self.request_redraw();
                Ok(self.devtools_state())
            }
            _ => Err(format!("this application does not know `{method}`")),
        }
    }

    /// A widget as `find` and `at` answer: its id, name, box and centre, and its words and
    /// label when it has them.
    fn summary(&self, index: usize, extras: &Extras) -> Value {
        let node = &self.devtools.nodes[index];
        let at = center(node.rect);
        json!({
            "id": dt::id_text(node.id),
            "name": node.name,
            "rect": { "x": node.rect.x, "y": node.rect.y, "width": node.rect.width, "height": node.rect.height },
            "center": { "x": at.x, "y": at.y },
            "text": extras.texts.get(&node.id.as_u64()),
            "label": extras.semantics.get(&node.id.as_u64()).and_then(|s| s.label.clone()),
        })
    }

    /// Where a request that acts aims: the point it names, or the centre of the widget it
    /// names — the one on top when several match.
    fn devtools_target(&self, params: &Value) -> Result<(Point, Option<WidgetId>), String> {
        if let Some(at) = point_of(params) {
            return Ok((at, None));
        }
        let finder = Finder::of(params)
            .ok_or("name a widget (`text`, `label`, `type`, `key`, `id`) or a point (`x`, `y`)")?;
        let ui = self.ui.as_ref().ok_or("nothing is laid out yet")?;
        let extras = Extras::of(ui.scene().primitives(), ui.semantics());
        let found = finder.find(&self.devtools.nodes, &extras);
        let node = found
            .last()
            .map(|&i| &self.devtools.nodes[i])
            .ok_or_else(|| nothing_matches(params))?;
        Ok((center(node.rect), Some(node.id)))
    }

    /// A tap at `at`: a finger down and up, as the platform delivers one. A mouse's click on a
    /// desktop, a finger elsewhere.
    fn devtools_tap(&mut self, at: Point) {
        let touch = !cfg!(desktop);
        for kind in [PointerKind::Down, PointerKind::Up] {
            self.pointer_event(PointerEvent {
                kind,
                position: at,
                touch,
            });
        }
        self.request_redraw();
    }

    /// The switches a developer flips while looking, as they are.
    fn devtools_state(&self) -> Value {
        let platform = frus_widgets::default_target_platform();
        let platform = PLATFORMS
            .iter()
            .find(|(_, p)| *p == platform)
            .map(|(name, _)| *name);
        json!({
            "animationSpeed": frus_widgets::animation_speed(),
            "debugPaint": self.debug.outlines,
            "inspector": self.inspector,
            "performanceOverlay": self.debug.performance,
            "brightness": match self.brightness() {
                frus_widgets::Brightness::Light => "light",
                frus_widgets::Brightness::Dark => "dark",
            },
            "platform": platform,
            "textScale": self.debug.text_scale.unwrap_or(self.platform.text_scaler),
            "selectMode": self.devtools.select_mode,
            "repaintRainbow": self.devtools.rainbow,
            "selected": self.devtools.selected.map(dt::id_text),
            "size": self.devtools_size(),
        })
    }

    /// Flips the switches `params` names; those it does not name stay as they are.
    fn devtools_set(&mut self, params: &Value) -> Result<(), String> {
        if let Some(speed) = params.get("animationSpeed").and_then(Value::as_f64) {
            frus_widgets::set_animation_speed(speed as f32);
        }
        if let Some(on) = params.get("debugPaint").and_then(Value::as_bool) {
            self.debug.outlines = on;
        }
        if let Some(on) = params.get("inspector").and_then(Value::as_bool) {
            self.inspector = on;
        }
        if let Some(on) = params.get("performanceOverlay").and_then(Value::as_bool) {
            self.debug.performance = on;
            self.debug.last_costs = None;
            self.debug.history = FrameHistory::default();
            self.frame_stats.set_enabled(on);
        }
        match params.get("brightness") {
            Some(Value::String(b)) if b == "light" => {
                self.debug.brightness = Some(frus_widgets::Brightness::Light)
            }
            Some(Value::String(b)) if b == "dark" => {
                self.debug.brightness = Some(frus_widgets::Brightness::Dark)
            }
            Some(Value::Null) => self.debug.brightness = None,
            Some(other) => {
                return Err(format!(
                    "brightness is \"light\", \"dark\" or null, not {other}"
                ))
            }
            None => {}
        }
        match params.get("platform") {
            Some(Value::String(name)) => {
                let platform = PLATFORMS
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case(name))
                    .map(|(_, p)| *p)
                    .ok_or_else(|| format!("no platform is called {name:?}"))?;
                frus_widgets::set_debug_default_target_platform_override(Some(platform));
            }
            Some(Value::Null) => frus_widgets::set_debug_default_target_platform_override(None),
            _ => {}
        }
        match params.get("textScale") {
            Some(Value::Null) => self.debug.text_scale = None,
            Some(scale) => {
                if let Some(scale) = scale.as_f64() {
                    self.debug.text_scale = Some((scale as f32).clamp(0.5, 4.0));
                }
            }
            None => {}
        }
        if let Some(on) = params.get("selectMode").and_then(Value::as_bool) {
            self.devtools.select_mode = on;
        }
        if let Some(on) = params.get("repaintRainbow").and_then(Value::as_bool) {
            self.devtools.rainbow = on;
        }
        Ok(())
    }

    /// A pointer event from the window in select mode: it selects the widget it lands on and
    /// goes no further. `false` when select mode is off, and the event is the application's.
    pub(super) fn devtools_pointer(&mut self, event: &PointerEvent) -> bool {
        if !self.devtools.select_mode {
            return false;
        }
        self.cursor = event.position;
        if event.kind == PointerKind::Down {
            if let Some(node) = frus_widgets::node_at(&self.devtools.nodes, event.position) {
                self.devtools.selected = Some(node.id);
                log::info!(
                    target: "frus::dev",
                    "DevTools: selected {} {}",
                    node.name,
                    dt::id_text(node.id)
                );
            }
        }
        self.request_redraw();
        true
    }

    /// The widget to highlight over the application: the one under the pointer in select
    /// mode, the one selected otherwise.
    pub(super) fn devtools_highlight<'n>(
        &self,
        nodes: &'n [frus_widgets::InspectorNode],
    ) -> Option<&'n frus_widgets::InspectorNode> {
        if self.devtools.select_mode {
            if let Some(node) = frus_widgets::node_at(nodes, self.cursor) {
                return Some(node);
            }
        }
        let selected = self.devtools.selected?;
        nodes.iter().find(|n| n.id == selected)
    }
}

/// A colour of the rainbow, the `n`th: hues a golden angle apart, so that two paints in a row
/// never look alike.
fn rainbow(n: u32) -> frus_widgets::Color {
    let hue = (n as f32 * 0.618_034).fract() * 6.0;
    let x = 1.0 - (hue % 2.0 - 1.0).abs();
    let (r, g, b) = match hue as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    frus_widgets::Color::rgba(r, g, b, 0.9)
}

/// `frame` with the repaint rainbow over it: a border around the whole frame, coloured by
/// how many frames have been drawn, and one around each repaint boundary, coloured by how
/// many times it has been painted rather than replayed. A colour that stays put is a part of
/// the screen that was not painted again. `frame` is in physical pixels, `scale` times the
/// boundaries' logical ones.
pub(super) fn paint_rainbow(
    frame: &Scene,
    boundaries: &[(Rect, u32)],
    frames: u32,
    surface: Size,
    scale: f32,
) -> Scene {
    let mut scene = on_top_of(frame);
    scene.set_clip(Rect::UNBOUNDED);
    scene.set_owner(0);
    let width = (3.0 * scale).max(2.0);
    let whole = Rect::new(0.0, 0.0, surface.width * scale, surface.height * scale);
    scene.draw_rect(
        whole,
        frus_widgets::Color::TRANSPARENT,
        0.0,
        width,
        rainbow(frames),
    );
    for (rect, painted) in boundaries {
        let rect = Rect::new(
            rect.x * scale,
            rect.y * scale,
            rect.width * scale,
            rect.height * scale,
        );
        scene.draw_rect(
            rect,
            rainbow(*painted).with_alpha(0.25),
            0.0,
            width,
            rainbow(*painted),
        );
    }
    scene
}

/// Answers the screenshots asked for with `frame`, the frame just drawn: `width` by `height`
/// physical pixels, drawn again offscreen as `s` does.
#[cfg(desktop)]
pub(super) fn answer_screenshots(
    requests: Vec<dt::Request>,
    frame: &Scene,
    width: f32,
    height: f32,
) {
    if requests.is_empty() {
        return;
    }
    let (w, h) = (
        width.round().max(1.0) as u32,
        height.round().max(1.0) as u32,
    );
    let drawn = frus_gpu::render_offscreen(frame, w, h, frus_gpu::backdrop());
    for request in requests {
        let id = dt::parse(&request.body)
            .map(|c| c.id)
            .unwrap_or(Value::Null);
        let outcome = match &drawn {
            Some(image) => Ok(json!({
                "format": "png",
                "width": image.width,
                "height": image.height,
                "data": dt::base64(&crate::dev_commands::png(image.width, image.height, &image.rgba)),
            })),
            None => Err("no GPU to draw the screenshot with".to_string()),
        };
        request.answer(dt::answer(&id, outcome));
    }
}

/// The same on the web: the canvas, read in the task that drew it.
#[cfg(web)]
pub(super) fn answer_screenshots(requests: Vec<dt::Request>, window: Option<&Window>) {
    if requests.is_empty() {
        return;
    }
    let picture = window.and_then(crate::web_dev::canvas_png);
    for request in requests {
        let id = dt::parse(&request.body)
            .map(|c| c.id)
            .unwrap_or(Value::Null);
        let outcome = match &picture {
            Some(data) => Ok(json!({ "format": "png", "data": data })),
            None => Err("the page's canvas could not be read".to_string()),
        };
        request.answer(dt::answer(&id, outcome));
    }
}

/// Android: the device's own screenshot, which `frus run` takes, is the one asked for.
#[cfg(not(any(desktop, web)))]
pub(super) fn answer_screenshots(requests: Vec<dt::Request>) {
    for request in requests {
        let id = dt::parse(&request.body)
            .map(|c| c.id)
            .unwrap_or(Value::Null);
        request.answer(dt::answer(
            &id,
            Err(
                "the application takes no screenshot of itself here: frus run takes the device's"
                    .into(),
            ),
        ));
    }
}

#[cfg(test)]
mod tests {
    use frus_widgets::__serde_json::json;
    use frus_widgets::{component, Column, Text, TextField, Widget};

    use crate::app::testing::Driver;
    use crate::FrusApp;

    /// `type` writes into the field with the focus through the field's own editing, not
    /// through the keys: on Android the keys' path is closed to typing while the input
    /// bridge is installed, and the characters sent there were dropped.
    #[test]
    fn typing_reaches_the_field_with_the_focus() {
        let app = FrusApp::from_fn(|_| {
            Box::new(component(|cx| {
                let name = cx.use_state(String::new);
                let typed = name.clone();
                Box::new(
                    Column::new()
                        .child(
                            TextField::new(name.get())
                                .label("Name")
                                .on_input(move |t: String| typed.set(t)),
                        )
                        .child(Text::new(format!("Hello {}", name.get()))),
                ) as Box<dyn Widget>
            }))
        });
        let mut driver = Driver::new(app, 400.0, 300.0);
        driver.run(0.1);
        // The field, found by its label and tapped, has the focus.
        let field = {
            let (ui, _) = driver.frame_parts().unwrap();
            ui.semantics()
                .iter()
                .find(|(_, _, sem)| sem.label.as_deref() == Some("Name"))
                .map(|(_, rect, _)| *rect)
                .expect("the field")
        };
        let at = frus_widgets::Point::new(field.x + 10.0, field.y + field.height / 2.0);
        driver.press(at);
        driver.release(at);
        driver.run(0.1);
        assert!(driver.focused().is_some(), "the tap focused the field");
        let answer = driver
            .devtools_call("type", json!({ "text": "Ada" }))
            .unwrap();
        assert_eq!(answer["typed"], 3);
        driver.run(0.1);
        let texts: Vec<String> = driver.texts().into_iter().map(|(t, _)| t).collect();
        assert!(texts.iter().any(|t| t == "Hello Ada"), "{texts:?}");
    }
}
