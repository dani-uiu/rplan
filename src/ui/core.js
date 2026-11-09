"use strict";
/* rplan — core: state, calendar math, parsing, saving, toolbar, KPIs, issues. */

const TOKEN = document.querySelector('meta[name="rplan-token"]').content;
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
if (typeof T !== 'function') window.T = (s, ...a) => String(s).replace(/\{(\d)\}/g, (_, i) => a[i] ?? '');
const ICON = {
  plus: '<path d="M12 5v14M5 12h14"/>', undo: '<path d="M9 14L4 9l5-5"/><path d="M4 9h11a5 5 0 010 10h-3"/>',
  redo: '<path d="M15 14l5-5-5-5"/><path d="M20 9H9a5 5 0 000 10h3"/>', zin: '<circle cx="11" cy="11" r="7"/><path d="M21 21l-4-4M8 11h6M11 8v6"/>',
  zout: '<circle cx="11" cy="11" r="7"/><path d="M21 21l-4-4M8 11h6"/>', fit: '<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>',
  level: '<path d="M3 7h6M3 12h10M3 17h14M17 4l3 3-3 3"/>', fire: '<path d="M12 3s5 4.5 5 9.5a5 5 0 01-10 0c0-2 1-3.5 2-4.5 0 2 1 3 2 3 0-3-1-5 1-8z"/>',
  dl: '<path d="M12 4v11M7 10l5 5 5-5M5 20h14"/>', up: '<path d="M12 20V9M7 14l5-5 5 5M5 4h14"/>', trash: '<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>',
  cal: '<rect x="3" y="5" width="18" height="16" rx="2"/><path d="M3 10h18M8 3v4M16 3v4"/>', ind: '<path d="M4 6h16M10 12h10M10 18h10M4 10l3 2.5L4 15"/>',
  out: '<path d="M4 6h16M10 12h10M10 18h10M7 10l-3 2.5L7 15"/>', side: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/>',
  base: '<path d="M3 17h18M6 13h8M6 9h12"/>', cols: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16M15 4v16"/>',
  chev: '<path d="M6 9l6 6 6-6"/>', search: '<circle cx="11" cy="11" r="7"/><path d="M21 21l-4-4"/>', filter: '<path d="M4 5h16l-6 8v6l-4-2v-4z"/>',
  sort: '<path d="M7 4v16M3 16l4 4 4-4M17 20V4M13 8l4-4 4 4"/>', group: '<rect x="3" y="4" width="18" height="6" rx="1.5"/><rect x="3" y="14" width="18" height="6" rx="1.5"/>',
  bell: '<path d="M6 8a6 6 0 1112 0c0 7 3 8 3 8H3s3-1 3-8M10 20a2 2 0 004 0"/>', report: '<path d="M6 3h9l4 4v14H6z"/><path d="M9 12h6M9 16h6M9 8h3"/>',
  hist: '<path d="M3 12a9 9 0 109-9 9 9 0 00-7 3.4M3 4v4h4"/><path d="M12 7v5l3 2"/>', repeat: '<path d="M17 2l3 3-3 3M3 11V9a4 4 0 014-4h13M7 22l-3-3 3-3M21 13v2a4 4 0 01-4 4H4"/>',
  gantt: '<path d="M3 6h10M7 12h12M5 18h8"/>', net: '<rect x="2" y="4" width="7" height="6" rx="1.5"/><rect x="15" y="14" width="7" height="6" rx="1.5"/><path d="M9 7h3v10h3"/>',
  tree: '<rect x="9" y="2" width="6" height="5" rx="1"/><rect x="2" y="16" width="6" height="5" rx="1"/><rect x="16" y="16" width="6" height="5" rx="1"/><path d="M12 7v4M5 16v-3h14v3"/>',
  usage: '<path d="M4 20V10M10 20V4M16 20v-7M22 20H2"/>', people: '<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20c.8-3.5 3.4-5.5 6.5-5.5s5.7 2 6.5 5.5M16 4.5a3.5 3.5 0 010 7M19 14.8c1.4.9 2.3 2.7 2.6 5.2"/>',
};
const ic = n => `<svg class="i" viewBox="0 0 24 24">${ICON[n]}</svg>`;
function store(k, v) { try { if (v === undefined) return localStorage.getItem('rplan.' + k); localStorage.setItem('rplan.' + k, v); } catch (e) { return null; } }

/* ---------- language & formats ---------- */
let LANG = store('lang') || ((navigator.language || 'en').toLowerCase().startsWith('es') ? 'es' : 'en');
let FMT = {};
function setupFormats() {
  const o = (x) => new Intl.DateTimeFormat(LANG, { timeZone: 'UTC', ...x });
  FMT = {
    nice: o({ weekday: 'short', day: 'numeric', month: 'short' }), full: o({ weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' }),
    mmdd: o({ day: '2-digit', month: '2-digit' }), month: o({ month: 'short', year: 'numeric' }), monthOnly: o({ month: 'short' }),
    wd: o({ weekday: 'short' }), wdLong: o({ weekday: 'long' }), num: new Intl.NumberFormat(LANG, { maximumFractionDigits: 1 }),
    int: new Intl.NumberFormat(LANG, { maximumFractionDigits: 0 }),
  };
  document.documentElement.lang = LANG;
}
setupFormats();
// Weekday names starting Sunday (index = JS getUTCDay).
const DOW_EN = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const wdName = i => FMT.wd.format(new Date(Date.UTC(2026, 10, 1 + i)));
const wdLong = i => FMT.wdLong.format(new Date(Date.UTC(2026, 10, 1 + i)));
const ORDER = [1, 2, 3, 4, 5, 6, 0];
const STD = [[480, 720], [780, 1020]];
const KIND = () => ({ FS: T('Finish → Start'), SS: T('Start → Start'), FF: T('Finish → Finish'), SF: T('Start → Finish') });

/* ---------- state ---------- */
let D = null, undo = [], redo = [], drag = null, dragRow = null, suppressClick = false, dlgKind = null, chain = Promise.resolve();
let sel = new Set(), primary = null, anchor = null, focusCrit = false, files = null, usage = null, holidaysList = null;
let inspTask = null; // the task the details panel currently shows
const VIEWS = ['gantt', 'network', 'wbs', 'usage', 'resources'];
let view = VIEWS.includes(location.hash.slice(1)) ? location.hash.slice(1) : store('view') || 'gantt';
if (!VIEWS.includes(view)) view = 'gantt';
let dw = +store('dw') || 0;
let inspOpen = store('insp') === '1';
let collapsed = new Set();
let search = '', filter = store('filter') || 'all', sortBy = store('sort') || 'file', groupBy = store('group') || 'none';
const RH = 36, PAD = 3, RRH = 52;
const inspW = () => inspOpen && sel.size ? 370 : 0;

/* ---------- calendar & formatting ---------- */
let START = 0, EXC = new Map();
const ymd = s => { const [y, m, d] = s.split('-').map(Number); return Date.UTC(y, m - 1, d); };
const dateOf = d => new Date(START + d * 864e5);
const dateStr = d => dateOf(d).toISOString().slice(0, 10);
const hm = m => `${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
const dtStr = m => `${dateStr(Math.floor(m / 1440))}T${hm(((m % 1440) + 1440) % 1440)}`;
const wdOf = d => dateOf(d).getUTCDay();
const dayHours = d => EXC.has(d) ? EXC.get(d).hours : D.week[wdOf(d)];
const isOff = d => dayHours(d).length === 0;
const nice = d => FMT.nice.format(dateOf(d));
const niceY = d => FMT.full.format(dateOf(d));
const mmdd = d => FMT.mmdd.format(dateOf(d));
const offOf = s => Math.round((ymd(s) - START) / 864e5);
const sDay = t => Math.floor(t.es / 1440);
const fDay = t => t.ef > t.es ? Math.floor((t.ef - 1) / 1440) : Math.floor(t.es / 1440);
const dmin = () => D.dayMinutes;
const hoursStr = h => h.length ? h.map(([a, b]) => `${hm(a)}-${hm(b)}`).join(' ') : 'off';
const hoursLen = h => h.reduce((a, [x, y]) => a + y - x, 0);
const cl = s => String(s ?? '').replace(/[|;\r\n]/g, ' ').trim();
const fieldName = s => String(s ?? '').replace(/[|;=,\r\n]/g, ' ').replace(/\s+/g, ' ').trim();
const rn = s => cl(s).replace(/[,\[\]#]/g, ' ').replace(/\s+/g, ' ').trim();
const num = x => String(Math.round((+x || 0) * 10000) / 10000);
function workDays(a, b) { let n = 0; for (let d = a; d <= b; d++) if (!isOff(d)) n++; return n; }
function fmtDur(m, elapsed) {
  if (!m) return '0';
  if (elapsed) return m % 1440 === 0 ? `${m / 1440}ed` : m % 60 === 0 ? `${m / 60}eh` : `${m}em`;
  const d = dmin(); if (m % d === 0) return `${m / d}d`; if (m % 60 === 0) return `${m / 60}h`;
  // Effort-driven tasks can land on odd minutes: show them as days or hours, not "1280m".
  if (m >= d) return `${+(m / d).toFixed(2)}d`;
  return m >= 60 ? `${+(m / 60).toFixed(2)}h` : `${m}m`;
}
const fmtHours = m => `${FMT.num.format(m / 60)}h`;
function fmtDays(m) { const v = m / dmin(); return (Math.abs(v - Math.round(v)) < 0.05 ? FMT.int.format(Math.round(v)) : FMT.num.format(v)) + 'd'; }
function money(x) { const v = Math.round(+x || 0); return (v < 0 ? '-' : '') + (D.currency || '$') + FMT.int.format(Math.abs(v)); }
function when(m, isEnd) {
  const d = Math.floor((isEnd ? m - 1 : m) / 1440), h = dayHours(d), t = m - d * 1440;
  const edge = h.length && (t === h[0][0] || t === h[h.length - 1][1]);
  return `${nice(d)}${edge ? '' : ' ' + hm(t % 1440)}`;
}
const lagStr = d => d.pct ? (d.pct > 0 ? '+' : '') + d.pct + '%' : d.lag ? (d.lag > 0 ? '+' : '-') + fmtDur(Math.abs(d.lag), d.elapsed) : '';
const kindStr = d => `${d.id}${d.kind === 'FS' && !d.pct ? '' : d.kind}${lagStr(d)}`;
const CONTOURS = ['flat', 'front', 'back', 'bell', 'early', 'late', 'turtle'];
const CONTOUR_LABEL = () => ({ flat: T('Flat'), front: T('Front-loaded'), back: T('Back-loaded'), bell: T('Bell'), early: T('Early peak'), late: T('Late peak'), turtle: T('Turtle') });
// Extra details of one person's assignment, as written inside the brackets.
const asgMore = a => `${a.delay ? ` delay=${fmtDur(a.delay)}` : ''}${a.work != null ? ` work=${fmtHours(a.work)}` : ''}${a.contour && a.contour !== 'flat' ? ` contour=${a.contour}` : ''}`;
// A person's capacity on a given day (it can change by date).
const capOn = (r, d) => { let c = r.capacity || 100; for (const [day, v] of r.caps || []) if (day <= d) c = v; return c; };
// Is this [day, units, …] entry of a person's workload above their capacity that day?
const overOn = l => { const r = resOf(l.name) || l; return ([d, u]) => u > capOn(r, d); };
const depsStr = t => t.deps.map(kindStr).join(', ');
const resOf = name => D.resources.find(r => r.name === name);
function whoStr(t) {
  return t.who.map(a => a.form === 'qty' ? `${a.name}[${num(a.amount)}]` : a.form === 'money' ? `${a.name}[$${num(a.amount)}]` : a.units === 100 && !asgMore(a) ? a.name : `${a.name}[${a.units}%${asgMore(a)}]`).join(', ');
}
function nowRel() { const n = new Date(); return Math.round((Date.UTC(n.getFullYear(), n.getMonth(), n.getDate(), n.getHours(), n.getMinutes()) - START) / 60000); }
const todayOff = () => Math.floor(nowRel() / 1440);
function noClick() { suppressClick = true; setTimeout(() => { suppressClick = false; }, 0); }
const localDate = () => { const n = new Date(); return `${n.getFullYear()}-${String(n.getMonth() + 1).padStart(2, '0')}-${String(n.getDate()).padStart(2, '0')}`; };

/* ---------- parsing user input (mirrors the .plan format) ---------- */
function parseDurE(s) {
  s = String(s).trim().toLowerCase().replace(/\?$/, '').replace(',', '.');
  const m = s.match(/^(\d+(?:\.\d+)?|\.\d+)\s*([a-z]*)$/);
  if (!m) throw new Error(T('"{0}" isn\'t a duration — try 3d, 4h, 30m, 2w, 3ed or 0 for a milestone', s));
  const u = m[2] || 'd', n = +m[1];
  const table = [[/^(d|days?|dias?|días?)$/, dmin(), false], [/^(h|hrs?|hours?|horas?)$/, 60, false], [/^(m|mins?|minutes?|minutos?)$/, 1, false], [/^(w|wks?|weeks?|sem|semanas?)$/, dmin() * 5, false], [/^(mo|mon|months?|mes|meses)$/, dmin() * 20, false], [/^(emo|emonths?)$/, 43200, true], [/^(ed|edays?)$/, 1440, true], [/^(eh|ehours?)$/, 60, true], [/^(em|emin)$/, 1, true], [/^(ew|eweeks?)$/, 10080, true]];
  const hit = table.find(([re]) => re.test(u));
  if (!hit) throw new Error(T('"{0}": unknown unit — use d, h, m, w or ed', s));
  return [Math.round(n * hit[1]), hit[2]];
}
const parseDur = s => parseDurE(s)[0];
function parseEffort(s) { s = String(s).trim(); if (!s) return null; const [v] = parseDurE(/^\d+(\.\d+)?$/.test(s) ? s + 'h' : s); return v; }
function parseLagE(s) {
  s = String(s).trim(); if (!s || s === '0') return [0, false, null];
  const m = s.match(/^([+-])?\s*(.+)$/), p = m[2].match(/^(\d+(?:\.\d+)?)\s*%$/);
  if (p) return [0, false, (m[1] === '-' ? -1 : 1) * Math.round(+p[1])];
  const [v, e] = parseDurE(m[2]); return [m[1] === '-' ? -v : v, e, null];
}
function parseDeps(s) {
  const out = [];
  for (const part of String(s).split(',').map(x => x.trim()).filter(Boolean)) {
    const m = part.match(/^#?(\d+)\s*(FS|SS|FF|SF|FC|CC|FF|CF)?\s*(?:([+-])\s*(.+))?$/i);
    if (!m) throw new Error(T('"{0}" isn\'t a link — examples: 3, 3SS, 3FS+2d, 3FF-4h', part));
    const kind = { FC: 'FS', CC: 'SS', CF: 'SF' }[(m[2] || '').toUpperCase()] || (m[2] || 'FS').toUpperCase();
    let lag = 0, elapsed = false, pct = null;
    if (m[3]) [lag, elapsed, pct] = parseLagE(m[3] + m[4]);
    out.push({ id: +m[1], kind, lag, elapsed, pct });
  }
  return out;
}
function parseWho(s) {
  const out = [];
  for (const part of String(s).split(',').map(x => x.trim()).filter(Boolean)) {
    const m = part.match(/^(.+?)\s*(?:\[([^\]]*)\]?)?$/);
    const name = rn(m[1]);
    if (!name) throw new Error(T('"{0}": missing a name', part));
    let money = false, pctSign = false, amount = null, delay = 0, work = null, contour = 'flat';
    for (const tok of (m[2] || '').replace(/\$\s+/g, '$').replace(/\s+%/g, '%').trim().split(/\s+/).filter(Boolean)) {
      const kv = tok.match(/^(delay|work|contour)=(.+)$/i);
      if (kv) {
        const k = kv[1].toLowerCase();
        if (k === 'delay') delay = parseDur(kv[2]);
        else if (k === 'work') work = parseEffort(kv[2]);
        else { contour = kv[2].toLowerCase(); if (!CONTOURS.includes(contour)) throw new Error(T('"{0}": the contour can be flat, front, back, bell, early, late or turtle', part)); }
        continue;
      }
      const am = tok.match(/^(\$)?([\d.,]+)(%)?$/);
      if (!am) throw new Error(T('"{0}": units must be 1–1000%', part));
      money = !!am[1]; pctSign = !!am[3]; amount = +am[2].replace(',', '.');
    }
    const r = resOf(name);
    let a;
    if (money || r?.kind === 'cost') a = { name, units: 100, amount: amount ?? 0, form: 'money' };
    else if (r?.kind === 'material' && !pctSign) a = { name, units: 100, amount: amount ?? 1, form: 'qty' };
    else { const u = amount == null ? 100 : Math.round(amount); if (u < 1 || u > 1000) throw new Error(T('"{0}": units must be 1–1000%', part)); a = { name, units: u, amount: u, form: 'pct', delay, work, contour }; }
    if (!out.some(x => x.name === name)) out.push(a);
  }
  return out;
}
function parseHours(s) {
  s = String(s).trim();
  if (!s || /^(off|libre|no)$/i.test(s)) return [];
  const out = [];
  for (const p of s.split(/[\s,]+/).filter(Boolean)) {
    const m = p.match(/^(\d{1,2}):(\d{2})-(\d{1,2}):(\d{2})$/);
    if (!m) throw new Error(T('"{0}" isn\'t a time range — use 08:00-12:00', p));
    const a = +m[1] * 60 + +m[2], b = +m[3] * 60 + +m[4];
    if (b <= a || b > 1440 || +m[2] > 59 || +m[4] > 59) throw new Error(T('"{0}" isn\'t a valid range', p));
    out.push([a, b]);
  }
  out.sort((x, y) => x[0] - y[0]);
  for (let i = 1; i < out.length; i++) if (out[i][0] < out[i - 1][1]) throw new Error(T('Those working hours overlap'));
  return out;
}
function parseRanges(s) {
  return String(s).split(',').map(x => x.trim()).filter(Boolean).map(p => {
    const m = p.match(/^(\d{4}-\d{2}-\d{2})(?:\s*\.\.\s*(\d{4}-\d{2}-\d{2}))?$/);
    if (!m) throw new Error(T('"{0}" — use dates like 2026-12-24 or 2026-12-24..2026-12-31', p));
    const a = offOf(m[1]), b = offOf(m[2] || m[1]);
    if (b < a) throw new Error(T('"{0}" ends before it starts', p));
    return [a, b];
  });
}
const rangeStr = (a, b) => a === b ? dateStr(a) : `${dateStr(a)}..${dateStr(b)}`;

/* ---------- tree helpers ---------- */
function parentsOf(ts) { const st = [], out = []; ts.forEach((t, i) => { while (st.length && ts[st[st.length - 1]].level >= t.level) st.pop(); out[i] = st.length ? st[st.length - 1] : -1; st.push(i); }); return out; }
function subtreeEnd(ts, i) { let j = i + 1; while (j < ts.length && ts[j].level > ts[i].level) j++; return j; }
function relatives(i) {
  const par = parentsOf(D.tasks), out = new Set([D.tasks[i].id]);
  for (let p = par[i]; p >= 0; p = par[p]) out.add(D.tasks[p].id);
  for (let j = i + 1; j < subtreeEnd(D.tasks, i); j++) out.add(D.tasks[j].id);
  return out;
}
const tIdx = id => D.tasks.findIndex(t => t.id === id);
const task = id => D.tasks.find(t => t.id === id);

/* ---------- server ---------- */
async function api(method, url, body, headers = {}) {
  let r;
  try { r = await fetch(url, { method, body, headers: { 'X-Rplan-Token': TOKEN, ...(D && method !== 'GET' ? { 'X-Rplan-Rev': D.rev } : {}), ...headers } }); }
  catch (e) { return { ok: false, error: T("Can't reach rplan — is it still running?") }; }
  let j; try { j = await r.json(); } catch (e) { j = { ok: false, error: T('Unexpected response ({0})', r.status) }; }
  if (r.status === 409) { toast(tr(j.error), 1); await load(); j.handled = true; }
  return j;
}
function accept(j) {
  D = j; START = ymd(D.start); EXC = new Map();
  for (const [d, name] of D.holidays) EXC.set(d, { from: d, to: d, name, hours: [] });
  for (const e of D.exceptions) for (let d = e.from; d <= e.to && d <= e.from + 4000; d++) EXC.set(d, e);
  const ids = new Set(D.tasks.map(t => t.id));
  sel = new Set([...sel].filter(id => ids.has(id)));
  if (!ids.has(primary)) primary = sel.size ? [...sel][sel.size - 1] : null;
  try { collapsed = new Set(JSON.parse(store('collapsed:' + D.path) || '[]')); } catch (e) { collapsed = new Set(); }
  $('#dl-people').innerHTML = D.resources.map(r => `<option value="${esc(r.name)}">`).join('');
  usage = null;
}
let firstLoad = true;
async function load() {
  const j = await api('GET', '/api/project');
  if (!j.ok) return toast(tr(j.error), 1);
  accept(j);
  if (firstLoad && !dw) { firstLoad = false; return fit(); }
  firstLoad = false; render();
}
function model() {
  return {
    name: D.name, start: D.start, status: D.status, currency: D.currency, dayMinutes: D.dayMinutes, pool: D.pool,
    week: D.week.map(h => h.map(x => [...x])),
    exceptions: D.exceptions.map(e => ({ ...e, hours: e.hours.map(x => [...x]) })),
    recurring: [...D.recurring],
    calendars: D.calendars.map(c => ({ name: c.name, week: c.week.map(h => h.map(x => [...x])) })),
    resources: D.resources.filter(r => r.declared && !r.pooled).map(r => ({ ...r, off: r.off.map(x => [...x]), week: r.week ? r.week.map(h => h.map(x => [...x])) : null, rates: r.rates.map(x => [...x]), caps: r.caps.map(x => [...x]) })),
    fields: D.fields.map(f => ({ ...f, values: [...f.values] })), leveling: { ...D.leveling },
    tasks: D.tasks.map(t => ({
      id: t.id, name: t.name, level: t.level, dur: t.dur, elapsed: t.elapsed, work: t.work, deps: t.deps.map(d => ({ ...d })), who: t.who.map(a => ({ ...a })), pct: t.pct,
      constraint: t.constraint ? { ...t.constraint } : null, deadline: t.deadline, fixed: t.fixed, delay: t.delay, baselines: t.baselines.map(b => ({ ...b })), note: t.note,
      as: t.as, af: t.af, ac: t.ac, ot: t.ot, splits: t.splits.map(x => [...x]), manual: t.manual, inactive: t.inactive, priority: t.priority, cal: t.cal, sub: t.sub, xdeps: t.xdeps.map(x => ({ ...x })),
      fixedUnits: t.fixedUnits, lsplits: t.lsplits.map(x => [...x]), x: { ...t.x },
    })),
  };
}
const mt = (m, id) => m.tasks.find(t => t.id === id);
function ensureRes(m, name) {
  let r = m.resources.find(r => r.name === name);
  if (!r) {
    const src = D.resources.find(r => r.name === name);
    r = src ? { ...src, off: src.off.map(x => [...x]), week: src.week ? src.week.map(h => h.map(x => [...x])) : null, rates: src.rates.map(x => [...x]), caps: src.caps.map(x => [...x]) } : { name, kind: 'work', rate: 0, capacity: 100, off: [], perUse: 0, overtime: 0, unit: '', week: null, rates: [], caps: [], accrue: 'prorated', group: '', initials: '', email: '', note: '' };
    r.declared = true; r.pooled = false; m.resources.push(r);
  }
  return r;
}
const weekLines = w => w.map((h, i) => `${DOW_EN[i]} ${hoursStr(h)}`);
function toText(m) {
  const L = [], lines = [];
  const add = (s, id = null) => { L.push(s); lines.push(id); };
  add(`project ${cl(m.name) || 'Untitled'}`);
  add(`start ${m.start}`);
  if (m.status != null) add(`status ${dateStr(m.status)}`);
  if (m.currency && m.currency !== '$') add(`currency ${cl(m.currency)}`);
  if (m.pool) add(`pool ${cl(m.pool)}`);
  weekLines(m.week).forEach(w => add(`week ${w}`));
  add(`dayhours ${+(m.dayMinutes / 60).toFixed(4)}`);
  for (const e of m.exceptions) add(e.hours.length ? `workday ${rangeStr(e.from, e.to)} ${hoursStr(e.hours)} ${cl(e.name)}`.trim() : `holiday ${rangeStr(e.from, e.to)} ${cl(e.name)}`.trim());
  for (const r of m.recurring) add(`holiday every ${cl(r)}`);
  for (const c of m.calendars) add(`calendar ${cl(c.name)}${weekLines(c.week).map(w => ` | week ${w}`).join('')}`);
  for (const r of m.resources) {
    let s = `resource ${rn(r.name)}`;
    if (r.kind && r.kind !== 'work') s += ` | type ${r.kind}`;
    if (r.unit) s += ` | unit ${rn(r.unit)}`;
    s += ` | rate ${num(r.rate)} | capacity ${r.capacity || 100}`;
    if (+r.perUse) s += ` | peruse ${num(r.perUse)}`;
    if (+r.overtime) s += ` | overtime ${num(r.overtime)}`;
    for (const [d, v] of r.rates || []) s += ` | rate ${num(v)} from ${dateStr(d)}`;
    for (const [d, v] of r.caps || []) s += ` | capacity ${v} from ${dateStr(d)}`;
    if (r.accrue && r.accrue !== 'prorated') s += ` | accrue ${r.accrue}`;
    for (const k of ['group', 'initials', 'email', 'note']) if (cl(r[k])) s += ` | ${k} ${cl(r[k])}`;
    if (r.week) s += weekLines(r.week).map(w => ` | week ${w}`).join('');
    if (r.off.length) s += ` | off ${r.off.map(([a, b]) => rangeStr(a, b)).join(', ')}`;
    add(s);
  }
  for (const f of m.fields || []) add(`field ${fieldName(f.name)} | ${f.kind}${f.values.length ? ' | ' + f.values.map(v => cl(v).replace(/,/g, ' ')).join(', ') : ''}`);
  const lv = m.leveling || {};
  if (lv.slack || lv.split || (lv.order && lv.order !== 'standard')) add(`leveling ${[lv.slack ? 'slack' : '', lv.split ? 'split' : '', lv.order && lv.order !== 'standard' ? 'order=' + lv.order : ''].filter(Boolean).join(' ')}`);
  for (const t of m.tasks) {
    const at = [];
    if (t.constraint) at.push(t.constraint.type === 'alap' ? 'alap' : `${t.constraint.type}=${dtStr(t.constraint.at)}`);
    if (t.deadline != null) at.push(`deadline=${dateStr(t.deadline)}`);
    if (t.work != null) at.push(`work=${Math.round(t.work)}m`);
    if (+t.fixed) at.push(`cost=${num(t.fixed)}`);
    if (t.delay > 0) at.push(`delay=${t.delay}m`);
    if (t.as != null) at.push(`as=${dtStr(t.as)}`);
    if (t.af != null) at.push(`af=${dtStr(t.af)}`);
    if (t.ac != null) at.push(`ac=${num(t.ac)}`);
    if (t.ot > 0) at.push(`ot=${t.ot}m`);
    if (t.splits.length) at.push(`split=${t.splits.map(([a, b]) => rangeStr(a, b)).join(',')}`);
    if (t.manual != null) at.push(`manual=${dtStr(t.manual)}`);
    if (t.inactive) at.push('inactive');
    if (t.priority !== 500) at.push(`priority=${t.priority}`);
    if (t.cal) at.push(`cal=${cl(t.cal)}`);
    if (t.sub) at.push(`sub=${cl(t.sub)}`);
    if (t.fixedUnits && t.work != null) at.push('type=units');
    if (t.lsplits?.length) at.push(`lsplit=${t.lsplits.map(([a, b]) => rangeStr(a, b)).join(',')}`);
    for (const [k, v] of Object.entries(t.x || {})) if (fieldName(k) && cl(v)) at.push(`x:${fieldName(k)}=${cl(v)}`);
    if (t.xdeps.length) at.push(`ext=${t.xdeps.map(x => `${cl(x.file).replace(/,/g, ' ')}#${x.id}${x.kind === 'FS' ? '' : x.kind}${x.lag ? (x.lag > 0 ? '+' : '-') + Math.abs(x.lag) + 'm' : ''}`).join(',')}`);
    for (const b of t.baselines) at.push(`base${b.n || ''}=${dtStr(b.s)}/${dtStr(b.f)}/${num(b.cost)}/${Math.round(b.work)}`);
    if (t.note) at.push(`note=${cl(t.note)}`);
    const deps = t.deps.map(d => `${d.id}${d.kind}${d.pct ? (d.pct > 0 ? '+' : '') + d.pct + '%' : d.lag ? (d.lag > 0 ? '+' : '-') + Math.abs(d.lag) + (d.elapsed ? 'em' : 'm') : ''}`).join(',');
    const more = a => `${a.delay ? ` delay=${Math.round(a.delay)}m` : ''}${a.work != null ? ` work=${Math.round(a.work)}m` : ''}${a.contour && a.contour !== 'flat' ? ` contour=${a.contour}` : ''}`;
    const who = t.who.map(a => a.form === 'qty' ? `${rn(a.name)}[${num(a.amount)}]` : a.form === 'money' ? `${rn(a.name)}[$${num(a.amount)}]` : a.units === 100 && !more(a) ? rn(a.name) : `${rn(a.name)}[${a.units}%${more(a)}]`).join(',');
    const dur = `${Math.max(0, Math.round(t.dur))}${t.elapsed ? 'em' : 'm'}`;
    add(`${'  '.repeat(Math.max(0, t.level))}task ${t.id} | ${cl(t.name) || 'New task'} | ${dur} | ${deps} | ${who} | ${t.pct}${at.length ? ' | ' + at.join('; ') : ''}`, t.id);
  }
  return { text: L.join('\n') + '\n', lines };
}
function friendly(err, lines) { const m = /^line (\d+): ([\s\S]*)$/.exec(err || ''); if (m) { const id = lines[+m[1] - 1]; return id ? T('Task #{0}: {1}', id, tr(m[2])) : tr(m[2]); } return tr(err); }
function remember(text) { undo.push(text); if (undo.length > 300) undo.shift(); redo = []; }
function enqueue(fn) { const p = chain.then(fn); chain = p.catch(() => {}); return p; }
function commit(fn, msg) { return enqueue(() => doCommit(fn, msg)); }
async function doCommit(fn, msg) {
  const m = model();
  let r;
  try { r = fn(m); } catch (e) { toast(e.message, 1); render(); return false; }
  if (r === false) { render(); return false; }
  const { text, lines } = toText(m);
  const prev = D, note = typeof msg === 'string' ? msg : 'Edit';
  const j = await api('POST', '/api/save?note=' + encodeURIComponent(note.slice(0, 120)), text);
  if (!j.ok) { if (!j.handled) toast(friendly(j.error, lines), 1); render(); return false; }
  if (j.text !== prev.text) remember(prev.text);
  accept(j); render(); saved();
  const note2 = typeof msg === 'function' ? msg(j) : msg;
  if (note2) toast(note2);
  warnChanges(prev, j);
  return true;
}
function action(url, confirmMsg) {
  if (confirmMsg && !confirm(confirmMsg)) return;
  return enqueue(async () => {
    const prev = D, j = await api('POST', url);
    if (!j.ok) { if (!j.handled) toast(tr(j.error), 1); return; }
    if (j.text !== prev.text) remember(prev.text);
    accept(j); render(); saved(); if (j.note) toast(tr(j.note));
    warnChanges(prev, j);
  });
}
function restore(from, to, msg) {
  return enqueue(async () => {
    if (!from.length) return toast(msg === 'Undone' ? T('Nothing to undo') : T('Nothing to redo'));
    const text = from.pop(), cur = D.text, j = await api('POST', '/api/save?note=' + encodeURIComponent(msg), text);
    if (!j.ok) { from.push(text); if (!j.handled) toast(tr(j.error), 1); return; }
    to.push(cur); accept(j); render(); saved(); toast(T(msg));
  });
}
const doUndo = () => restore(undo, redo, 'Undone');
const doRedo = () => restore(redo, undo, 'Redone');

/* ---------- live warnings after a change ---------- */
function warnChanges(prev, next) {
  if (!prev || !next || prev.path !== next.path) return;
  const msgs = [];
  const pf = Math.floor((prev.finish - 1) / 1440), nf = Math.floor((next.finish - 1) / 1440);
  if (nf > pf && prev.tasks.length) msgs.push(T('This pushes the finish from {0} to {1} (+{2} working days)', nice(pf), nice(nf), workDays(pf + 1, nf)));
  const wasLate = new Set(prev.tasks.filter(t => t.late).map(t => t.id));
  const nowLate = next.tasks.filter(t => t.late && !wasLate.has(t.id));
  if (nowLate.length) msgs.push(T('{0} will now miss its deadline', nowLate.map(t => `“${t.name}”`).join(', ')));
  const overBefore = new Map(prev.loads.map(l => [l.name, l.over]));
  for (const l of next.loads) if (l.over > (overBefore.get(l.name) || 0)) {
    const first = l.days.find(overOn(l));
    msgs.push(T('{0} is now over-booked ({1} days, starting {2})', l.name, l.over, first ? nice(first[0]) : ''));
  }
  for (const m of msgs.slice(0, 3)) toast('⚠ ' + m, 'warn');
}

/* ---------- issues ---------- */
function behindOf(t, now) {
  if (t.summary || t.edur === 0 || t.done || t.inactive) return false;
  const st = D.status != null ? (D.status + 1) * 1440 : now;
  if (st <= t.es) return false;
  const b = t.baselines.find(x => x.n === 0), s = b ? b.s : t.es, f = b ? b.f : t.ef;
  const exp = Math.min(1, Math.max(0, (st - s) / Math.max(1, f - s))) * 100;
  return t.pct + 10 < exp;
}
function issues() {
  const out = [], now = nowRel();
  for (const w of D.warnings) out.push({ icon: '🔗', text: tr(w), sub: T('Linked file problem') });
  for (const t of D.tasks) {
    if (t.inactive) continue;
    if (t.late) out.push({ icon: '⏰', text: T('“{0}” finishes after its deadline', t.name), sub: T('Finish {0} · deadline {1}', when(t.ef, true), nice(t.deadline)), task: t.id, sev: 2 });
    if (t.conflict) out.push({ icon: '📌', text: T('“{0}”: its date constraint can\'t be met', t.name), sub: consLabel(t.constraint), task: t.id, sev: 2 });
    if (behindOf(t, now)) out.push({ icon: '🐢', text: T('“{0}” is behind schedule', t.name), sub: T('{0}% done', t.pct), task: t.id, sev: 1 });
  }
  for (const l of D.loads) {
    if (!l.over) continue;
    const weeks = [...new Set(l.days.filter(overOn(l)).map(([d]) => d - ((wdOf(d) + 6) % 7)))];
    out.push({ icon: '👥', text: T('{0} is over-booked on {1} day(s)', l.name, l.over), sub: weeks.slice(0, 4).map(w => T('week of {0}', nice(w))).join(' · '), person: l.name, sev: 2 });
  }
  const unassigned = D.tasks.filter(t => !t.summary && t.edur > 0 && !t.who.length && !t.sub && !t.inactive).length;
  if (unassigned && D.resources.length) out.push({ icon: 'ℹ️', text: T('{0} task(s) have nobody assigned', unassigned), sub: T('Filter: Unassigned'), filter: 'unassigned', sev: 0 });
  return out.sort((a, b) => (b.sev || 0) - (a.sev || 0));
}

/* ---------- render ---------- */
function focusSel(el) {
  if (!el || !el.dataset) return null;
  const keys = ['id', 'f', 'insp', 'k', 'cal', 'd', 'res', 'name', 'c'].filter(k => el.dataset[k] !== undefined);
  if (!keys.length) return el.id === 'q' ? '#q' : null;
  return keys.map(k => `[data-${k}="${CSS.escape(el.dataset[k])}"]`).join('');
}
function render() {
  if (!D) return;
  const a = document.activeElement, inInsp = !!a?.closest?.('#insp');
  const fsel = focusSel(a);
  // Details-panel fields aren't tied to a task id: if the panel now shows a different
  // task, don't carry focus or half-typed text over (it would edit the wrong task).
  const sameTask = !inInsp || inspTask === primary, keepVal = sameTask;
  const fval = a && 'value' in a ? a.value : null, fs = a && a.selectionStart, fe = a && a.selectionEnd;
  const v = $('#main .view'), scroll = v ? [v.dataset.v, v.scrollLeft, v.scrollTop] : null, iscroll = $('#insp').scrollTop;
  document.title = `${D.name} · rplan`;
  if (document.activeElement !== $('#pname')) $('#pname').value = D.name;
  $('#pstart').value = D.start;
  $('#pstatus').value = D.status != null ? dateStr(D.status) : '';
  $('#fpath').textContent = D.path;
  renderTabs(); renderToolbar(); renderKpis(); renderHints();
  const html = view === 'gantt' ? gantt() : view === 'network' ? network() : view === 'wbs' ? wbsView() : view === 'usage' ? usageView() : people();
  $('#main').innerHTML = html;
  const nv = $('#main .view');
  if (scroll && nv && nv.dataset.v === scroll[0]) { nv.scrollLeft = scroll[1]; nv.scrollTop = scroll[2]; }
  if (view === 'gantt') ganttMounted();
  inspector(); $('#insp').scrollTop = iscroll;
  if (dlgKind && $('#dlg').open) renderDialog();
  if (typeof printOpen === 'function' && printOpen()) renderPreview();
  if (fsel && sameTask) {
    const el = document.querySelector(fsel);
    if (el && el !== document.activeElement) {
      el.focus();
      if (keepVal && fval != null && el.value !== fval && el.type !== 'checkbox' && el.tagName !== 'SELECT') el.value = fval;
      try { if (fs != null) el.setSelectionRange(fs, fe); } catch (e) {}
    }
  }
}
function renderTabs() {
  const tabs = [['gantt', 'gantt', T('Gantt')], ['network', 'net', T('Network')], ['wbs', 'tree', T('WBS')], ['usage', 'usage', T('Usage')], ['resources', 'people', T('People')]];
  $('#tabs').innerHTML = tabs.map(([v, i, l]) => `<button data-v="${v}" class="${view === v ? 'on' : ''}">${ic(i)}<span class="hide-m">${l}</span></button>`).join('');
  $('#lang').textContent = LANG === 'es' ? 'EN' : 'ES';
  $$('[data-t]').forEach(el => { el.dataset.en = el.dataset.en || el.textContent; el.textContent = T(el.dataset.en); });
}
function renderHints() {
  $('#hints').innerHTML = [[T('N'), T('new')], ['Tab', T('indent')], ['Del', T('delete')], ['Ctrl+Z', T('undo')], ['Ctrl+F', T('search')], ['?', T('all shortcuts')]].map(([k, v]) => `<span><kbd>${k}</kbd> ${v}</span>`).join('');
  $('#saved').textContent = '● ' + T('Saved');
}
function renderToolbar() {
  const hasBase = D.tasks.some(t => t.baselines.length), leveled = D.tasks.some(t => t.delay > 0), n = sel.size, iss = issues();
  const sevN = iss.filter(i => i.sev >= 1).length;
  const scales = [['D', 30, T('Days')], ['W', 10, T('Weeks')], ['M', 3, T('Months')], ['Q', 1, T('Quarters')], ['Y', 0.35, T('Years')]];
  const cur = scales.reduce((b, s) => Math.abs(Math.log(s[1] / dw)) < Math.abs(Math.log(b[1] / dw)) ? s : b, scales[0]);
  const filtered = filter !== 'all' || search || sortBy !== 'file' || groupBy !== 'none';
  $('#toolbar').innerHTML = `
    <button class="btn primary" data-act="add" title="${T('New task')} (N)">${ic('plus')}${T('Task')}</button>
    <button class="btn icon" data-act="outdent" title="${T('Outdent')} (Shift+Tab)" ${n ? '' : 'disabled'}>${ic('out')}</button>
    <button class="btn icon" data-act="indent" title="${T('Indent — make a subtask')} (Tab)" ${n ? '' : 'disabled'}>${ic('ind')}</button>
    <span class="sep"></span>
    <button class="btn icon" data-act="undo" title="${T('Undo')} (Ctrl+Z)" ${undo.length ? '' : 'disabled'}>${ic('undo')}</button>
    <button class="btn icon" data-act="redo" title="${T('Redo')} (Ctrl+Y)" ${redo.length ? '' : 'disabled'}>${ic('redo')}</button>
    <span class="sep"></span>
    <label class="search" title="Ctrl+F">${ic('search')}<input id="q" value="${esc(search)}" placeholder="${T('Search tasks…')}" autocomplete="off"></label>
    <button class="btn ${filter !== 'all' ? 'on' : ''}" data-menu="filter" title="${T('Filter')}">${ic('filter')}<span class="hide-l">${T('Filter')}</span><span class="caret">▾</span></button>
    <button class="btn icon ${sortBy !== 'file' ? 'on' : ''}" data-menu="sort" title="${T('Sort')}">${ic('sort')}</button>
    <button class="btn icon ${groupBy !== 'none' ? 'on' : ''}" data-menu="group" title="${T('Group')}">${ic('group')}</button>
    <button class="btn icon" data-menu="columns" title="${T('Columns')}">${ic('cols')}</button>
    ${filtered ? `<button class="btn sm" data-act="clear-filters">${T('Clear')}</button>` : ''}
    <span class="sep"></span>
    <div class="seg" title="${T('Time scale')}">${scales.map(([k, v, l]) => `<button data-act="scale" data-v="${v}" class="${cur === scales.find(s => s[0] === k) && view !== 'network' && view !== 'wbs' ? 'on' : ''}" title="${l}">${T(k)}</button>`).join('')}</div>
    <button class="btn icon" data-act="zout" title="${T('Zoom out')} (-)">${ic('zout')}</button>
    <button class="btn icon" data-act="zin" title="${T('Zoom in')} (+)">${ic('zin')}</button>
    <button class="btn icon" data-act="fit" title="${T('Fit to window')} (F)">${ic('fit')}</button>
    <span class="sep"></span>
    <button class="btn icon ${focusCrit ? 'hot' : ''}" data-act="crit" title="${T('Critical path')} (C)">${ic('fire')}</button>
    <button class="btn" data-menu="level" title="${T('Resource leveling')}">${ic('level')}<span class="hide-l">${T('Level')}</span>${leveled ? ' <span class="chip y">on</span>' : ''}<span class="caret">▾</span></button>
    <button class="btn" data-menu="baseline" title="${T('Baselines')}">${ic('base')}<span class="hide-l">${T('Baseline')}</span>${hasBase ? ' <span class="chip n">✓</span>' : ''}<span class="caret">▾</span></button>
    <button class="btn" data-act="calendar" title="${T('Working days, hours and holidays')}">${ic('cal')}<span class="hide-l">${T('Calendar')}</span></button>
    <span class="grow"></span>
    <button class="btn ${sevN ? 'hot' : ''}" data-menu="issues" title="${T('Problems')}">${ic('bell')}${iss.length ? `<span class="badge-n ${sevN ? '' : 'y'}">${iss.length}</span>` : ''}</button>
    <button class="btn" data-menu="project">${ic('plus')}<span class="hide-l">${T('Project')}</span><span class="caret">▾</span></button>
    <button class="btn" data-act="reports" title="${T('Printable reports')}">${ic('report')}<span class="hide-l">${T('Reports')}</span></button>
    <button class="btn" data-menu="export">${ic('dl')}<span class="hide-l">${T('Export')}</span><span class="caret">▾</span></button>
    <button class="btn icon ${inspOpen ? 'on' : ''}" data-act="details" title="${T('Details panel')} (Enter)">${ic('side')}</button>`;
}
function renderKpis() {
  const leaves = D.tasks.filter(t => !t.summary && !t.inactive), now = nowRel();
  const over = D.loads.filter(l => l.over > 0), crit = leaves.filter(t => t.crit).length;
  const late = D.tasks.filter(t => t.late).length, behind = leaves.filter(t => behindOf(t, now)).length;
  const based = D.tasks.filter(t => t.baselines.some(b => b.n === 0));
  let extra = '';
  if (based.length) {
    const bf = Math.max(...based.map(t => t.baselines.find(b => b.n === 0).f)), a = Math.floor((bf - 1) / 1440), b = Math.floor((D.finish - 1) / 1440);
    const s = a <= b ? workDays(a + 1, b) : -workDays(b + 1, a);
    extra += `<div class="kpi ${s > 0 ? 'warn' : 'good'}" title="${T('Finish compared with the saved baseline')}"><span>${T('vs baseline')}</span><b>${s > 0 ? T('+{0}d late', s) : s < 0 ? T('{0}d early', -s) : T('on time')}</b></div>`;
  }
  if (D.status != null && D.ev.bac > 0) {
    const spi = D.ev.pv > 0 ? D.ev.ev / D.ev.pv : null, cpi = D.ev.ac > 0 ? D.ev.ev / D.ev.ac : null;
    const cls = x => x == null ? '' : x >= 0.98 ? 'good' : x >= 0.9 ? 'amber' : 'warn';
    extra += `<div class="kpi click ${cls(spi)}" data-act="report" data-r="ev" title="${T('Schedule performance index: earned ÷ planned value')}"><span>SPI</span><b>${spi == null ? '—' : spi.toFixed(2)}</b></div>`;
    extra += `<div class="kpi click ${cls(cpi)}" data-act="report" data-r="ev" title="${T('Cost performance index: earned value ÷ actual cost')}"><span>CPI</span><b>${cpi == null ? '—' : cpi.toFixed(2)}</b></div>`;
  }
  $('#kpis').innerHTML = `
    <div class="kpi"><span>${T('Finish')}</span><b>${D.tasks.length ? niceY(Math.floor((D.finish - 1) / 1440)) : '—'}</b></div>
    <div class="kpi"><span>${T('Duration')}</span><b>${fmtDays(D.span)} <small>${T('work')}</small></b></div>
    <div class="kpi" style="min-width:140px"><span>${T('Progress')}</span><b>${(+D.progress).toFixed(0)}%</b><div class="pbar"><div style="width:${D.progress}%"></div></div></div>
    <div class="kpi"><span>${T('Cost')}</span><b>${money(D.cost)}</b></div>
    <div class="kpi"><span>${T('Work')}</span><b>${fmtHours(D.work)}</b></div>
    <div class="kpi"><span>${T('Tasks')}</span><b>${leaves.length}</b></div>
    <div class="kpi click ${crit ? 'warn' : ''}" data-act="filter-to" data-v="critical"><span>${T('Critical')}</span><b>${crit}</b></div>
    <div class="kpi click ${over.length ? 'warn' : 'good'}" data-act="go-people" title="${esc(over.map(l => l.name).join(', '))}"><span>${T('Over-allocated')}</span><b>${over.length === 1 ? T('1 person') : over.length ? T('{0} people', over.length) : T('None ✓')}</b></div>
    ${late ? `<div class="kpi click warn" data-act="filter-to" data-v="late"><span>${T('Past deadline')}</span><b>${late}</b></div>` : ''}
    ${behind ? `<div class="kpi click amber" data-act="filter-to" data-v="behind"><span>${T('Behind')}</span><b>${behind}</b></div>` : ''}
    ${extra}`;
}
function consLabel(c) {
  if (!c) return T('As soon as possible');
  if (c.type === 'alap') return T('As late as possible');
  const d = when(c.at);
  return { snet: T('Start no earlier than {0}', d), snlt: T('Start no later than {0}', d), fnet: T('Finish no earlier than {0}', d), fnlt: T('Finish no later than {0}', d), mso: T('Must start on {0}', d), mfo: T('Must finish on {0}', d) }[c.type];
}

/* ---------- menus ---------- */
function menuItems(name) {
  const people = D.resources.filter(r => r.kind === 'work').map(r => r.name);
  const tick = (cur, v) => cur === v ? 'on' : '';
  const m = {
    look: [['h', T('Chart style')], ...STYLES.map(([v, l, d]) => [`style:${v}`, l(), d(), tick(store('style') || 'modern', v)]), ['-'], ['h', T('Mode')],
      ['theme:dark', T('Dark'), '', tick(document.documentElement.dataset.theme, 'dark')], ['theme:light', T('Light'), '', tick(document.documentElement.dataset.theme, 'light')]],
    uizoom: [['h', T('App zoom')], ...UI_ZOOMS.map(z => [`uiz:${z}`, Math.round(z * 100) + '%', z === 1 ? 'Ctrl+0' : '', Math.abs(uiZ - z) < 0.001 ? 'on' : ''])],
    level: [['level', T('Level resources'), T('delay clashing tasks')], ['unlevel', T('Clear leveling')], ['-'], ['h', T('Options')],
      ['lvl-opt:slack', T('Only within slack'), T('never moves the finish'), D.leveling.slack ? 'on' : ''], ['lvl-opt:split', T('Can pause work under way'), '', D.leveling.split ? 'on' : ''],
      ['h', T('Which task moves first')], ...[['standard', T('Lowest priority, most slack')], ['priority', T('Lowest priority, then the later task')], ['id', T('The later task (by ID)')]].map(([v, l]) => [`lvl-order:${v}`, l, '', tick(D.leveling.order, v)]),
      ['-'], ['go-people', T('Show workloads')]],
    baseline: [...Array(11).keys()].flatMap(n => {
      const has = D.tasks.some(t => t.baselines.some(b => b.n === n));
      if (n > 3 && !has) return [];
      return [[`baseline:${n}`, n ? T('Save baseline {0}', n) : T('Save baseline'), has ? T('saved') : ''], ...(has ? [[`baseline-clear:${n}`, n ? T('Clear baseline {0}', n) : T('Clear baseline')]] : [])];
    }).concat([['-'], ['h', T('Tracking')], ['update-progress', T('Update progress to the status date'), D.status != null ? nice(D.status) : T('set a status date first')], ['tracking', T('Tracking Gantt (plan vs baseline)'), '', store('tracking') === '1' ? 'on' : '']]),
    export: [['exp-xml', T('ProjectLibre / MS Project (.xml)')], ['exp-html', T('Shareable Gantt (.html)')], ['exp-plan', T('Plan file (.plan)')], ['-'], ['print', T('Print / save as PDF…'), 'Ctrl+P'], ['reports', T('Printable reports…')]],
    project: [['open', T('Open or create a project…'), 'Ctrl+O'], ['history', T('Version history…')], ['-'], ['recurring', T('Add a recurring task…')], ['-'], ['new-project', T('Clear this project'), T('keeps your calendar')], ['sample', T('Load the sample project')], ['-'], ['import', T('Import ProjectLibre / MS Project…'), '.xml']],
    filter: [['h', T('Show')], ...[['all', T('All tasks')], ['critical', T('Critical')], ['late', T('Past deadline')], ['behind', T('Behind schedule')], ['notstarted', T('Not started')], ['inprogress', T('In progress')], ['done', T('Completed')], ['milestones', T('Milestones')], ['summaries', T('Summary tasks')], ['unassigned', T('Unassigned')], ['inactive', T('Inactive')]].map(([v, l]) => [`filter:${v}`, l, '', tick(filter, v)]),
      ...(people.length ? [['h', T('Assigned to')], ...people.map(p => [`filter:person:${p}`, p, '', tick(filter, 'person:' + p)])] : [])],
    sort: [['file', T('Outline order')], ['start', T('Start date')], ['finish', T('Finish date')], ['dur', T('Duration')], ['slack', T('Slack (least first)')], ['name', T('Name')], ['cost', T('Cost (highest first)')], ['priority', T('Priority (highest first)')]].map(([v, l]) => [`sort:${v}`, l, '', tick(sortBy, v)]),
    group: [['none', T('No grouping')], ['person', T('Person')], ['phase', T('Phase (top summary)')], ['status', T('Status')], ['critical', T('Critical / not critical')], ...D.fields.map(f => ['x:' + f.name, f.name])].map(([v, l]) => [`group:${v}`, l, '', tick(groupBy, v)]),
  }[name];
  return m;
}
function openMenu(name, btn) {
  closeMenu();
  const el = document.createElement('div');
  el.className = 'menu'; el.id = 'menu'; el.dataset.for = name;
  if (name === 'issues') {
    const iss = issues();
    el.innerHTML = iss.length ? iss.map((x, k) => `<button class="issue" data-act="issue" data-k="${k}"><i>${x.icon}</i><span>${esc(x.text)}<small>${esc(x.sub || '')}</small></span></button>`).join('') : `<div class="empty">✓ ${T('No problems found')}</div>`;
    el.style.width = '420px';
  } else if (name === 'columns') {
    el.innerHTML = `<div class="mh">${T('Columns')}</div>` + Object.entries(COLUMNS).filter(([k]) => k !== 'name').map(([k, c]) => `<label><input type="checkbox" data-c="${k}" ${visibleCols().includes(k) ? 'checked' : ''}> ${c.label()}</label>`).join('')
      + (D.fields.length ? `<div class="mh">${T('Custom fields')}</div>` + D.fields.map(f => `<label><input type="checkbox" data-c="x:${esc(f.name)}" ${visibleCols().includes('x:' + f.name) ? 'checked' : ''}> ${esc(f.name)}</label>`).join('') : '') + `<hr><button data-act="cols-reset">${T('Reset to default')}</button>`;
  } else {
    el.innerHTML = menuItems(name).map(([a, l, k, on]) => a === '-' ? '<hr>' : a === 'h' ? `<div class="mh">${l}</div>` : `<button data-act="${esc(a)}" class="${on || ''}">${esc(l)}<small>${esc(k || '')}</small></button>`).join('');
  }
  document.body.appendChild(el);
  const r = btn.getBoundingClientRect(), z = Z();
  el.style.top = r.bottom / z + 4 + 'px';
  el.style.left = Math.max(8, Math.min(r.left / z, vw() - el.offsetWidth - 8)) + 'px';
  // Open upwards when there's no room below (e.g. the zoom menu in the status bar).
  if (r.bottom / z + 4 + el.offsetHeight > vh()) el.style.top = Math.max(8, r.top / z - el.offsetHeight - 4) + 'px';
}
function closeMenu() { $('#menu')?.remove(); }

/* ---------- misc UI ---------- */
function toast(msg, kind) {
  const box = $('#toasts'); while (box.children.length >= 3) box.firstChild.remove();
  const t = document.createElement('div'); t.className = 'toast' + (kind === 1 ? ' err' : kind === 'warn' ? ' warn' : ''); t.textContent = msg;
  box.appendChild(t); setTimeout(() => t.remove(), kind ? 6500 : 2800);
}
let savedT; function saved() { const s = $('#saved'); s.style.opacity = 1; clearTimeout(savedT); savedT = setTimeout(() => s.style.opacity = 0, 1600); }
const tip = () => $('#tip');
function showTip(e, html) { const t = tip(), z = Z(); t.innerHTML = html; t.style.display = 'block'; t.style.left = Math.min(e.clientX / z + 16, vw() - t.offsetWidth - 10) + 'px'; t.style.top = Math.min(e.clientY / z + 16, vh() - t.offsetHeight - 10) + 'px'; }
function hideTip() { tip().style.display = 'none'; }
function applyTheme(t) { document.documentElement.dataset.theme = t; }
function applyStyle(s) { if (s && s !== 'modern') document.documentElement.dataset.style = s; else delete document.documentElement.dataset.style; }
/* App zoom: scales the whole interface (the time scale has its own zoom).
   Mouse positions and getBoundingClientRect() are in screen pixels, layout values in
   page pixels: divide by Z() to go from one to the other. */
const STYLES = [
  ['modern', () => T('Modern'), () => T('default')],
  ['corporate', () => T('Corporate'), () => T('navy, flat')],
  ['classic', () => T('Classic'), () => T('like MS Project')],
  ['executive', () => T('Executive'), () => T('graphite, minimal')],
  ['mono', () => T('Black & white'), () => T('for printing')],
];
const UI_ZOOMS = [0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5];
let uiZ = Math.max(0.5, Math.min(2.5, +store('uiz') || 1));
const Z = () => uiZ;
const vw = () => document.documentElement.clientWidth, vh = () => document.documentElement.clientHeight;
function applyUiZoom() {
  const r = document.documentElement;
  r.style.zoom = uiZ === 1 ? '' : uiZ; r.style.setProperty('--zf', uiZ);
  const z = $('#uiz'); if (z) z.textContent = Math.round(uiZ * 100) + '%';
}
function setUiZoom(z) {
  uiZ = Math.round(Math.max(0.5, Math.min(2.5, z)) * 100) / 100; store('uiz', uiZ);
  applyUiZoom(); closeMenu(); hideTip(); render();
}
function uiZoomStep(dir) {
  const next = dir > 0 ? UI_ZOOMS.find(z => z > uiZ + 0.001) : [...UI_ZOOMS].reverse().find(z => z < uiZ - 0.001);
  setUiZoom(dir === 0 ? 1 : next ?? uiZ);
}
function setView(v) { view = v; store('view', v); history.replaceState(null, '', '#' + v); if (v === 'usage' && !usage) loadUsage(); render(); }
async function loadUsage() { const j = await api('GET', '/api/usage'); if (j.ok) { usage = j; if (view === 'usage') render(); } }
function zoom(f) { dw = Math.max(0.2, Math.min(80, dw * f)); store('dw', dw); render(); }
function fit() {
  const GW = view === 'resources' ? 700 : view === 'usage' ? 0 : colsDef().reduce((a, c) => a + c[2], 0);
  const days = Math.max(Math.ceil(D.finish / 1440) + 4, 10);
  dw = Math.max(0.2, Math.min(60, (vw() - GW - inspW() - 30) / (days + PAD)));
  store('dw', dw); render();
}
