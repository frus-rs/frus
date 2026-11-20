// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! **The page's own text**, on the web (milestone 830).
//!
//! A frus page paints its words into a canvas, and as far as the browser is concerned the page
//! has no text: find-in-page finds nothing, a printed page is a picture cut at the window, and a
//! search engine indexes an empty page. The accessibility bridge (`a11y_web`) does put the words
//! in the document, but as canvas fallback content, which is never laid out: the browser knows
//! the words exist and not where they are, and a find never reaches them.
//!
//! This module gives the page its text three ways, from what the frame records
//! ([`frus_widgets::Ui::page_text`]):
//!
//! - **A text layer over the canvas.** One transparent element per text the frame laid out, at
//!   its place and size, in a layer that takes no pointer and is hidden from assistive
//!   technology (the bridge already reads the same words to it). The browser's find searches it
//!   and draws its highlight over the canvas. Each scroll region is a box of its own in the
//!   layer, as large as the region's view and scrolled as far as the region is, holding the
//!   region's content **whole** — the texts scrolled out of view included. A find that lands on
//!   one of those scrolls the box to show it, as a browser does for any match, and the box's
//!   `scroll` event is the request this module hands the shell: the region is moved where the
//!   browser put the box ([`TextLayer::take_scrolls`]). While the application scrolls, only the
//!   box's position changes; the texts inside it keep theirs.
//! - **The rows a list did not build.** A list builds only the rows in view. One that says what
//!   its rows say ([`frus_widgets::ListView::item_text`]) is asked for all of them when the
//!   reader opens find-in-page (Ctrl+F, F3: see [`find_opened`]), and they are laid in its box
//!   at the places the list itself would scroll to for them. Not before: a list of ten thousand
//!   rows costs nothing until a find asks for it.
//! - **A plain document**, made from the same texts in reading order — headings as headings,
//!   a list's rows as a list — for printing (`beforeprint` puts it in the page and a print
//!   style shows it alone) and for search engines (`window.__frusPageText()`, which
//!   `frus build web` writes into the page it builds).
//!
//! Best-effort like the other bridges: with no document, the page keeps having no text, and
//! nothing worse. Only the web has any of this; elsewhere the frame records nothing.

use std::collections::{HashMap, HashSet};

use frus_widgets::{PageList, Rect, Scrollable, WidgetId};

/// A scroll region as the text layer sees it: where its view is, how far its content goes, and
/// how far it is scrolled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Region {
    pub id: WidgetId,
    /// The region it sits in, if any.
    pub parent: Option<WidgetId>,
    /// Its view, in the window's logical pixels.
    pub viewport: Rect,
    /// The largest offset on each axis.
    pub max: (f32, f32),
    /// Whether each axis runs from the far end.
    pub reverse: (bool, bool),
    /// The application's offset on each axis.
    pub offset: (f32, f32),
}

impl Region {
    /// `area`, scrolled to `offset`.
    #[cfg_attr(not(web), allow(dead_code))]
    pub(crate) fn of(area: &Scrollable, offset: (f32, f32)) -> Self {
        Self {
            id: area.id,
            parent: area.host,
            viewport: area.viewport,
            max: (area.max_x, area.max_y),
            reverse: (area.reverse_x, area.reverse_y),
            offset,
        }
    }

    /// How far the content's top-left corner is past the view's — what the page calls the
    /// box's `scrollLeft` and `scrollTop`. An axis that runs from the far end counts from it.
    pub(crate) fn scrolled(&self) -> (f32, f32) {
        let axis = |reverse: bool, offset: f32, max: f32| {
            let max = max.max(0.0);
            if reverse {
                (max - offset).clamp(0.0, max)
            } else {
                offset.clamp(0.0, max)
            }
        };
        (
            axis(self.reverse.0, self.offset.0, self.max.0),
            axis(self.reverse.1, self.offset.1, self.max.1),
        )
    }

    /// The application's offset that scrolls the content `scrolled` past the view: the
    /// inverse of [`scrolled`](Self::scrolled).
    pub(crate) fn offset_for(&self, scrolled: (f32, f32)) -> (f32, f32) {
        Region {
            offset: scrolled,
            ..*self
        }
        .scrolled()
    }

    /// The content's top-left corner, in the window.
    pub(crate) fn content_origin(&self) -> (f32, f32) {
        let (x, y) = self.scrolled();
        (self.viewport.x - x, self.viewport.y - y)
    }

    /// The content's size: the view and as far as it scrolls.
    pub(crate) fn content_size(&self) -> (f32, f32) {
        (
            self.viewport.width + self.max.0.max(0.0),
            self.viewport.height + self.max.1.max(0.0),
        )
    }
}

/// Where box `rect` of the window goes in the layer: in the content of `region`, or in the
/// layer itself — which lies over the canvas — when it is in none.
pub(crate) fn place(rect: Rect, region: Option<&Region>) -> Rect {
    match region {
        Some(region) => {
            let (x, y) = region.content_origin();
            Rect::new(rect.x - x, rect.y - y, rect.width, rect.height)
        }
        None => rect,
    }
}

/// Where each row of a list lies in its region's content, from `spans` — each row's start and
/// extent along the list — for a list running `across` or down, from its far end when
/// `reverse`: `(x, y, width, height)` per row.
pub(crate) fn row_boxes(
    region: &Region,
    across: bool,
    spans: &[(f32, f32)],
) -> Vec<(f32, f32, f32, f32)> {
    let (width, height) = region.content_size();
    let (length, reverse) = if across {
        (width, region.reverse.0)
    } else {
        (height, region.reverse.1)
    };
    spans
        .iter()
        .map(|&(start, extent)| {
            let at = if reverse {
                length - start - extent
            } else {
                start
            };
            if across {
                (at, 0.0, extent, region.viewport.height)
            } else {
                (0.0, at, region.viewport.width, extent)
            }
        })
        .collect()
}

/// `text` made safe to put in HTML, its line breaks kept.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("<br>"),
            c => out.push(c),
        }
    }
    out
}

/// Whether a text in scroll region `scroll` is inside one of `lists`, going up through
/// `parents`.
fn inside(
    mut scroll: Option<WidgetId>,
    lists: &HashSet<WidgetId>,
    parents: &HashMap<WidgetId, Option<WidgetId>>,
) -> bool {
    // A region is never its own ancestor; the bound only guards against a map that says so.
    for _ in 0..=parents.len() {
        let Some(id) = scroll else {
            return false;
        };
        if lists.contains(&id) {
            return true;
        }
        scroll = parents.get(&id).copied().flatten();
    }
    false
}

/// One text, for the plain document: its region, its words, and whether it is a heading.
pub(crate) type DocumentText<'a> = (Option<WidgetId>, &'a str, bool);

/// The page's text as a **plain document**, for printing and for search engines: the texts in
/// reading order, a heading as `<h2>` and the rest as paragraphs, and each list that says what
/// its rows say as a `<ul>` of **all** its rows, where the list is — its rows built this frame
/// left out, being among them. `parents` says which region each region is in.
pub(crate) fn document_html<'a>(
    texts: impl IntoIterator<Item = Option<DocumentText<'a>>>,
    lists: &[PageList],
    parents: &HashMap<WidgetId, Option<WidgetId>>,
) -> String {
    let listed: HashSet<WidgetId> = lists.iter().map(|list| list.id).collect();
    let mut out = String::from("<main>");
    let mut pending = lists.iter().peekable();
    let write_list = |out: &mut String, list: &PageList| {
        out.push_str("<ul>");
        for index in 0..list.count {
            out.push_str("<li>");
            out.push_str(&escape((list.item_text)(index).trim()));
            out.push_str("</li>");
        }
        out.push_str("</ul>");
    };
    for (index, text) in texts.into_iter().enumerate() {
        while let Some(list) = pending.next_if(|list| list.at <= index) {
            write_list(&mut out, list);
        }
        let Some((scroll, words, heading)) = text else {
            continue;
        };
        if words.trim().is_empty() || inside(scroll, &listed, parents) {
            continue;
        }
        let tag = if heading { "h2" } else { "p" };
        out.push_str(&format!("<{tag}>{}</{tag}>", escape(words.trim())));
    }
    for list in pending {
        write_list(&mut out, list);
    }
    out.push_str("</main>");
    out
}

/// A CSS length, in pixels, short.
fn px(value: f32) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    format!("{rounded}px")
}

/// The inline style of a box at `(x, y)`, `width` by `height`, scaled by `k` — the CSS pixels
/// of one logical pixel.
pub(crate) fn box_style(x: f32, y: f32, width: f32, height: f32, k: f32) -> String {
    format!(
        "left:{};top:{};width:{};height:{}",
        px(x * k),
        px(y * k),
        px(width * k),
        px(height * k)
    )
}

#[cfg(web)]
pub(crate) use live::{find_opened, TextLayer};

#[cfg(web)]
mod live {
    use std::cell::{Cell, RefCell};
    use std::collections::{HashMap, HashSet};
    use std::rc::Rc;
    use std::sync::Arc;

    use wasm_bindgen::prelude::*;
    use wasm_bindgen::JsCast;
    use web_sys::{Document, Element, HtmlCanvasElement, HtmlElement};
    use winit::platform::web::WindowExtWebSys;
    use winit::window::Window;

    use frus_widgets::{PageList, PageText, WidgetId};

    use super::{box_style, document_html, place, px, row_boxes, Region};

    /// The id of the layer, of its style sheet, of the printed document and of the page's
    /// text written in at build time.
    const LAYER: &str = "frus-text";
    const STYLE: &str = "frus-text-style";
    const PRINT: &str = "frus-print";
    const STATIC: &str = "frus-page-text";

    /// The layer's style sheet, and the print style that shows the plain document alone.
    ///
    /// The words are transparent: the canvas draws them. A find's highlight is the browser's,
    /// made translucent and drawn without its own copy of the words, so the canvas's show
    /// through it (`::search-text`, which a browser without it ignores, drawing its own).
    ///
    /// A region's box scrolls (`overflow: auto`, its bar hidden), though nothing but a find
    /// scrolls it: Chrome on Android leaves an `overflow: hidden` box where it is when its find
    /// bar lands on a match out of view (seen on a phone, milestone 830). The pointer goes
    /// through the layer, so a wheel or a finger still scrolls the application, not the box.
    const CSS: &str = "\
#frus-text{position:fixed;left:0;top:0;width:0;height:0;overflow:hidden;pointer-events:none;\
color:transparent;font-family:system-ui,-apple-system,'Segoe UI',Roboto,sans-serif;\
margin:0;padding:0;z-index:0}\
#frus-text div{position:absolute;margin:0;padding:0;box-sizing:border-box}\
#frus-text .s{overflow:auto;scrollbar-width:none}\
#frus-text .s::-webkit-scrollbar{display:none}\
#frus-text .s.n{scrollbar-width:auto}\
#frus-text .z{position:relative}\
#frus-text .t{white-space:pre-wrap;overflow-wrap:anywhere}\
#frus-text ::search-text{color:transparent;background:rgb(255 214 0/.45)}\
#frus-text ::search-text:current{background:rgb(255 140 0/.6)}\
#frus-print{display:none}\
@media print{\
body:has(>#frus-print)>:not(#frus-print){display:none!important}\
html:has(#frus-print),body:has(>#frus-print){height:auto!important;overflow:visible!important;\
background:#fff!important;display:block!important}\
#frus-print{display:block!important;color:#000;font:12pt/1.45 system-ui,sans-serif;margin:0}\
#frus-print h2{font-size:16pt;margin:1em 0 .4em}\
#frus-print p{margin:0 0 .5em}\
#frus-print ul{margin:0 0 .5em;padding-left:1.4em}\
}";

    /// Whether the build measures what the layer costs: `FRUS_MEASURE_PAGE_TEXT` set when
    /// `frus-shell` is compiled. The page then has `window.__frusPageTextCost()`: the frames
    /// published, the milliseconds they took in all, the longest, and the last laying-in of a
    /// list's rows for a find. Off, none of it is compiled in.
    const MEASURE: bool = option_env!("FRUS_MEASURE_PAGE_TEXT").is_some();

    fn now() -> f64 {
        web_sys::window()
            .and_then(|page| page.performance())
            .map_or(0.0, |clock| clock.now())
    }

    thread_local! {
        /// What the layer cost, when [`MEASURE`]: frames, total and longest milliseconds, and
        /// the last find's rows.
        static COST: Cell<(f64, f64, f64, f64)> = const { Cell::new((0.0, 0.0, 0.0, 0.0)) };
        /// Set when the reader opens find-in-page, until the next frame lays out the rows
        /// lists did not build.
        static FIND: Cell<bool> = const { Cell::new(false) };
        /// The window to wake when it is set.
        static WAKE: RefCell<Option<Arc<Window>>> = const { RefCell::new(None) };
    }

    /// The reader opened find-in-page (Ctrl+F, F3): the next frame lays every row of the lists
    /// that can say what their rows say into the layer, for the find to reach. Called from the
    /// key listener, before the browser opens its find bar.
    #[inline(never)]
    pub(crate) fn find_opened() {
        FIND.with(|find| find.set(true));
        WAKE.with(|wake| {
            if let Some(window) = wake.borrow().as_ref() {
                window.request_redraw();
            }
        });
    }

    /// `el`'s numeric property `name` (`scrollTop`…).
    fn number(el: &Element, name: &str) -> f64 {
        js_sys::Reflect::get(el, &name.into())
            .ok()
            .and_then(|value| value.as_f64())
            .unwrap_or(0.0)
    }

    fn set_number(el: &Element, name: &str, value: f64) {
        let _ = js_sys::Reflect::set(el, &name.into(), &value.into());
    }

    fn div(document: &Document, class: &str) -> Option<HtmlElement> {
        let el = document
            .create_element("div")
            .ok()?
            .dyn_into::<HtmlElement>()
            .ok()?;
        el.set_class_name(class);
        Some(el)
    }

    /// The regions the browser scrolled, and to where, in CSS pixels.
    type Scrolls = Rc<RefCell<Vec<(WidgetId, (f64, f64))>>>;

    /// Where a list puts its rows: whether it is reversed, and each row's start and extent.
    type ListSpans<'a> = dyn Fn(WidgetId) -> Option<(bool, Vec<(f32, f32)>)> + 'a;

    /// A scroll region's box in the layer.
    struct LiveRegion {
        /// The view: clips, and is scrolled.
        outer: HtmlElement,
        /// The content, as large as it is: what the view scrolls over.
        sizer: HtmlElement,
        parent: Option<WidgetId>,
        style: String,
        sizer_style: String,
        /// Where this module last scrolled the box, in CSS pixels: a `scroll` event that finds
        /// it there is the echo of that, not the browser's request.
        expected: Rc<Cell<(f64, f64)>>,
        _scroll: Closure<dyn FnMut(web_sys::Event)>,
        /// The rows of a list laid in for a find, and how many items the list had then.
        rows: Option<(usize, Vec<HtmlElement>)>,
    }

    /// A text's element in the layer.
    struct LiveText {
        el: HtmlElement,
        parent: Option<WidgetId>,
        words: String,
        heading: bool,
        style: String,
    }

    struct State {
        document: Document,
        canvas: HtmlCanvasElement,
        root: HtmlElement,
        root_style: String,
        regions: HashMap<WidgetId, LiveRegion>,
        texts: HashMap<WidgetId, LiveText>,
        /// The frame's texts in reading order, each with its region: what the plain document
        /// is made from, with the words kept in `texts`.
        order: Vec<(WidgetId, Option<WidgetId>)>,
        lists: Vec<PageList>,
        parents: HashMap<WidgetId, Option<WidgetId>>,
        /// Whether the page's text written in at build time has been taken out.
        static_gone: bool,
        print: Option<HtmlElement>,
    }

    impl State {
        fn document(&self) -> String {
            document_html(
                self.order.iter().map(|(id, scroll)| {
                    self.texts
                        .get(id)
                        .map(|text| (*scroll, text.words.as_str(), text.heading))
                }),
                &self.lists,
                &self.parents,
            )
        }

        /// The element a box in `region` goes in: the region's content, or the layer.
        fn container(&self, region: Option<WidgetId>) -> &HtmlElement {
            region
                .and_then(|id| self.regions.get(&id))
                .map_or(&self.root, |live| &live.sizer)
        }
    }

    /// The window's text layer.
    pub(crate) struct TextLayer {
        state: Rc<RefCell<State>>,
        /// The regions the browser scrolled, and to where, in CSS pixels.
        scrolls: Scrolls,
        window: Arc<Window>,
        _print: [Closure<dyn FnMut(web_sys::Event)>; 2],
        _text: Closure<dyn FnMut() -> JsValue>,
    }

    impl TextLayer {
        /// Lays the layer over `window`'s canvas. `None` without a canvas or a document.
        #[inline(never)]
        pub(crate) fn new(window: &Arc<Window>) -> Option<Self> {
            let canvas = window.canvas()?;
            let document = canvas.owner_document()?;
            let page = web_sys::window()?;

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
            let root = div(&document, "")?;
            root.set_id(LAYER);
            root.set_attribute("aria-hidden", "true").ok();
            // Right after the canvas: over it, and under anything the page puts over both.
            let parent = canvas.parent_node()?;
            parent
                .insert_before(&root, canvas.next_sibling().as_ref())
                .ok()?;

            let state = Rc::new(RefCell::new(State {
                document,
                canvas,
                root,
                root_style: String::new(),
                regions: HashMap::new(),
                texts: HashMap::new(),
                order: Vec::new(),
                lists: Vec::new(),
                parents: HashMap::new(),
                static_gone: false,
                print: None,
            }));

            // Printing: the plain document goes in for the print, and comes out after.
            let before = {
                let state = state.clone();
                Closure::wrap(Box::new(move |_event: web_sys::Event| {
                    let Ok(mut state) = state.try_borrow_mut() else {
                        return;
                    };
                    let html = state.document();
                    if let Some(old) = state.print.take() {
                        old.remove();
                    }
                    let Some(el) = div(&state.document, "") else {
                        return;
                    };
                    el.set_id(PRINT);
                    el.set_inner_html(&html);
                    if let Some(body) = state.document.body() {
                        if body.append_child(&el).is_ok() {
                            state.print = Some(el);
                        }
                    }
                }) as Box<dyn FnMut(web_sys::Event)>)
            };
            let after = {
                let state = state.clone();
                Closure::wrap(Box::new(move |_event: web_sys::Event| {
                    if let Ok(mut state) = state.try_borrow_mut() {
                        if let Some(el) = state.print.take() {
                            el.remove();
                        }
                    }
                }) as Box<dyn FnMut(web_sys::Event)>)
            };
            let target: &web_sys::EventTarget = page.as_ref();
            let _ = target
                .add_event_listener_with_callback("beforeprint", before.as_ref().unchecked_ref());
            let _ = target
                .add_event_listener_with_callback("afterprint", after.as_ref().unchecked_ref());

            // The plain document, for `frus build web` to write into the page it builds.
            let text = {
                let state = state.clone();
                Closure::wrap(Box::new(move || {
                    state
                        .try_borrow()
                        .map(|state| JsValue::from_str(&state.document()))
                        .unwrap_or(JsValue::NULL)
                }) as Box<dyn FnMut() -> JsValue>)
            };
            let _ = js_sys::Reflect::set(&page, &"__frusPageText".into(), text.as_ref());

            if MEASURE {
                let cost = Closure::wrap(Box::new(|| {
                    let (frames, total, longest, rows) = COST.with(Cell::get);
                    JsValue::from(js_sys::Array::of4(
                        &frames.into(),
                        &total.into(),
                        &longest.into(),
                        &rows.into(),
                    ))
                }) as Box<dyn FnMut() -> JsValue>);
                let _ = js_sys::Reflect::set(&page, &"__frusPageTextCost".into(), cost.as_ref());
                cost.forget();
            }
            WAKE.with(|wake| *wake.borrow_mut() = Some(window.clone()));
            Some(Self {
                state,
                scrolls: Rc::new(RefCell::new(Vec::new())),
                window: window.clone(),
                _print: [before, after],
                _text: text,
            })
        }

        /// The regions the browser scrolled since the last frame — a find bringing a match into
        /// view — each with how far its content is now scrolled past its view, in logical
        /// pixels at `k` CSS pixels each. The last request for a region wins.
        pub(crate) fn take_scrolls(&self, k: f32) -> Vec<(WidgetId, (f32, f32))> {
            let mut taken: Vec<(WidgetId, (f32, f32))> = Vec::new();
            let k = f64::from(k.max(0.01));
            for (id, (x, y)) in std::mem::take(&mut *self.scrolls.borrow_mut()) {
                let at = ((x / k) as f32, (y / k) as f32);
                match taken.iter_mut().find(|(seen, _)| *seen == id) {
                    Some(entry) => entry.1 = at,
                    None => taken.push((id, at)),
                }
            }
            taken
        }

        /// Publishes the frame's text: `texts` and `lists` as the frame recorded them, in
        /// `regions`; `spans` says where a list's items lie (whether it runs across, and each
        /// item's start and extent). `k` is the CSS pixels of a logical pixel, and `origin`
        /// where the application's screen starts on the canvas, in CSS pixels.
        #[inline(never)]
        pub(crate) fn update(
            &self,
            texts: &[PageText],
            lists: &[PageList],
            regions: &[Region],
            spans: &ListSpans<'_>,
            k: f32,
            origin: (f32, f32),
        ) {
            let started = if MEASURE { now() } else { 0.0 };
            self.publish(texts, lists, regions, spans, k, origin);
            if MEASURE {
                let took = now() - started;
                COST.with(|cost| {
                    let (frames, total, longest, rows) = cost.get();
                    cost.set((frames + 1.0, total + took, longest.max(took), rows));
                });
            }
        }

        #[inline(never)]
        fn publish(
            &self,
            texts: &[PageText],
            lists: &[PageList],
            regions: &[Region],
            spans: &ListSpans<'_>,
            k: f32,
            origin: (f32, f32),
        ) {
            let Ok(mut guard) = self.state.try_borrow_mut() else {
                return;
            };
            let state = &mut *guard;

            // The layer lies over the canvas, wherever the page put it.
            let canvas_box = state.canvas.get_bounding_client_rect();
            let root_style = format!(
                "left:{};top:{};width:{};height:{}",
                px(canvas_box.left() as f32 + origin.0),
                px(canvas_box.top() as f32 + origin.1),
                px(canvas_box.width() as f32 - origin.0),
                px(canvas_box.height() as f32 - origin.1)
            );
            if root_style != state.root_style {
                state.root.set_attribute("style", &root_style).ok();
                state.root_style = root_style;
            }

            let find = FIND.with(|find| find.replace(false));
            let by_id: HashMap<WidgetId, &Region> = regions.iter().map(|r| (r.id, r)).collect();
            state.parents = regions.iter().map(|r| (r.id, r.parent)).collect();

            // The regions gone take their boxes, and what is still in them, out of the page;
            // what is still in the frame is put back below.
            state.regions.retain(|id, live| {
                let keep = by_id.contains_key(id);
                if !keep {
                    live.outer.remove();
                }
                keep
            });
            // Parents come before their children in the frame's order.
            for region in regions {
                let parent = region.parent.filter(|p| by_id.contains_key(p));
                let at = place(region.viewport, parent.and_then(|p| by_id.get(&p).copied()));
                let style = box_style(at.x, at.y, at.width, at.height, k);
                let (width, height) = region.content_size();
                let sizer_style = format!("width:{};height:{}", px(width * k), px(height * k));
                if !state.regions.contains_key(&region.id) {
                    let Some(live) = self.new_region(&state.document, region.id) else {
                        continue;
                    };
                    state.regions.insert(region.id, live);
                }
                let container = state.container(parent).clone();
                let Some(live) = state.regions.get_mut(&region.id) else {
                    continue;
                };
                if live.parent != parent || live.outer.parent_node().is_none() {
                    container.append_child(&live.outer).ok();
                    live.parent = parent;
                }
                if live.style != style {
                    live.outer.set_attribute("style", &style).ok();
                    live.style = style;
                }
                if live.sizer_style != sizer_style {
                    live.sizer.set_attribute("style", &sizer_style).ok();
                    live.sizer_style = sizer_style;
                }
                // Scrolled as far as the application's region is, within what the box can
                // scroll: a region pulled past its end (an overscroll, a bounce) puts the box at
                // that end. Asked for more, the browser would stop the box there itself, and
                // the scroll event of that stop would read as a find's request and end the
                // bounce.
                let (x, y) = region.scrolled();
                let el: &Element = live.outer.as_ref();
                let room = (
                    (number(el, "scrollWidth") - number(el, "clientWidth")).max(0.0),
                    (number(el, "scrollHeight") - number(el, "clientHeight")).max(0.0),
                );
                let want = (
                    f64::from(x * k).clamp(0.0, room.0),
                    f64::from(y * k).clamp(0.0, room.1),
                );
                let now = (number(el, "scrollLeft"), number(el, "scrollTop"));
                if (now.0 - want.0).abs() > 0.5 || (now.1 - want.1).abs() > 0.5 {
                    live.expected.set(want);
                    set_number(el, "scrollLeft", want.0);
                    set_number(el, "scrollTop", want.1);
                }
            }

            // The rows of the lists that say what their rows say, laid in for a find. Kept
            // until the list goes or changes its length; laid again at the next find.
            let listed: HashSet<WidgetId> = lists.iter().map(|list| list.id).collect();
            for (id, live) in state.regions.iter_mut() {
                let stale = match (&live.rows, lists.iter().find(|list| list.id == *id)) {
                    (Some(_), None) => true,
                    (Some((count, _)), Some(list)) => *count != list.count,
                    _ => false,
                };
                if stale || (find && live.rows.is_some()) {
                    for row in live.rows.take().map(|(_, rows)| rows).unwrap_or_default() {
                        row.remove();
                    }
                }
            }
            if find {
                let started = if MEASURE { now() } else { 0.0 };
                for list in lists {
                    let (Some(region), Some((across, spans))) =
                        (by_id.get(&list.id), spans(list.id))
                    else {
                        continue;
                    };
                    // The size of the list's own texts, or the default.
                    let (size, line) = texts
                        .iter()
                        .find(|text| text.scroll == Some(list.id))
                        .map_or((16.0, 24.0), |text| {
                            (text.words.size, text.words.line_height)
                        });
                    let Some(live) = state.regions.get_mut(&list.id) else {
                        continue;
                    };
                    let mut rows = Vec::with_capacity(list.count);
                    for (index, (x, y, width, height)) in
                        row_boxes(region, across, &spans).into_iter().enumerate()
                    {
                        let Some(el) = div(&state.document, "t") else {
                            break;
                        };
                        el.set_attribute(
                            "style",
                            &format!(
                                "{};font-size:{};line-height:{}",
                                box_style(x, y, width, height, k),
                                px(size * k),
                                px(line * k)
                            ),
                        )
                        .ok();
                        el.set_text_content(Some((list.item_text)(index).trim()));
                        live.sizer.append_child(&el).ok();
                        rows.push(el);
                    }
                    live.rows = Some((list.count, rows));
                }
                if MEASURE {
                    let took = now() - started;
                    COST.with(|cost| {
                        let (frames, total, longest, _) = cost.get();
                        cost.set((frames, total, longest, took));
                    });
                }
            }
            let laid: HashSet<WidgetId> = state
                .regions
                .iter()
                .filter(|(id, live)| live.rows.is_some() && listed.contains(id))
                .map(|(id, _)| *id)
                .collect();

            // The texts.
            let mut present = HashSet::with_capacity(texts.len());
            state.order.clear();
            for text in texts {
                state.order.push((text.id, text.scroll));
                // The rows of a list laid in whole stand for the ones it built.
                if super::inside(text.scroll, &laid, &state.parents) {
                    continue;
                }
                present.insert(text.id);
                let parent = text.scroll.filter(|id| by_id.contains_key(id));
                let at = place(text.rect, parent.and_then(|p| by_id.get(&p).copied()));
                let style = format!(
                    "{};font-size:{};line-height:{}",
                    box_style(at.x, at.y, at.width, at.height, k),
                    px(text.words.size * k),
                    px(text.words.line_height * k)
                );
                let container = state.container(parent).clone();
                match state.texts.get_mut(&text.id) {
                    Some(live) => {
                        if live.parent != parent || live.el.parent_node().is_none() {
                            container.append_child(&live.el).ok();
                            live.parent = parent;
                        }
                        if live.style != style {
                            live.el.set_attribute("style", &style).ok();
                            live.style = style;
                        }
                        if live.words != text.words.words {
                            live.el.set_text_content(Some(&text.words.words));
                            live.words.clone_from(&text.words.words);
                        }
                        live.heading = text.words.heading;
                    }
                    None => {
                        let Some(el) = div(&state.document, "t") else {
                            continue;
                        };
                        el.set_attribute("style", &style).ok();
                        el.set_text_content(Some(&text.words.words));
                        container.append_child(&el).ok();
                        state.texts.insert(
                            text.id,
                            LiveText {
                                el,
                                parent,
                                words: text.words.words.clone(),
                                heading: text.words.heading,
                                style,
                            },
                        );
                    }
                }
            }
            state.texts.retain(|id, live| {
                let keep = present.contains(id);
                if !keep {
                    live.el.remove();
                }
                keep
            });
            // The rows laid in are not in `texts`, and the document still wants the words of the
            // texts they stand for: those are taken from the list.
            state.lists = lists.to_vec();

            // The page's text written in at build time is for whoever reads the page without
            // running it; once the application shows, the layer says the same, where it shows.
            if !state.static_gone && !texts.is_empty() {
                if let Some(el) = state.document.get_element_by_id(STATIC) {
                    el.remove();
                }
                state.static_gone = true;
            }
        }

        /// A region's box, and the listener that hears the browser scroll it.
        fn new_region(&self, document: &Document, id: WidgetId) -> Option<LiveRegion> {
            let outer = div(document, "s")?;
            let sizer = div(document, "z")?;
            outer.append_child(&sizer).ok()?;
            let expected = Rc::new(Cell::new((0.0, 0.0)));
            let listener = {
                let expected = expected.clone();
                let scrolls = self.scrolls.clone();
                let window = self.window.clone();
                let el: Element = outer.clone().into();
                Closure::wrap(Box::new(move |_event: web_sys::Event| {
                    let at = (number(&el, "scrollLeft"), number(&el, "scrollTop"));
                    let (x, y) = expected.get();
                    // The echo of a scroll this module made is not a request; a browser rounds
                    // what it is given to its own pixels.
                    if (at.0 - x).abs() <= 1.0 && (at.1 - y).abs() <= 1.0 {
                        return;
                    }
                    expected.set(at);
                    scrolls.borrow_mut().push((id, at));
                    window.request_redraw();
                    repaint_later(&el);
                }) as Box<dyn FnMut(web_sys::Event)>)
            };
            let target: &web_sys::EventTarget = outer.as_ref();
            target
                .add_event_listener_with_callback("scroll", listener.as_ref().unchecked_ref())
                .ok()?;
            Some(LiveRegion {
                outer,
                sizer,
                parent: None,
                style: String::new(),
                sizer_style: String::new(),
                expected,
                _scroll: listener,
                rows: None,
            })
        }
    }

    /// Has the browser paint `el`'s box again a moment after a find scrolled it. Chrome on
    /// Android scrolls the box to the match but does not draw the match's highlight in it
    /// until the box is painted again (seen on a phone, milestone 830): its scroll bar's
    /// style, changed and changed back, is a change nobody sees that has it painted.
    fn repaint_later(el: &Element) {
        let Some(page) = web_sys::window() else {
            return;
        };
        let at = |delay: i32, class: &'static str| {
            let el = el.clone();
            let callback = Closure::once_into_js(move || {
                let _ = el.set_attribute("class", class);
            });
            let _ = page.set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.unchecked_ref(),
                delay,
            );
        };
        at(150, "s n");
        at(200, "s");
    }

    impl Drop for TextLayer {
        fn drop(&mut self) {
            if let Ok(state) = self.state.try_borrow() {
                state.root.remove();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frus_widgets::PageWords;
    use std::rc::Rc;

    fn id(raw: u64) -> WidgetId {
        WidgetId::from_u64(raw)
    }

    fn region(offset: (f32, f32), reverse: (bool, bool)) -> Region {
        Region {
            id: id(1),
            parent: None,
            viewport: Rect::new(10.0, 50.0, 300.0, 400.0),
            max: (0.0, 1600.0),
            reverse,
            offset,
        }
    }

    /// **A region's box scrolls as far as the region**, and a scroll of the box comes back as
    /// the same offset — from the top, and for a list that runs from its far end.
    #[test]
    fn a_box_scrolls_as_far_as_its_region_and_back() {
        let down = region((0.0, 300.0), (false, false));
        assert_eq!(down.scrolled(), (0.0, 300.0));
        assert_eq!(down.offset_for((0.0, 900.0)), (0.0, 900.0));
        assert_eq!(down.content_origin(), (10.0, -250.0));

        // A list from its far end, at rest at its start, shows the end of its content.
        let up = region((0.0, 0.0), (false, true));
        assert_eq!(up.scrolled(), (0.0, 1600.0));
        assert_eq!(up.offset_for((0.0, 1600.0)), (0.0, 0.0));
        assert_eq!(up.offset_for((0.0, 100.0)), (0.0, 1500.0));
        // Beyond the content is clamped to it.
        assert_eq!(down.offset_for((0.0, 5000.0)), (0.0, 1600.0));
    }

    /// **A text scrolled out of view keeps its place in the content**: wherever the region is
    /// scrolled, the text sits at the same place in its box.
    #[test]
    fn a_text_keeps_its_place_in_the_content_while_it_scrolls() {
        // At content y 1200: drawn at 50 + 1200 - offset.
        for offset in [0.0, 300.0, 1100.0] {
            let region = region((0.0, offset), (false, false));
            let drawn = Rect::new(20.0, 50.0 + 1200.0 - offset, 100.0, 20.0);
            assert_eq!(
                place(drawn, Some(&region)),
                Rect::new(10.0, 1200.0, 100.0, 20.0)
            );
        }
        // Outside any region it is where it is drawn.
        let drawn = Rect::new(20.0, 30.0, 100.0, 20.0);
        assert_eq!(place(drawn, None), drawn);
        assert_eq!(
            region((0.0, 0.0), (false, false)).content_size(),
            (300.0, 2000.0)
        );
    }

    /// **Rows not built are laid where the list puts them**, down a list or from its far end.
    #[test]
    fn rows_not_built_are_laid_where_the_list_puts_them() {
        let spans = [(0.0, 50.0), (50.0, 80.0)];
        let down = region((0.0, 0.0), (false, false));
        assert_eq!(
            row_boxes(&down, false, &spans),
            vec![(0.0, 0.0, 300.0, 50.0), (0.0, 50.0, 300.0, 80.0)]
        );
        // From the far end: the first row at the bottom of the 2000 px of content.
        let up = region((0.0, 0.0), (false, true));
        assert_eq!(
            row_boxes(&up, false, &spans),
            vec![(0.0, 1950.0, 300.0, 50.0), (0.0, 1870.0, 300.0, 80.0)]
        );
    }

    fn text(
        raw: u64,
        words: &str,
        heading: bool,
        scroll: Option<WidgetId>,
    ) -> frus_widgets::PageText {
        frus_widgets::PageText {
            id: id(raw),
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            words: PageWords {
                words: words.into(),
                size: 14.0,
                line_height: 20.0,
                heading,
            },
            scroll,
        }
    }

    /// **The plain document reads in order**: a heading as one, the words escaped, and a list
    /// that says what its rows say **whole**, where it is — its built rows not twice.
    #[test]
    fn the_plain_document_reads_in_order_with_every_row() {
        let list = id(9);
        let nested = id(10);
        let texts = [
            text(1, "Contacts", true, None),
            text(2, "Row 3", false, Some(list)),
            text(3, "Inside a row's own scroll", false, Some(nested)),
            text(4, "Tom & <Jerry>\nsecond line", false, None),
        ];
        let lists = [PageList {
            id: list,
            count: 3,
            item_text: Rc::new(|i| format!(" Row {i} ")),
            at: 1,
        }];
        let parents: HashMap<_, _> = [(list, None), (nested, Some(list))].into_iter().collect();
        let html = document_html(
            texts
                .iter()
                .map(|t| Some((t.scroll, t.words.words.as_str(), t.words.heading))),
            &lists,
            &parents,
        );
        assert_eq!(
            html,
            "<main><h2>Contacts</h2><ul><li>Row 0</li><li>Row 1</li><li>Row 2</li></ul>\
             <p>Tom &amp; &lt;Jerry&gt;<br>second line</p></main>"
        );
    }

    /// A list after every text still comes, and an empty text does not.
    #[test]
    fn a_list_at_the_end_and_empty_words() {
        let lists = [PageList {
            id: id(5),
            count: 1,
            item_text: Rc::new(|_| "only".into()),
            at: 1,
        }];
        let html = document_html([Some((None, "  ", false)), None], &lists, &HashMap::new());
        assert_eq!(html, "<main><ul><li>only</li></ul></main>");
    }

    #[test]
    fn styles_are_in_css_pixels() {
        assert_eq!(
            box_style(10.0, 20.5, 100.0, 0.333, 2.0),
            "left:20px;top:41px;width:200px;height:0.67px"
        );
    }
}
