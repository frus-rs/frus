// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! A small browser (milestones 799 and 804): an address bar, back and forward, a progress
//! bar, and a JavaScript channel the page answers on. The same code on Android, Windows
//! (WebView2) and the web (a frame); on macOS, Linux and iOS the page shows its stand-in.

use std::cell::RefCell;

use frus_widgets::{
    request_rebuild, spawn_local, JavaScriptMessage, NavigationDelegate, Text,
    TextEditingController, WebViewController, WebViewWidget,
};

use crate::prelude::*;

/// The page the browser opens on: local, so the round trip works with no network. Its button
/// posts to the `Frus` channel, and `frus.reply` is what the application calls back.
pub(crate) const START_PAGE: &str = "<!doctype html><html><head>\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>frus start page</title>\
<style>body{font-family:sans-serif;margin:16px;background:#e8f5e9;color:#1b5e20}\
button{font-size:16px;padding:8px 14px;border-radius:8px;border:1px solid #2e7d32;background:#c8e6c9}\
p{line-height:1.5}</style></head><body>\
<h1>A web page in a frus browser</h1>\
<p>Type an address above, or send a message to the application through a JavaScript \
channel:</p>\
<button onclick=\"Frus.postMessage('Hello from the page at '+new Date().toLocaleTimeString())\">\
Send to frus</button>\
<p id=\"reply\">No reply yet.</p>\
<script>window.frus={reply:function(t){document.getElementById('reply').textContent=t;}};</script>\
<p><a href=\"https://example.com\">example.com</a> (needs the network)</p>\
</body></html>";

/// The screen.
pub(crate) struct BrowserPage;

/// The browser's state: the page, the address being typed, and what the page last said.
pub(crate) struct BrowserState {
    web: WebViewController,
    address: TextEditingController,
    heard: Rc<RefCell<String>>,
    title: Rc<RefCell<String>>,
}

impl StatefulWidget for BrowserPage {
    type State = BrowserState;

    fn create_state(&self) -> BrowserState {
        let web = WebViewController::new();
        let address = TextEditingController::new();
        let heard = Rc::new(RefCell::new(String::new()));
        {
            let heard = heard.clone();
            // Weak: the controller holds this hook, which must not hold the controller.
            let reply = web.downgrade();
            web.add_javascript_channel("Frus", move |message: JavaScriptMessage| {
                *heard.borrow_mut() = message.message.clone();
                // The round trip: the application answers the page.
                let answer = serde_json_quote(&format!("frus heard: {}", message.message));
                if let Some(web) = reply.upgrade() {
                    web.run_javascript(format!("window.frus && frus.reply({answer})"));
                }
                request_rebuild();
            })
            .expect("a valid channel name");
        }
        {
            let address = address.clone();
            web.set_navigation_delegate(NavigationDelegate::new().on_url_change(move |change| {
                if let Some(url) = &change.url {
                    if !url.starts_with("data:") && !url.starts_with("about:") {
                        address.set_text(url.clone());
                    }
                }
            }));
        }
        web.load_html_string(START_PAGE, None);
        BrowserState {
            web,
            address,
            heard,
            title: Rc::new(RefCell::new(String::new())),
        }
    }
}

/// `text` as a JavaScript string literal.
fn serde_json_quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What was typed, as an address: a scheme is added when there is none.
pub(crate) fn address_of(typed: &str) -> String {
    let typed = typed.trim();
    if typed.contains("://") || typed.starts_with("about:") || typed.starts_with("data:") {
        typed.to_owned()
    } else {
        format!("https://{typed}")
    }
}

impl State for BrowserState {
    type Widget = BrowserPage;

    fn build(&self, cx: &StateContext<Self>) -> Box<dyn Widget> {
        let theme = cx.theme().clone();
        let web = &self.web;
        let go = {
            let (web, address) = (web.clone(), self.address.clone());
            on(move || web.load_request(address_of(&address.text())))
        };
        let nav = |icon, label: &str, enabled: bool, act: Callback| {
            let mut b = IconButton::new(icon).label(label);
            if enabled {
                b = b.on_press(act);
            }
            b
        };
        let back = {
            let web = web.clone();
            on(move || web.go_back())
        };
        let forward = {
            let web = web.clone();
            on(move || web.go_forward())
        };
        let reload = {
            let web = web.clone();
            on(move || web.reload())
        };
        let ask_title = {
            let (web, title) = (web.clone(), self.title.clone());
            on(move || {
                let asked = web.run_javascript_returning_result("document.title");
                let title = title.clone();
                // Dropping the handle leaves the task running.
                let _task = spawn_local(async move {
                    *title.borrow_mut() = match asked.await {
                        Ok(value) => value,
                        Err(error) => error.to_string(),
                    };
                    request_rebuild();
                });
            })
        };
        let bar = Flex::row()
            .gap(4.0)
            .padding(8.0)
            .child(nav(Icons::ARROW_BACK, "Back", web.can_go_back(), back))
            .child(nav(
                Icons::ARROW_FORWARD,
                "Forward",
                web.can_go_forward(),
                forward,
            ))
            .child(nav(Icons::REFRESH, "Reload", true, reload))
            .child(
                Expanded::new(
                    TextField::new(self.address.text())
                        .size(15.0)
                        .placeholder("Address")
                        .controller(&self.address)
                        .on_submit(go.clone()),
                )
                .flex(1.0),
            )
            .child(nav(Icons::ARROW_FORWARD, "Go", true, go));
        let progress = if web.is_loading() {
            f32::from(web.progress()) / 100.0
        } else {
            0.0
        };
        let heard = self.heard.borrow().clone();
        let title = self.title.borrow().clone();
        let footer = Flex::column()
            .gap(6.0)
            .padding(12.0)
            .child(
                text(if heard.is_empty() {
                    "The page has not posted to the Frus channel yet.".to_owned()
                } else {
                    format!("The page posted: {heard}")
                })
                .size(14.0)
                .color(theme.on_surface),
            )
            .child(
                Flex::row()
                    .gap(12.0)
                    .child(
                        button("Ask the page its title", ask_title)
                            .variant(Variant::Outlined)
                            .size(14.0),
                    )
                    .child(text(title).size(14.0).color(theme.muted)),
            );
        let body = Flex::column()
            .flex(1.0)
            .child(bar)
            .child(LinearProgressIndicator::new(progress))
            .child(
                WebViewWidget::new(web)
                    .semantic_label(web.title().unwrap_or_else(|| "Web page".to_owned())),
            )
            // The footer is the last thing on the screen: kept above the system's navigation
            // bar.
            .child(SafeArea::new(footer).edges(frus_widgets::Edges {
                top: false,
                ..frus_widgets::Edges::ALL
            }));
        let router = cx.router();
        Box::new(
            Scaffold::new()
                .background(theme.background)
                .app_bar(
                    AppBar::new()
                        .title(Text::new(
                            web.title().unwrap_or_else(|| "Browser".to_owned()),
                        ))
                        .leading(IconButton::new(Icons::CLOSE).label("Close").on_press(on(
                            move || {
                                router.pop();
                            },
                        )))
                        .build(),
                )
                .body(body)
                .build(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_address_gets_a_scheme_when_it_has_none() {
        assert_eq!(address_of(" example.com "), "https://example.com");
        assert_eq!(address_of("http://a.test/x"), "http://a.test/x");
        assert_eq!(address_of("about:blank"), "about:blank");
    }

    #[test]
    fn a_reply_is_a_javascript_string() {
        assert_eq!(serde_json_quote("a \"b\"\n"), "\"a \\\"b\\\"\\n\"");
    }
}
