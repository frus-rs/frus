// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The **accessibility** bridge for the web: maps frus's semantic tree
//! ([`frus_widgets::Ui::semantics`]) onto real DOM elements, so a browser's screen
//! reader (NVDA, JAWS, VoiceOver, Narrator, TalkBack) can read and operate the interface.
//!
//! AccessKit has no web adapter, so this is written against the frus semantic tree the
//! desktop bridge reads: one element per node, diffed against last frame's set.
//!
//! **A layer laid over the canvas, not the canvas's fallback content** (milestone 845). The
//! first bridge (milestone 564) nested its elements inside the `<canvas>`, which the browser
//! never lays out. Read through the accessibility tree, that costs more than it looks:
//!
//! - no node has a box. A reader that touches the screen to hear what is under the finger
//!   (VoiceOver on a phone, TalkBack in Chrome) finds nothing but the canvas, and a desktop
//!   reader draws its highlight around nothing;
//! - the live machinery watches what is rendered, which fallback content is not.
//!
//! So each element is placed at its node's rectangle in a transparent layer over the canvas
//! that takes no pointer: a press goes on to the canvas below, as before; a reader's touch
//! lands on the element at that place; a reader's "activate" is a `click` on the element.
//!
//! What the words are made of matters too. A node's own words (a text, a heading, a field's
//! contents) are **generated content** (`::before { content: attr(data-t) }`): exposed to the
//! accessibility tree as text, so a reader browsing the page reads them, and **not** found by
//! the browser's find-in-page, which already finds them once, in the text layer (milestone 830).
//!
//! Keys: an element of the layer is not inside the canvas, so a key pressed while it has the
//! browser's focus does not reach winit by itself. A button keeps Enter and Space (the browser
//! turns them into the `click` this module listens for); a slider takes its arrows as an
//! adjustment of **its** widget, which is also what an assistive technology's "increment"
//! sends; the browser keeps its own shortcuts; every other key is handed on to the canvas,
//! where the shell treats it as any key.
//!
//! **Cost.** With no change in the tree, a frame compares each node's annotation and
//! rectangle with last frame's and touches the DOM not at all; a node that changed has only
//! the attributes that changed written. Nothing here knows whether an assistive technology is
//! running — the web does not say — so this runs for every page.

use frus_widgets::{Rect, Role, SemanticsProperties, Toggled};

/// The ARIA role for a frus role, or `None` for a node read as plain text — the web's
/// counterpart to `a11y::to_ak_role`.
///
/// Static text has no ARIA role of its own (`text` is not one: a browser reads it as an
/// element with no role, and a reader skips the name of such an element). It is an element
/// with no role whose words are its content.
fn aria_role(role: Role) -> Option<&'static str> {
    Some(match role {
        Role::None => "group",
        Role::Label => return None,
        Role::Heading => "heading",
        Role::Button => "button",
        Role::Link => "link",
        Role::CheckBox => "checkbox",
        Role::Switch => "switch",
        Role::RadioButton => "radio",
        Role::Slider => "slider",
        Role::TextInput => "textbox",
        Role::Image => "img",
        Role::Tab => "tab",
        Role::ListItem => "listitem",
        Role::ProgressBar => "progressbar",
        Role::Dialog => "dialog",
        // A focusable separator with a value is the splitter of the accessibility
        // specification's window-splitter pattern.
        Role::Splitter => "separator",
        Role::Tree => "tree",
        Role::TreeItem => "treeitem",
    })
}

/// Whether a node of this role is one the shell may give the focus to, and so needs to be
/// focusable when it is not a button. Text, a heading, a picture and a progress bar are read,
/// not focused: made focusable, a reader announces them as something to interact with.
fn focus_target(role: Role) -> bool {
    !matches!(
        role,
        Role::Label | Role::Heading | Role::Image | Role::ProgressBar
    )
}

/// Whether the arrows of a node of this role change its value: a slider's, a splitter's.
pub(crate) fn adjustable(sem: &SemanticsProperties) -> bool {
    matches!(sem.role, Role::Slider | Role::Splitter) && sem.range.is_some() && !sem.disabled
}

/// The element a node is shown as: its tag and every attribute it carries, in a fixed
/// order. Diffed attribute by attribute against last frame's, so only what changed is
/// written.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NodeDom {
    /// `button` for a node that can be pressed — the browser turns Enter and Space on it into
    /// a `click`, and a reader offers to press it — and `div` for the rest.
    pub tag: &'static str,
    pub attrs: Vec<(&'static str, String)>,
}

impl NodeDom {
    /// The value of attribute `name`, if it is set.
    #[cfg(test)]
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }
}

fn number(value: f32) -> String {
    // `1` rather than `1.0`, as a reader would say it.
    if value.fract() == 0.0 && value.abs() < 1e9 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// The element for a node's annotation.
pub(crate) fn node_dom(sem: &SemanticsProperties) -> NodeDom {
    let button = sem.clickable;
    let mut attrs: Vec<(&'static str, String)> = Vec::new();
    if button {
        attrs.push(("type", "button".into()));
    } else if focus_target(sem.role) {
        // Not a Tab stop of the browser's (the shell moves the focus), but a target the shell
        // can give the browser's focus to when its own focus moves there.
        attrs.push(("tabindex", "-1".into()));
    }
    // An image with nothing to say is decoration: announced, it is only the word "image".
    if sem.role == Role::Image && sem.label.is_none() && sem.value.is_none() {
        attrs.push(("aria-hidden", "true".into()));
        return NodeDom { tag: "div", attrs };
    }
    if let Some(role) = aria_role(sem.role) {
        attrs.push(("role", role.into()));
    }
    // A masked field is a password field: the reader is told so, and its words are dots.
    if sem.obscured && sem.role == Role::TextInput {
        attrs.push(("aria-roledescription", "password".into()));
    }

    // The words. Text and a heading say theirs as their content, the way a page's own text
    // is read; a field shows its contents as its content, which is its value to a reader;
    // anything else is named.
    match sem.role {
        Role::Label | Role::Heading => {
            let words = sem.value.as_ref().or(sem.label.as_ref());
            if let Some(words) = words {
                attrs.push(("data-t", words.clone()));
            }
        }
        _ => {
            if let Some(label) = &sem.label {
                attrs.push(("aria-label", label.clone()));
            }
        }
    }
    match (sem.readable_value(), sem.role) {
        (_, Role::Label | Role::Heading) => {}
        (Some(value), Role::TextInput) => attrs.push(("data-t", value)),
        (Some(value), Role::Slider | Role::Splitter | Role::ProgressBar) => {
            attrs.push(("aria-valuetext", value));
        }
        // A role with no slot for a value (a badge on a button, a caption under a step)
        // has it read after its name.
        (Some(value), _) => attrs.push(("aria-description", value)),
        (None, _) => {}
    }

    match sem.toggled {
        Toggled::None => {}
        // A tab, and a tree's item, are *selected*, not ticked: `aria-checked` on a tab is
        // not read at all, so the tab showing was never said to be the one showing.
        toggled if matches!(sem.role, Role::Tab | Role::TreeItem) => {
            attrs.push(("aria-selected", (toggled == Toggled::True).to_string()));
        }
        // A button that stays down (a filter chip, a toolbar toggle) is *pressed*: a button
        // has no checked state, and the browser drops `aria-checked` on one.
        toggled if sem.role == Role::Button => {
            let pressed = match toggled {
                Toggled::True => "true",
                Toggled::Mixed => "mixed",
                _ => "false",
            };
            attrs.push(("aria-pressed", pressed.into()));
        }
        Toggled::False => attrs.push(("aria-checked", "false".into())),
        Toggled::True => attrs.push(("aria-checked", "true".into())),
        Toggled::Mixed => attrs.push(("aria-checked", "mixed".into())),
    }
    if let Some(expanded) = sem.expanded {
        attrs.push(("aria-expanded", expanded.to_string()));
    }
    if sem.disabled {
        attrs.push(("aria-disabled", "true".into()));
    }
    // A range that is not a number (a progress of nothing done out of nothing to do) says
    // nothing rather than "NaN".
    let range = sem
        .range
        .filter(|(min, value, max)| min.is_finite() && value.is_finite() && max.is_finite());
    if let Some((min, value, max)) = range {
        attrs.push(("aria-valuemin", number(min)));
        attrs.push(("aria-valuenow", number(value)));
        attrs.push(("aria-valuemax", number(max)));
    }
    if sem.role == Role::Dialog {
        // What is behind a dialog is not to be read while it is open.
        attrs.push(("aria-modal", "true".into()));
    }
    NodeDom {
        tag: if button { "button" } else { "div" },
        attrs,
    }
}

/// The element's place, in the layer's CSS pixels: the node's rectangle scaled by `k`, the
/// CSS pixels of one of the layout's. Rounded to a hundredth so a still node is not moved by
/// a float's noise.
pub(crate) fn place(rect: Rect, k: f32) -> [i32; 4] {
    let at = |v: f32| (v * k * 100.0).round() as i32;
    [
        at(rect.x),
        at(rect.y),
        at(rect.width.max(0.0)),
        at(rect.height.max(0.0)),
    ]
}

/// The inline style of an element placed at `place` (hundredths of a CSS pixel).
pub(crate) fn place_style(place: [i32; 4]) -> String {
    let px = |v: i32| format!("{}px", v as f32 / 100.0);
    format!(
        "left:{};top:{};width:{};height:{}",
        px(place[0]),
        px(place[1]),
        px(place[2]),
        px(place[3])
    )
}

/// What a key pressed on one of the layer's elements does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum KeyRoute {
    /// Left to the browser: Enter and Space on a button, which it turns into a `click`; its
    /// own shortcuts.
    Browser,
    /// Moves the value of the node's widget: `true` up, `false` down.
    Adjust(bool),
    /// Handed on to the canvas, for the shell, and kept from the browser.
    Shell,
}

/// Where a key pressed on an element goes. `button` is whether the element is one,
/// `adjustable` whether its arrows change its widget's value.
pub(crate) fn key_route(
    key: &str,
    cmd: bool,
    alt: bool,
    shift: bool,
    button: bool,
    adjustable: bool,
) -> KeyRoute {
    let plain = !cmd && !alt && !shift;
    if button && plain && matches!(key, "Enter" | " ") {
        return KeyRoute::Browser;
    }
    if adjustable && plain {
        match key {
            "ArrowUp" | "ArrowRight" => return KeyRoute::Adjust(true),
            "ArrowDown" | "ArrowLeft" => return KeyRoute::Adjust(false),
            _ => {}
        }
    }
    if crate::web_keys::browser_owns(key, cmd, alt, shift) {
        return KeyRoute::Browser;
    }
    KeyRoute::Shell
}

/// The id of the layer, which the editing element (milestone 711) also knows, and of its
/// style sheet.
#[cfg_attr(not(web), allow(dead_code))]
pub(crate) const LAYER: &str = "frus-semantics";
#[cfg_attr(not(web), allow(dead_code))]
const STYLE: &str = "frus-semantics-style";

/// The layer's style sheet. Everything is transparent and takes no pointer; nothing in it
/// draws, the canvas does. `overflow: clip` rather than `hidden`: a box that hides its
/// overflow can still be scrolled by a script or a reader moving to a node out of view, which
/// would shift every node; a clipped one cannot be.
#[cfg_attr(not(web), allow(dead_code))]
const CSS: &str = "\
#frus-semantics{position:fixed;left:0;top:0;width:0;height:0;overflow:clip;pointer-events:none;\
color:transparent;margin:0;padding:0;z-index:0}\
#frus-semantics>*{position:absolute;margin:0;padding:0;border:0;box-sizing:border-box;\
background:transparent;color:transparent;outline:none;overflow:hidden;font:12px/1 sans-serif;\
white-space:nowrap;pointer-events:none;-webkit-appearance:none;appearance:none}\
#frus-semantics [data-t]::before{content:attr(data-t)}";

#[cfg(web)]
pub(crate) use live::{A11y, A11yAction};

#[cfg(web)]
mod live {
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;
    use std::sync::Arc;

    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::{Element, HtmlCanvasElement, HtmlElement, KeyboardEvent};
    use winit::platform::web::WindowExtWebSys;
    use winit::window::Window;

    use frus_widgets::{Rect, SemanticsProperties, WidgetId};

    use super::{
        adjustable, key_route, node_dom, place, place_style, KeyRoute, NodeDom, CSS, LAYER, STYLE,
    };

    /// An action a screen reader asked for, replayed by the shell's main loop.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub(crate) enum A11yAction {
        /// Activate the widget, that is, click it.
        Click(WidgetId),
        /// Give the widget focus.
        Focus(WidgetId),
        /// Move the widget's value up (`true`) or down: an arrow on a slider, or an assistive
        /// technology's increment and decrement, which a browser sends as those arrows.
        Adjust(WidgetId, bool),
    }

    type Actions = Rc<RefCell<Vec<A11yAction>>>;
    type Listener = Closure<dyn FnMut(web_sys::Event)>;

    /// One live element backing a widget's accessibility, what it was last given, and the
    /// listeners keeping it wired — dropped together when the widget leaves the tree.
    struct Live {
        el: HtmlElement,
        dom: NodeDom,
        sem: SemanticsProperties,
        place: [i32; 4],
        hidden: bool,
        /// Whether the arrows adjust the widget, read by the key listener.
        adjustable: Rc<Cell<bool>>,
        _listeners: Vec<Listener>,
    }

    /// A window's **live** accessibility bridge: a layer of elements over the canvas.
    pub(crate) struct A11y {
        canvas: HtmlCanvasElement,
        root: HtmlElement,
        root_style: String,
        live_region: HtmlElement,
        nodes: HashMap<WidgetId, Live>,
        /// The nodes in the order the layer has them, which is the order they are read in.
        order: Vec<WidgetId>,
        actions: Actions,
        /// Set for the duration of a `.focus()` call we made ourselves, so the `focus`
        /// listener it fires is not queued back as an assistive-technology request.
        programmatic_focus: Rc<Cell<bool>>,
        focus: Option<WidgetId>,
        /// The field edited through the page's own element (milestone 711), whose node here is
        /// hidden: the element is the field's one node while it is.
        editing: Option<WidgetId>,
        title: String,
        announce: String,
        /// Cloned into every listener, so it can wake the loop (`request_redraw`) — the
        /// web only runs a frame it was asked for, and a screen reader's action is not
        /// otherwise anything winit knows to redraw for.
        window: Arc<Window>,
    }

    impl A11y {
        /// Creates the bridge over `window`'s canvas. `None` if there is no canvas or no
        /// document: nothing here is worth a panic over.
        pub(crate) fn new(window: &Arc<Window>) -> Option<Self> {
            let canvas = window.canvas()?;
            let document = canvas.owner_document()?;
            // The canvas is a picture of the interface; the layer is what is read. It keeps
            // the application's name, for a reader that lands on it, and no role that would
            // change how a reader reads the page (`application` made a reader that browses
            // pass over every text and heading in it).
            canvas.remove_attribute("role").ok();
            if document.get_element_by_id(STYLE).is_none() {
                let style = document.create_element("style").ok()?;
                style.set_id(STYLE);
                style.set_text_content(Some(CSS));
                let head = document
                    .query_selector("head")
                    .ok()
                    .flatten()
                    .or_else(|| document.body().map(Into::into))?;
                head.append_child(&style).ok()?;
            }
            let root: HtmlElement = document
                .create_element("div")
                .ok()?
                .dyn_into::<HtmlElement>()
                .ok()?;
            root.set_id(LAYER);
            // A landmark named after the application: a reader can go straight to it.
            root.set_attribute("role", "region").ok();
            canvas
                .parent_node()?
                .insert_before(&root, canvas.next_sibling().as_ref())
                .ok()?;
            let live_region = new_live_region(&canvas)?;
            Some(Self {
                canvas,
                root,
                root_style: String::new(),
                live_region,
                nodes: HashMap::new(),
                order: Vec::new(),
                actions: Rc::new(RefCell::new(Vec::new())),
                programmatic_focus: Rc::new(Cell::new(false)),
                focus: None,
                editing: None,
                title: String::new(),
                announce: String::new(),
                window: window.clone(),
            })
        }

        /// Publishes the frame's semantic tree: creates the elements for new widgets,
        /// updates the ones that changed, removes the ones no longer present, and keeps them
        /// in the tree's order. `k` is the CSS pixels of one of the layout's, `origin` where
        /// the application's screen starts on the canvas, in CSS pixels.
        ///
        /// `editing` is the field edited through the page's own element (milestone 711). Its
        /// node is hidden for as long as it is: a reader browsing the page would otherwise
        /// find the field twice, once here and once as the element.
        #[allow(clippy::too_many_arguments)]
        pub(crate) fn update(
            &mut self,
            nodes: &[(WidgetId, Rect, SemanticsProperties)],
            focus: Option<WidgetId>,
            editing: Option<WidgetId>,
            title: &str,
            announce: &str,
            k: f32,
            origin: (f32, f32),
        ) {
            if title != self.title {
                self.title = title.to_string();
                self.canvas.set_attribute("aria-label", title).ok();
                self.root.set_attribute("aria-label", title).ok();
            }
            // The layer lies over the canvas, wherever the page put it.
            let canvas_box = self.canvas.get_bounding_client_rect();
            let root_style = place_style(place(
                Rect::new(
                    canvas_box.left() as f32 + origin.0,
                    canvas_box.top() as f32 + origin.1,
                    canvas_box.width() as f32 - origin.0,
                    canvas_box.height() as f32 - origin.1,
                ),
                1.0,
            ));
            if root_style != self.root_style {
                self.root.set_attribute("style", &root_style).ok();
                self.root_style = root_style;
            }

            let present: HashSet<WidgetId> = nodes.iter().map(|(id, _, _)| *id).collect();
            // The element that has the browser's focus, if it is one of ours: when it is
            // taken out of the page below, the browser drops the focus on the document, and
            // the keys that followed (an Escape, a Tab) would reach no one. Found checking the
            // selection bar (milestone 568): activating one of its buttons replaces the bar's
            // elements.
            let active = self
                .canvas
                .owner_document()
                .and_then(|document| document.active_element());
            let is_active = |el: &HtmlElement| {
                active
                    .as_ref()
                    .is_some_and(|active| active == AsRef::<Element>::as_ref(el))
            };
            let mut lost_focus = false;
            let root = &self.root;
            self.nodes.retain(|id, live| {
                let keep = present.contains(id);
                if !keep {
                    lost_focus |= is_active(&live.el);
                    let _ = root.remove_child(&live.el);
                }
                keep
            });

            let mut reordered = self.order.len() != nodes.len();
            for (index, (id, rect, sem)) in nodes.iter().enumerate() {
                reordered |= self.order.get(index) != Some(id);
                let hidden = Some(*id) == editing;
                let at = place(*rect, k);
                let mut respawn = false;
                if let Some(live) = self.nodes.get_mut(id) {
                    if live.sem != *sem {
                        let dom = node_dom(sem);
                        if dom.tag == live.dom.tag {
                            write_attrs(&live.el, &live.dom, &dom);
                            live.dom = dom;
                            live.sem = sem.clone();
                            live.adjustable.set(adjustable(sem));
                        } else {
                            // Built for the wrong tag: a widget that became clickable needs
                            // the `<button>`.
                            respawn = true;
                        }
                    }
                }
                if respawn {
                    if let Some(old) = self.nodes.remove(id) {
                        lost_focus |= is_active(&old.el);
                        let _ = self.root.remove_child(&old.el);
                    }
                }
                if !self.nodes.contains_key(id) {
                    if let Some(live) = self.spawn(*id, sem) {
                        self.nodes.insert(*id, live);
                        reordered = true;
                    }
                }
                if let Some(live) = self.nodes.get_mut(id) {
                    if live.place != at {
                        live.el.set_attribute("style", &place_style(at)).ok();
                        live.place = at;
                    }
                    if live.hidden != hidden {
                        live.el.set_hidden(hidden);
                        live.hidden = hidden;
                    }
                }
            }

            // In the tree's order: a node added to a list goes where the list has it, not at
            // the end of everything, or a reader reading on would meet it last.
            if reordered {
                self.order = nodes.iter().map(|(id, _, _)| *id).collect();
                let mut next: Option<web_sys::Node> = None;
                for id in self.order.iter().rev() {
                    let Some(live) = self.nodes.get(id) else {
                        continue;
                    };
                    let node: &web_sys::Node = live.el.as_ref();
                    if node.next_sibling() != next || node.parent_node().is_none() {
                        let _ = self.root.insert_before(node, next.as_ref());
                    }
                    next = Some(node.clone());
                }
            }

            // A field being edited keeps the focus on the element, which the editor gave it:
            // its node here is hidden, and a hidden element takes no focus. When the editing
            // ends, the node is shown and gets the focus back.
            if focus != self.focus || editing != self.editing || lost_focus {
                self.focus = focus;
                self.editing = editing;
                if let Some(live) = focus.and_then(|id| self.nodes.get(&id)) {
                    self.programmatic_focus.set(true);
                    // Not scrolled to: the layer is placed, not scrolled, and the shell has
                    // already shown what it focused.
                    let options = web_sys::FocusOptions::new();
                    options.set_prevent_scroll(true);
                    let _ = live.el.focus_with_options(&options);
                    self.programmatic_focus.set(false);
                }
            }

            if announce != self.announce {
                self.announce = announce.to_string();
                self.live_region.set_text_content(Some(announce));
            }
        }

        /// Builds the element for a new widget and attaches its listeners.
        fn spawn(&self, id: WidgetId, sem: &SemanticsProperties) -> Option<Live> {
            let document = self.canvas.owner_document()?;
            let dom = node_dom(sem);
            let el: HtmlElement = document
                .create_element(dom.tag)
                .ok()?
                .dyn_into::<HtmlElement>()
                .ok()?;
            write_attrs(
                &el,
                &NodeDom {
                    tag: dom.tag,
                    attrs: Vec::new(),
                },
                &dom,
            );
            let button = dom.tag == "button";
            let adjust = Rc::new(Cell::new(adjustable(sem)));
            let mut listeners = Vec::new();
            let mut listen = |kind: &str, closure: Listener| {
                let _ = AsRef::<web_sys::EventTarget>::as_ref(&el)
                    .add_event_listener_with_callback(kind, closure.as_ref().unchecked_ref());
                listeners.push(closure);
            };
            if button {
                // A reader's "activate", and Enter or Space on the button.
                let actions = self.actions.clone();
                let window = self.window.clone();
                listen(
                    "click",
                    Closure::new(move |_event: web_sys::Event| {
                        actions.borrow_mut().push(A11yAction::Click(id));
                        window.request_redraw();
                    }),
                );
            }
            {
                // The focus a reader moved here is the shell's focus too — unless it moved
                // here because this module called `.focus()`, which is an echo, not a request.
                let actions = self.actions.clone();
                let programmatic = self.programmatic_focus.clone();
                let window = self.window.clone();
                listen(
                    "focus",
                    Closure::new(move |_event: web_sys::Event| {
                        if !programmatic.get() {
                            actions.borrow_mut().push(A11yAction::Focus(id));
                            window.request_redraw();
                        }
                    }),
                );
            }
            for kind in ["keydown", "keyup"] {
                let actions = self.actions.clone();
                let window = self.window.clone();
                let canvas = self.canvas.clone();
                let adjust = adjust.clone();
                listen(
                    kind,
                    Closure::new(move |event: web_sys::Event| {
                        let Some(key) = event.dyn_ref::<KeyboardEvent>() else {
                            return;
                        };
                        let cmd = key.ctrl_key() || key.meta_key();
                        let route = key_route(
                            &key.key(),
                            cmd,
                            key.alt_key(),
                            key.shift_key(),
                            button,
                            adjust.get(),
                        );
                        match route {
                            KeyRoute::Browser => {}
                            KeyRoute::Adjust(up) => {
                                key.prevent_default();
                                if kind == "keydown" {
                                    actions.borrow_mut().push(A11yAction::Adjust(id, up));
                                    window.request_redraw();
                                }
                            }
                            KeyRoute::Shell => {
                                // The shell's: the browser must not act on it as well — a Tab
                                // must not move its focus on its own.
                                key.prevent_default();
                                crate::web_editing::forward(&canvas, kind, key);
                            }
                        }
                    }),
                );
            }
            Some(Live {
                el,
                dom,
                sem: sem.clone(),
                // Placed by the caller.
                place: [i32::MIN; 4],
                hidden: false,
                adjustable: adjust,
                _listeners: listeners,
            })
        }

        /// Takes the actions the screen reader asked for since the last frame, emptying
        /// the queue.
        pub(crate) fn take_actions(&self) -> Vec<A11yAction> {
            std::mem::take(&mut self.actions.borrow_mut())
        }
    }

    /// Writes the attributes of `new` that `old` did not have, or had with another value, and
    /// removes those `new` no longer has.
    fn write_attrs(el: &HtmlElement, old: &NodeDom, new: &NodeDom) {
        let element: &Element = el.as_ref();
        for (name, _) in &old.attrs {
            if !new.attrs.iter().any(|(n, _)| n == name) {
                element.remove_attribute(name).ok();
            }
        }
        for (name, value) in &new.attrs {
            let same = old.attrs.iter().any(|(n, v)| n == name && v == value);
            if !same {
                element.set_attribute(name, value).ok();
            }
        }
        // A button's `disabled` takes it out of the focus and the clicks, as the widget is.
        if new.tag == "button" {
            let was = old.attrs.iter().any(|(n, _)| *n == "aria-disabled");
            let is = new.attrs.iter().any(|(n, _)| *n == "aria-disabled");
            if was != is || old.attrs.is_empty() {
                if is {
                    element.set_attribute("disabled", "").ok();
                } else {
                    element.remove_attribute("disabled").ok();
                }
            }
        }
    }

    /// Creates the **live region**: an invisible, polite status the assistive technology
    /// speaks when its text **changes** — `aria-live="polite"`, the same contract the
    /// desktop bridge gives AccessKit's `Live::Polite`. Every announcement goes through it:
    /// a widget's own (`announce`) and the words of the nodes marked live, which the shell
    /// gathers (milestone 810).
    ///
    /// A real, laid-out element, hidden the ordinary "visually hidden but present" way (a
    /// 1×1 box, clipped, never `display: none`): the live machinery watches what is
    /// rendered, and a region the browser does not lay out is never announced.
    fn new_live_region(canvas: &HtmlCanvasElement) -> Option<HtmlElement> {
        let document = canvas.owner_document()?;
        let body = document.body()?;
        let el: HtmlElement = document
            .create_element("div")
            .ok()?
            .dyn_into::<HtmlElement>()
            .ok()?;
        el.set_attribute("aria-live", "polite").ok();
        el.set_attribute("role", "status").ok();
        el.set_attribute(
            "style",
            "position:absolute;width:1px;height:1px;overflow:hidden;\
             clip:rect(0,0,0,0);white-space:nowrap;",
        )
        .ok();
        body.append_child(&el).ok()?;
        Some(el)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sem(role: Role) -> SemanticsProperties {
        SemanticsProperties::new(role)
    }

    #[test]
    fn roles_map_to_aria() {
        assert_eq!(aria_role(Role::Button), Some("button"));
        assert_eq!(aria_role(Role::CheckBox), Some("checkbox"));
        assert_eq!(aria_role(Role::Slider), Some("slider"));
        assert_eq!(aria_role(Role::ProgressBar), Some("progressbar"));
        assert_eq!(aria_role(Role::Tree), Some("tree"));
        assert_eq!(aria_role(Role::TreeItem), Some("treeitem"));
        assert_eq!(aria_role(Role::TextInput), Some("textbox"));
        assert_eq!(aria_role(Role::Dialog), Some("dialog"));
        assert_eq!(aria_role(Role::Splitter), Some("separator"));
    }

    #[test]
    fn text_is_content_not_a_name_on_a_made_up_role() {
        // `role="text"` is not an ARIA role: the browser read the node as one with no role
        // and a name, which a reader browsing the page does not say.
        let dom = node_dom(&sem(Role::Label).label("Notifications"));
        assert_eq!(dom.attr("role"), None);
        assert_eq!(dom.attr("aria-label"), None);
        assert_eq!(dom.attr("data-t"), Some("Notifications"));
        assert_eq!(dom.attr("tabindex"), None, "text is read, not focused");
        // A text's value is its words when it has one, as on the desktop.
        let dom = node_dom(&sem(Role::Label).label("x").value("Volume: 40%"));
        assert_eq!(dom.attr("data-t"), Some("Volume: 40%"));
    }

    #[test]
    fn a_heading_says_its_words_as_content() {
        let dom = node_dom(&sem(Role::Heading).label("Settings"));
        assert_eq!(dom.attr("role"), Some("heading"));
        assert_eq!(dom.attr("data-t"), Some("Settings"));
        assert_eq!(dom.attr("tabindex"), None);
    }

    #[test]
    fn a_tab_is_selected_not_checked() {
        let dom = node_dom(&sem(Role::Tab).label("Controls").toggled(true).clickable());
        assert_eq!(dom.tag, "button");
        assert_eq!(dom.attr("role"), Some("tab"));
        assert_eq!(dom.attr("aria-selected"), Some("true"));
        assert_eq!(dom.attr("aria-checked"), None);
        let other = node_dom(&sem(Role::Tab).label("About").toggled(false).clickable());
        assert_eq!(other.attr("aria-selected"), Some("false"));
    }

    #[test]
    fn a_checkbox_and_a_switch_are_checked() {
        let dom = node_dom(&sem(Role::CheckBox).label("Done").toggled(true).clickable());
        assert_eq!(dom.attr("aria-checked"), Some("true"));
        let dom = node_dom(&sem(Role::Switch).maybe_toggled(None).clickable());
        assert_eq!(dom.attr("aria-checked"), Some("mixed"));
        assert_eq!(dom.attr("role"), Some("switch"));
    }

    #[test]
    fn a_button_that_stays_down_is_pressed() {
        // A filter chip: `aria-checked` on a button is dropped by the browser.
        let dom = node_dom(&sem(Role::Button).label("Active").toggled(true).clickable());
        assert_eq!(dom.attr("aria-pressed"), Some("true"));
        assert_eq!(dom.attr("aria-checked"), None);
        let dom = node_dom(&sem(Role::Button).label("Done").toggled(false).clickable());
        assert_eq!(dom.attr("aria-pressed"), Some("false"));
        let plain = node_dom(&sem(Role::Button).label("Add").clickable());
        assert_eq!(plain.attr("aria-pressed"), None);
    }

    #[test]
    fn a_range_that_is_not_a_number_is_left_out() {
        let dom = node_dom(&sem(Role::ProgressBar).range(0.0, f32::NAN, 100.0));
        assert_eq!(dom.attr("aria-valuenow"), None);
        assert_eq!(dom.attr("aria-valuemin"), None);
    }

    #[test]
    fn a_tree_item_is_selected_and_expanded() {
        let dom = node_dom(&sem(Role::TreeItem).toggled(true).expanded(false));
        assert_eq!(dom.attr("aria-selected"), Some("true"));
        assert_eq!(dom.attr("aria-expanded"), Some("false"));
    }

    #[test]
    fn a_slider_carries_its_range_and_its_words() {
        let dom = node_dom(&sem(Role::Slider).range(0.0, 0.4, 1.0).value("40%"));
        assert_eq!(dom.attr("aria-valuemin"), Some("0"));
        assert_eq!(dom.attr("aria-valuenow"), Some("0.4"));
        assert_eq!(dom.attr("aria-valuemax"), Some("1"));
        assert_eq!(dom.attr("aria-valuetext"), Some("40%"));
        assert_eq!(dom.attr("tabindex"), Some("-1"));
        assert!(adjustable(&sem(Role::Slider).range(0.0, 0.4, 1.0)));
        assert!(!adjustable(
            &sem(Role::Slider).range(0.0, 0.4, 1.0).disabled(true)
        ));
        assert!(!adjustable(&sem(Role::ProgressBar).range(0.0, 0.4, 1.0)));
    }

    #[test]
    fn a_field_shows_its_contents_and_hides_a_password() {
        let dom = node_dom(&sem(Role::TextInput).label("City").value("Paris"));
        assert_eq!(dom.attr("aria-label"), Some("City"));
        assert_eq!(dom.attr("data-t"), Some("Paris"));
        let dom = node_dom(
            &sem(Role::TextInput)
                .label("Password")
                .value("hunter2")
                .obscured(true),
        );
        assert_eq!(dom.attr("data-t"), Some("\u{2022}".repeat(7).as_str()));
        assert_eq!(dom.attr("aria-roledescription"), Some("password"));
    }

    #[test]
    fn a_disabled_button_and_an_expanded_menu() {
        let dom = node_dom(&sem(Role::Button).label("Less").disabled(true).clickable());
        assert_eq!(dom.attr("aria-disabled"), Some("true"));
        let dom = node_dom(
            &sem(Role::Button)
                .label("Option A")
                .expanded(true)
                .clickable(),
        );
        assert_eq!(dom.attr("aria-expanded"), Some("true"));
    }

    #[test]
    fn an_image_with_nothing_to_say_is_hidden() {
        let dom = node_dom(&sem(Role::Image));
        assert_eq!(dom.attr("aria-hidden"), Some("true"));
        assert_eq!(dom.attr("role"), None);
        let dom = node_dom(&sem(Role::Image).label("Avatar"));
        assert_eq!(dom.attr("aria-hidden"), None);
        assert_eq!(dom.attr("role"), Some("img"));
        assert_eq!(dom.attr("aria-label"), Some("Avatar"));
    }

    #[test]
    fn a_dialog_is_modal_and_a_badge_is_a_description() {
        assert_eq!(
            node_dom(&sem(Role::Dialog).label("Palette")).attr("aria-modal"),
            Some("true")
        );
        let dom = node_dom(&sem(Role::Button).label("Inbox").value("3 new").clickable());
        assert_eq!(dom.attr("aria-description"), Some("3 new"));
    }

    #[test]
    fn a_node_is_placed_at_its_rectangle_scaled() {
        let rect = Rect::new(10.0, 20.5, 100.0, 40.0);
        assert_eq!(place(rect, 1.0), [1000, 2050, 10000, 4000]);
        assert_eq!(place(rect, 2.0), [2000, 4100, 20000, 8000]);
        assert_eq!(
            place_style(place(rect, 1.0)),
            "left:10px;top:20.5px;width:100px;height:40px"
        );
    }

    #[test]
    fn keys_go_to_the_browser_the_slider_or_the_shell() {
        // Enter and Space on a button are the browser's `click`.
        assert_eq!(
            key_route("Enter", false, false, false, true, false),
            KeyRoute::Browser
        );
        assert_eq!(
            key_route(" ", false, false, false, true, false),
            KeyRoute::Browser
        );
        // On anything else, the shell's.
        assert_eq!(
            key_route("Enter", false, false, false, false, false),
            KeyRoute::Shell
        );
        assert_eq!(
            key_route("Tab", false, false, false, true, false),
            KeyRoute::Shell
        );
        assert_eq!(
            key_route("Tab", false, false, true, true, false),
            KeyRoute::Shell
        );
        assert_eq!(
            key_route("Escape", false, false, false, true, false),
            KeyRoute::Shell
        );
        // A slider's arrows move it.
        assert_eq!(
            key_route("ArrowRight", false, false, false, false, true),
            KeyRoute::Adjust(true)
        );
        assert_eq!(
            key_route("ArrowUp", false, false, false, false, true),
            KeyRoute::Adjust(true)
        );
        assert_eq!(
            key_route("ArrowLeft", false, false, false, false, true),
            KeyRoute::Adjust(false)
        );
        assert_eq!(
            key_route("ArrowDown", false, false, false, false, true),
            KeyRoute::Adjust(false)
        );
        assert_eq!(
            key_route("ArrowDown", false, false, false, false, false),
            KeyRoute::Shell
        );
        // The browser keeps its own shortcuts.
        assert_eq!(
            key_route("F5", false, false, false, false, false),
            KeyRoute::Browser
        );
        assert_eq!(
            key_route("r", true, false, false, true, false),
            KeyRoute::Browser
        );
    }
}
