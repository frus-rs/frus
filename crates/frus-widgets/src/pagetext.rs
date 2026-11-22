// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The words a frame shows, as the **page's own text** (milestone 830).
//!
//! A page drawn into a canvas has no text as far as the browser is concerned: find-in-page,
//! printing and search engines all find nothing. The web shell answers with a layer of real,
//! transparent text laid over the canvas, and with a plain document for printing and indexing,
//! both made from what this module records: each text the frame laid out, where, and in which
//! scroll region — **including the texts scrolled out of view**, which the accessibility tree
//! leaves out — and, for a list that builds only the rows in view, what its other rows say when
//! the application tells it ([`ListView::item_text`](crate::ListView::item_text)).
//!
//! Recorded only when the shell asks ([`Runtime::page_text`](crate::Runtime::page_text)): the
//! web shell does, and no other platform pays anything for it.

use std::rc::Rc;

use crate::interaction::WidgetId;
use frus_core::Rect;

/// What a text widget says, and at what size, for the page's text — what
/// [`Widget::page_words`](crate::Widget::page_words) answers.
#[derive(Clone, Debug, PartialEq)]
pub struct PageWords {
    /// The words, as they read.
    pub words: String,
    /// The font size, in logical pixels.
    pub size: f32,
    /// The height of one line, in logical pixels.
    pub line_height: f32,
    /// Whether the text is a heading: a landmark, printed and indexed as one.
    pub heading: bool,
}

/// A text the frame laid out, for the page's text.
#[derive(Clone, Debug, PartialEq)]
pub struct PageText {
    /// The text widget.
    pub id: WidgetId,
    /// Where it was laid out, in the window's logical pixels — out of view when it is
    /// scrolled out of view.
    pub rect: Rect,
    /// What it says.
    pub words: PageWords,
    /// The innermost scroll region it is in, if any.
    pub scroll: Option<WidgetId>,
}

/// A list that builds only the rows in view, and can say what all of them say: see
/// [`ListView::item_text`](crate::ListView::item_text).
#[derive(Clone)]
pub struct PageList {
    /// The list, which is also its scroll region.
    pub id: WidgetId,
    /// How many items.
    pub count: usize,
    /// What item `index` says.
    pub item_text: Rc<dyn Fn(usize) -> String>,
    /// How many [`PageText`]s the frame had recorded when it reached the list: where its
    /// rows go in reading order.
    pub at: usize,
}

impl std::fmt::Debug for PageList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageList")
            .field("id", &self.id)
            .field("count", &self.count)
            .field("at", &self.at)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use crate::{build_ui, Flex, IndexedStack, ListView, Runtime, SingleChildScrollView, Text};
    use crate::{Theme, Widget};
    use frus_core::Size;

    fn page() -> Flex<()> {
        let mut long = Flex::column();
        for i in 0..40 {
            long = long.child(Text::new(format!("Line {i}")).no_wrap());
        }
        Flex::column()
            .child(Text::new("Title").heading().no_wrap())
            .child(SingleChildScrollView::new().height(200.0).child(long))
    }

    fn frame(tree: &dyn Widget<()>, runtime: &Runtime) -> crate::Ui<()> {
        build_ui(tree, Size::new(300.0, 400.0), runtime, &Theme::default())
    }

    /// **Nothing is recorded unless the shell asks**: the other platforms pay nothing.
    #[test]
    fn nothing_is_recorded_unless_asked() {
        let ui = frame(&page(), &Runtime::default());
        assert!(ui.page_text().is_empty());
        assert!(ui.page_lists().is_empty());
    }

    /// **The texts scrolled out of view are recorded**, in reading order, each with its scroll
    /// region and where it is laid out — below the view — which the accessibility tree, made
    /// of what shows, leaves out.
    #[test]
    fn texts_scrolled_out_of_view_are_recorded_with_their_region() {
        let mut runtime = Runtime::default();
        runtime.page_text = true;
        let ui = frame(&page(), &runtime);
        let texts = ui.page_text();
        assert_eq!(texts.len(), 41, "the title and the 40 lines");
        assert_eq!(texts[0].words.words, "Title");
        assert!(texts[0].words.heading);
        assert_eq!(texts[0].scroll, None);
        let last = &texts[40];
        assert_eq!(last.words.words, "Line 39");
        let region = ui.scroll_regions()[0].id;
        assert_eq!(last.scroll, Some(region));
        assert!(
            last.rect.y > 400.0,
            "laid out below the window: {:?}",
            last.rect
        );
        assert!(last.words.size > 0.0 && last.words.line_height >= last.words.size);
        assert!(
            !ui.semantics()
                .iter()
                .any(|(_, _, s)| s.label.as_deref() == Some("Line 39")),
            "the accessibility tree holds only what shows"
        );
    }

    /// **An app bar's title is a heading on the page** (milestone 833), as it is to a screen
    /// reader: the page a build writes for a route takes it for its title. The texts beside it
    /// are not.
    #[test]
    fn an_app_bars_title_is_a_heading_on_the_page() {
        let mut runtime = Runtime::default();
        runtime.page_text = true;
        let tree = Flex::column()
            .child(crate::AppBar::new().title(Text::new("Cart")))
            .child(Text::new("Your cart is empty").no_wrap())
            .child(crate::Semantics::heading(Text::new("Section").no_wrap()));
        let ui = frame(&tree, &runtime);
        let texts: Vec<(&str, bool)> = ui
            .page_text()
            .iter()
            .map(|t| (t.words.words.as_str(), t.words.heading))
            .collect();
        assert_eq!(
            texts,
            [
                ("Cart", true),
                ("Your cart is empty", false),
                ("Section", true)
            ]
        );
    }

    /// **A list that says what its rows say is recorded**, where it comes in reading order —
    /// and its words are not asked for: that waits for a find, a print or a build.
    #[test]
    fn a_list_that_says_what_its_rows_say_is_recorded_unasked() {
        let asked = std::rc::Rc::new(std::cell::Cell::new(0));
        let counted = asked.clone();
        let tree = Flex::column().child(Text::new("Above").no_wrap()).child(
            ListView::new(10_000, 40.0, |i| Text::new(format!("Row {i}")).no_wrap())
                .height(200.0)
                .item_text(move |i| {
                    counted.set(counted.get() + 1);
                    format!("Row {i}")
                }),
        );
        let mut runtime = Runtime::default();
        runtime.page_text = true;
        let ui = frame(&tree, &runtime);
        let lists = ui.page_lists();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0].count, 10_000);
        assert_eq!(lists[0].at, 1, "after the text above it");
        assert_eq!(asked.get(), 0, "no row's words asked for by a frame");
        assert!(ui.page_text().len() < 20, "only the rows built are texts");
        assert_eq!((lists[0].item_text)(4321), "Row 4321");
        // Where each row lies, for a find to scroll to.
        let (across, spans) = runtime.list_item_spans(lists[0].id).unwrap();
        assert!(!across);
        assert_eq!(spans.len(), 10_000);
        assert_eq!(spans[4321], (4321.0 * 40.0, 40.0));
    }

    /// **What is not drawn is not on the page**: a layer of an indexed stack that is not shown.
    #[test]
    fn a_layer_not_shown_is_not_on_the_page() {
        let tree = IndexedStack::new(1)
            .child(Text::new("Hidden page").no_wrap())
            .child(Text::new("Shown page").no_wrap());
        let mut runtime = Runtime::default();
        runtime.page_text = true;
        let ui = frame(&tree, &runtime);
        let words: Vec<_> = ui
            .page_text()
            .iter()
            .map(|t| t.words.words.as_str())
            .collect();
        assert_eq!(words, ["Shown page"]);
    }
}
