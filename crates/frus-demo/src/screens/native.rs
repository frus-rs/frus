// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The native views screen (milestone 794): an Android web view and an Android text field
//! inside a scrolling frus page, and a frus dialog that opens over both. Elsewhere than on
//! Android each view shows its stand-in.

use frus_widgets::{AlertDialog, AndroidView, PlatformViewController, PlatformViewEvent, Text};

use crate::prelude::*;

/// The page the web view shows: local, so it needs no network.
pub(crate) const NATIVE_PAGE: &str = "<!doctype html><html><head>\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>A native web view</title>\
<style>body{font-family:sans-serif;margin:16px;background:#fff8e1;color:#3e2723}\
h1{font-size:22px;margin:0 0 8px}li{margin:6px 0}\
button{font-size:16px;padding:8px 14px;border-radius:8px;border:1px solid #8d6e63;background:#ffe0b2}\
</style></head><body>\
<h1>Hello from a native WebView</h1>\
<p>This page is drawn by Android's own browser engine, inside a frus page.</p>\
<ul><li>It scrolls with the page around it.</li><li>A frus dialog covers it.</li>\
<li>A screen reader reads it.</li></ul>\
<button onclick=\"this.textContent='Pressed '+(++window.n||(window.n=1))+' time(s)'\">Press me</button>\
</body></html>";

/// The screen.
pub(crate) struct NativePage;

/// What the page keeps: the web view's controller, what the views last said, and whether the
/// dialog is open.
pub(crate) struct NativeState {
    web: PlatformViewController,
    typed: String,
    heard: String,
    dialog: bool,
}

impl StatefulWidget for NativePage {
    type State = NativeState;

    fn create_state(&self) -> NativeState {
        NativeState {
            web: PlatformViewController::new(),
            typed: String::new(),
            heard: String::new(),
            dialog: false,
        }
    }
}

impl State for NativeState {
    type Widget = NativePage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let theme = &theme;
        let filler = |n: usize| {
            Card::new().child(
                Container::new().padding(16.0).child(
                    text(format!(
                        "frus row {n}: drag the page to scroll the native views with it."
                    ))
                    .size(15.0)
                    .color(theme.on_surface),
                ),
            )
        };
        let heard = cx.handler(|s: &mut NativeState, event: PlatformViewEvent| {
            s.heard = format!("{}: {}", event.name, event.data);
        });
        let typed = cx.handler(|s: &mut NativeState, event: PlatformViewEvent| {
            if event.name == "changed" || event.name == "submitted" {
                s.typed = event.data;
            }
        });
        let web = self.web.clone();
        let mut column = Flex::column().gap(12.0).padding(16.0).child(
            text("Native Android views inside a frus page")
                .size(18.0)
                .color(theme.on_surface),
        );
        for n in 1..=3 {
            column = column.child(filler(n));
        }
        column = column
            .child(
                AndroidView::new("frus/webview", NATIVE_PAGE)
                    .height(280.0)
                    .controller(&self.web)
                    .semantic_label("Web page")
                    .on_event(move |event| Some(heard(event.clone()))),
            )
            .child(
                Flex::row()
                    .gap(12.0)
                    .child(
                        button(
                            "Ask the page its title",
                            on(move || web.invoke("evaluateJavascript", "document.title")),
                        )
                        .variant(Variant::Outlined)
                        .size(14.0),
                    )
                    .child(
                        button("Open a dialog", cx.callback(|s| s.dialog = true))
                            .variant(Variant::Filled)
                            .size(14.0),
                    ),
            )
            .child(
                text(format!("The web view said: {}", self.heard))
                    .size(14.0)
                    .color(theme.muted),
            )
            .child(
                AndroidView::new("frus/edittext", "A native EditText: type here")
                    .height(56.0)
                    .on_event(move |event| Some(typed(event.clone()))),
            )
            .child(
                text(format!("The native field holds: {}", self.typed))
                    .size(14.0)
                    .color(theme.muted),
            );
        for n in 4..=10 {
            column = column.child(filler(n));
        }
        let body = SingleChildScrollView::new().flex(1.0).child(column);
        let router = cx.router();
        let page = Scaffold::new()
            .background(theme.background)
            .app_bar(
                AppBar::new()
                    .title(Text::new("Native views"))
                    .leading(IconButton::new(Icons::ARROW_BACK).on_press(on(move || {
                        router.pop();
                    })))
                    .build(),
            )
            .body(body)
            .build();
        Box::new(
            AlertDialog::new(self.dialog)
                .title("A frus dialog")
                .content("It is drawn over the native views, and takes the touches there.")
                .action(button("Close", cx.callback(|s| s.dialog = false)))
                .on_dismiss(cx.callback(|s| s.dialog = false))
                .body(page),
        )
    }
}
