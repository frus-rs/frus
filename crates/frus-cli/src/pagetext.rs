// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

//! The page's text, written into the site `frus build web` makes (milestone 830).
//!
//! A frus page paints its words into a canvas: a search engine, or a link preview, that reads
//! the page without running it finds no text at all. Once the site is built, this runs it in a
//! browser with no window, asks the application for its first screen as a plain document
//! (`window.__frusPageText()`, which the web shell defines), and writes that into
//! the site's `index.html` between two markers:
//!
//! ```html
//! <!-- frus:page-text --> … <!-- /frus:page-text -->
//! ```
//!
//! inserted after the `<body>` start tag. The site is the copy of `web/` the build makes in the
//! target directory (`target/frus-web/<package>/`): `web/index.html`, in the sources, is never
//! written. The document is out of sight but in the page — the way text for assistive
//! technology is — and the application takes it out once it shows the same words over its
//! canvas. A build with no browser to run it in, or whose page does not answer, leaves the page
//! as `web/` has it and says so: the site works either way.
//! `frus build web --no-page-text` skips the step.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Where the page's text starts and ends in `index.html`.
const START: &str = "<!-- frus:page-text -->";
const END: &str = "<!-- /frus:page-text -->";

/// The element that holds it: present for whoever reads the page, out of sight for whoever
/// looks at it, until the application replaces it.
const HOLDER: &str = "<div id=\"frus-page-text\" style=\"position:absolute;width:1px;height:1px;\
overflow:hidden;clip-path:inset(50%);white-space:normal\">";

/// How long the page has to answer: loading and compiling a release module, and its first
/// frames, on a slow machine.
const PATIENCE: Duration = Duration::from_secs(60);

/// `page` with `text` — the application's plain document — between the markers, which are put
/// right after the opening `<body>` tag when the page has none. `None` when the page has no
/// `<body>` tag to put them after.
///
/// The page is read as markup, not searched as a string: a `<body>` written in a comment, in a
/// `<style>` (the demo's says "winit appends its canvas to <body>") or in a script is not the
/// tag, and markers there are not the block.
pub(crate) fn splice(page: &str, text: &str) -> Option<String> {
    let block = format!("{START}{HOLDER}{text}</div>{END}");
    let found = scan(page);
    let close = found.body?;
    if let Some((start, end)) = found.block {
        return Some(format!("{}{block}{}", &page[..start], &page[end..]));
    }
    Some(format!("{}\n    {block}{}", &page[..close], &page[close..]))
}

/// What [`splice`] needs to know of a page.
#[derive(Debug, Default, PartialEq)]
struct Found {
    /// Just past the `>` of the real `<body …>` start tag.
    body: Option<usize>,
    /// From the start marker to just past the end marker, both real comments after the body tag.
    block: Option<(usize, usize)>,
}

/// Elements whose content is text, not markup: a `<` in them opens no tag.
const RAW_TEXT: [&str; 6] = ["script", "style", "title", "textarea", "noscript", "xmp"];

/// Walks `page` as markup: comments, the content of raw-text elements and quoted attribute
/// values are stepped over whole.
fn scan(page: &str) -> Found {
    // ASCII lowercasing keeps every byte where it was.
    let lower = page.to_ascii_lowercase();
    let mut found = Found::default();
    let mut start = None;
    let mut i = 0;
    while let Some(off) = lower[i..].find('<') {
        let at = i + off;
        let rest = &lower[at..];
        if let Some(inside) = rest.strip_prefix("<!--") {
            let Some(len) = inside.find("-->") else {
                break;
            };
            let end = at + 4 + len + 3;
            if found.body.is_some() {
                let comment = &page[at..end];
                if comment == START && start.is_none() && found.block.is_none() {
                    start = Some(at);
                } else if comment == END {
                    if let Some(from) = start.take() {
                        found.block = Some((from, end));
                    }
                }
            }
            i = end;
        } else if let Some(name) = RAW_TEXT.iter().find(|name| opens(rest, name)) {
            let Some(tag_end) = tag_end(&lower, at) else {
                break;
            };
            let close = format!("</{name}");
            match lower[tag_end..].find(&close) {
                Some(len) => i = tag_end + len + close.len(),
                None => break,
            }
        } else if opens(rest, "body") && found.body.is_none() {
            let Some(end) = tag_end(&lower, at) else {
                break;
            };
            found.body = Some(end);
            i = end;
        } else {
            i = at + 1;
        }
    }
    found
}

/// Whether `rest` starts with the start tag of `name`: `<name` then a space, `>` or `/`.
fn opens(rest: &str, name: &str) -> bool {
    rest.len() > name.len() + 1
        && rest.as_bytes()[0] == b'<'
        && &rest.as_bytes()[1..=name.len()] == name.as_bytes()
        && matches!(
            rest.as_bytes()[name.len() + 1],
            b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\r' | b'\x0c'
        )
}

/// Just past the `>` that ends the tag starting at `at`, a `>` in a quoted value not counting.
fn tag_end(page: &str, at: usize) -> Option<usize> {
    let mut quote = None;
    for (i, c) in page[at..].char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(at + i + 1),
            _ => {}
        }
    }
    None
}

/// Runs the site in `site` in a browser with no window, takes the application's text, and
/// writes it into `site/index.html`. Best-effort: anything that stops it is said, and the page
/// is left as it was.
pub(crate) fn write(site: &Path) {
    match take(site) {
        Ok(Some(text)) => {
            let index = site.join("index.html");
            let page = match std::fs::read_to_string(&index) {
                Ok(page) => page,
                Err(e) => {
                    crate::report::warn(&format!("could not read {}: {e}", index.display()));
                    return;
                }
            };
            let Some(new) = splice(&page, &text) else {
                crate::report::warn(
                    "the page has no <body> tag: the first screen's text was not written into it",
                );
                return;
            };
            if new != page {
                if let Err(e) = std::fs::write(&index, new) {
                    crate::report::warn(&format!("could not write {}: {e}", index.display()));
                    return;
                }
            }
            println!(
                "Wrote the first screen's text into the site's index.html, for search engines \
                 and link previews ({} bytes).",
                text.len()
            );
        }
        Ok(None) => crate::report::warn(
            "no Chrome or Edge here to run the page in: the page's text was not written into \
             the site's index.html, which search engines then read as empty (--no-page-text \
             skips this)",
        ),
        Err(why) => crate::report::warn(&format!(
            "the page's text was not written into the site's index.html: {why}"
        )),
    }
}

/// The application's text, taken from the site run in a browser. `None` with no browser.
fn take(site: &Path) -> Result<Option<String>, String> {
    let Some(browser) = crate::devices::browser_exe() else {
        return Ok(None);
    };
    let slot: Arc<Mutex<Option<String>>> = Arc::default();
    let server = crate::serve::start_site(
        crate::serve::Site {
            dir: site.to_path_buf(),
            page_text: Some(slot.clone()),
            ..crate::serve::Site::default()
        },
        0,
    )?;
    let profile = std::env::temp_dir().join(format!("frus-page-text-{}", std::process::id()));
    let mut child = Command::new(&browser)
        .arg("--headless=new")
        .arg(format!("--user-data-dir={}", profile.display()))
        .args([
            "--no-first-run",
            "--no-default-browser-check",
            "--enable-unsafe-webgpu",
            "--ignore-gpu-blocklist",
            // A page with no window may be put in the background, its frames stopped.
            "--disable-renderer-backgrounding",
            "--disable-background-timer-throttling",
            "--disable-backgrounding-occluded-windows",
            "--window-size=1280,900",
        ])
        .arg(&server.url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", browser.display()))?;
    let started = Instant::now();
    let text = loop {
        if let Some(text) = slot.lock().ok().and_then(|mut slot| slot.take()) {
            break Ok(text);
        }
        if started.elapsed() > PATIENCE {
            break Err(format!(
                "the page did not give its text within {} s (does it start in {}?)",
                PATIENCE.as_secs(),
                browser.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    stop(&mut child);
    // The browser lets go of its profile a moment after it ends.
    for _ in 0..20 {
        if std::fs::remove_dir_all(&profile).is_ok() || !profile.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    text.map(Some)
}

/// Ends the browser and every process it started: on Windows they outlive their parent.
fn stop(child: &mut Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first time, the text goes right after `<body>`, between the markers.
    #[test]
    fn the_text_goes_after_the_body_tag_the_first_time() {
        let page = "<html><body class=\"x\">\n<script></script></body></html>";
        let out = splice(page, "<main><p>Hello</p></main>").unwrap();
        assert!(out.starts_with(
            "<html><body class=\"x\">\n    <!-- frus:page-text --><div id=\"frus-page-text\""
        ));
        assert!(out.contains("<main><p>Hello</p></main></div><!-- /frus:page-text -->\n<script>"));
    }

    /// The next time, it replaces what is between the markers, and nothing else.
    #[test]
    fn the_next_time_it_replaces_the_old_text() {
        let page =
            "<body>\n    <!-- frus:page-text -->old<!-- /frus:page-text --><p>kept</p></body>";
        let out = splice(page, "<main>new</main>").unwrap();
        assert!(!out.contains("old"));
        assert!(out.contains("<main>new</main></div><!-- /frus:page-text --><p>kept</p>"));
        assert_eq!(out.matches(START).count(), 1);
        // And again: the same page.
        assert_eq!(splice(&out, "<main>new</main>").unwrap(), out);
    }

    #[test]
    fn a_page_without_a_body_is_left_alone() {
        assert_eq!(splice("<p>no body</p>", "x"), None);
        // `<bodyguard>` is not `<body>`; neither is one in a comment.
        assert_eq!(splice("<!-- <body> --><bodyguard>", "x"), None);
    }

    /// The demo's page: its style says "winit appends its canvas to <body>" in a CSS comment.
    /// The text goes after the real tag, not into the style sheet (where a crawler sees
    /// nothing), and a second build replaces it rather than adding another.
    #[test]
    fn a_body_in_a_css_comment_is_not_the_tag() {
        let page = "<!doctype html>\n<html lang=\"en\">\n  <head>\n    <style>\n      \
            /* winit appends its canvas to <body> and sizes it from this box */\n      \
            canvas { display: block; }\n    </style>\n    \
            <script>if (a <body) {}</script>\n  </head>\n  \
            <!-- the <body> below -->\n  <body data-x=\"a>b\">\n    <canvas></canvas>\n  \
            </body>\n</html>\n";
        let out = splice(page, "<main><p>Hello</p></main>").unwrap();
        assert!(
            out.contains("to <body> and sizes it from this box */"),
            "{out}"
        );
        assert!(out.contains(
            "<body data-x=\"a>b\">\n    <!-- frus:page-text --><div id=\"frus-page-text\""
        ));
        assert_eq!(out.matches(START).count(), 1);
        let style_end = out.find("</style>").unwrap();
        assert!(out.find(START).unwrap() > style_end);
        // Built again: replaced in place.
        let again = splice(&out, "<main><p>Bye</p></main>").unwrap();
        assert_eq!(again.matches(START).count(), 1);
        assert!(!again.contains("Hello") && again.contains("<p>Bye</p>"));
        assert_eq!(again.replace("Bye", "Hello"), out);
    }

    /// Markers written in a script or before the body are not the block.
    #[test]
    fn markers_outside_the_body_are_not_the_block() {
        let page = "<head><script>s = '<!-- frus:page-text -->x<!-- /frus:page-text -->';\
            </script></head><body>\n</body>";
        let out = splice(page, "new").unwrap();
        assert!(out.contains("s = '<!-- frus:page-text -->x<!-- /frus:page-text -->';"));
        assert!(out.contains("<body>\n    <!-- frus:page-text --><div"));
        assert_eq!(out.matches("new</div>").count(), 1);
    }
}
