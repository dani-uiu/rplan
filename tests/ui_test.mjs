// End-to-end UI tests: drives the real interface in headless Edge/Chrome over the
// DevTools protocol (no npm packages needed — uses Node 22+'s built-in WebSocket).
//
//   node tests/ui_test.mjs [path/to/rplan.exe] [--shots <dir>]
//
// Set BROWSER=<path to chrome/msedge> to use a different browser.

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const args = process.argv.slice(2);
const shotsAt = args.indexOf('--shots');
const shots = shotsAt >= 0 ? args.splice(shotsAt, 2)[1] : null;
const exe = path.resolve(args[0] || (process.platform === 'win32' ? 'target/debug/rplan.exe' : 'target/debug/rplan'));
const browser = process.env.BROWSER || [
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  '/usr/bin/google-chrome', '/usr/bin/chromium', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
].find(p => fs.existsSync(p));
if (!browser) { console.error('No Chrome/Edge found — set BROWSER=...'); process.exit(2); }

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'rplan-ui-'));
const plan = path.join(tmp, 'test.plan');
const sleep = ms => new Promise(r => setTimeout(r, ms));
const procs = [];
process.on('exit', () => { for (const p of procs) try { p.kill(); } catch (e) {} });
let failures = 0, passed = 0;

function run(cmd, a) {
  return new Promise((res, rej) => { const p = spawn(cmd, a); let out = ''; p.stdout.on('data', d => out += d); p.on('exit', c => c === 0 ? res(out) : rej(new Error(`${cmd} exited ${c}`))); });
}
async function waitFor(fn, what, ms = 5000) {
  const t0 = Date.now();
  for (;;) { try { const v = await fn(); if (v) return v; } catch (e) {} if (Date.now() - t0 > ms) throw new Error(`timed out waiting for ${what}`); await sleep(60); }
}

// ---------- start rplan ----------
await run(exe, ['demo', plan]);
const srv = spawn(exe, ['ui', plan, '--no-open', '--port', '7950']);
procs.push(srv);
let srvOut = '';
srv.stdout.on('data', d => srvOut += d);
const url = await waitFor(() => (srvOut.match(/http:\/\/127\.0\.0\.1:\d+\//) || [])[0], 'rplan to start');

// ---------- start the browser ----------
const dport = 9400 + Math.floor(Math.random() * 400);
const br = spawn(browser, ['--headless=new', '--disable-gpu', `--remote-debugging-port=${dport}`, `--user-data-dir=${path.join(tmp, 'profile')}`, '--window-size=1500,950', '--no-first-run', '--no-default-browser-check', 'about:blank']);
procs.push(br);
const target = await waitFor(async () => (await (await fetch(`http://127.0.0.1:${dport}/json/list`)).json()).find(t => t.type === 'page'), 'the browser', 15000);
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
let seq = 0;
const pending = new Map();
ws.onmessage = m => { const j = JSON.parse(m.data); if (j.id && pending.has(j.id)) { pending.get(j.id)(j); pending.delete(j.id); } };
const cdp = (method, params = {}) => new Promise((res, rej) => { const id = ++seq; pending.set(id, j => j.error ? rej(new Error(j.error.message)) : res(j.result)); ws.send(JSON.stringify({ id, method, params })); });
async function js(expr) {
  const r = await cdp('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error('page error: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text));
  return r.result.value;
}
const key = (k, code, vk, modifiers = 0) => cdp('Input.dispatchKeyEvent', { type: 'keyDown', key: k, code, windowsVirtualKeyCode: vk, modifiers }).then(() => cdp('Input.dispatchKeyEvent', { type: 'keyUp', key: k, code, windowsVirtualKeyCode: vk, modifiers }));
const mouse = (type, x, y) => cdp('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons: type === 'mouseReleased' ? 0 : 1, clickCount: 1 });
async function dragMouse(x1, y1, x2, y2) { await mouse('mouseMoved', x1, y1); await mouse('mousePressed', x1, y1); for (let i = 1; i <= 8; i++) await mouse('mouseMoved', x1 + (x2 - x1) * i / 8, y1 + (y2 - y1) * i / 8); await mouse('mouseReleased', x2, y2); }
const rect = sel => js(`(() => { const el = document.querySelector(${JSON.stringify(sel)}); el.scrollIntoView({ block: 'center', inline: 'nearest' }); const r = el.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; })()`);
const file = () => fs.readFileSync(plan, 'utf8');
const taskLine = id => file().split('\n').find(l => new RegExp(`^\\s*task ${id} \\|`).test(l)) || '';
const settle = () => js('chain.then(() => new Promise(r => setTimeout(r, 30)))');
async function shot(name) { if (!shots) return; fs.mkdirSync(shots, { recursive: true }); const r = await cdp('Page.captureScreenshot', { format: 'png' }); fs.writeFileSync(path.join(shots, name + '.png'), Buffer.from(r.data, 'base64')); }

async function test(name, fn) {
  try { await fn(); passed++; console.log(`  ✓ ${name}`); }
  catch (e) {
    failures++;
    const msgs = await js('[...document.querySelectorAll(".toast")].map(t => t.textContent).join(" | ")').catch(() => '');
    console.log(`  ✗ ${name}\n      ${e.message}${msgs ? '\n      on screen: ' + msgs : ''}`);
    await shot('FAIL-' + name.replace(/\W+/g, '_'));
  }
}
function ok(cond, msg) { if (!cond) throw new Error(msg); }

await cdp('Page.enable');
await cdp('Runtime.enable');
await cdp('Emulation.setDeviceMetricsOverride', { width: 1500, height: 950, deviceScaleFactor: 1, mobile: false });
await cdp('Page.navigate', { url: url + '#gantt' });
await waitFor(() => js('typeof D !== "undefined" && !!D && !!document.querySelector("[data-row]")'), 'the app to load', 10000);
await js('window.confirm = () => true; localStorage.clear()');

console.log(`rplan UI tests (${path.basename(browser)}, ${url})`);

await test('loads the project with summaries, KPIs and critical path', async () => {
  ok((await js('document.title')).includes('Website Launch'), 'title');
  ok(await js('document.querySelectorAll("[data-row]").length') === 11, 'expected 11 rows');
  ok(await js('document.querySelectorAll(".sbar").length') === 2, 'expected 2 summary bars');
  ok((await js('document.querySelector("#kpis").innerText')).includes('1 person'), 'over-allocation KPI');
  ok(await js('document.querySelectorAll(".bar.c").length') >= 4, 'critical bars');
  await shot('01-gantt');
});

await test('favicon loads (ICO and SVG) under the page’s security policy', async () => {
  const load = src => js(`new Promise(r => { const i = new Image(); i.onload = () => r(i.naturalWidth); i.onerror = () => r(0); i.src = ${JSON.stringify(src)}; })`);
  ok(await load('/favicon.ico') > 0, '/favicon.ico failed to load');
  ok(await load('/favicon.svg') > 0, '/favicon.svg failed to load');
  ok(await js('document.querySelectorAll("link[rel=icon]").length') === 2, 'icon links');
});

await test('security: API rejects requests without the session token', async () => {
  ok(await js('fetch("/api/project").then(r => r.status)') === 403, 'GET without token should be 403');
  ok(await js('fetch("/api/save", { method: "POST", body: "x" }).then(r => r.status)') === 403, 'POST without token should be 403');
  ok(await js('fetch("/api/project", { headers: { "X-Rplan-Token": TOKEN } }).then(r => r.status)') === 200, 'with token should be 200');
});

await test('add a task by typing a name and pressing Enter (linked after the selection)', async () => {
  await js('document.querySelector("[data-row=\\"8\\"] .num").click()');
  await js(`(() => { const el = document.querySelector('[data-f="new"]'); el.value = 'Write release notes'; el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); })()`);
  await settle();
  ok(file().includes('Write release notes'), 'task not saved');
  ok(/\| Write release notes \| 1d \| 8 \|/.test(taskLine(12)), 'expected new task 12 after #8, linked to it: ' + taskLine(12));
});

await test('edit duration, predecessors and people in the grid', async () => {
  const set = (id, f, v) => js(`(() => { const el = document.querySelector('[data-id="${id}"][data-f="${f}"]'); el.value = ${JSON.stringify(v)}; el.dispatchEvent(new Event('change', { bubbles: true })); })()`);
  await set(12, 'dur', '4h'); await settle();
  ok(taskLine(12).includes('| 4h |'), 'duration: ' + taskLine(12));
  await set(12, 'deps', '8, 6SS+1d'); await settle();
  ok(taskLine(12).includes('| 8,6SS+1d |'), 'deps: ' + taskLine(12));
  await set(12, 'who', 'Cy[50%], Dee'); await settle();
  ok(taskLine(12).includes('Cy[50%],Dee'), 'who: ' + taskLine(12));
  await set(12, 'deps', '12'); await settle();
  ok(taskLine(12).includes('| 8,6SS+1d |'), 'self-link must be rejected');
  ok((await js('document.querySelector("#toasts").innerText')).includes("can't depend on itself"), 'error toast');
});

await test('Tab indents the selected task under the one above; Shift+Tab outdents', async () => {
  const r = await rect('[data-row="12"] .num');
  await mouse('mousePressed', r.x + 8, r.y + 8); await mouse('mouseReleased', r.x + 8, r.y + 8);
  ok(await js('document.activeElement === document.body'), 'clicking the # cell should leave the inputs');
  await key('Tab', 'Tab', 9); await settle();
  ok(/^  task 12 /.test(taskLine(12)), 'indent: ' + JSON.stringify(taskLine(12)));
  ok(/^task 8 \| QA & fixes \|  \|/.test(taskLine(8)), 'task 8 should now be a summary');
  await key('Tab', 'Tab', 9, 8); await settle();
  ok(/^task 12 /.test(taskLine(12)), 'outdent: ' + JSON.stringify(taskLine(12)));
});

await test('Alt+↑ moves a task above its previous sibling', async () => {
  const before = file().indexOf('task 12 ');
  await key('ArrowUp', 'ArrowUp', 38, 1); await settle();
  ok(file().indexOf('task 12 ') < file().indexOf('task 8 ') && file().indexOf('task 12 ') < before, 'task 12 should be above task 8');
});

await test('drag a bar’s right edge to change its duration', async () => {
  const r = await rect('[data-bar="4"]');
  const dw = await js('dw');
  await dragMouse(r.x + r.w - 2, r.y + r.h / 2, r.x + r.w - 2 + dw * 3, r.y + r.h / 2);
  await settle();
  const d = +(taskLine(4).match(/\| (\d+)d \|/) || [])[1];
  ok(d >= 9 && d <= 11, 'Backend API should be ~10d now: ' + taskLine(4));
});

await test('drag a bar to move it (sets "start no earlier than")', async () => {
  const r = await rect('[data-bar="6"]');
  const dw = await js('dw');
  await dragMouse(r.x + 10, r.y + r.h / 2, r.x + 10 + dw * 2, r.y + r.h / 2);
  await settle();
  ok(/snet=2026-11-1\d/.test(taskLine(6)), 'constraint: ' + taskLine(6));
});

await test('drag the ○ handle onto another bar to link them', async () => {
  await js('document.querySelectorAll(".hl").forEach(h => h.style.opacity = 1)');
  const h = await rect('[data-bar="3"] .hl'), b = await rect('[data-bar="6"]');
  await dragMouse(h.x + h.w / 2, h.y + h.h / 2, b.x + b.w / 2, b.y + b.h / 2);
  await settle();
  ok(/\| 2,3 \|/.test(taskLine(6)), 'link 3 → 6: ' + taskLine(6));
});

await test('Ctrl+Z undoes and Ctrl+Y redoes', async () => {
  await js('document.activeElement.blur()');
  await key('z', 'KeyZ', 90, 2); await settle(); await sleep(100);
  ok(/\| 2 \|/.test(taskLine(6)), 'undo should remove the link: ' + taskLine(6));
  await key('y', 'KeyY', 89, 2); await settle(); await sleep(100);
  ok(/\| 2,3 \|/.test(taskLine(6)), 'redo should restore it: ' + taskLine(6));
});

await test('details panel edits link type, lag, constraint and deadline', async () => {
  ok(await js('document.querySelector("#insp").hidden'), 'details panel starts closed');
  // Double-clicking a bar opens the details panel for that task.
  await js('document.querySelector("[data-bar=\\"7\\"]").scrollIntoView({ block: "center", inline: "center" })');
  const r = await rect('[data-bar="7"]');
  await cdp('Input.dispatchMouseEvent', { type: 'mousePressed', x: r.x + 4, y: r.y + 10, button: 'left', buttons: 1, clickCount: 1 });
  await cdp('Input.dispatchMouseEvent', { type: 'mouseReleased', x: r.x + 4, y: r.y + 10, button: 'left', buttons: 0, clickCount: 1 });
  await cdp('Input.dispatchMouseEvent', { type: 'mousePressed', x: r.x + 4, y: r.y + 10, button: 'left', buttons: 1, clickCount: 2 });
  await cdp('Input.dispatchMouseEvent', { type: 'mouseReleased', x: r.x + 4, y: r.y + 10, button: 'left', buttons: 0, clickCount: 2 });
  await waitFor(() => js('!!document.querySelector("[data-insp=dep-kind]")'), 'inspector');
  await js(`(() => { const s = document.querySelector('[data-insp="dep-kind"][data-k="1"]'); s.value = 'SS'; s.dispatchEvent(new Event('change', { bubbles: true })); })()`); await settle();
  await js(`(() => { const s = document.querySelector('[data-insp="dep-lag"][data-k="1"]'); s.value = '+2d'; s.dispatchEvent(new Event('change', { bubbles: true })); })()`); await settle();
  ok(taskLine(7).includes('| 4,5SS+2d |'), 'link type/lag: ' + taskLine(7));
  await js(`(() => { const s = document.querySelector('[data-insp="deadline"]'); s.value = '2026-11-25'; s.dispatchEvent(new Event('change', { bubbles: true })); })()`); await settle();
  ok(taskLine(7).includes('deadline=2026-11-25'), 'deadline: ' + taskLine(7));
  ok((await js('document.querySelector("#kpis").innerText')).toLowerCase().includes('past deadline'), 'late KPI should appear');
  await shot('02-details');
  await js('document.querySelector("#insp [data-act=details]").click()');
  ok(await js('document.querySelector("#insp").hidden'), 'details panel closes');
});

await test('calendar: change working week, hours/day and add a holiday', async () => {
  await js('document.querySelector("[data-act=calendar]").click()');
  await waitFor(() => js('document.querySelector("#dlg").open'), 'calendar dialog');
  await js(`(() => { const c = document.querySelector('[data-cal="wk-on"][data-d="6"]'); c.checked = true; c.dispatchEvent(new Event('change', { bubbles: true })); })()`); await settle();
  ok(/week Mon-Sat 08:00-12:00 13:00-17:00/.test(file()), 'Saturday working: ' + file().split('\n').filter(l => l.startsWith('week')).join(' / '));
  await js(`(() => { const c = document.querySelector('[data-cal="wk-h"][data-d="6"]'); c.value = '09:00-13:00'; c.dispatchEvent(new Event('change', { bubbles: true })); })()`); await settle();
  ok(/week Sat 09:00-13:00/.test(file()), 'Saturday hours');
  await js(`(() => { const c = document.querySelector('[data-cal="dh"]'); c.value = '7.5'; c.dispatchEvent(new Event('change', { bubbles: true })); })()`); await settle();
  ok(/dayhours 7\.5/.test(file()), 'dayhours');
  await js('document.querySelector("[data-act=ex-add][data-p=off]").click()'); await settle();
  ok((file().match(/^holiday /gm) || []).length === 2, 'holiday added');
  await shot('03-calendar');
  await js('document.querySelector("[data-act=dlg-close]").click()');
  await key('z', 'KeyZ', 90, 2); await settle(); await key('z', 'KeyZ', 90, 2); await settle(); await key('z', 'KeyZ', 90, 2); await settle(); await key('z', 'KeyZ', 90, 2); await settle();
  ok(!/dayhours/.test(file()) && /week Mon-Fri/.test(file()), 'undo restores the calendar');
});

await test('click a timeline date to toggle a holiday', async () => {
  const n = (file().match(/^holiday /gm) || []).length;
  await js('document.querySelector(".days [data-day=\\"10\\"]").click()'); await settle();
  ok((file().match(/^holiday /gm) || []).length === n + 1 && file().includes('holiday 2026-11-19'), 'holiday on Nov 19');
  await js('document.querySelector(".days [data-day=\\"10\\"]").click()'); await settle();
  ok(!file().includes('holiday 2026-11-19'), 'toggled back');
});

await test('level resources (delays, no links) then clear', async () => {
  await js('document.querySelector("[data-menu=level]").click()');
  await js('document.querySelector("#menu [data-act=level]").click()');
  await settle(); await waitFor(() => js('D.tasks.some(t => t.delay > 0)'), 'leveling');
  ok((await js('document.querySelector("#kpis").innerText')).includes('None'), 'nobody over-allocated after leveling');
  ok(/delay=/.test(file()), 'delay saved');
  await js('document.querySelector("[data-menu=level]").click()');
  await js('document.querySelector("#menu [data-act=unlevel]").click()');
  await waitFor(() => js('!D.tasks.some(t => t.delay > 0)'), 'clear leveling');
});

await test('save a baseline, then slips show after a change', async () => {
  await js('document.querySelector("[data-menu=baseline]").click()');
  await js('document.querySelector("#menu [data-act=baseline]").click()');
  await waitFor(() => js('D.tasks.every(t => t.base)'), 'baseline');
  ok(/base=2026-11-09T08:00\//.test(file()), 'baseline saved');
  await js(`(() => { const el = document.querySelector('[data-id="5"][data-f="dur"]'); el.value = '8d'; el.dispatchEvent(new Event('change', { bubbles: true })); })()`); await settle();
  ok(await js('D.tasks.find(t => t.id === 5).slip') > 0, 'Frontend should slip');
  ok((await js('document.querySelector("#kpis").innerText')).toLowerCase().includes('vs baseline'), 'baseline KPI');
  ok((await js('document.querySelector("[data-row=\\"5\\"] .slipc").textContent')) === '+2d', 'slip column for Frontend');
  ok(await js('document.querySelectorAll(".base").length') > 5, 'baseline bars drawn');
  await shot('04-baseline');
});

await test('copy & paste rows (spreadsheet style)', async () => {
  await js(`(() => { const dt = new DataTransfer(); dt.setData('text/plain', 'Task Name\\tDuration\\nDeploy\\t2 days\\n  Smoke test\\t4h\\t\\tCy\\t50'); document.activeElement.blur(); document.body.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true })); })()`);
  await settle();
  const deploy = file().split('\n').find(l => l.includes('| Deploy |')), smoke = file().split('\n').find(l => l.includes('Smoke test'));
  // "Smoke test" is indented under "Deploy", so Deploy becomes its summary.
  ok(deploy && /^\s*task \d+ \| Deploy \|  \|/.test(deploy), 'Deploy should be a summary: ' + deploy);
  ok(smoke && smoke.startsWith('  ') && smoke.includes('| 4h |') && smoke.includes('| Cy | 50'), 'Smoke test: ' + smoke);
});

await test('multi-select with Shift+click and delete', async () => {
  const n = await js('D.tasks.length');
  const ids = await js('D.tasks.slice(-2).map(t => t.id)');
  await js(`document.querySelector('[data-row="${ids[0]}"] .num').click()`);
  const r = await rect(`[data-row="${ids[1]}"] .num`);
  await cdp('Input.dispatchMouseEvent', { type: 'mousePressed', x: r.x + 5, y: r.y + 5, button: 'left', buttons: 1, clickCount: 1, modifiers: 8 });
  await cdp('Input.dispatchMouseEvent', { type: 'mouseReleased', x: r.x + 5, y: r.y + 5, button: 'left', buttons: 0, clickCount: 1, modifiers: 8 });
  ok(await js('sel.size') === 2, 'two selected');
  await key('Delete', 'Delete', 46); await settle();
  ok(await js('D.tasks.length') === n - 2, 'deleted both');
});

await test('people view: edit capacity, rate and days off', async () => {
  await key('3', 'Digit3', 51); await sleep(50);
  ok(await js('document.querySelectorAll("[data-res=name]").length') >= 4, 'people rows');
  const set = (f, v) => js(`(() => { const el = document.querySelector('[data-res="${f}"][data-name="Bo"]'); el.value = ${JSON.stringify(v)}; el.dispatchEvent(new Event('change', { bubbles: true })); })()`);
  await set('capacity', '50%'); await settle();
  await set('rate', '72'); await settle();
  await set('off', '2026-11-20..2026-11-23'); await settle();
  ok(/resource Bo \| rate 72 \| capacity 50 \| off 2026-11-20\.\.2026-11-23/.test(file()), file().split('\n').find(l => l.startsWith('resource Bo')));
  ok(await js('document.querySelectorAll(".cell.over").length') > 0, 'Bo at 50% should be over-allocated');
  await shot('05-people');
});

await test('network view renders a card per task', async () => {
  await key('2', 'Digit2', 50); await sleep(50);
  ok(await js('document.querySelectorAll(".node").length') === await js('D.tasks.filter(t => !t.summary).length'), 'node count');
  ok(await js('document.querySelectorAll(".nedge.c").length') > 0, 'critical edges');
  await shot('06-network');
  await key('1', 'Digit1', 49);
});

await test('import a ProjectLibre / MS Project XML file', async () => {
  const xml = fs.readFileSync(new URL('./fixtures/projectlibre.xml', import.meta.url), 'utf8');
  await js(`(() => { const dt = new DataTransfer(); dt.items.add(new File([${JSON.stringify(xml)}], 'projectlibre.xml')); const i = document.querySelector('#file'); i.files = dt.files; i.dispatchEvent(new Event('change', { bubbles: true })); })()`);
  await waitFor(() => js('D.name === "Office Move"'), 'import');
  ok(file().includes('project Office Move') && file().includes('Pack boxes'), 'imported plan saved');
  ok(await js('D.tasks.some(t => t.summary)'), 'summary tasks imported');
  await key('z', 'KeyZ', 90, 2); await settle();
  ok(file().includes('Website Launch'), 'undo import');
});

await test('Project menu: new blank project (with a welcome screen), then undo', async () => {
  await js('document.querySelector("[data-menu=project]").click()');
  await js('document.querySelector("#menu [data-act=new-project]").click()');
  await waitFor(() => js('D.tasks.length === 0'), 'blank project');
  ok(!/^\s*task /m.test(file()) && file().startsWith('project test\n'), 'file should be empty and named after it: ' + file().slice(0, 60));
  ok(await js('!!document.querySelector(".welcome")'), 'welcome screen');
  await shot('08-blank');
  await js('document.querySelector(".welcome [data-act=sample]").click()');
  await waitFor(() => js('D.name === "Website Launch"'), 'sample from the welcome screen');
  await key('z', 'KeyZ', 90, 2); await settle(); await key('z', 'KeyZ', 90, 2); await settle();
  ok(await js('D.tasks.length') > 5 && file().includes('Website Launch'), 'undo brings the project back');
});

await test('Ctrl+O: create a new project file, then switch back', async () => {
  await js('document.activeElement.blur()');
  await key('o', 'KeyO', 79, 2);
  await waitFor(() => js('document.querySelector("#dlg").open && !!document.querySelector(".fileit.cur")'), 'projects dialog');
  await js(`document.querySelector('#new-file-name').value = 'Office Move 2027'`);
  await js('document.querySelector("[data-act=create-file]").click()');
  await waitFor(() => js('D.name === "Office Move 2027"'), 'new project');
  const created = path.join(tmp, 'Office Move 2027.plan');
  ok(fs.existsSync(created) && fs.readFileSync(created, 'utf8').startsWith('project Office Move 2027'), 'new file on disk');
  ok(await js('undo.length') === 0, 'fresh undo history');
  await js(`(() => { const el = document.querySelector('[data-f="new"]'); el.value = 'Book the van'; el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })); })()`);
  await settle();
  ok(fs.readFileSync(created, 'utf8').includes('Book the van'), 'edits go to the new file');
  ok(!file().includes('Book the van'), 'original file untouched');
  await shot('09-projects');
  await key('o', 'KeyO', 79, 2);
  await waitFor(() => js('!!document.querySelector("[data-act=open-file][data-file=\\"test.plan\\"]")'), 'file list');
  await shot('10-open-dialog');
  await js('document.querySelector("[data-act=open-file][data-file=\\"test.plan\\"]").click()');
  await waitFor(() => js('D.path.endsWith("test.plan")'), 'switch back');
});

await test('external edits to the .plan file reload automatically', async () => {
  fs.writeFileSync(plan, file().replace('project Website Launch', 'project Edited Elsewhere'));
  await waitFor(() => js('D.name === "Edited Elsewhere"'), 'live reload', 6000);
});

await test('stale writes are refused instead of overwriting newer changes', async () => {
  const r = await js(`fetch('/api/save', { method: 'POST', headers: { 'X-Rplan-Token': TOKEN, 'X-Rplan-Rev': '1' }, body: D.text }).then(r => r.status)`);
  ok(r === 409, 'expected 409, got ' + r);
});

await shot('07-final');
console.log(`\n${passed} passed, ${failures} failed`);
ws.close();
for (const p of procs) try { p.kill(); } catch (e) {}
await sleep(300);
try { fs.rmSync(tmp, { recursive: true, force: true }); } catch (e) {}
process.exit(failures ? 1 : 0);
