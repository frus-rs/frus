#!/usr/bin/env python3
"""Checks the demo's accessibility bridge in a real browser (milestones 564 and 845, #18).

What no unit test can say: that the DOM the shell projects (`a11y_web.rs`) is what
Chromium's own accessibility engine actually computes, and that the loop closes both
ways — a widget's semantics reach the accessibility tree, and an action taken *through*
that tree (focus, then the browser's own Enter-activates-a-button behaviour) reaches
the widget back. Driven over the DevTools protocol's Accessibility and DOM domains,
which is what a screen reader itself reads from and writes to; this is not a heavier
DOM query, it is the same computed tree NVDA, JAWS and Narrator consume.

    pip install websocket-client
    # build the demo for the web and serve it (crates/frus-demo/web/README.md), then:
    python3 scripts/web-accessibility-check.py http://127.0.0.1:8080/index.html

Needs a browser with WebGPU. Set BROWSER to its executable if it is not found.
"""
import json, os, shutil, subprocess, sys, tempfile, time, urllib.request

import websocket

PORT = 9334
# A headless page is sometimes put in the background — hidden, its frames stopped — and a
# key the shell handles then shows nothing until a frame that never comes. Every check
# starts its browser with these, and `open_at` turns focus emulation on.
FOREGROUND = [
    "--disable-renderer-backgrounding", "--disable-background-timer-throttling",
    "--disable-backgrounding-occluded-windows",
]
CANDIDATES = [
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    "microsoft-edge", "google-chrome", "chromium", "chrome",
]


def find_browser():
    if os.environ.get("BROWSER"):
        return os.environ["BROWSER"]
    for candidate in CANDIDATES:
        if os.path.exists(candidate) or shutil.which(candidate):
            return candidate
    sys.exit("no browser found: set BROWSER")


class Page:
    def __init__(self, ws_url):
        self.ws = websocket.create_connection(ws_url, timeout=30, suppress_origin=True)
        self.n = 0
        self.call("Runtime.enable")
        self.call("Page.enable")
        self.call("DOM.enable")
        self.call("Accessibility.enable")

    def call(self, method, **params):
        self.n += 1
        self.ws.send(json.dumps({"id": self.n, "method": method, "params": params}))
        while True:
            msg = json.loads(self.ws.recv())
            if msg.get("id") == self.n:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})

    def js(self, expression):
        return self.call("Runtime.evaluate", expression=expression, returnByValue=True)["result"].get("value")

    def wait(self, seconds):
        time.sleep(seconds)

    def click(self, x, y):
        for kind in ("mousePressed", "mouseReleased"):
            self.call("Input.dispatchMouseEvent", type=kind, x=x, y=y, button="left", clickCount=1)

    def type(self, text):
        for ch in text:
            self.call("Input.dispatchKeyEvent", type="keyDown", key=ch, text=ch)
            self.call("Input.dispatchKeyEvent", type="keyUp", key=ch)
        self.call("Input.dispatchKeyEvent", type="keyDown", key="Enter", code="Enter", windowsVirtualKeyCode=13, text="\r")
        self.call("Input.dispatchKeyEvent", type="keyUp", key="Enter", code="Enter", windowsVirtualKeyCode=13)

    def press(self, key, code, vk):
        self.call("Input.dispatchKeyEvent", type="rawKeyDown", key=key, code=code, windowsVirtualKeyCode=vk)
        self.call("Input.dispatchKeyEvent", type="keyUp", key=key, code=code, windowsVirtualKeyCode=vk)

    def activate(self):
        """A real Enter on the focused element: a keydown that **carries its character**, which is
        what makes a browser activate a button. A bare `rawKeyDown` does not, and only ever
        looked like it worked because the shell also read the key itself."""
        self.call("Input.dispatchKeyEvent", type="keyDown", key="Enter", code="Enter",
                  windowsVirtualKeyCode=13, text="\r")
        self.call("Input.dispatchKeyEvent", type="keyUp", key="Enter", code="Enter", windowsVirtualKeyCode=13)

    def ax_tree(self):
        """Every node the browser's accessibility engine currently computes for the page."""
        return self.call("Accessibility.getFullAXTree")["nodes"]

    def find(self, tree, role=None, name_contains=None):
        for node in tree:
            if node.get("ignored"):
                continue
            r = node.get("role", {}).get("value")
            nm = node.get("name", {}).get("value", "")
            if role is not None and r != role:
                continue
            if name_contains is not None and name_contains not in nm:
                continue
            return node
        return None

    def prop(self, node, name):
        for p in node.get("properties", []):
            if p.get("name") == name:
                return p.get("value", {}).get("value")
        return None

    def focus_backend_node(self, backend_node_id):
        self.call("DOM.focus", backendNodeId=backend_node_id)


def open_at(url, user_agent=None, platform=None):
    """A new tab on `url`, in the foreground. With `user_agent` (and `platform`, for
    `navigator.platform`), the page is told it runs somewhere else before it loads."""
    tab = json.load(urllib.request.urlopen(urllib.request.Request(
        f"http://127.0.0.1:{PORT}/json/new?about:blank", method="PUT")))
    page = Page(tab["webSocketDebuggerUrl"])
    page.call("Emulation.setFocusEmulationEnabled", enabled=True)
    if user_agent:
        page.call("Emulation.setUserAgentOverride", userAgent=user_agent, platform=platform or "")
    page.call("Page.navigate", url=url)
    page.call("Page.bringToFront")
    return page


def close_browser():
    """Closes the browser listening on `PORT` through the protocol itself. On Windows the process
    `Popen` started is only a launcher: the browser outlives it, and killing it, its tree
    included, leaves the browser serving the port, where the next check's tab then opens."""
    try:
        version = json.load(urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json/version", timeout=2))
        ws = websocket.create_connection(version["webSocketDebuggerUrl"], timeout=5, suppress_origin=True)
        ws.send(json.dumps({"id": 1, "method": "Browser.close"}))
        ws.close()
    except Exception:
        pass


failures = []


def expect(label, got, want):
    ok = got == want
    print(f"  {'ok  ' if ok else 'FAIL'} {label}: {got}" + ("" if ok else f"   (wanted {want})"))
    if not ok:
        failures.append(label)


def expect_true(label, got):
    print(f"  {'ok  ' if got else 'FAIL'} {label}")
    if not got:
        failures.append(label)


def named(tree, role, name):
    """The first node of `role` whose name contains `name`, ignored ones left out."""
    for node in tree:
        if node.get("ignored"):
            continue
        if node.get("role", {}).get("value") == role and name in node.get("name", {}).get("value", ""):
            return node
    return None


def index_of(tree, node):
    if not node:
        return -1
    return next((i for i, n in enumerate(tree) if n["nodeId"] == node["nodeId"]), -1)


def box(p, node):
    """The node's box on the page, `(left, top, width, height)` in CSS pixels, or `None` when the
    browser laid it out nowhere: what a reader touching the screen, or drawing its highlight
    around the node, has to go on."""
    try:
        quad = p.call("DOM.getBoxModel", backendNodeId=node["backendDOMNodeId"])["model"]["border"]
    except Exception:
        return None
    return (quad[0], quad[1], quad[2] - quad[0], quad[5] - quad[1])


def on_node(p, node, body):
    """Runs `function () { body }` with `this` the node's element: what an assistive technology
    does to an element (a click for "activate", an arrow for "increment"), done from here."""
    obj = p.call("DOM.resolveNode", backendNodeId=node["backendDOMNodeId"])["object"]["objectId"]
    return p.call("Runtime.callFunctionOn", objectId=obj, functionDeclaration="function () { " + body + " }",
                  returnByValue=True)["result"].get("value")


def value(node):
    return node.get("value", {}).get("value") if node else None


def sliders(p):
    return [n for n in p.ax_tree() if not n.get("ignored") and n.get("role", {}).get("value") == "slider"]


def show(label, node, p=None):
    """One line of evidence: the node as the accessibility tree has it."""
    if not node:
        print(f"    {label}: (none)")
        return
    props = {q["name"]: q.get("value", {}).get("value") for q in node.get("properties", [])}
    keep = {k: v for k, v in props.items()
            if k not in ("invalid", "settable", "readonly", "required", "multiline", "focusable")}
    extra = f" value={value(node)!r}" if "value" in node else ""
    at = box(p, node) if p else None
    where = f" box={tuple(round(v) for v in at)}" if at else ""
    print(f"    {label}: {node['role']['value']} {node.get('name', {}).get('value', '')!r} {keep}{extra}{where}")


def main(base):
    profile = tempfile.mkdtemp(prefix="frus-a11y-check-")
    browser = subprocess.Popen([
        find_browser(), "--headless=new", f"--remote-debugging-port={PORT}", f"--user-data-dir={profile}",
        "--no-first-run", "--enable-unsafe-webgpu", "--enable-features=Vulkan,WebGPU", "--ignore-gpu-blocklist",
        "--force-renderer-accessibility", "--window-size=420,800", *FOREGROUND, "about:blank",
    ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        for _ in range(50):
            try:
                urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json", timeout=2)
                break
            except Exception:
                time.sleep(0.2)

        print("the page is read as a page: a landmark named after the application")
        p = open_at(base)
        p.wait(7)
        tree = p.ax_tree()
        expect("no role=application (a reader browsing the page skipped every text in it)",
               p.find(tree, role="application"), None)
        region = p.find(tree, role="region")
        show("landmark", region)
        expect_true("a region landmark named after the application",
                    bool(region and region.get("name", {}).get("value")))

        print("text is text, and a tab says which one is showing")
        words = named(tree, "StaticText", "Nothing to show")
        show("empty-state text", words)
        expect_true("a text node is read as text (StaticText)", words is not None)
        expect("...not as an element with no role and a name", named(tree, "generic", "Nothing to show"), None)
        show("heading", named(tree, "heading", ""))
        tab = named(tree, "tab", "Tasks")
        show("tab", tab)
        expect("the tab showing is selected", p.prop(tab, "selected") if tab else None, True)
        chips = [n for n in tree if not n.get("ignored") and n.get("role", {}).get("value") == "button"
                 and p.prop(n, "pressed") is not None]
        show("filter chip", chips[0] if chips else None)
        expect_true("a filter chip is a button that says whether it is pressed", bool(chips))
        progress = p.find(tree, role="progressbar")
        show("progress", progress)
        expect_true("a progress of nothing out of nothing says no 'NaN'",
                    bool(progress) and "NaN" not in json.dumps(progress))
        unnamed = [n for n in tree if not n.get("ignored") and n.get("role", {}).get("value") == "image"
                   and not n.get("name", {}).get("value")]
        expect("unnamed images (each read as just 'image')", len(unnamed), 0)

        print("every node has a box over the place it is drawn")
        menu = p.find(tree, role="button", name_contains="Menu")
        show("menu", menu, p)
        b = box(p, menu) if menu else None
        expect_true("the 'Menu' button has a laid-out box of its own", bool(b and b[2] > 4 and b[3] > 4))
        add = p.find(tree, role="button", name_contains="Add")
        b2 = box(p, add) if add else None
        show("add", add, p)
        expect_true("'Add' has a box too, elsewhere", bool(b2 and b and b2 != b))
        if b:
            hit = p.js(f"(() => {{ const e = document.elementFromPoint({b[0] + b[2] / 2}, {b[1] + b[3] / 2});"
                       " return e && e.tagName; })()")
            expect("a press there still lands on the canvas (the layer takes no pointer)", hit, "CANVAS")

        print("adding a task announces it and names the checkbox after it")
        field = p.find(p.ax_tree(), role="textbox")
        expect_true("the add field is exposed as a textbox", field is not None)
        if field:
            p.focus_backend_node(field["backendDOMNodeId"]); p.wait(0.5)
        p.type("Wash the dog"); p.wait(2)
        tree = p.ax_tree()
        check = p.find(tree, role="checkbox", name_contains="Wash the dog")
        show("checkbox", check, p)
        expect_true("a checkbox named after the new task exists", check is not None)
        if check:
            expect("...unchecked to start", p.prop(check, "checked"), "false")
        clear = p.find(tree, role="button", name_contains="Clear completed")
        expect_true("...read where the list has it, before 'Clear completed', not appended last",
                    0 <= index_of(tree, check) < index_of(tree, clear))
        live = p.find(tree, name_contains="Wash the dog added")
        status = p.find(tree, role="status")
        show("live region", status, p)
        show("its words", live)
        expect_true("a live region carries the 'added' announcement", live is not None)
        expect("...politely", p.prop(status, "live") if status else None, "polite")
        expect_true("...and is rendered (it has a box): the live machinery watches what is",
                    bool(status and box(p, status)))

        print("focusing and activating the checkbox through the accessibility tree ticks it")
        if check:
            p.focus_backend_node(check["backendDOMNodeId"])
            p.wait(0.3)
            p.activate()
            p.wait(1)
        tree = p.ax_tree()
        check = p.find(tree, role="checkbox", name_contains="Wash the dog")
        expect_true("the checkbox is still there after activation", check is not None)
        if check:
            expect("...and is now checked", p.prop(check, "checked"), "true")

        print("the browser's focus follows the application's")
        label = "document.activeElement && (document.activeElement.getAttribute('aria-label') || '')"
        before = p.js(label)
        p.press("Tab", "Tab", 9); p.wait(0.6)
        in_layer = p.js("!!(document.activeElement && document.activeElement.parentElement"
                        " && document.activeElement.parentElement.id === 'frus-semantics')")
        after = p.js(label)
        print(f"    focus: {before!r} -> Tab -> {after!r}")
        expect_true("Tab, pressed on a node, moved the focus to another node of the layer",
                    in_layer and after != before)
        focused = [n for n in p.ax_tree() if not n.get("ignored") and p.prop(n, "focused")
                   and n.get("role", {}).get("value") != "RootWebArea"]
        show("focused in the accessibility tree", focused[-1] if focused else None)
        expect_true("...and the accessibility tree has that node focused",
                    bool(focused and focused[-1].get("name", {}).get("value") == after))

        print("a delete button is reachable and named for what it does")
        expect_true("a 'Delete task' button exists for the row",
                    p.find(p.ax_tree(), role="button", name_contains="Delete task") is not None)

        print("frames that change nothing write nothing to the layer")
        p.js("window.__mut = 0; new MutationObserver((m) => { window.__mut += m.length; })"
             ".observe(document.getElementById('frus-semantics'),"
             " {subtree: true, attributes: true, childList: true, characterData: true})")
        for i in range(30):
            p.call("Input.dispatchMouseEvent", type="mouseMoved", x=20 + i * 10, y=300 + (i % 5) * 20)
            time.sleep(0.03)
        p.wait(0.5)
        expect("mutations of the layer over 30 hover frames", p.js("window.__mut"), 0)

        print("settings: a switch, a radio, a slider, an assistive technology's increment")
        s = open_at(base.split("#")[0] + "#/settings")
        s.wait(7)
        tree = s.ax_tree()
        show("heading", named(tree, "heading", "Settings"), s)
        expect_true("'Settings' is a heading", named(tree, "heading", "Settings") is not None)
        controls = named(tree, "tab", "Controls")
        expect("the 'Controls' tab is selected", s.prop(controls, "selected") if controls else None, True)
        switch = s.find(tree, role="switch")
        show("switch", switch, s)
        if switch:
            on_node(s, switch, "this.click()")
            s.wait(1)
            switch = s.find(s.ax_tree(), role="switch")
            show("switch after a reader's click", switch)
            expect("a reader's click turns the switch on", s.prop(switch, "checked"), "true")
        medium = named(s.ax_tree(), "radio", "Medium")
        if medium:
            on_node(s, medium, "this.click()")
            s.wait(1)
            medium = named(s.ax_tree(), "radio", "Medium")
            expect("a reader's click picks 'Medium'", s.prop(medium, "checked"), "true")
        found = sliders(s)
        volume = found[0] if found else None
        show("volume", volume, s)
        if volume:
            s.focus_backend_node(volume["backendDOMNodeId"]); s.wait(0.3)
            for _ in range(2):
                s.press("ArrowRight", "ArrowRight", 39)
                s.wait(0.3)
            s.wait(0.5)
            volume = sliders(s)[0]
            show("volume after ArrowRight twice", volume)
            expect_true("the arrows on the focused slider moved it", (value(volume) or 0) > 0)

        def quantity_and_less():
            tree = s.ax_tree()
            q = next((n for n in tree if not n.get("ignored") and n.get("role", {}).get("value") == "slider"
                      and s.prop(n, "valuemax") == 20), None)
            less = named(tree, "button", "Moins") or named(tree, "button", "Less")
            return q, less

        quantity, less = quantity_and_less()
        show("quantity", quantity)
        show("less", less)
        expect("'Less' is disabled at 0", s.prop(less, "disabled") if less else None, True)
        if quantity:
            # An assistive technology's "increment" on a slider it has not focused: what a
            # browser sends for it is the arrow, on the element.
            on_node(s, quantity, "this.dispatchEvent(new KeyboardEvent('keydown',"
                                 " {key: 'ArrowUp', bubbles: true, cancelable: true}))")
            s.wait(1)
            quantity, less = quantity_and_less()
            show("quantity after an increment", quantity)
            expect("an increment, on a slider without the focus, moved it to 1", value(quantity), 1)
            expect("...and 'Less' is no longer disabled", s.prop(less, "disabled") if less else None, None)
        show("dropdown", named(s.ax_tree(), "button", "Option A"))
    finally:
        close_browser()
        if os.name == "nt":
            subprocess.run(["taskkill", "/PID", str(browser.pid), "/T", "/F"], capture_output=True)
        browser.kill()
        shutil.rmtree(profile, ignore_errors=True)
    print("FAILED: " + ", ".join(failures) if failures else "all good")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8080/index.html"))
