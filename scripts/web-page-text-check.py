#!/usr/bin/env python3
"""Checks, in a real browser, that a frus page has text (milestone 830, #212).

A page drawn into a canvas has no text for the browser: find-in-page finds nothing, a print
is a picture cut at the window, a search engine reads an empty page. The shell now lays a
transparent text layer over the canvas, and writes a plain document for printing and for
`frus build web`. The check, on the demo built with `frus build web`:

- **the served page**, fetched as a search engine does, without running it, carries the
  first screen's words (`frus build web` wrote them in);
- **find-in-page** (`window.find`, which searches what the browser's find bar searches)
  finds a word on screen, and one scrolled out of view, which the application then scrolls
  to: its accessibility tree, which holds only what shows, has the word afterwards;
- **a list that builds only the rows in view** (the journal's 5000 rows): after Ctrl+F,
  row 4321 is found, and the list scrolls to it;
- **printing** (`Page.printToPDF`) gives pages of text: every row of the journal, which the
  PDF's text says.

    pip install websocket-client pypdf
    # build the demo with `frus build web` (crates/frus-demo), serve the site it prints
    # (target/frus-web/frus-demo/), then:
    python3 scripts/web-page-text-check.py http://localhost:8080/index.html

Needs a browser with WebGPU. Set BROWSER to its executable if it is not found.
"""
import base64, importlib.util, io, json, os, shutil, subprocess, sys, tempfile, time, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("a11y", os.path.join(HERE, "web-accessibility-check.py"))
a11y = importlib.util.module_from_spec(spec)
spec.loader.exec_module(a11y)

sys.stdout.reconfigure(encoding="utf-8")
failures = []


def expect(label, got, want):
    ok = got == want
    print(f"  {'ok  ' if ok else 'FAIL'} {label}: {got!r}" + ("" if ok else f"   (wanted {want!r})"))
    if not ok:
        failures.append(label)


def expect_true(label, got, detail=""):
    print(f"  {'ok  ' if got else 'FAIL'} {label}" + (f": {detail}" if detail else ""))
    if not got:
        failures.append(label)


def until(p, expression, seconds=8.0):
    """Waits for `expression` to be truthy in the page, and returns its value."""
    end = time.time() + seconds
    value = None
    while time.time() < end:
        value = p.js(expression)
        if value:
            return value
        p.wait(0.2)
    return value


# Every text of the layer, with whether it shows inside the box of its region (or the layer).
TEXTS = """[...document.querySelectorAll('#frus-text .t')].map((el) => {
  const r = el.getBoundingClientRect();
  const box = el.closest('.s') || document.getElementById('frus-text');
  const b = box.getBoundingClientRect();
  const shows = r.bottom > b.top + 1 && r.top < b.bottom - 1 && r.right > b.left + 1 && r.left < b.right - 1;
  return { text: el.textContent, shows, inRegion: !!el.closest('.s') };
})"""

# The words the accessibility bridge holds: what the application has on screen.
ON_SCREEN = "[...document.querySelectorAll('canvas [aria-label]')].map((el) => el.getAttribute('aria-label'))"


def find(p, word):
    """`window.find`, from the top of the page: what the find bar does with `word`.

    The find bar then scrolls the match into view, through every scrollable box around it;
    `window.find` in a headless browser selects the match and scrolls nothing (measured: no
    box moved, no scroll event). So that step is done here as the bar does it,
    `scrollIntoView` on the match, which moves the region's box and fires its scroll event."""
    found = p.js(f"(() => {{ getSelection().removeAllRanges(); return window.find({json.dumps(word)}, true, false, true); }})()")
    if found:
        p.js("getSelection().getRangeAt(0).startContainer.parentElement"
             ".scrollIntoView({block: 'center', inline: 'nearest'})")
    return found


def ctrl_f(p):
    p.call("Input.dispatchKeyEvent", type="rawKeyDown", key="Control", code="ControlLeft",
           windowsVirtualKeyCode=17, modifiers=2)
    p.call("Input.dispatchKeyEvent", type="rawKeyDown", key="f", code="KeyF",
           windowsVirtualKeyCode=70, modifiers=2)
    p.call("Input.dispatchKeyEvent", type="keyUp", key="f", code="KeyF", windowsVirtualKeyCode=70, modifiers=2)
    p.call("Input.dispatchKeyEvent", type="keyUp", key="Control", code="ControlLeft", windowsVirtualKeyCode=17)


def visible_text(page):
    """The text a crawler reads in `page`: outside comments, styles and scripts."""
    from html.parser import HTMLParser

    class Reader(HTMLParser):
        def __init__(self):
            super().__init__()
            self.skip = 0
            self.words = []

        def handle_starttag(self, tag, attrs):
            if tag in ("style", "script"):
                self.skip += 1

        def handle_endtag(self, tag):
            if tag in ("style", "script"):
                self.skip -= 1

        def handle_data(self, data):
            if not self.skip and data.strip():
                self.words.append(data.strip())

    reader = Reader()
    reader.feed(page)
    return reader.words


def pdf_text(data):
    try:
        import pypdf
    except ImportError:
        return None, 0
    reader = pypdf.PdfReader(io.BytesIO(data))
    return "\n".join(page.extract_text() or "" for page in reader.pages), len(reader.pages)


def main(base):
    root = base.split("#")[0]
    print("the page a search engine fetches")
    served = urllib.request.urlopen(root, timeout=10).read().decode("utf-8")
    expect_true("it carries the first screen's text between the markers",
                "<!-- frus:page-text -->" in served and "<main>" in served,
                f"{served.count('<p>')} paragraphs, {served.count('<h2>')} headings")
    block = served.split("<!-- frus:page-text -->", 1)[-1].split("<!-- /frus:page-text -->", 1)[0]
    written, words = visible_text(block), visible_text(served)
    expect_true("an HTML parser reads that text as the page's (not in a style or a comment)",
                bool(written) and all(w in words for w in written),
                f"{len(written)} texts, e.g. {written[:4]}")
    out = os.environ.get("PAGE_TEXT_OUT")
    if out:
        os.makedirs(out, exist_ok=True)
        with open(os.path.join(out, "served.html"), "w", encoding="utf-8") as f:
            f.write(served)

    profile = tempfile.mkdtemp(prefix="frus-page-text-check-")
    browser = subprocess.Popen([
        a11y.find_browser(), "--headless=new", f"--remote-debugging-port={a11y.PORT}", f"--user-data-dir={profile}",
        "--no-first-run", "--enable-unsafe-webgpu", "--enable-features=Vulkan,WebGPU", "--ignore-gpu-blocklist",
        "--window-size=1000,800", *a11y.FOREGROUND, "about:blank",
    ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(50):
            try:
                urllib.request.urlopen(f"http://127.0.0.1:{a11y.PORT}/json", timeout=2)
                break
            except Exception:
                time.sleep(0.2)
        p = a11y.open_at(root + "#/settings")
        until(p, "document.querySelectorAll('#frus-text .t').length > 3", 20)
        p.wait(1.5)
        expect("the text written in at build time is gone once the layer shows",
               p.js("!!document.getElementById('frus-page-text')"), False)

        print("find-in-page, on screen")
        texts = p.js(TEXTS)
        shown = [t["text"] for t in texts if t["shows"] and len(t["text"]) > 4]
        hidden = [t["text"] for t in texts if not t["shows"] and t["inRegion"] and len(t["text"]) > 4
                  and t["text"] not in shown]
        print(f"  ({len(texts)} texts in the layer, {len(shown)} showing, {len(hidden)} scrolled out of view)")
        expect_true("the layer has texts on screen", bool(shown))
        if shown:
            word = shown[len(shown) // 2]
            expect(f"find reaches {word!r}", find(p, word), True)
            box = p.js("(() => { const r = getSelection().getRangeAt(0).getBoundingClientRect();"
                       " const c = document.querySelector('canvas').getBoundingClientRect();"
                       " return r.width > 0 && r.top >= c.top && r.bottom <= c.bottom; })()")
            expect("its highlight is over the canvas", box, True)

        print("find-in-page, scrolled out of view")
        expect_true("the layer has texts scrolled out of view", bool(hidden))
        if hidden:
            word = hidden[-1]
            before = p.js(ON_SCREEN)
            expect(f"{word!r} is not on screen before", word in before, False)
            expect(f"find reaches {word!r}", find(p, word), True)
            on = until(p, f"({ON_SCREEN}).includes({json.dumps(word)})", 4)
            expect("the application scrolled to it: it is on screen", bool(on), True)

        print("a list of 5000 rows, of which a dozen are built")
        p.js("location.hash = '#/journal'")
        until(p, "[...document.querySelectorAll('#frus-text .t')].some((el) => el.textContent === 'Row 1')", 10)
        p.wait(1)
        rows = p.js("document.querySelectorAll('#frus-text .t').length")
        print(f"  ({rows} texts in the layer before the find)")
        expect("row 4321 is not in the page before a find", find(p, "Row 4321"), False)
        ctrl_f(p)
        laid = until(p, "document.querySelectorAll('#frus-text .t').length > 5000", 8)
        expect_true("Ctrl+F lays every row in the page", bool(laid),
                    f"{p.js('document.querySelectorAll(`#frus-text .t`).length')} texts")
        cost = p.js("window.__frusPageTextCost && window.__frusPageTextCost()")
        if cost:
            print(f"  (cost: {int(cost[0])} frames, {cost[1]:.1f} ms in all, longest {cost[2]:.2f} ms,"
                  f" laying the rows in {cost[3]:.1f} ms)")
        expect("find reaches row 4321", find(p, "Row 4321"), True)
        on = until(p, f"({ON_SCREEN}).includes('Row 4321')", 4)
        expect("the list scrolled to it: it is built and on screen", bool(on), True)

        print("printing")
        pdf = p.call("Page.printToPDF", printBackground=False)
        data = base64.b64decode(pdf["data"])
        if out:
            with open(os.path.join(out, "journal.pdf"), "wb") as f:
                f.write(data)
        text, pages = pdf_text(data)
        if text is None:
            print("  (pypdf is not installed: the PDF's text is not read)")
        else:
            print(f"  ({pages} pages, {len(text)} characters)")
            expect_true("the PDF has many pages", pages > 20, f"{pages}")
            for row in ("Row 1", "Row 2500", "Row 5000"):
                expect_true(f"the PDF has {row!r}", f"{row}\n" in text + "\n")
        expect("the printed document is gone after the print",
               p.js("!!document.getElementById('frus-print')"), False)
        expect("(the page stayed in the foreground)", p.js("document.visibilityState"), "visible")
    finally:
        subprocess.run(["taskkill", "/F", "/T", "/PID", str(browser.pid)], capture_output=True)
        browser.kill()
        shutil.rmtree(profile, ignore_errors=True)
    print("FAILED: " + ", ".join(failures) if failures else "all good")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "http://localhost:8080/index.html"))
