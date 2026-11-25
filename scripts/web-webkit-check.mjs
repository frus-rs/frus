// Checks a frus page in WebKit, the engine of Safari, through Playwright (milestone 833).
//
// Safari runs only on Apple's systems; Playwright's WebKit is the same engine built for Linux,
// Windows and macOS. On the demo built with `frus build web` (crates/frus-demo), served from
// the site it prints (target/frus-web/frus-demo/), this checks that:
//
// - the page starts and draws (WebGPU or WebGL 2, whichever the engine offers);
// - the text layer is over the canvas, holding the words on screen and the ones scrolled out
//   of view;
// - `window.find` (what the find bar searches) reaches a word on screen and one scrolled out
//   of view; whether WebKit's find scrolls the region's box on its own is reported, and the
//   application follows the box either way;
// - the print style shows the plain document alone (the print media emulated, `beforeprint`
//   sent: Playwright prints to PDF in Chromium only).
//
//     npm i playwright && npx playwright install --with-deps webkit
//     node scripts/web-webkit-check.mjs http://localhost:8080/ [out-dir]
import { webkit } from 'playwright';
import fs from 'node:fs';
import path from 'node:path';

const base = (process.argv[2] || 'http://localhost:8080/').split('#')[0];
const out = process.argv[3] || '.';
fs.mkdirSync(out, { recursive: true });
const failures = [];
const expect = (label, ok, detail = '') => {
  console.log(`  ${ok ? 'ok  ' : 'FAIL'} ${label}${detail ? `: ${detail}` : ''}`);
  if (!ok) failures.push(label);
};
const ON_SCREEN = "[...document.querySelectorAll('#frus-semantics > *')].map((el) => el.getAttribute('aria-label') || el.getAttribute('data-t')).filter(Boolean)";
const BOXES = "[...document.querySelectorAll('#frus-text .s')].map((el) => Math.round(el.scrollTop))";

const browser = await webkit.launch();
try {
  const page = await browser.newPage({ viewport: { width: 1000, height: 800 } });
  const logs = [];
  page.on('console', (m) => logs.push(`${m.type()}: ${m.text()}`));
  page.on('pageerror', (e) => logs.push(`pageerror: ${e.message}`));
  console.log(`webkit ${browser.version()}: the demo's settings screen`);
  const served = await (await fetch(base)).text();
  expect('the served page carries the first screen\'s text', served.includes('<!-- frus:page-text -->'));
  await page.goto(base + '#/settings');
  const gpu = await page.evaluate(async () => {
    const webgpu = !!(navigator.gpu && (await navigator.gpu.requestAdapter().catch(() => null)));
    const webgl2 = !!document.createElement('canvas').getContext('webgl2');
    return { webgpu, webgl2 };
  });
  console.log(`  (WebGPU adapter: ${gpu.webgpu}; WebGL 2: ${gpu.webgl2})`);
  let layer = 0;
  for (let i = 0; i < 80 && layer <= 3; i++) {
    await page.waitForTimeout(250);
    layer = await page.evaluate(() => document.querySelectorAll('#frus-text .t').length);
  }
  const status = await page.evaluate(() => {
    const s = document.getElementById('status');
    return s && !s.hidden ? s.innerText : '';
  });
  expect('the application started and drew', layer > 3, status ? `the page says: ${status}` : `${layer} texts in the layer`);
  await page.waitForTimeout(1500);
  await page.screenshot({ path: path.join(out, 'webkit-settings.png') });
  if (layer > 3) {
    expect('the text written in at build time is gone once the layer shows',
      !(await page.evaluate(() => !!document.getElementById('frus-page-text'))));
    const onScreen = await page.evaluate(ON_SCREEN);
    expect('find reaches "Volume", on screen', await page.evaluate(() => {
      getSelection().removeAllRanges();
      return window.find('Volume', false, false, true);
    }));
    const word = 'Vibrate 400 ms';
    expect(`"${word}" is not on screen before`, !onScreen.some((t) => t.includes(word)));
    const before = await page.evaluate(BOXES);
    const found = await page.evaluate((w) => {
      getSelection().removeAllRanges();
      return window.find(w, false, false, true);
    }, word);
    expect(`find reaches "${word}", out of view`, found);
    await page.waitForTimeout(500);
    const after = await page.evaluate(BOXES);
    console.log(`  (the boxes, scrolled: ${JSON.stringify(before)} before the find, ${JSON.stringify(after)} after)`);
    if (JSON.stringify(before) === JSON.stringify(after)) {
      // As in a headless Chrome, the find selected and did not scroll: the find bar's step.
      await page.evaluate(() => {
        const node = getSelection().anchorNode;
        (node && node.parentElement)?.scrollIntoView({ block: 'center' });
      });
      console.log('  (window.find did not scroll: the match was brought into view as the find bar does)');
    }
    let shown = false;
    for (let i = 0; i < 24 && !shown; i++) {
      await page.waitForTimeout(250);
      shown = (await page.evaluate(ON_SCREEN)).some((t) => t.includes(word));
    }
    expect('the application scrolled to it: it is on screen', shown);
    await page.screenshot({ path: path.join(out, 'webkit-found.png') });

    console.log('printing (the print media and beforeprint)');
    await page.evaluate(() => { location.hash = '#/journal'; });
    await page.waitForTimeout(2500);
    await page.emulateMedia({ media: 'print' });
    await page.evaluate(() => window.dispatchEvent(new Event('beforeprint')));
    const print = await page.evaluate(() => {
      const doc = document.getElementById('frus-print');
      const canvas = document.querySelector('canvas');
      return {
        rows: doc ? doc.querySelectorAll('li').length : 0,
        shows: doc ? getComputedStyle(doc).display !== 'none' : false,
        canvasHidden: canvas ? getComputedStyle(canvas).display === 'none' : false,
        text: doc ? doc.innerText.slice(0, 80) : '',
      };
    });
    expect('the printed document holds every row of the journal', print.rows >= 5000, `${print.rows} rows`);
    expect('the print style shows it and hides the canvas', print.shows && print.canvasHidden);
    await page.screenshot({ path: path.join(out, 'webkit-print.png') });
    await page.evaluate(() => window.dispatchEvent(new Event('afterprint')));
    expect('the printed document is gone after the print',
      !(await page.evaluate(() => !!document.getElementById('frus-print'))));
  }
  fs.writeFileSync(path.join(out, 'webkit-console.log'), logs.join('\n'));
} finally {
  await browser.close();
}
console.log(failures.length ? `FAILED: ${failures.join(', ')}` : 'all passed');
process.exit(failures.length ? 1 : 0);
