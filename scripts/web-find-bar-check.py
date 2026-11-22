#!/usr/bin/env python3
"""Checks the browser's **real find bar** on a frus page, on Windows (milestone 833).

`web-page-text-check.py` drives a headless browser, where `window.find` selects a match but
scrolls nothing, so it brings the match on screen itself. This one opens a real, windowed
browser, presses Ctrl+F on the keyboard and types the words into the browser's own find bar,
the way a person does, and then reads the page:

- a word that is **only in a row scrolled out of view** is found, the application scrolls to
  it (its accessibility tree, which holds only what shows, has the word afterwards), and the
  find highlight is on screen;
- a screenshot of the window is taken before and after.

Keys go to the browser's window only: it is found by its process id, kept on top while the
keys are sent, and every key is refused unless that window is the one in front.

    pip install websocket-client pillow
    # build the demo with `frus build web` (crates/frus-demo), serve target/frus-web/frus-demo/:
    python scripts/web-find-bar-check.py http://localhost:8080/ edge   [out-dir]
    python scripts/web-find-bar-check.py http://localhost:8080/ firefox [out-dir]

Edge and Chrome are read over the DevTools protocol, Firefox over WebDriver BiDi.
"""
import ctypes, ctypes.wintypes as wt, json, os, shutil, subprocess, sys, tempfile, time, urllib.request

import websocket
from PIL import ImageGrab

sys.stdout.reconfigure(encoding="utf-8")

BROWSERS = {
    "edge": [r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
             r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"],
    "chrome": [r"C:\Program Files\Google\Chrome\Application\chrome.exe",
               r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"],
    "firefox": [r"C:\Program Files\Mozilla Firefox\firefox.exe"],
}
PORT = 9339
WORD = os.environ.get("FIND_WORD", "Vibrate 400")
# The screen to search: the settings (a scroll region), or the journal (`FIND_ROUTE=/journal
# FIND_WORD="Row 4321"`: a lazy list of 5000 rows whose unbuilt rows Ctrl+F lays in).
ROUTE = os.environ.get("FIND_ROUTE", "/settings")
# The words the accessibility bridge holds: what the application has on screen.
ON_SCREEN = "[...document.querySelectorAll('canvas [aria-label]')].map((el) => el.getAttribute('aria-label'))"
# How far each region's box in the text layer is scrolled.
BOXES = "[...document.querySelectorAll('#frus-text .s')].map((el) => Math.round(el.scrollTop))"

failures = []


def expect_true(label, got, detail=""):
    print(f"  {'ok  ' if got else 'FAIL'} {label}" + (f": {detail}" if detail else ""))
    if not got:
        failures.append(label)


# ---- Windows: the window, in front, and keys for it alone -------------------------------

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
try:
    ctypes.WinDLL("shcore").SetProcessDpiAwareness(2)
except Exception:
    user32.SetProcessDPIAware()

ULONG_PTR = ctypes.c_size_t


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", wt.WORD), ("wScan", wt.WORD), ("dwFlags", wt.DWORD), ("time", wt.DWORD),
                ("dwExtraInfo", ULONG_PTR)]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [("dx", wt.LONG), ("dy", wt.LONG), ("mouseData", wt.DWORD), ("dwFlags", wt.DWORD),
                ("time", wt.DWORD), ("dwExtraInfo", ULONG_PTR)]


class INPUT_UNION(ctypes.Union):
    _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT), ("pad", ctypes.c_byte * 32)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wt.DWORD), ("u", INPUT_UNION)]


KEYEVENTF_KEYUP, KEYEVENTF_UNICODE = 0x2, 0x4
VK_CONTROL, VK_RETURN, VK_ESCAPE = 0x11, 0x0D, 0x1B
HWND_TOPMOST, HWND_NOTOPMOST = wt.HWND(-1), wt.HWND(-2)
SWP_NOMOVE, SWP_NOSIZE = 0x2, 0x1
user32.GetForegroundWindow.restype = wt.HWND
user32.SetWindowPos.argtypes = [wt.HWND, wt.HWND, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_int, wt.UINT]


def windows_of(pids):
    """The visible, titled top-level windows of the processes `pids`."""
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def each(hwnd, _):
        pid = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if pid.value in pids and user32.IsWindowVisible(hwnd):
            n = user32.GetWindowTextLengthW(hwnd)
            if n:
                buf = ctypes.create_unicode_buffer(n + 1)
                user32.GetWindowTextW(hwnd, buf, n + 1)
                found.append((hwnd, buf.value))
        return True

    user32.EnumWindows(each, 0)
    return found


def tree_pids(root):
    """`root` and every process under it."""
    out = subprocess.run(["powershell", "-NoProfile", "-Command",
                          "Get-CimInstance Win32_Process | ForEach-Object { \"$($_.ParentProcessId),$($_.ProcessId)\" }"],
                         capture_output=True, text=True).stdout
    children = {}
    for line in out.splitlines():
        parts = line.strip().split(",")
        if len(parts) == 2 and parts[0].isdigit() and parts[1].isdigit():
            children.setdefault(int(parts[0]), []).append(int(parts[1]))
    pids, todo = set(), [root]
    while todo:
        pid = todo.pop()
        if pid not in pids:
            pids.add(pid)
            todo.extend(children.get(pid, []))
    return pids


def bring_to_front(hwnd):
    user32.ShowWindow(hwnd, 9)  # SW_RESTORE
    user32.SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE)
    fg = user32.GetForegroundWindow()
    fg_thread = user32.GetWindowThreadProcessId(fg, None)
    me = kernel32.GetCurrentThreadId()
    user32.AttachThreadInput(me, fg_thread, True)
    user32.BringWindowToTop(hwnd)
    user32.SetForegroundWindow(hwnd)
    user32.AttachThreadInput(me, fg_thread, False)
    time.sleep(0.4)
    return user32.GetForegroundWindow() == hwnd


OWN = set()  # the browser's processes: its find bar is a window of its own, owned by the page's


def in_front(hwnd):
    """Whether the window in front is `hwnd` or another of the browser's (its find bar)."""
    fg = user32.GetForegroundWindow()
    if fg == hwnd:
        return True
    pid = wt.DWORD()
    user32.GetWindowThreadProcessId(fg, ctypes.byref(pid))
    return pid.value in OWN


def send(hwnd, inputs):
    """Sends `inputs`, only while the browser's window is the one in front."""
    if not in_front(hwnd):
        raise RuntimeError("the browser's window is not in front: no key sent")
    array = (INPUT * len(inputs))(*inputs)
    user32.SendInput(len(inputs), array, ctypes.sizeof(INPUT))


def key(vk, up=False):
    return INPUT(type=1, u=INPUT_UNION(ki=KEYBDINPUT(wVk=vk, dwFlags=KEYEVENTF_KEYUP if up else 0)))


def char(c, up=False):
    flags = KEYEVENTF_UNICODE | (KEYEVENTF_KEYUP if up else 0)
    return INPUT(type=1, u=INPUT_UNION(ki=KEYBDINPUT(wScan=ord(c), dwFlags=flags)))


def ctrl_f(hwnd):
    send(hwnd, [key(VK_CONTROL), key(ord("F")), key(ord("F"), True), key(VK_CONTROL, True)])


def type_text(hwnd, text):
    for c in text:
        send(hwnd, [char(c), char(c, True)])
        time.sleep(0.06)


def shot(hwnd, path):
    rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    image = ImageGrab.grab(bbox=(rect.left, rect.top, rect.right, rect.bottom), all_screens=True)
    image.save(path)
    return image


def highlight_pixels(image):
    """Pixels of the find highlight: the layer's translucent yellow or orange over the page."""
    n = 0
    for r, g, b in image.convert("RGB").get_flattened_data():
        if r > 150 and 60 < g < 230 and b < 90 and r - b > 110:
            n += 1
    return n


# ---- reading the page --------------------------------------------------------------------

class Cdp:
    def __init__(self):
        for _ in range(100):
            try:
                tabs = json.load(urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json", timeout=2))
                tab = next(t for t in tabs if t.get("type") == "page")
                break
            except Exception:
                time.sleep(0.2)
        self.ws = websocket.create_connection(tab["webSocketDebuggerUrl"], timeout=30, suppress_origin=True)
        self.n = 0

    def js(self, expression):
        self.n += 1
        self.ws.send(json.dumps({"id": self.n, "method": "Runtime.evaluate",
                                 "params": {"expression": expression, "returnByValue": True}}))
        while True:
            msg = json.loads(self.ws.recv())
            if msg.get("id") == self.n:
                return msg.get("result", {}).get("result", {}).get("value")


class Bidi:
    def __init__(self):
        for _ in range(150):
            try:
                self.ws = websocket.create_connection(f"ws://127.0.0.1:{PORT}/session", timeout=30,
                                                      suppress_origin=True)
                break
            except Exception:
                time.sleep(0.2)
        self.n = 0
        self.call("session.new", capabilities={})
        self.context = self.call("browsingContext.getTree")["contexts"][0]["context"]

    def call(self, method, **params):
        self.n += 1
        self.ws.send(json.dumps({"id": self.n, "method": method, "params": params}))
        while True:
            msg = json.loads(self.ws.recv())
            if msg.get("id") == self.n:
                if msg.get("type") == "error":
                    raise RuntimeError(f"{method}: {msg}")
                return msg.get("result", {})

    def js(self, expression):
        result = self.call("script.evaluate", expression=f"JSON.stringify({expression})",
                           target={"context": self.context}, awaitPromise=False)
        value = result.get("result", {}).get("value")
        return None if value is None else json.loads(value)

    def print_pdf(self):
        return self.call("browsingContext.print", context=self.context, background=False)["data"]


def until(page, expression, seconds):
    end = time.time() + seconds
    value = None
    while time.time() < end:
        try:
            value = page.js(expression)
        except Exception:
            value = None
        if value:
            return value
        time.sleep(0.25)
    return value


def launch(kind, url, profile):
    exe = next((p for p in BROWSERS[kind] if os.path.exists(p)), None)
    if not exe:
        sys.exit(f"{kind} is not installed")
    if kind == "firefox":
        with open(os.path.join(profile, "user.js"), "w") as f:
            for pref in ['"browser.shell.checkDefaultBrowser", false',
                         '"browser.startup.homepage_override.mstone", "ignore"',
                         '"browser.aboutwelcome.enabled", false',
                         '"datareporting.policy.dataSubmissionEnabled", false',
                         '"toolkit.telemetry.reportingpolicy.firstRun", false',
                         '"browser.tabs.warnOnClose", false',
                         '"dom.webgpu.enabled", true']:
                f.write(f"user_pref({pref});\n")
        args = [exe, "-profile", profile, "-no-remote", "-new-instance", "--remote-debugging-port",
                str(PORT), "-width", "1000", "-height", "700", url]
    else:
        args = [exe, f"--user-data-dir={profile}", f"--remote-debugging-port={PORT}", "--no-first-run",
                "--no-default-browser-check", "--window-size=1000,700", "--window-position=80,40",
                "--lang=en-US", "--disable-features=Translate",
                # A private window: no account signed in, no sync prompt over the page.
                "--inprivate" if kind == "edge" else "--guest",
                "--new-window", url]
    return subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def main(base, kind, out):
    os.makedirs(out, exist_ok=True)
    root = base.split("#")[0].rstrip("/") + "/"
    profile = tempfile.mkdtemp(prefix=f"frus-find-bar-{kind}-")
    browser = launch(kind, root + "#" + ROUTE, profile)
    hwnd = None
    try:
        page = Bidi() if kind == "firefox" else Cdp()
        if kind == "firefox":
            page.call("browsingContext.navigate", context=page.context, url=root + "#" + ROUTE, wait="complete")
        print(f"{kind}: a real window, the demo at {ROUTE}")
        layer = until(page, "document.querySelectorAll('#frus-text .t').length", 40)
        expect_true("the text layer is over the canvas", bool(layer), f"{layer} texts")
        time.sleep(2.0)
        renderer = page.js("(() => { const c = document.querySelector('canvas');"
                           " return navigator.gpu ? 'webgpu offered' : 'no webgpu (webgl 2)'; })()")
        print(f"  ({renderer}; user agent {page.js('navigator.userAgent')})")
        for _ in range(50):
            OWN.clear()
            OWN.update(tree_pids(browser.pid))
            wins = windows_of(OWN)
            if wins:
                break
            time.sleep(0.2)
        if not wins:
            sys.exit("the browser's window was not found")

        def area(win):
            rect = wt.RECT()
            user32.GetWindowRect(win[0], ctypes.byref(rect))
            return (rect.right - rect.left) * (rect.bottom - rect.top)

        # The page's window, not a bubble the browser opened over it.
        hwnd, title = max(wins, key=area)
        print(f"  (window {hwnd:#x}: {title!r})")
        if not bring_to_front(hwnd):
            sys.exit("the browser's window could not be brought in front: nothing was typed")
        before_on = page.js(ON_SCREEN) or []
        expect_true(f"{WORD!r} is not on screen before", not any(WORD in t for t in before_on))
        boxes_before = page.js(BOXES)
        shot(hwnd, os.path.join(out, f"{kind}-before.png"))
        if os.environ.get("FIND_TRACE"):
            page.js("""(() => { window.__log = []; const t0 = performance.now();
              const d = Object.getOwnPropertyDescriptor(Element.prototype, 'scrollTop');
              Object.defineProperty(Element.prototype, 'scrollTop', { configurable: true,
                get() { return d.get.call(this); },
                set(v) { if (this.classList.contains('s')) window.__log.push(['set', Math.round(performance.now() - t0), Math.round(v)]); d.set.call(this, v); } });
              for (const el of document.querySelectorAll('#frus-text .s'))
                el.addEventListener('scroll', () => window.__log.push(['event', Math.round(performance.now() - t0), Math.round(d.get.call(el))]), true);
              const raf = window.requestAnimationFrame;
              return 1; })()""")
        ctrl_f(hwnd)
        time.sleep(0.8)
        type_text(hwnd, WORD)
        time.sleep(0.6)
        send(hwnd, [key(VK_RETURN), key(VK_RETURN, True)])
        if os.environ.get("FIND_TRACE"):
            for _ in range(30):
                print("   trace", page.js("[...document.querySelectorAll('#frus-text .s')].map((el) =>"
                                        " [Math.round(el.scrollTop), el.scrollHeight, el.clientHeight,"
                                        " el.querySelectorAll('.t').length])"),
                      [t for t in (page.js(ON_SCREEN) or []) if t.startswith("Row")][:2])
                time.sleep(0.1)
            print("   log", page.js("JSON.stringify(window.__log)"))
            print("   match", page.js(f"""JSON.stringify([...document.querySelectorAll('#frus-text .t')]
              .filter((el) => el.textContent.includes({json.dumps(WORD)}))
              .map((el) => [el.textContent, el.offsetTop, el.getAttribute('style'), el.parentElement.className,
                            el.parentElement.getAttribute('style')]))"""))
            print("   sel", page.js("String(getSelection())"))
        on = until(page, f"({ON_SCREEN}).some((t) => t.includes({json.dumps(WORD)}))", 6)
        time.sleep(0.8)
        after = shot(hwnd, os.path.join(out, f"{kind}-after.png"))
        boxes_after = page.js(BOXES)
        print(f"  (the layer's boxes, scrolled: {boxes_before} before, {boxes_after} after)")
        expect_true("the find bar scrolled the region's box", boxes_after != boxes_before)
        expect_true(f"the application scrolled to {WORD!r}: it is on screen", bool(on))
        n = highlight_pixels(after)
        expect_true("the find highlight shows in the window", n > 40, f"{n} highlight pixels")
        send(hwnd, [key(VK_ESCAPE), key(VK_ESCAPE, True)])
        if kind == "firefox" and os.environ.get("PRINT", "1") == "1":
            print("firefox: printing")
            import base64, io
            import pypdf
            page.js("location.hash = '#/journal'")
            time.sleep(2.5)
            data = base64.b64decode(page.print_pdf())
            with open(os.path.join(out, "firefox-journal.pdf"), "wb") as f:
                f.write(data)
            reader = pypdf.PdfReader(io.BytesIO(data))
            text = "\n".join(p.extract_text() or "" for p in reader.pages)
            expect_true("the PDF has pages of text", len(reader.pages) > 1, f"{len(reader.pages)} pages")
            for row in ("Row 1", "Row 2500", "Row 5000"):
                expect_true(f"the PDF has {row!r}", f"{row}\n" in text + "\n" or f"{row} " in text)
            expect_true("the printed document is gone after the print",
                        not page.js("!!document.getElementById('frus-print')"))
    finally:
        if hwnd:
            user32.SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE)
        subprocess.run(["taskkill", "/F", "/T", "/PID", str(browser.pid)], capture_output=True)
        time.sleep(1)
        shutil.rmtree(profile, ignore_errors=True)
    print("FAILED: " + ", ".join(failures) if failures else "all passed")
    return 1 if failures else 0


if __name__ == "__main__":
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    sys.exit(main(sys.argv[1], sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else "."))
