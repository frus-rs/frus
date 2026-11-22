#!/usr/bin/env python3
"""Checks the pages `frus build web` writes for each route (milestone 833).

On an application whose addresses are paths (the navigation example), built with
`frus build web` and served from the site it prints (a plain static server, which answers only
the addresses that have a file):

- **fetched as a search engine does**, without running anything, each route's page carries
  that route's words, its own title and description, and a `<base>` back to the site's root;
  the redirecting `/checkout` has no page;
- **opened in a browser**, a route's page starts the application at that address (its
  relative URLs resolve from the root, the router reads the path), and the text written in at
  build time gives way to the live layer.

    pip install websocket-client
    # in frus-examples/navigation: frus build web, then serve target/frus-web/navigation/:
    python scripts/web-routes-check.py http://localhost:8080/
"""
import html.parser, importlib.util, os, sys, time, urllib.error, urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("a11y", os.path.join(HERE, "web-accessibility-check.py"))
a11y = importlib.util.module_from_spec(spec)
spec.loader.exec_module(a11y)
sys.stdout.reconfigure(encoding="utf-8")
failures = []


def expect_true(label, got, detail=""):
    print(f"  {'ok  ' if got else 'FAIL'} {label}" + (f": {detail}" if detail else ""))
    if not got:
        failures.append(label)


class Head(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.title, self.description, self.base, self._in_title = "", None, None, False

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "title":
            self._in_title = True
        elif tag == "meta" and attrs.get("name") == "description":
            self.description = attrs.get("content")
        elif tag == "base":
            self.base = attrs.get("href")

    def handle_endtag(self, tag):
        if tag == "title":
            self._in_title = False

    def handle_data(self, data):
        if self._in_title:
            self.title += data


def fetch(url):
    try:
        return urllib.request.urlopen(url, timeout=10).read().decode("utf-8")
    except urllib.error.HTTPError as e:
        return e.code


def main(root):
    root = root.rstrip("/") + "/"
    print("the pages a search engine fetches")
    home = fetch(root)
    for path, words in [("cart/", "Your cart is empty"), ("search/", "Search"), ("products/3/", "Road racer")]:
        page = fetch(root + path)
        if not isinstance(page, str):
            expect_true(f"/{path} has a page", False, f"HTTP {page}")
            continue
        head = Head()
        head.feed(page)
        block = page.split("<!-- frus:page-text -->", 1)[-1].split("<!-- /frus:page-text -->", 1)[0]
        expect_true(f"/{path} carries its own words", words in block and block != page, repr(words))
        expect_true(f"/{path} has its own title and description",
                    bool(head.title) and bool(head.description) and words.split()[0] in head.description,
                    f"{head.title!r}, {head.description!r}")
        expect_true(f"/{path} resolves its URLs from the root", head.base == "../" * path.count("/"), repr(head.base))
    expect_true("/checkout, which redirects, has no page", fetch(root + "checkout/") == 404)
    expect_true("the site's own page keeps its first screen", "Corner Shop" in home or "Today" in home)

    print("a route's page, opened in a browser")
    import subprocess, tempfile
    profile = tempfile.mkdtemp(prefix="frus-routes-check-")
    browser = subprocess.Popen([
        a11y.find_browser(), "--headless=new", f"--remote-debugging-port={a11y.PORT}", f"--user-data-dir={profile}",
        "--no-first-run", "--enable-unsafe-webgpu", "--ignore-gpu-blocklist", "--window-size=1000,800",
        *a11y.FOREGROUND, "about:blank",
    ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(50):
            try:
                urllib.request.urlopen(f"http://127.0.0.1:{a11y.PORT}/json", timeout=2)
                break
            except Exception:
                time.sleep(0.2)
        p = a11y.open_at(root + "products/3/")
        layer = 0
        for _ in range(80):
            time.sleep(0.25)
            layer = p.js("document.querySelectorAll('#frus-text .t').length") or 0
            if layer > 3:
                break
        expect_true("the application starts there", layer > 3, f"{layer} texts in the layer")
        time.sleep(1.0)
        words = p.js("[...document.querySelectorAll('#frus-text .t')].map((el) => el.textContent)") or []
        expect_true("it opens at /products/3", "Road racer" in words, ", ".join(words[:6]))
        expect_true("the address is the route's", p.js("location.pathname") in ("/products/3", "/products/3/"),
                    p.js("location.pathname"))
        expect_true("the text written in at build time gave way to the layer",
                    not p.js("!!document.getElementById('frus-page-text')"))
    finally:
        subprocess.run(["taskkill", "/F", "/T", "/PID", str(browser.pid)], capture_output=True)
        browser.kill()
    print("FAILED: " + ", ".join(failures) if failures else "all good")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "http://localhost:8080/"))
