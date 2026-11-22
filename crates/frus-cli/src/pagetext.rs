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
//!
//! **Every route** (milestone 833). The page is asked which locations to write a page for
//! (`Application::prerendered_paths`: a router's routes without a parameter, and the
//! locations it was given). For an application whose addresses are paths, the browser then
//! moves it to each, the way its back button would, and its text there goes into a page of its
//! own, `settings/index.html` for `/settings`, with a `<base href>` back to the site's root, the
//! title the application gave it (or its first heading before the site's title) and its first
//! words as the description. A location that redirects elsewhere gets no page. A list gives at
//! most 200 rows and a page at most [`MAX_PAGE_BYTES`]. Behind a `#`, every address is the one
//! page to a server: only the first screen is written.

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
    /// Just past the `>` of the `<head …>` start tag.
    head: Option<usize>,
    /// The `<title>` element, its end tag included, before the body.
    title: Option<(usize, usize)>,
    /// The `<meta name="description">` tag, before the body.
    description: Option<(usize, usize)>,
    /// Whether the page has a `<base>` tag before the body.
    base: bool,
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
            let Some(open_end) = tag_end(&lower, at) else {
                break;
            };
            let close = format!("</{name}");
            let Some(len) = lower[open_end..].find(&close) else {
                break;
            };
            let close_at = open_end + len;
            i = close_at + close.len();
            if *name == "title" && found.body.is_none() && found.title.is_none() {
                let Some(end) = tag_end(&lower, close_at) else {
                    break;
                };
                found.title = Some((at, end));
                i = end;
            }
        } else if found.body.is_none() && (opens(rest, "meta") || opens(rest, "base")) {
            let Some(end) = tag_end(&lower, at) else {
                break;
            };
            if opens(rest, "base") {
                found.base = true;
            } else if found.description.is_none() && names_description(&lower[at..end]) {
                found.description = Some((at, end));
            }
            i = end;
        } else if opens(rest, "head") && found.head.is_none() && found.body.is_none() {
            let Some(end) = tag_end(&lower, at) else {
                break;
            };
            found.head = Some(end);
            i = end;
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

/// Whether a lowercased `<meta …>` tag is `name="description"`, however it is quoted.
fn names_description(tag: &str) -> bool {
    [
        "name=\"description\"",
        "name='description'",
        "name=description",
    ]
    .iter()
    .any(|attr| tag.contains(attr))
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

/// `page` with its head made to say `title` and `description` (milestone 833), and, when
/// `base` is given and the page has no `<base>` of its own, with `<base href="{base}">`: a
/// route's page lies deeper than `index.html`, and its relative URLs (`./pkg/app.js`) must
/// keep meaning what they mean there. A title or description that is `None` is left as the
/// page has it. `None` when the page has no `<head>` tag.
pub(crate) fn with_head(
    page: &str,
    title: Option<&str>,
    description: Option<&str>,
    base: Option<&str>,
) -> Option<String> {
    let found = scan(page);
    let head = found.head?;
    // Edits from the end of the page backwards, so that each one's offsets still hold.
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut added = String::new();
    if let Some(base) = base.filter(|_| !found.base) {
        added.push_str(&format!("\n    <base href=\"{}\" />", attribute(base)));
    }
    if let Some(title) = title {
        let tag = format!("<title>{}</title>", text(title));
        match found.title {
            Some((start, end)) => edits.push((start, end, tag)),
            None => added.push_str(&format!("\n    {tag}")),
        }
    }
    if let Some(description) = description {
        let tag = format!(
            "<meta name=\"description\" content=\"{}\" />",
            attribute(description)
        );
        match found.description {
            Some((start, end)) => edits.push((start, end, tag)),
            None => added.push_str(&format!("\n    {tag}")),
        }
    }
    if !added.is_empty() {
        edits.push((head, head, added));
    }
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
    let mut out = page.to_string();
    for (start, end, with) in edits {
        out.replace_range(start..end, &with);
    }
    Some(out)
}

/// `value` made safe inside a double-quoted attribute.
fn attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `value` made safe as an element's text.
fn text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The most a page's text may weigh (milestone 833). A list's rows are already cut at 200 by
/// the page; a screen of many lists, or of very long texts, stops here.
pub(crate) const MAX_PAGE_BYTES: usize = 256 * 1024;

/// `document` — the application's plain document, `<main>` of headings, paragraphs and lists —
/// cut to at most about `max` bytes at the end of an element, its open list and `<main>`
/// closed again.
pub(crate) fn cap_document(document: &str, max: usize) -> String {
    if document.len() <= max {
        return document.to_string();
    }
    let mut cut = 0;
    for close in ["</p>", "</h2>", "</li>", "</ul>"] {
        let mut from = 0;
        while let Some(at) = document[from..].find(close) {
            let end = from + at + close.len();
            if end > max {
                break;
            }
            cut = cut.max(end);
            from = end;
        }
    }
    let kept = if cut == 0 { "<main>" } else { &document[..cut] };
    let open_lists = kept
        .matches("<ul>")
        .count()
        .saturating_sub(kept.matches("</ul>").count());
    let mut out = kept.to_string();
    for _ in 0..open_lists {
        out.push_str("</ul>");
    }
    out.push_str("</main>");
    out
}

/// The words of a plain document, without its tags, for a description: the first ones, cut
/// at a word to at most `max` characters, and whether it has a heading first: its words.
pub(crate) fn summary(document: &str, max: usize) -> (String, Option<String>) {
    let mut words = String::new();
    let mut heading = None;
    let mut rest = document;
    while let Some(at) = rest.find('<') {
        let (before, tag) = rest.split_at(at);
        words.push_str(before);
        let end = tag.find('>').map_or(tag.len(), |end| end + 1);
        let name = &tag[..end];
        if name == "<h2>" && heading.is_none() {
            if let Some(close) = tag[end..].find("</h2>") {
                heading = Some(unescape(&tag[end..end + close]).trim().to_string());
            }
        }
        // Each element its own sentence-ish part: `<p>a</p><p>b</p>` reads "a b".
        if name.starts_with("</") || name == "<br>" {
            words.push(' ');
        }
        rest = &tag[end..];
    }
    words.push_str(rest);
    let words = unescape(&words);
    let mut out = String::new();
    for word in words.split_whitespace() {
        if out.chars().count() + word.chars().count() + 1 > max {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    (out, heading.filter(|heading| !heading.is_empty()))
}

/// The five entities the plain document writes, read back.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// One page the application showed, as the browser posted it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Shown {
    /// The location the build asked for; `None` for the first screen.
    pub asked: Option<String>,
    /// Where the application was when its text held still.
    pub location: String,
    /// The document's title then.
    pub title: String,
    /// Its plain document, `None` when it never showed a word.
    pub text: Option<String>,
}

/// What the browser posted, read.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Taken {
    /// `"path"` or `"hash"`: how the application writes its addresses.
    pub strategy: String,
    /// The locations the application said to write a page for.
    pub paths: Vec<String>,
    /// The first screen first, then the locations visited.
    pub shown: Vec<Shown>,
    /// Whether the browser said it was done.
    pub done: bool,
}

/// Reads what the page posted to the build's server, in order. A message that is not one is
/// passed over.
pub(crate) fn read_posts(posts: &[String]) -> Taken {
    let mut taken = Taken::default();
    for post in posts {
        let Ok(value) = crate::json::parse(post) else {
            continue;
        };
        let string = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_string);
        match value.get("kind").and_then(|kind| kind.as_str()) {
            Some("done") => taken.done = true,
            Some("page") => {
                let asked = string("path");
                if asked.is_none() {
                    taken.strategy = string("strategy").unwrap_or_default();
                    taken.paths = value
                        .get("paths")
                        .and_then(|paths| paths.as_array())
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|path| path.as_str().map(str::to_string))
                        .collect();
                }
                taken.shown.push(Shown {
                    asked,
                    location: string("location").unwrap_or_default(),
                    title: string("title").unwrap_or_default(),
                    text: string("text").filter(|text| text != "<main></main>"),
                });
            }
            _ => {}
        }
    }
    taken
}

/// Where the page for `location` goes in the site, relative to it: `settings/index.html` for
/// `/settings`. `None` for the site's own page (`/`) and for a location no file can stand
/// for: one with a query or a fragment, an empty, `.` or `..` segment, or a character a file
/// name cannot hold on some system.
pub(crate) fn page_file(location: &str) -> Option<std::path::PathBuf> {
    let path = location.strip_prefix('/')?.trim_end_matches('/');
    if path.is_empty() || location.contains(['?', '#']) {
        return None;
    }
    let mut file = std::path::PathBuf::new();
    for segment in path.split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.contains(['\\', ':', '*', '"', '<', '>', '|'])
            || segment.chars().any(char::is_control)
        {
            return None;
        }
        file.push(segment);
    }
    Some(file.join("index.html"))
}

/// The `<base href>` that makes a page at `file` (relative to the site) resolve URLs as the
/// site's own `index.html` does: `../` for `settings/index.html`.
fn relative_base(file: &Path) -> String {
    let depth = file.components().count().saturating_sub(1);
    "../".repeat(depth)
}

/// Whether two locations are the same page: a trailing `/` does not count.
fn same_location(a: &str, b: &str) -> bool {
    let trim = |s: &str| s.trim_end_matches('/').to_string();
    trim(a) == trim(b)
}

/// Runs the site in `site` in a browser with no window, takes the application's text, and
/// writes it into `site/index.html` — and, for an application whose addresses are paths, into
/// a page of its own for each location it lists (`site/settings/index.html`, milestone 833).
/// Best-effort: anything that stops it is said, and the pages are left as they were.
pub(crate) fn write(site: &Path) {
    let taken = match take(site) {
        Ok(Some(taken)) => taken,
        Ok(None) => {
            crate::report::warn(
                "no Chrome or Edge here to run the page in: the page's text was not written into \
                 the site's index.html, which search engines then read as empty (--no-page-text \
                 skips this)",
            );
            return;
        }
        Err(why) => {
            crate::report::warn(&format!(
                "the page's text was not written into the site's index.html: {why}"
            ));
            return;
        }
    };
    let index = site.join("index.html");
    let page = match std::fs::read_to_string(&index) {
        Ok(page) => page,
        Err(e) => {
            crate::report::warn(&format!("could not read {}: {e}", index.display()));
            return;
        }
    };
    let Some(first) = taken.shown.first().filter(|first| first.asked.is_none()) else {
        return;
    };
    let Some(text) = first
        .text
        .as_deref()
        .map(|text| cap_document(text, MAX_PAGE_BYTES))
    else {
        crate::report::warn("the page showed no text: none was written into the site's index.html");
        return;
    };
    let Some(new) = splice(&page, &text) else {
        crate::report::warn(
            "the page has no <body> tag: the first screen's text was not written into it",
        );
        return;
    };
    // The site's own page keeps the title it was given, and a description it has; one it has
    // not is the first words of its first screen.
    let described = scan(&new).description.is_some();
    let (words, _) = summary(&text, 155);
    let new = match (described, words.is_empty()) {
        (false, false) => with_head(&new, None, Some(&words), None).unwrap_or(new),
        _ => new,
    };
    if new != page {
        if let Err(e) = std::fs::write(&index, &new) {
            crate::report::warn(&format!("could not write {}: {e}", index.display()));
            return;
        }
    }
    println!(
        "Wrote the first screen's text into the site's index.html, for search engines \
         and link previews ({} bytes).",
        text.len()
    );
    write_routes(site, &page, &taken);
}

/// A page of its own for each location the application listed (milestone 833), made from the
/// site's page as it was before the first screen's text went in.
fn write_routes(site: &Path, page: &str, taken: &Taken) {
    let others: Vec<&String> = taken
        .paths
        .iter()
        .filter(|path| page_file(path).is_some())
        .collect();
    if taken.strategy != "path" {
        if !others.is_empty() {
            crate::report::say(&format!(
                "The application's addresses come after a `#`, which a server never sees: the \
                 site is one page to a search engine, and only the first screen's text is in it. \
                 Addresses written as paths (`LocationStrategy::Path`) give each of its {} other \
                 routes a page of its own.",
                others.len()
            ));
        }
        return;
    }
    let site_title = taken
        .shown
        .first()
        .map(|first| first.title.as_str())
        .unwrap_or("");
    let (mut written, mut skipped) = (Vec::new(), Vec::new());
    for shown in taken.shown.iter().skip(1) {
        let Some(asked) = shown.asked.as_deref() else {
            continue;
        };
        let Some(file) = page_file(asked) else {
            continue;
        };
        if !same_location(asked, shown.location.split('#').next().unwrap_or("")) {
            skipped.push(format!("{asked} (it goes to {})", shown.location));
            continue;
        }
        let Some(text) = shown.text.as_deref() else {
            skipped.push(format!("{asked} (it showed no text)"));
            continue;
        };
        let path = site.join(&file);
        if path.exists() {
            skipped.push(format!("{asked} (the site has a page there already)"));
            continue;
        }
        let text = cap_document(text, MAX_PAGE_BYTES);
        let (words, heading) = summary(&text, 155);
        // The title the application gave this page, or its heading before the site's.
        let title = if !shown.title.is_empty() && shown.title != site_title {
            shown.title.clone()
        } else {
            match heading {
                Some(heading) if !site_title.is_empty() => format!("{heading} | {site_title}"),
                Some(heading) => heading,
                None => site_title.to_string(),
            }
        };
        let made = with_head(
            page,
            Some(&title)
                .filter(|title| !title.is_empty())
                .map(String::as_str),
            Some(&words)
                .filter(|words| !words.is_empty())
                .map(String::as_str),
            Some(&relative_base(&file)),
        )
        .and_then(|headed| splice(&headed, &text));
        let Some(made) = made else {
            skipped.push(format!("{asked} (the page has no <head> or <body> tag)"));
            continue;
        };
        let wrote = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, made));
        match wrote {
            Ok(()) => written.push(asked.to_string()),
            Err(e) => skipped.push(format!("{asked} (could not write {}: {e})", path.display())),
        }
    }
    let unvisited = others.len().saturating_sub(written.len() + skipped.len());
    if !written.is_empty() {
        println!(
            "Wrote a page with its own text for {} more address{}: {}.",
            written.len(),
            if written.len() == 1 { "" } else { "es" },
            written.join(", ")
        );
    }
    if !skipped.is_empty() {
        crate::report::say(&format!("No page written for: {}.", skipped.join("; ")));
    }
    if unvisited > 0 {
        crate::report::warn(&format!(
            "{unvisited} of the application's addresses were not reached in time: they have no \
             page of their own"
        ));
    }
    if !scan(page).base {
        crate::report::say(
            "An address with no page of its own (`/products/42`) is answered by your host's \
             fallback with index.html, whose relative URLs then need a `<base href=\"/\">` (or \
             the site's own path) in web/index.html.",
        );
    }
}

/// How long the page has to show each location the build moves it to, at most.
const PATIENCE_PER_ROUTE: Duration = Duration::from_secs(25);

/// The application's text, taken from the site run in a browser. `None` with no browser.
fn take(site: &Path) -> Result<Option<Taken>, String> {
    let Some(browser) = crate::devices::browser_exe() else {
        return Ok(None);
    };
    let slot: Arc<Mutex<Vec<String>>> = Arc::default();
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
            // With no GPU (a build machine), WebGL through the software renderer, which recent
            // browsers no longer fall back to on their own.
            "--enable-unsafe-swiftshader",
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
    let posts = || slot.lock().map(|posts| posts.clone()).unwrap_or_default();
    let taken = loop {
        let taken = read_posts(&posts());
        if taken.done {
            break Ok(taken);
        }
        // The first screen within `PATIENCE`, then each location within its own.
        let allowed = match taken.shown.first() {
            None => PATIENCE,
            Some(_) => PATIENCE + PATIENCE_PER_ROUTE * (taken.shown.len() as u32).max(1),
        };
        if started.elapsed() > allowed {
            break match taken.shown.first() {
                Some(_) => Ok(taken),
                None => Err(format!(
                    "the page did not give its text within {} s (does it start in {}?)",
                    PATIENCE.as_secs(),
                    browser.display()
                )),
            };
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
    taken.map(Some)
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

    const TEMPLATE: &str =
        "<!doctype html>\n<html>\n  <head>\n    <meta charset=\"utf-8\" />\n    \
        <meta name=\"description\" content=\"A frus application\" />\n    <title>shop</title>\n    \
        <style>/* <title>not this</title> */</style>\n  </head>\n  <body>\n    \
        <script type=\"module\">import('./pkg/shop.js')</script>\n  </body>\n</html>\n";

    /// A route's page says its own title and description, and resolves its URLs from the
    /// site's root (milestone 833).
    #[test]
    fn a_routes_head_has_its_title_description_and_base() {
        let out = with_head(
            TEMPLATE,
            Some("Cart | Shop & co"),
            Some("Your \"cart\" is empty"),
            Some("../"),
        )
        .unwrap();
        assert!(out.contains("<title>Cart | Shop &amp; co</title>"), "{out}");
        assert!(!out.contains("<title>shop</title>"));
        assert!(out.contains("<style>/* <title>not this</title> */</style>"));
        assert!(out
            .contains("<meta name=\"description\" content=\"Your &quot;cart&quot; is empty\" />"));
        assert!(!out.contains("A frus application"));
        assert!(out.contains("<head>\n    <base href=\"../\" />\n    <meta charset"));
        assert_eq!(out.matches("<base").count(), 1);
        // A page with a base of its own keeps it; a page with no description gets one.
        let based = TEMPLATE
            .replace("<meta charset=\"utf-8\" />", "<base href=\"/shop/\">")
            .replace(
                "<meta name=\"description\" content=\"A frus application\" />",
                "",
            );
        let out = with_head(&based, None, Some("Words"), Some("../../")).unwrap();
        assert!(out.contains("<base href=\"/shop/\">"));
        assert!(!out.contains("../../"));
        assert!(out.contains("<title>shop</title>"));
        assert!(out.contains("<head>\n    <meta name=\"description\" content=\"Words\" />"));
        assert_eq!(with_head("<body></body>", Some("x"), None, None), None);
    }

    /// The page for each location: under its path, never outside the site, never for a
    /// location no file can stand for.
    #[test]
    fn a_location_has_a_page_file_under_the_site() {
        use std::path::PathBuf;
        let file = |parts: &[&str]| parts.iter().collect::<PathBuf>();
        assert_eq!(
            page_file("/settings"),
            Some(file(&["settings", "index.html"]))
        );
        assert_eq!(
            page_file("/products/3/"),
            Some(file(&["products", "3", "index.html"]))
        );
        assert_eq!(
            relative_base(&file(&["products", "3", "index.html"])),
            "../../"
        );
        assert_eq!(relative_base(&file(&["cart", "index.html"])), "../");
        for none in [
            "/",
            "",
            "settings",
            "/a/../b",
            "/a//b",
            "/./a",
            "/search?q=x",
            "/a#b",
            "/c:/x",
            "/a\\b",
        ] {
            assert_eq!(page_file(none), None, "{none}");
        }
    }

    /// A page of many rows is cut at the end of an element, and stays a document.
    #[test]
    fn a_long_document_is_cut_at_an_element() {
        let rows: String = (0..1000).map(|i| format!("<li>Row {i}</li>")).collect();
        let document = format!("<main><h2>Title</h2><ul>{rows}</ul><p>after</p></main>");
        assert_eq!(cap_document(&document, 1 << 20), document);
        let cut = cap_document(&document, 200);
        assert!(cut.len() <= 200 + "</ul></main>".len(), "{cut}");
        assert!(cut.starts_with("<main><h2>Title</h2><ul><li>Row 0</li>"));
        assert!(cut.ends_with("</li></ul></main>"), "{cut}");
        assert!(!cut.contains("after"));
        assert_eq!(cap_document(&document, 10), "<main></main>");
    }

    /// A description is the first words, a title the first heading.
    #[test]
    fn a_summary_reads_the_words() {
        let (words, heading) = summary(
            "<main><h2>Tom &amp; Jerry</h2><p>One<br>two</p><ul><li>three</li></ul></main>",
            155,
        );
        assert_eq!(words, "Tom & Jerry One two three");
        assert_eq!(heading.as_deref(), Some("Tom & Jerry"));
        let (words, heading) = summary("<main><p>alpha beta gamma</p></main>", 11);
        assert_eq!(words, "alpha beta");
        assert_eq!(heading, None);
    }

    /// What the page posts: the first screen with the application's answer, then each
    /// location, then the end.
    #[test]
    fn the_posts_are_read_in_order() {
        let posts = [
            r#"{"kind":"page","path":null,"location":"/","title":"Shop","text":"<main><p>Home</p></main>","strategy":"path","paths":["/","/cart",3]}"#.to_string(),
            "not json".to_string(),
            r#"{"kind":"page","path":"/checkout","location":"/cart","title":"Shop","text":"<main></main>"}"#.to_string(),
            r#"{"kind":"done"}"#.to_string(),
        ];
        let taken = read_posts(&posts);
        assert!(taken.done);
        assert_eq!(taken.strategy, "path");
        assert_eq!(taken.paths, ["/", "/cart"]);
        assert_eq!(taken.shown.len(), 2);
        assert_eq!(taken.shown[0].asked, None);
        assert_eq!(
            taken.shown[0].text.as_deref(),
            Some("<main><p>Home</p></main>")
        );
        assert_eq!(taken.shown[1].asked.as_deref(), Some("/checkout"));
        assert_eq!(taken.shown[1].location, "/cart");
        assert_eq!(taken.shown[1].text, None);
        assert!(!read_posts(&posts[..2]).done);
    }

    fn shown(asked: Option<&str>, location: &str, title: &str, text: &str) -> Shown {
        Shown {
            asked: asked.map(str::to_string),
            location: location.into(),
            title: title.into(),
            text: Some(text.into()).filter(|text: &String| !text.is_empty()),
        }
    }

    /// Each location an application with path addresses lists gets a page of its own with its
    /// text; a redirect, a location the site already has a page at and an application whose
    /// addresses are after a `#` get none.
    #[test]
    fn each_route_gets_its_own_page() {
        let site = std::env::temp_dir().join(format!("frus-routes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&site);
        std::fs::create_dir_all(site.join("kept")).unwrap();
        std::fs::write(site.join("kept").join("index.html"), "mine").unwrap();
        let taken = Taken {
            strategy: "path".into(),
            paths: [
                "/",
                "/cart",
                "/checkout",
                "/kept",
                "/products/3",
                "/search?q=x",
            ]
            .map(String::from)
            .to_vec(),
            shown: vec![
                shown(None, "/", "Shop", "<main><p>Home</p></main>"),
                shown(
                    Some("/cart"),
                    "/cart",
                    "Shop",
                    "<main><h2>Cart</h2><p>Empty</p></main>",
                ),
                shown(
                    Some("/checkout"),
                    "/cart",
                    "Shop",
                    "<main><h2>Cart</h2></main>",
                ),
                shown(Some("/kept"), "/kept", "Shop", "<main><p>Kept</p></main>"),
                shown(
                    Some("/products/3"),
                    "/products/3",
                    "Boots",
                    "<main><p>Boots</p></main>",
                ),
            ],
            done: true,
        };
        write_routes(&site, TEMPLATE, &taken);
        let cart = std::fs::read_to_string(site.join("cart").join("index.html")).unwrap();
        assert!(cart.contains("<base href=\"../\" />"));
        assert!(cart.contains("<title>Cart | Shop</title>"));
        assert!(cart.contains("content=\"Cart Empty\""));
        assert!(
            cart.contains("<main><h2>Cart</h2><p>Empty</p></main></div><!-- /frus:page-text -->")
        );
        let product =
            std::fs::read_to_string(site.join("products").join("3").join("index.html")).unwrap();
        assert!(product.contains("<base href=\"../../\" />"));
        assert!(product.contains("<title>Boots</title>"), "{product}");
        assert!(!site.join("checkout").exists());
        assert_eq!(
            std::fs::read_to_string(site.join("kept").join("index.html")).unwrap(),
            "mine"
        );
        // Addresses after a `#`: the one page only.
        let hashed = std::env::temp_dir().join(format!("frus-routes-h-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&hashed);
        std::fs::create_dir_all(&hashed).unwrap();
        write_routes(
            &hashed,
            TEMPLATE,
            &Taken {
                strategy: "hash".into(),
                ..taken
            },
        );
        assert_eq!(std::fs::read_dir(&hashed).unwrap().count(), 0);
        std::fs::remove_dir_all(&site).unwrap();
        std::fs::remove_dir_all(&hashed).unwrap();
    }
}
