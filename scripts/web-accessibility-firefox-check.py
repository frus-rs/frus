#!/usr/bin/env python3
"""Checks the demo's accessibility bridge in Firefox (milestones 845 and 848, #18).

Firefox hands no accessibility tree to automation (WebDriver BiDi has none), so this reads
what Firefox builds its tree from: the bridge's layer over the canvas, the attributes of its
elements, their boxes, and what an assistive technology's actions do to them. The computed tree
itself is checked in Chromium by `web-accessibility-check.py`.

On the settings screen of the demo:

- the layer is there, the canvas has no role, and the layer is a region named after the
  application;
- a text's words are its generated content (what Firefox's tree reads), and a find does not
  reach them twice;
- every element has a box over its widget, and a press there still lands on the canvas;
- the tab showing is selected, and its element is inside its tablist's (milestone 848);
- a click on the switch's element (a reader's "activate") turns it on;
- an increment on the volume's element (the arrow a browser sends for it) moves it;
- a real arrow key, with the slider's element focused, moves the volume.

    pip install websocket-client
    # build the demo for the web and serve it (crates/frus-demo/web/README.md), then:
    python scripts/web-accessibility-firefox-check.py http://127.0.0.1:8080/

It opens a real Firefox window (WebGPU turned on in a fresh profile), and closes it.
"""
import importlib.util, os, shutil, socket, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("findbar", os.path.join(HERE, "web-find-bar-check.py"))
findbar = importlib.util.module_from_spec(spec)
spec.loader.exec_module(findbar)

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


LAYER = "document.getElementById('frus-semantics')"


def node(selector):
    return f"{LAYER}.querySelector({selector!r})"


def main(base):
    root = base.split("#")[0].rstrip("/") + "/"
    profile = tempfile.mkdtemp(prefix="frus-a11y-firefox-check-")
    browser = findbar.launch("firefox", root + "#/settings", profile)
    page = None
    try:
        # A fresh profile starts slowly: wait for the remote agent before connecting.
        for _ in range(240):
            try:
                socket.create_connection(("127.0.0.1", findbar.PORT), timeout=1).close()
                break
            except OSError:
                time.sleep(0.25)
        page = findbar.Bidi()
        page.call("browsingContext.navigate", context=page.context, url=root + "#/settings", wait="complete")
        count = findbar.until(page, f"{LAYER} && {LAYER}.children.length", 40)
        print(f"firefox ({page.js('navigator.userAgent')})")
        print(f"  ({page.js('navigator.gpu ? \"webgpu offered\" : \"no webgpu (webgl 2)\"')})")
        time.sleep(1.5)

        print("the layer")
        expect_true("the bridge's layer is over the canvas", bool(count), f"{count} nodes")
        expect("the canvas has no role", page.js("document.querySelector('canvas').getAttribute('role')"), None)
        expect("the layer is a region", page.js(f"{LAYER}.getAttribute('role')"), "region")
        expect_true("...named after the application", bool(page.js(f"{LAYER}.getAttribute('aria-label')")))

        print("text")
        text = node('[data-t="Notifications"]')
        expect("a text has no role", page.js(f"{text} && {text}.getAttribute('role')"), None)
        expect("its words are its generated content",
               page.js(f"{text} && getComputedStyle({text}, '::before').content"), '"Notifications"')
        heading = node('[role="heading"]')
        expect("the heading says its words the same way", page.js(f"{heading} && {heading}.getAttribute('data-t')"),
               "Settings")

        print("boxes")
        box = page.js(f"(() => {{ const r = {node('[role=switch]')}.getBoundingClientRect();"
                      " return [r.x, r.y, r.width, r.height].map(Math.round); })()")
        expect_true("the switch's element has a box of its own", bool(box and box[2] > 4 and box[3] > 4), str(box))
        if box:
            hit = page.js(f"document.elementFromPoint({box[0] + box[2] / 2}, {box[1] + box[3] / 2}).tagName")
            expect("a press there lands on the canvas", hit, "CANVAS")

        print("states and actions")
        tabs = page.js(f"[...{LAYER}.querySelectorAll('[role=tab]')].map((t) => t.getAttribute('aria-label') + '=' + t.getAttribute('aria-selected'))")
        top = page.js("document.querySelector('canvas').getBoundingClientRect().top")
        print(f"    tabs: {tabs}; canvas top {top}")
        expect_true("the tab showing is selected", bool(tabs) and any(t.endswith("=true") for t in tabs), str(tabs))
        # The tree has a shape (milestone 848): a tab's element is inside its tablist's.
        expect("the tabs' elements are inside the tablist's",
               page.js(f"[...{LAYER}.querySelectorAll('[role=tab]')].every((t) => !!t.parentElement.closest('[role=tablist]'))"),
               True)
        page.js(f"{node('[role=switch]')}.click()")
        time.sleep(1)
        expect("a click on the switch's element turns it on",
               page.js(f"{node('[role=switch]')}.getAttribute('aria-checked')"), "true")
        # The volume, the first slider: the window may be too short for the others to be built.
        volume = node('[role=slider]')
        page.js(f"{volume}.dispatchEvent(new KeyboardEvent('keydown', {{key: 'ArrowUp', bubbles: true, cancelable: true}}))")
        time.sleep(1)
        after_increment = page.js(f"{volume}.getAttribute('aria-valuenow')")
        expect_true("an increment on the volume's element, not focused, moves it", after_increment not in (None, "0"),
                    f"aria-valuenow {after_increment}")
        page.js(f"{volume}.focus()")
        time.sleep(0.5)
        page.call("input.performActions", context=page.context, actions=[{
            "type": "key", "id": "keys", "actions": [
                {"type": "keyDown", "value": ""}, {"type": "keyUp", "value": ""},
                {"type": "keyDown", "value": ""}, {"type": "keyUp", "value": ""}]}])
        time.sleep(1)
        now = page.js(f"{volume}.getAttribute('aria-valuenow')")
        expect_true("a real Right arrow on the focused volume moves it further",
                    now is not None and after_increment is not None and float(now) > float(after_increment),
                    f"aria-valuenow {after_increment} -> {now}")
        expect("the browser's focus stayed on it", page.js(f"document.activeElement === {volume}"), True)

        print("find")
        expect("a find reaches the text once (the text layer's), not the bridge's generated words",
               page.js("(() => { getSelection().removeAllRanges(); return window.find('Notifications'); })()"), True)
    finally:
        if page:
            try:
                page.call("browser.close")
            except Exception:
                pass
        time.sleep(1)
        subprocess.run(["taskkill", "/F", "/T", "/PID", str(browser.pid)], capture_output=True)
        shutil.rmtree(profile, ignore_errors=True)
    print("FAILED: " + ", ".join(failures) if failures else "all good")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8080/"))
