// Checks a frus page in every browser engine it can reach (milestone 847): Chromium (Edge or
// Chrome) with WebGPU, the same without WebGPU (WebGL 2), the same with a WebGPU that fails at
// the canvas, Firefox (Gecko) and WebKit (the engine of Safari), on the desktop and as phones.
//
// On the demo built with `frus build web` (crates/frus-demo), served from the folder it prints,
// each run checks that:
//
// - the application starts and draws: its text layer fills, the page's status box says nothing,
//   and the canvas is not one flat colour;
// - it draws through the GPU interface expected (the console's "GPU interface:" line);
// - it follows the platform the browser runs on: the demo's "Adaptive (…)" row names the
//   computer's system on a desktop browser and Android or iOS on a phone;
// - a mouse wheel (a swipe on a phone) scrolls the settings screen.
//
//     npm i playwright && npx playwright install webkit firefox
//     node scripts/web-browsers-check.mjs http://localhost:8080/ [out-dir] [run-name-filter]
//
// Chromium runs through the Edge installed on the computer (`BROWSER_CHANNEL=chrome` for
// Chrome); Firefox and WebKit are Playwright's builds. The Firefox installed on the computer
// (`FIREFOX` names its executable when it is not in the usual place) is run too, headless,
// over WebDriver BiDi: Playwright's Firefox is a build of its own. Each screenshot and each
// console is written to out-dir.
import { chromium, firefox, webkit, devices } from 'playwright';
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';

const base = (process.argv[2] || 'http://localhost:8080/').split('#')[0];
const out = process.argv[3] || '.';
const only = process.argv[4] || '';
fs.mkdirSync(out, { recursive: true });
const channel = process.env.BROWSER_CHANNEL || 'msedge';
const hostSystem = { win32: 'Windows', darwin: 'macOS', linux: 'Linux' }[process.platform] || 'Linux';
const installedFirefox = process.env.FIREFOX || {
  win32: 'C:/Program Files/Mozilla Firefox/firefox.exe',
  darwin: '/Applications/Firefox.app/Contents/MacOS/firefox',
  linux: '/usr/bin/firefox',
}[process.platform];
const failures = [];
const expect = (run, label, ok, detail = '') => {
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? `: ${detail}` : ''}`);
  if (!ok) failures.push(`${run}: ${label}`);
};

// What a page script can do to a browser before the application starts.
const NO_WEBGPU = `delete Navigator.prototype.gpu;`;
const NO_WEBGPU_CONTEXT = `{
  const get = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (kind, ...rest) {
    return kind === 'webgpu' ? null : get.call(this, kind, ...rest);
  };
}`;
// Only the page's own canvas refuses: a canvas the application makes for itself still works.
const PAGE_CANVAS_REFUSES_WEBGPU = `{
  const get = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = function (kind, ...rest) {
    return kind === 'webgpu' && this.isConnected ? null : get.call(this, kind, ...rest);
  };
}`;
const CONFIGURE_THROWS = `if (self.GPUCanvasContext) {
  GPUCanvasContext.prototype.configure = function () {
    throw new TypeError('this canvas format is not supported (a test)');
  };
}`;

const GPU_FLAGS = ['--enable-unsafe-webgpu', '--enable-features=Vulkan,WebGPU', '--ignore-gpu-blocklist'];
const edge = { engine: chromium, launch: { channel, args: GPU_FLAGS } };
const RUNS = [
  { name: 'chromium-webgpu', ...edge, gpu: 'WebGPU' },
  { name: 'chromium-webgl2', ...edge, init: NO_WEBGPU, gpu: 'WebGL 2' },
  { name: 'chromium-webgpu-no-canvas-context', ...edge, init: NO_WEBGPU_CONTEXT, gpu: 'WebGL 2' },
  { name: 'chromium-webgpu-page-canvas-refused', ...edge, init: PAGE_CANVAS_REFUSES_WEBGPU, gpu: 'WebGL 2' },
  { name: 'chromium-webgpu-configure-throws', ...edge, init: CONFIGURE_THROWS, gpu: 'WebGL 2' },
  { name: 'firefox-installed', bidi: installedFirefox },
  { name: 'firefox-playwright', engine: firefox, launch: {} },
  { name: 'webkit', engine: webkit, launch: {} },
  { name: 'phone-android-chromium', ...edge, device: 'Pixel 7', system: 'Android' },
  { name: 'phone-ios-webkit', engine: webkit, launch: {}, device: 'iPhone 15', system: 'iOS' },
].filter((run) => run.name.includes(only));

// The words the application shows: its text layer and the labels of what is on screen.
const WORDS = `[...document.querySelectorAll('#frus-text .t, canvas [aria-label]')]
  .map((el) => el.getAttribute('aria-label') || el.textContent).join('\\n')`;
const LAYER = "document.querySelectorAll('#frus-text .t').length";
const STATUS = "(() => { const s = document.getElementById('status'); return s && !s.hidden ? s.innerText : ''; })()";
// How far each scrolling region of the text layer is scrolled: it follows the application's.
const SCROLLED = "[...document.querySelectorAll('#frus-text .s')].map((el) => Math.round(el.scrollTop)).join(',')";
// A finger's swipe up, as pointer events, for an engine Playwright cannot swipe in.
const SWIPE = `(async () => {
  const canvas = document.querySelector('canvas');
  const r = canvas.getBoundingClientRect();
  const x = r.left + r.width / 2;
  const send = (type, y) => canvas.dispatchEvent(new PointerEvent(type, {
    pointerId: 7, pointerType: 'touch', isPrimary: true, clientX: x, clientY: y, bubbles: true,
    cancelable: true, buttons: type === 'pointerup' ? 0 : 1, button: 0, pressure: type === 'pointerup' ? 0 : 0.5,
  }));
  let y = r.top + r.height * 0.75;
  send('pointerdown', y);
  for (let i = 0; i < 12; i++) {
    await new Promise((done) => setTimeout(done, 16));
    y -= 30;
    send('pointermove', y);
  }
  send('pointerup', y);
})()`;

/** How many colours a PNG screenshot holds (8-bit RGB or RGBA, as browsers write them). */
function colours(png) {
  let pos = 8, width = 0, height = 0, type = 0;
  const idat = [];
  while (pos < png.length) {
    const len = png.readUInt32BE(pos);
    const kind = png.toString('ascii', pos + 4, pos + 8);
    const data = png.subarray(pos + 8, pos + 8 + len);
    if (kind === 'IHDR') { width = data.readUInt32BE(0); height = data.readUInt32BE(4); type = data[9]; }
    if (kind === 'IDAT') idat.push(data);
    pos += 12 + len;
  }
  const bpp = type === 6 ? 4 : 3;
  const raw = zlib.inflateSync(Buffer.concat(idat));
  const stride = width * bpp;
  const prev = Buffer.alloc(stride), row = Buffer.alloc(stride);
  const seen = new Set();
  for (let y = 0; y < height; y++) {
    const at = y * (stride + 1), filter = raw[at];
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? row[x - bpp] : 0, b = prev[x], c = x >= bpp ? prev[x - bpp] : 0;
      let v = raw[at + 1 + x];
      if (filter === 1) v += a;
      else if (filter === 2) v += b;
      else if (filter === 3) v += (a + b) >> 1;
      else if (filter === 4) { const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c); v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c; }
      row[x] = v & 255;
    }
    for (let x = 0; x < stride; x += bpp * 7) seen.add((row[x] << 16) | (row[x + 1] << 8) | row[x + 2]);
    row.copy(prev);
  }
  return seen.size;
}

const pause = (ms) => new Promise((done) => setTimeout(done, ms));

/** A browser driven by Playwright. */
async function playwrightDriver(run) {
  const browser = await run.engine.launch(run.launch);
  const phone = run.device ? { ...devices[run.device] } : null;
  // Chromium's device emulation reports the canvas's device-pixel box at the CSS size whatever
  // pixel ratio it emulates, so the canvas would be drawn at the wrong scale (L-055): the phone
  // is emulated at a ratio of 1 there. WebKit's emulation has no such fault.
  if (phone && run.engine === chromium) phone.deviceScaleFactor = 1;
  const context = await browser.newContext(phone || { viewport: { width: 1000, height: 800 } });
  if (run.init) await context.addInitScript(run.init);
  const page = await context.newPage();
  const logs = [];
  page.on('console', (m) => logs.push(`${m.type()}: ${m.text()}`));
  page.on('pageerror', (e) => logs.push(`pageerror: ${e.message}`));
  const chromiumEngine = browser.browserType().name() === 'chromium';
  return {
    label: `${browser.browserType().name()} ${browser.version()}${run.device ? `, as a ${run.device}` : ''}`,
    logs,
    goto: (url) => page.goto(url),
    eval: (expression) => page.evaluate(expression),
    screenshot: (file) => page.screenshot({ path: file }),
    async scroll() {
      const box = page.viewportSize();
      if (!run.device) {
        await page.mouse.move(box.width / 2, box.height / 2);
        for (let i = 0; i < 3; i++) {
          await page.mouse.wheel(0, 200);
          await pause(100);
        }
      } else if (chromiumEngine) {
        // A real touch swipe up: trusted touch points, as the screen sends them.
        const cdp = await context.newCDPSession(page);
        const x = Math.round(box.width / 2);
        let y = Math.round(box.height * 0.75);
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y }] });
        for (let i = 0; i < 12; i++) {
          await pause(16);
          y -= 30;
          await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x, y }] });
        }
        await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      } else {
        await page.evaluate(SWIPE);
      }
    },
    close: () => browser.close(),
  };
}

/** The Firefox installed on the computer, headless, over WebDriver BiDi. */
async function bidiDriver(run) {
  if (!run.bidi || !fs.existsSync(run.bidi)) throw new Error(`no Firefox at ${run.bidi}`);
  const port = 9300 + Math.floor(Math.random() * 500);
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'frus-browsers-firefox-'));
  const proc = spawn(run.bidi, ['--headless', '--no-remote', '--profile', profile, `--remote-debugging-port=${port}`,
    '--width=1000', '--height=800', 'about:blank'], { stdio: 'ignore', env: { ...process.env, MOZ_HEADLESS: '1' } });
  const stop = () => {
    try {
      if (process.platform === 'win32') {
        // Firefox's launcher starts the browser again as a process of its own and leaves: its
        // tree is gone by now. Every process of this profile, and only those, is ended.
        execFileSync('powershell', ['-NoProfile', '-Command',
          `Get-CimInstance Win32_Process -Filter "Name='firefox.exe'" | Where-Object { $_.CommandLine -like '*${path.basename(profile)}*' } | ForEach-Object { taskkill /T /F /PID $_.ProcessId }`,
        ], { stdio: 'ignore' });
        execFileSync('taskkill', ['/T', '/F', '/PID', String(proc.pid)], { stdio: 'ignore' });
      } else {
        proc.kill('SIGKILL');
      }
    } catch { /* already gone */ }
    try {
      fs.rmSync(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 });
    } catch { /* still held: the system's temporary folder keeps it */ }
  };
  try {
    let ws;
    for (let i = 0; i < 160 && !ws; i++) {
      await pause(250);
      ws = await new Promise((done) => {
        const socket = new WebSocket(`ws://127.0.0.1:${port}/session`);
        socket.onopen = () => done(socket);
        socket.onerror = () => done(undefined);
      });
    }
    if (!ws) throw new Error('Firefox did not open its WebDriver BiDi port');
    const pending = new Map();
    const logs = [];
    let next = 1;
    ws.onmessage = (message) => {
      const data = JSON.parse(message.data);
      if (data.id && pending.has(data.id)) {
        const { done, fail } = pending.get(data.id);
        pending.delete(data.id);
        if (data.type === 'error') fail(new Error(`${data.error}: ${data.message}`));
        else done(data.result);
      } else if (data.method === 'log.entryAdded') {
        logs.push(`${data.params.level}: ${data.params.text}`);
      }
    };
    const send = (method, params = {}) => new Promise((done, fail) => {
      const id = next++;
      pending.set(id, { done, fail });
      ws.send(JSON.stringify({ id, method, params }));
    });
    const session = await send('session.new', { capabilities: {} });
    const context = (await send('browsingContext.getTree', {})).contexts[0].context;
    await send('browsingContext.setViewport', { context, viewport: { width: 1000, height: 800 } });
    await send('session.subscribe', { events: ['log.entryAdded'] });
    const evaluate = async (expression) => {
      const result = await send('script.evaluate', {
        expression, target: { context }, awaitPromise: true, resultOwnership: 'none',
      });
      if (result.type === 'exception') throw new Error(result.exceptionDetails.text);
      return result.result.value;
    };
    return {
      label: `firefox ${session.capabilities.browserVersion} (installed, WebDriver BiDi)`,
      logs,
      goto: (url) => send('browsingContext.navigate', { context, url, wait: 'complete' }),
      eval: evaluate,
      async screenshot(file) {
        const shot = Buffer.from((await send('browsingContext.captureScreenshot', { context })).data, 'base64');
        fs.writeFileSync(file, shot);
        return shot;
      },
      async scroll() {
        // The pointer over the list first, as a hand puts it there: a wheel turns where the
        // pointer is.
        await send('input.performActions', {
          context,
          actions: [{ type: 'pointer', id: 'mouse', parameters: { pointerType: 'mouse' }, actions: [{ type: 'pointerMove', x: 500, y: 400 }] }],
        });
        const steps = [];
        for (let i = 0; i < 3; i++) {
          steps.push({ type: 'scroll', x: 500, y: 400, deltaX: 0, deltaY: 200 });
          steps.push({ type: 'pause', duration: 100 });
        }
        await send('input.performActions', { context, actions: [{ type: 'wheel', id: 'wheel', actions: steps }] });
      },
      async close() {
        await send('session.end', {}).catch(() => {});
        ws.close();
        stop();
      },
    };
  } catch (err) {
    stop();
    throw err;
  }
}

for (const run of RUNS) {
  let driver;
  try {
    driver = await (run.bidi ? bidiDriver(run) : playwrightDriver(run));
  } catch (err) {
    console.log(`${run.name}: cannot be launched here (${err.message.split('\n')[0]})`);
    failures.push(`${run.name}: launch`);
    continue;
  }
  try {
    console.log(`${run.name} (${driver.label})`);
    await driver.goto(base + '#/cupertino');
    let layer = 0;
    for (let i = 0; i < 100 && layer <= 3; i++) {
      await pause(200);
      layer = await driver.eval(LAYER).catch(() => 0);
    }
    const status = await driver.eval(STATUS);
    expect(run.name, 'the application started', layer > 3, status ? `the page says: ${status}` : `${layer} texts in the layer`);
    await pause(1000);
    // The last word on it: a page canvas that refuses WebGPU after the trial says so after it.
    const gpuLine = driver.logs.findLast((l) => l.includes('GPU interface:')) || '(no "GPU interface" line)';
    console.log(`  (${gpuLine.replace(/^\w+: /, '')})`);
    if (run.gpu) expect(run.name, `it draws through ${run.gpu}`, gpuLine.includes(`GPU interface: ${run.gpu}`));
    const shot = await driver.screenshot(path.join(out, `${run.name}-cupertino.png`));
    const count = colours(shot);
    expect(run.name, 'the canvas is drawn (not one flat colour)', count > 8, `${count} colours sampled`);
    const words = await driver.eval(WORDS);
    const adaptive = (words.match(/Adaptive \(([^)]+)\)/) || [])[1] || '(not found)';
    const system = run.system || hostSystem;
    expect(run.name, `it follows ${system}'s conventions`, adaptive === system, `the demo says "${adaptive}"`);

    await driver.eval("location.hash = '#/settings'");
    await pause(1500);
    const before = await driver.eval(SCROLLED);
    await driver.scroll();
    await pause(1200);
    const after = await driver.eval(SCROLLED);
    expect(run.name, 'the settings screen scrolls', before !== after, `regions at ${before || 'none'}, then ${after || 'none'}`);
    await driver.screenshot(path.join(out, `${run.name}-settings.png`));
  } catch (err) {
    expect(run.name, 'the run went through', false, err.message.split('\n')[0]);
  } finally {
    fs.writeFileSync(path.join(out, `${run.name}-console.log`), driver.logs.join('\n'));
    await driver.close();
  }
}
console.log(failures.length ? `FAILED: ${failures.join(', ')}` : 'all passed');
process.exit(failures.length ? 1 : 0);
