"use strict";
/* rplan — print studio. Page-accurate preview and printing of the Gantt chart, the task
   table, the WBS chart, the network diagram, usage, people and the reports.
   Gantt and table pages are drawn here as vector graphics at the exact paper size, with
   their own margins, header and footer, so the preview is what the printer (or the PDF)
   gets. The other views are tiled across pages; reports flow from page to page. */

const PAPERS = { A4: [210, 297], A3: [297, 420], A5: [148, 210], Letter: [215.9, 279.4], Legal: [215.9, 355.6], Tabloid: [279.4, 431.8] };
const MM = 96 / 25.4; // CSS pixels per millimetre
const PRINT_WHAT = () => [['gantt', T('Gantt chart')], ['table', T('Task table')], ['wbs', T('WBS chart')], ['network', T('Network diagram')], ['usage', T('Usage')], ['resources', T('People')], ['report', T('Report')]];
// Printable columns and their widths (drawing pixels).
const PRINT_COLS = { id: 26, wbs: 36, name: 210, dur: 46, work: 50, start: 64, finish: 64, deps: 58, who: 100, pct: 38, cost: 64, slack: 46, bstart: 64, bfinish: 64, slip: 46, deadline: 60, type: 80, note: 170 };
const PRH = 20, PHDR = 34, PTITLE = 44, PLEG = 22; // row, header, title block, legend heights

function psDefaults() {
  return {
    what: 'gantt', report: 'summary', paper: LANG === 'es' ? 'A4' : 'Letter', orient: 'landscape', margin: 10,
    fit: 'width', wide: 1, tall: 1, scale: 100, from: '', to: '', unit: 'auto', rows: 'all',
    cols: ['wbs', 'name', 'dur', 'start', 'finish', 'who', 'pct'],
    repeat: true, links: true, base: true, prog: true, labels: true, legend: true, title: true, stripes: true, lines: true,
    style: '', head: ['{project}', '{view}', '{date}'], foot: ['{file}', '', T('Page {0} of {1}', '{page}', '{pages}')], pz: 'fit',
  };
}
let PS = null, printPrev = null, psTimer = null, psCount = 0;
function psLoad() { let saved = {}; try { saved = JSON.parse(store('print') || '{}') || {}; } catch (e) {} PS = { ...psDefaults(), ...saved }; }
const psSave = () => store('print', JSON.stringify(PS));
const printOpen = () => !!document.getElementById('print');

function openPrint(what) {
  psLoad();
  if (what) PS.what = what;
  if (what === 'report' && typeof report !== 'undefined') PS.report = report.kind;
  if (!usage) loadUsage().then(() => { if (printOpen()) renderPreview(); });
  closeMenu(); hideTip(); closeReport();
  if (!printPrev) printPrev = { theme: document.documentElement.dataset.theme, style: document.documentElement.dataset.style || 'modern' };
  let el = $('#print');
  if (!el) {
    el = document.createElement('div'); el.id = 'print'; document.body.appendChild(el);
    el.addEventListener('input', psInput); el.addEventListener('change', psInput);
    el.addEventListener('click', psClick);
  }
  renderPrint();
}
function closePrint() {
  $('#print')?.remove(); $('#page-setup').textContent = '';
  if (printPrev) { applyTheme(printPrev.theme); applyStyle(printPrev.style); printPrev = null; }
  render();
}
// Paper is white: the studio shows (and prints) the light look, in the chosen style.
function psLook() { applyTheme('light'); applyStyle(PS.style || printPrev?.style || 'modern'); }

/* ---------- settings panel ---------- */
function renderPrint() {
  psLook();
  const chart = PS.what === 'gantt' || PS.what === 'table', html = ['wbs', 'network', 'usage', 'resources'].includes(PS.what), rep = PS.what === 'report';
  const opt = (v, l, cur) => `<option value="${esc(v)}" ${String(cur) === String(v) ? 'selected' : ''}>${esc(l)}</option>`;
  const chk = (k, l) => `<label><input type="checkbox" data-ps="${k}" ${PS[k] ? 'checked' : ''}> ${esc(l)}</label>`;
  const colLabel = k => k === 'id' ? '#' : COLUMNS[k].label();
  $('#print').innerHTML = `<aside class="ps-side">
    <h3>${ic('report')}${T('Print')}<button class="x" data-psact="close" title="${T('Close')} (Esc)">×</button></h3>
    <label class="ps-f"><span>${T('What to print')}</span><select data-ps="what">${PRINT_WHAT().map(([v, l]) => opt(v, l, PS.what)).join('')}</select></label>
    ${rep ? `<label class="ps-f"><span>${T('Report')}</span><select data-ps="report">${REPORTS().map(([v, l]) => opt(v, l, PS.report)).join('')}</select></label>` : ''}
    <div class="ps-row">
      <label class="ps-f" style="flex:1"><span>${T('Paper')}</span><select data-ps="paper">${Object.keys(PAPERS).map(p => opt(p, p, PS.paper)).join('')}</select></label>
      <label class="ps-f" style="flex:1"><span>${T('Orientation')}</span><select data-ps="orient">${opt('landscape', T('Landscape'), PS.orient)}${opt('portrait', T('Portrait'), PS.orient)}</select></label>
      <label class="ps-f" style="width:70px"><span>${T('Margin')}</span><input type="number" min="0" max="40" step="1" data-ps="margin" value="${PS.margin}" title="mm"></label>
    </div>
    ${rep ? '' : `<label class="ps-f"><span>${T('Size')}</span><select data-ps="fit">${opt('width', T('Fit the width to N pages'), PS.fit)}${opt('page', T('Fit on N × M pages'), PS.fit)}${opt('scale', T('Scale'), PS.fit)}</select></label>
    <div class="ps-row">
      ${PS.fit === 'scale' ? `<label class="ps-f" style="flex:1"><span>${T('Scale')} %</span><input type="number" min="10" max="400" step="5" data-ps="scale" value="${PS.scale}"></label>` :
        `<label class="ps-f" style="flex:1"><span>${T('Pages wide')}</span><input type="number" min="1" max="50" data-ps="wide" value="${PS.wide}"></label>
         ${PS.fit === 'page' ? `<label class="ps-f" style="flex:1"><span>${T('Pages tall')}</span><input type="number" min="1" max="200" data-ps="tall" value="${PS.tall}"></label>` : ''}`}
    </div>`}
    ${chart ? `
    <label class="ps-f"><span>${T('Rows')}</span><select data-ps="rows">${[['all', T('All tasks')], ['shown', T('As shown in the Gantt (filters, collapsed)')], ['leaves', T('Tasks only (no summaries)')], ['summaries', T('Summary tasks')], ['critical', T('Critical')], ['active', T('Not finished')], ['milestones', T('Milestones')]].map(([v, l]) => opt(v, l, PS.rows)).join('')}</select></label>
    <div class="ps-f"><span>${T('Columns')}</span><div class="ps-checks">${Object.keys(PRINT_COLS).map(k => `<label><input type="checkbox" data-pc="${k}" ${PS.cols.includes(k) ? 'checked' : ''}> ${esc(colLabel(k))}</label>`).join('')}</div></div>` : ''}
    ${PS.what === 'gantt' ? `
    <div class="ps-row">
      <label class="ps-f" style="flex:1"><span>${T('From')}</span><input type="date" data-ps="from" value="${PS.from}"></label>
      <label class="ps-f" style="flex:1"><span>${T('To')}</span><input type="date" data-ps="to" value="${PS.to}"></label>
    </div>
    <label class="ps-f"><span>${T('Time scale')}</span><select data-ps="unit">${[['auto', T('Automatic')], ['day', T('Days')], ['week', T('Weeks')], ['month', T('Months')], ['quarter', T('Quarters')]].map(([v, l]) => opt(v, l, PS.unit)).join('')}</select></label>
    <div class="ps-f"><span>${T('Show')}</span><div class="ps-checks">${chk('links', T('Links'))}${chk('base', T('Baseline'))}${chk('prog', T('Progress'))}${chk('labels', T('Bar labels'))}${chk('stripes', T('Days off'))}${chk('lines', T('Status date'))}${chk('legend', T('Legend'))}${chk('repeat', T('Columns on every page'))}</div></div>` : ''}
    ${chart ? `<div class="ps-checks">${chk('title', T('Title block on page 1'))}</div>` : ''}
    <label class="ps-f"><span>${T('Chart style')}</span><select data-ps="style">${opt('', T('Same as the app'), PS.style)}${STYLES.map(([v, l]) => opt(v, l(), PS.style)).join('')}</select></label>
    <div class="ps-f"><span>${T('Header')}</span><div class="ps-hf">${[0, 1, 2].map(i => `<input type="text" data-ps="head" data-i="${i}" value="${esc(PS.head[i] || '')}" placeholder="${[T('left'), T('center'), T('right')][i]}">`).join('')}</div></div>
    <div class="ps-f"><span>${T('Footer')}</span><div class="ps-hf">${[0, 1, 2].map(i => `<input type="text" data-ps="foot" data-i="${i}" value="${esc(PS.foot[i] || '')}" placeholder="${[T('left'), T('center'), T('right')][i]}">`).join('')}</div>
      <small class="mut">${T('Fields')}: {project} {file} {view} {date} {status} {start} {finish} {page} {pages}</small></div>
    ${html ? `<p class="mut" style="margin:0;font-size:12px">${T('The view is printed as it looks in the app; pages are cut from it.')}</p>` : ''}
    <div class="ps-acts">
      <button class="btn primary" data-psact="print">${ic('dl')}${T('Print / save as PDF')}</button>
      <button class="btn" data-psact="reset" title="${T('Back to the default settings')}">${T('Reset')}</button>
    </div>
    <small class="mut">${T('Tip: pick “Save as PDF” as the printer to share it.')}</small>
  </aside>
  <section class="ps-prev"><div class="ps-bar"><span id="ps-count" class="ps-count"></span>
    <select data-ps="pz">${[['fit', T('Fit')], ['0.5', '50%'], ['0.75', '75%'], ['1', '100%'], ['1.5', '150%']].map(([v, l]) => opt(v, l, PS.pz)).join('')}</select></div>
    <div class="ps-sheets" id="ps-sheets"></div></section>`;
  renderPreview();
}
function psInput(e) {
  const el = e.target;
  e.stopPropagation();
  if (el.dataset.pc) {
    if (e.type !== 'change') return;
    const on = new Set(PS.cols); el.checked ? on.add(el.dataset.pc) : on.delete(el.dataset.pc);
    PS.cols = Object.keys(PRINT_COLS).filter(k => on.has(k));
  } else if (el.dataset.ps) {
    const k = el.dataset.ps;
    if (el.type === 'checkbox') { if (e.type !== 'change') return; PS[k] = el.checked; }
    else if (k === 'head' || k === 'foot') { PS[k] = [...PS[k]]; PS[k][+el.dataset.i] = el.value; }
    else if (el.type === 'number') { const v = +el.value; if (!(v >= +el.min)) return; PS[k] = Math.min(+el.max || 1e9, v); }
    else PS[k] = el.value;
    psSave();
    // These change which settings are offered: rebuild the panel.
    if (['what', 'fit'].includes(k) || (k === 'style' && e.type === 'change')) return renderPrint();
  } else return;
  psSave();
  clearTimeout(psTimer); psTimer = setTimeout(renderPreview, e.type === 'input' && el.type === 'text' ? 250 : 60);
}
function psClick(e) {
  const b = e.target.closest('[data-psact]');
  if (!b) { if (!e.target.closest('[data-act]')) e.stopPropagation(); return; }
  e.stopPropagation();
  const a = b.dataset.psact;
  if (a === 'close') closePrint();
  if (a === 'print') psPrint();
  if (a === 'reset') { PS = psDefaults(); psSave(); renderPrint(); }
}

/* ---------- pages ---------- */
function psPaper() { let [w, h] = PAPERS[PS.paper] || PAPERS.A4; if (PS.orient === 'landscape') [w, h] = [h, w]; return { w, h }; }
function psBox() {
  const { w, h } = psPaper(), m = Math.max(0, Math.min(40, +PS.margin || 0));
  const hh = PS.head.some(x => (x || '').trim()) ? 7 : 0, fh = PS.foot.some(x => (x || '').trim()) ? 7 : 0;
  return { w, h, m, hh, fh, cw: (w - 2 * m) * MM, ch: (h - 2 * m - hh - fh) * MM };
}
function psTokens(s, k, n) {
  const what = PS.what === 'report' ? (REPORTS().find(r => r[0] === PS.report) || [, ''])[1] : (PRINT_WHAT().find(w => w[0] === PS.what) || [, ''])[1];
  const fin = D.tasks.length ? niceY(Math.floor((D.finish - 1) / 1440)) : '';
  return String(s || '').replace(/\{(\w+)\}/g, (m, f) => ({
    project: D.name, file: (D.path || '').split(/[\\/]/).pop(), view: what, date: new Date().toLocaleDateString(LANG),
    status: D.status != null ? niceY(D.status) : '', start: niceY(0), finish: fin, page: k, pages: n,
  })[f] ?? m);
}
function sheetHtml(box, body, k, n) {
  const line = (arr, cls) => `<div class="${cls}">${[0, 1, 2].map(i => `<span>${esc(psTokens(arr[i], k, n))}</span>`).join('')}</div>`;
  return `<div class="sheet" style="width:${box.w}mm;height:${box.h}mm;--sh:${box.h}mm;padding:${box.m}mm">${box.hh ? line(PS.head, 'sh-h') : ''}<div class="sh-b" style="width:${box.cw}px;height:${box.ch}px">${body}</div>${box.fh ? line(PS.foot, 'sh-f') : ''}</div>`;
}
function renderPreview() {
  const host = $('#ps-sheets'); if (!host) return;
  const box = psBox();
  let pages;
  try {
    if (PS.what === 'report') pages = null;
    else if (PS.what === 'gantt' || PS.what === 'table') pages = ganttPages(box);
    else pages = htmlPages(box);
  } catch (err) { pages = [`<p style="padding:20px;color:#c00">${esc(err.message)}</p>`]; }
  if (pages) {
    psCount = pages.length;
    host.innerHTML = pages.map((p, k) => sheetHtml(box, p, k + 1, pages.length)).join('');
  } else {
    const body = { summary: repSummary, who: repWho, cash: repCash, ev: repEv, tasks: repTasks, problems: repProblems }[PS.report]();
    const m = Math.max(PS.margin, box.hh || box.fh ? 12 : 0), contentH = (box.h - 2 * m) * MM;
    host.innerHTML = `<div class="sheet flow" style="width:${box.w}mm;min-height:${box.h}mm;padding:${m}mm ${box.m}mm"><div class="paper">${body}</div></div>`;
    const sh = host.firstChild, tall = sh.scrollHeight - 2 * m * MM;
    psCount = Math.max(1, Math.ceil(tall / contentH - 0.01));
    for (let i = 1; i < psCount; i++) sh.insertAdjacentHTML('beforeend', `<div class="pbreak" style="top:${m * MM + i * contentH}px"><span>${T('page {0}', i + 1)}</span></div>`);
  }
  $('#ps-count').textContent = psCount === 1 ? T('1 page') : T('{0} pages', psCount);
  const pz = PS.pz === 'fit' ? Math.min(1.5, ($('.ps-prev').clientWidth - 48) / (box.w * MM)) : +PS.pz;
  host.style.setProperty('--pz', Math.max(0.1, pz));
}

/* ---------- Gantt and table pages (vector) ---------- */
const PRINT_PAL = {
  modern: { bar: '#4f8cff', crit: '#ff4d6d', done: '#32b37a', ms: '#e8a800', msC: '#ff4d6d', sum: '#2a3142', prog: 'rgba(0,0,0,.25)', base: '#aab2c2', link: '#8a94a6', linkC: '#ff4d6d', r: 3, h: 12, head: '#f1f3f8', alt: '#f8f9fc', grid: '#e3e7ee', acc: '#7c5cff', ink: '#fff' },
  corporate: { bar: '#2f6db5', crit: '#c0392b', done: '#5f8f75', ms: '#1f3a5f', msC: '#c0392b', sum: '#1f2d3d', prog: 'rgba(0,0,0,.33)', base: '#a3adb8', link: '#7f8c8d', linkC: '#c0392b', r: 1, h: 11, head: '#e8edf3', alt: '#f6f8fa', grid: '#dfe4ea', acc: '#1f4e79', ink: '#fff' },
  classic: { bar: '#8db3e2', crit: '#f08c8c', done: '#9cc59e', ms: '#000', msC: '#c00000', sum: '#000', prog: '#1f3864', thin: true, base: '#9a9a9a', link: '#3f6fae', linkC: '#c00000', r: 0, h: 9, stroke: '#3f6fae', strokeC: '#b02020', head: '#e9edf2', alt: '#fff', grid: '#d4d8de', acc: '#2b579a', ink: '#000' },
  executive: { bar: '#6b7b8c', crit: '#a93226', done: '#9aa8b5', ms: '#2c3e50', msC: '#a93226', sum: '#2c3e50', prog: 'rgba(255,255,255,.4)', base: '#b8c0c8', link: '#95a5a6', linkC: '#a93226', r: 2, h: 10, head: '#eef0f2', alt: '#fafbfb', grid: '#e1e4e8', acc: '#34495e', ink: '#fff' },
  mono: { bar: '#fff', crit: '#000', done: 'url(#ps-hatch)', ms: '#000', msC: '#000', sum: '#000', prog: 'url(#ps-hatch)', progC: 'url(#ps-hatchw)', base: '#888', link: '#000', linkC: '#000', dash: '3 2', r: 0, h: 11, stroke: '#000', strokeC: '#000', head: '#eee', alt: '#fff', grid: '#c8c8c8', acc: '#000', ink: '#000' },
};
const psPal = () => PRINT_PAL[PS.style || printPrev?.style || 'modern'] || PRINT_PAL.modern;
const psDateFmt = () => new Intl.DateTimeFormat(LANG, { day: '2-digit', month: '2-digit', year: '2-digit', timeZone: 'UTC' });
function psRows() {
  if (PS.rows === 'shown') return rowsModel().map(r => r.kind === 'task' ? { t: D.tasks[r.i] } : { group: r.label, count: r.count });
  const keep = {
    all: () => true, leaves: t => !t.summary, summaries: t => t.summary, critical: t => t.crit, active: t => !t.done,
    milestones: t => !t.summary && t.edur === 0,
  }[PS.rows] || (() => true);
  return D.tasks.filter(keep).map(t => ({ t }));
}
function psCell(k, t, df) {
  const ms = !t.summary && t.edur === 0, base = t.baselines.find(b => b.n === 0);
  const d = (m, end) => m == null ? '' : df.format(dateOf(Math.floor((end ? m - 1 : m) / 1440)));
  switch (k) {
    case 'id': return t.id;
    case 'wbs': return t.wbs;
    case 'name': return t.name;
    case 'dur': return t.summary ? fmtDays(t.span) : ms ? '0' : fmtDur(t.edur, t.elapsed);
    case 'work': return t.workT ? fmtHours(t.workT) : '';
    case 'start': return d(t.es);
    case 'finish': return t.edur > 0 || t.summary ? d(t.ef, true) : d(t.es);
    case 'deps': return depsStr(t);
    case 'who': return t.summary ? '' : whoStr(t);
    case 'pct': return Math.round(t.summary ? t.pctR : t.pct) + '%';
    case 'cost': return t.cost ? money(t.cost) : '';
    case 'slack': return t.done ? '' : fmtDays(t.slack);
    case 'bstart': return base ? d(base.s) : '';
    case 'bfinish': return base ? d(base.f, true) : '';
    case 'slip': return t.slip == null ? '' : t.slip === 0 ? '0' : (t.slip > 0 ? '+' : '') + fmtDays(t.slip);
    case 'deadline': return t.deadline != null ? df.format(dateOf(t.deadline)) : '';
    case 'type': return typeLabel(t);
    case 'note': return t.note || '';
  }
  return '';
}
function psRange() {
  let a = Infinity, b = -Infinity;
  for (const t of D.tasks) {
    a = Math.min(a, sDay(t)); b = Math.max(b, Math.ceil(t.ef / 1440));
    if (PS.base) for (const x of t.baselines) if (x.n === 0) { a = Math.min(a, Math.floor(x.s / 1440)); b = Math.max(b, Math.ceil(x.f / 1440)); }
    if (t.deadline != null) b = Math.max(b, t.deadline + 1);
  }
  if (!isFinite(a)) { a = 0; b = 14; }
  a -= 1; b += Math.max(2, Math.ceil((b - a) * (PS.labels ? 0.14 : 0.04)));
  if (PS.from) a = offOf(PS.from);
  if (PS.to) b = offOf(PS.to) + 1;
  return [a, Math.max(b, a + 1)];
}
function psUnits(dayW) {
  const fixed = { day: ['month', 'day'], week: ['month', 'week'], month: ['year', 'month'], quarter: ['year', 'quarter'] }[PS.unit];
  if (fixed) return fixed;
  return dayW >= 13 ? ['month', 'day'] : dayW >= 3.2 ? ['month', 'week'] : dayW >= 0.9 ? ['year', 'month'] : ['year', 'quarter'];
}
// Works out the scale, the day width and how the drawing is cut into pages.
function ganttLayout(box, nRows) {
  const withChart = PS.what === 'gantt';
  const cols = PS.cols.filter(k => PRINT_COLS[k]);
  const TW = cols.reduce((a, k) => a + PRINT_COLS[k], 0);
  const [d0, d1] = psRange(), days = d1 - d0;
  const extraTop = PS.title ? PTITLE : 0, legend = withChart && PS.legend ? PLEG : 0;
  let s = 1, dayW = 0, wide = 1;
  if (!withChart) {
    s = PS.fit === 'scale' ? PS.scale / 100 : Math.min(1.6, box.cw / Math.max(TW, 1));
  } else if (PS.fit === 'scale') {
    s = PS.scale / 100;
    dayW = { day: 20, week: 5, month: 1.3, quarter: 0.45 }[PS.unit] || (days <= 45 ? 20 : days <= 200 ? 5 : days <= 900 ? 1.3 : 0.45);
  } else {
    wide = Math.max(1, Math.round(PS.wide));
    s = TW > box.cw * 0.62 ? box.cw * 0.62 / TW : 1;
    if (PS.fit === 'page') {
      const tall = Math.max(1, Math.round(PS.tall)), per = Math.ceil(nRows / tall);
      s = Math.min(s, box.ch / (PHDR + per * PRH + legend + extraTop / tall + 2));
    }
    const avail = wide * box.cw / s - TW * (PS.repeat ? wide : 1);
    dayW = Math.max(0.05, avail / days);
  }
  // Horizontal cuts: [table shown?, chart x from, chart x to] for each page across.
  const across = [];
  if (!withChart) across.push([true, 0, 0]);
  else {
    const total = days * dayW;
    let x = 0;
    for (let j = 0; j < 200 && x < total - 0.5; j++) {
      const table = TW > 0 && (j === 0 || PS.repeat), w = box.cw / s - (table ? TW : 0);
      if (w <= 4) throw new Error(T('The table is wider than the page: choose fewer columns, a larger paper or a smaller scale.'));
      across.push([table, x, Math.min(total, x + w)]); x += w;
    }
    if (!across.length) across.push([TW > 0, 0, 0]);
  }
  // Vertical cuts: [first row, end row] for each page down.
  const down = [];
  for (let r = 0, k = 0; k === 0 || r < nRows; k++) {
    const room = box.ch / s - PHDR - legend - (k === 0 ? extraTop : 0);
    const per = Math.max(1, Math.floor(room / PRH));
    down.push([r, Math.min(nRows, r + per)]); r += per;
    if (k > 5000) break;
  }
  return { s, dayW, d0, d1, cols, TW, across, down, withChart, extraTop, legend };
}
function ganttPages(box) {
  const rows = psRows(), L = ganttLayout(box, rows.length), pal = psPal(), df = psDateFmt();
  const X = m => (m / 1440 - L.d0) * L.dayW;
  const rowOf = new Map(); rows.forEach((r, k) => { if (r.t) rowOf.set(r.t.id, k); });
  const indent = !(PS.rows === 'shown' && isFlat());
  const minLevel = Math.min(...rows.filter(r => r.t).map(r => r.t.level), 0);
  const [topU, botU] = psUnits(L.dayW);
  const pages = [];
  let pid = 0;
  for (const [r0, r1] of L.down) {
    for (const [table, x0, x1] of L.across) {
      pid++;
      const first = r0 === 0, top = first ? L.extraTop : 0, vbW = box.cw / L.s, vbH = box.ch / L.s;
      const tw = table ? L.TW : 0, cw = x1 - x0, bodyH = (r1 - r0) * PRH;
      let g = `<defs>
        <pattern id="ps-hatch" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="4" height="4" fill="#fff"/><rect width="1.6" height="4" fill="#000"/></pattern>
        <pattern id="ps-hatchw" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><rect width="1.6" height="4" fill="#fff"/></pattern>
        <marker id="ps-ar${pid}" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="5" markerHeight="5" orient="auto"><path d="M0,0 L8,4 L0,8z" fill="${pal.link}"/></marker>
        <marker id="ps-arc${pid}" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="5" markerHeight="5" orient="auto"><path d="M0,0 L8,4 L0,8z" fill="${pal.linkC}"/></marker></defs>`;
      if (first && PS.title) {
        const fin = D.tasks.length ? niceY(Math.floor((D.finish - 1) / 1440)) : '—';
        g += `<text x="0" y="17" font-size="16" font-weight="700" fill="#111">${esc(D.name)}</text>
          <text x="0" y="33" font-size="10" fill="#555">${esc([`${niceY(0)} – ${fin}`, T('{0} tasks', D.tasks.filter(t => !t.summary).length), D.status != null ? T('status date {0}', niceY(D.status)) : '', `${T('Progress')} ${Math.round(D.progress)}%`, D.cost ? money(D.cost) : ''].filter(Boolean).join('  ·  '))}</text>
          <rect x="0" y="${PTITLE - 6}" width="${vbW}" height="2" fill="${pal.acc}"/>`;
      }
      // table
      if (tw) {
        let x = 0, head = '', body = '', clips = '';
        g += `<rect x="0" y="${top}" width="${tw}" height="${PHDR}" fill="${pal.head}"/>`;
        for (let i = r0; i < r1; i++) if ((i - r0) % 2 && pal.alt !== '#fff') g += `<rect x="0" y="${top + PHDR + (i - r0) * PRH}" width="${tw}" height="${PRH}" fill="${pal.alt}"/>`;
        L.cols.forEach((k, c) => {
          const w = PRINT_COLS[k], cid = `ps-c${pid}-${c}`, right = ['dur', 'work', 'pct', 'cost', 'slack', 'slip', 'id'].includes(k);
          clips += `<clipPath id="${cid}"><rect x="${x + 2}" y="${top}" width="${w - 4}" height="${PHDR + bodyH}"/></clipPath>`;
          head += `<text x="${right ? x + w - 4 : x + 4}" y="${top + PHDR - 9}" font-size="8.5" font-weight="700" fill="#333" ${right ? 'text-anchor="end"' : ''} clip-path="url(#${cid})">${esc(k === 'id' ? '#' : COLUMNS[k].label().toUpperCase())}</text>`;
          for (let i = r0; i < r1; i++) {
            const r = rows[i], y = top + PHDR + (i - r0) * PRH + 13.5;
            if (r.group != null) { if (c === 0) body += `<text x="4" y="${y}" font-size="10" font-weight="700" fill="#111">${esc(r.group)} (${r.count})</text>`; continue; }
            const t = r.t, v = psCell(k, t, df); if (v === '' || v == null) continue;
            const ind = k === 'name' && indent ? (t.level - minLevel) * 11 : 0;
            body += `<text x="${right ? x + w - 4 : x + 4 + ind}" y="${y}" font-size="10" fill="${t.inactive ? '#999' : '#111'}" ${t.summary ? 'font-weight="700"' : ''} ${right ? 'text-anchor="end"' : ''} ${t.inactive && k === 'name' ? 'text-decoration="line-through"' : ''} clip-path="url(#${cid})">${esc(v)}</text>`;
          }
          if (c) g += `<line x1="${x}" x2="${x}" y1="${top}" y2="${top + PHDR + bodyH}" stroke="${pal.grid}"/>`;
          x += w;
        });
        g += clips + head + body + `<line x1="${tw}" x2="${tw}" y1="${top}" y2="${top + PHDR + bodyH}" stroke="#999"/>`;
      }
      // chart
      if (L.withChart && cw > 0) {
        let c = `<rect x="${x0}" y="0" width="${cw}" height="${PHDR}" fill="${pal.head}"/>`;
        const dA = Math.floor(L.d0 + x0 / L.dayW), dB = Math.ceil(L.d0 + x1 / L.dayW);
        if (PS.stripes && L.dayW >= 2.5) for (let d = dA; d <= dB; d++) if (isOff(d)) c += `<rect x="${X(d * 1440)}" y="${PHDR}" width="${L.dayW}" height="${bodyH}" fill="${EXC.has(d) ? '#fff3d6' : '#f1f2f5'}"/>`;
        for (let i = r0; i < r1; i++) c += `<line x1="${x0}" x2="${x1}" y1="${PHDR + (i - r0 + 1) * PRH}" y2="${PHDR + (i - r0 + 1) * PRH}" stroke="${pal.grid}" stroke-width=".6"/>`;
        const tier = (unit, isTop) => {
          let d = unitStart(unit, dA), out = '';
          while (d <= dB) {
            const nx = unitNext(unit, d), xa = Math.max(X(d * 1440), x0), xb = Math.min(X(nx * 1440), x1), y = isTop ? 0 : 16;
            out += `<line x1="${X(d * 1440)}" x2="${X(d * 1440)}" y1="${y}" y2="${isTop ? 16 : PHDR + (isTop ? 0 : bodyH)}" stroke="${isTop ? '#bbb' : pal.grid}" stroke-width="${isTop ? 1 : 0.6}"/>`;
            const lab = String(unitLabel(unit, d, isTop)), fits = xb - xa > lab.length * (isTop ? 5.6 : 5) + 4;
            if (fits) out += `<text x="${isTop ? xa + 4 : (xa + xb) / 2}" y="${isTop ? 12 : 29}" font-size="${isTop ? 9.5 : 8.5}" ${isTop ? 'font-weight="700"' : 'text-anchor="middle"'} fill="${isTop ? '#222' : '#555'}">${esc(lab)}</text>`;
            d = nx;
          }
          return out;
        };
        c += tier(botU, false) + tier(topU, true) + `<line x1="${x0}" x2="${x1}" y1="16" y2="16" stroke="#bbb"/><line x1="${x0}" x2="${x1}" y1="${PHDR}" y2="${PHDR}" stroke="#999"/>`;
        if (PS.lines && D.status != null) c += `<line x1="${X((D.status + 1) * 1440)}" x2="${X((D.status + 1) * 1440)}" y1="${PHDR}" y2="${PHDR + bodyH}" stroke="#d98200" stroke-width="1.2" stroke-dasharray="4 3"/>`;
        // geometry for links
        const geo = new Map(), h = pal.h;
        for (let i = r0; i < r1; i++) {
          const t = rows[i].t; if (!t) continue;
          const y = PHDR + (i - r0) * PRH, x1b = X(t.es), x2b = Math.max(X(t.ef), x1b + 1.5);
          geo.set(t.id, t.summary ? { x1: x1b, x2: x2b, y: y + 8 } : !t.summary && t.edur === 0 ? { x1: x1b - 5, x2: x1b + 5, y: y + PRH / 2 } : { x1: x1b, x2: x2b, y: y + PRH / 2 });
        }
        if (PS.links) for (let i = r0; i < r1; i++) {
          const t = rows[i].t; if (!t) continue;
          for (const dp of t.deps) {
            const a = geo.get(dp.id), b = geo.get(t.id); if (!a || !b) continue;
            const crit = t.crit && task(dp.id)?.crit, fromEnd = dp.kind === 'FS' || dp.kind === 'FF', toStart = dp.kind === 'FS' || dp.kind === 'SS';
            const xa = fromEnd ? a.x2 : a.x1, xb = toStart ? b.x1 : b.x2, ax = xa + (fromEnd ? 5 : -5), end = xb + (toStart ? -1 : 1);
            const p = (toStart && ax <= xb - 3) || (!toStart && ax >= xb + 3) ? `M${xa},${a.y} H${ax} V${b.y} H${end}` : `M${xa},${a.y} H${ax} V${a.y + (b.y > a.y ? PRH / 2 : -PRH / 2)} H${xb + (toStart ? -7 : 7)} V${b.y} H${end}`;
            c += `<path d="${p}" fill="none" stroke="${crit ? pal.linkC : pal.link}" stroke-width="${crit ? 1.1 : 0.8}" ${pal.dash && !crit ? `stroke-dasharray="${pal.dash}"` : ''} marker-end="url(#ps-ar${crit ? 'c' : ''}${pid})"/>`;
          }
        }
        for (let i = r0; i < r1; i++) {
          const t = rows[i].t; if (!t) continue;
          const y = PHDR + (i - r0) * PRH, gx = geo.get(t.id), op = t.inactive ? ' opacity=".4"' : '';
          if (gx.x2 < x0 - 200 || gx.x1 > x1 + 4) continue;
          const b = t.baselines.find(x => x.n === 0);
          if (PS.base && b) c += `<rect x="${X(b.s)}" y="${y + (PRH + h) / 2 + 1}" width="${Math.max(X(b.f) - X(b.s), 1.5)}" height="2.6" fill="${pal.base}"/>`;
          let lab = '';
          if (t.summary) {
            c += `<path d="M${gx.x1},${y + 5} H${gx.x2} V${y + 13} L${gx.x2 - 4},${y + 9} H${gx.x1 + 4} L${gx.x1},${y + 13} Z" fill="${pal.sum}"${op}/>`;
            if (PS.prog && t.pctR > 0 && pal !== PRINT_PAL.mono) c += `<rect x="${gx.x1}" y="${y + 7}" width="${(gx.x2 - gx.x1) * t.pctR / 100}" height="2" fill="#fff" opacity=".55"/>`;
          } else if (t.edur === 0) {
            const cx = X(t.es), cy = y + PRH / 2;
            c += `<path d="M${cx},${cy - 5.5} L${cx + 5.5},${cy} L${cx},${cy + 5.5} L${cx - 5.5},${cy} Z" fill="${t.crit ? pal.msC : pal.ms}"${op}/>`;
            lab = df.format(dateOf(Math.floor(t.es / 1440)));
          } else {
            const by = y + (PRH - h) / 2, w = gx.x2 - gx.x1;
            const fill = t.crit ? pal.crit : t.done ? pal.done : pal.bar, st = t.crit ? pal.strokeC : pal.stroke;
            c += `<rect x="${gx.x1}" y="${by}" width="${w}" height="${h}" rx="${pal.r}" fill="${fill}" ${st ? `stroke="${st}" stroke-width=".8"` : ''}${op}/>`;
            if (PS.prog && t.pct > 0 && !t.done) {
              const ph = pal.thin ? 3 : h, py = pal.thin ? by + (h - 3) / 2 : by;
              c += `<rect x="${gx.x1}" y="${py}" width="${w * t.pct / 100}" height="${ph}" rx="${pal.thin ? 0 : pal.r}" fill="${t.crit && pal.progC ? pal.progC : pal.prog}"/>`;
            }
            lab = t.who.length ? whoStr(t) : '';
          }
          if (t.deadline != null) { const dx = X((t.deadline + 1) * 1440); c += `<path d="M${dx - 3.5},${y + 1} H${dx + 3.5} L${dx},${y + 6} Z" fill="${t.late ? '#d0021b' : '#3c8d40'}"/>`; }
          if (PS.labels && lab) c += `<text x="${gx.x2 + 5}" y="${y + 13.5}" font-size="9" fill="#444">${esc(lab)}</text>`;
        }
        g += `<svg x="${tw}" y="${top}" width="${cw}" height="${PHDR + bodyH}" viewBox="${x0} 0 ${cw} ${PHDR + bodyH}" overflow="hidden">${c}</svg>`;
      }
      if (L.legend) g += legendSvg(pal, vbH - PLEG + 6, vbW);
      pages.push(`<svg class="ps-svg" width="${box.cw}" height="${box.ch}" viewBox="0 0 ${vbW} ${vbH}" font-family="${esc(getComputedStyle(document.body).fontFamily)}">${g}</svg>`);
    }
  }
  return pages;
}
function legendSvg(pal, y, w) {
  const items = [
    [`<rect width="18" height="${pal.h}" y="${-pal.h / 2}" rx="${pal.r}" fill="${pal.bar}" ${pal.stroke ? `stroke="${pal.stroke}" stroke-width=".8"` : ''}/>`, T('Task')],
    [`<rect width="18" height="${pal.h}" y="${-pal.h / 2}" rx="${pal.r}" fill="${pal.crit}" ${pal.strokeC ? `stroke="${pal.strokeC}" stroke-width=".8"` : ''}/>`, T('Critical')],
    [`<rect width="18" height="${pal.h}" y="${-pal.h / 2}" rx="${pal.r}" fill="${pal.done}" ${pal.stroke ? `stroke="${pal.stroke}" stroke-width=".8"` : ''}/>`, T('Done')],
    [`<path d="M9,-5.5 L14.5,0 L9,5.5 L3.5,0 Z" fill="${pal.ms}"/>`, T('Milestone')],
    [`<path d="M0,-4 H18 V4 L14,0 H4 L0,4 Z" fill="${pal.sum}"/>`, T('Summary')],
  ];
  if (PS.base && D.tasks.some(t => t.baselines.some(b => b.n === 0))) items.push([`<rect width="18" height="2.6" y="-1.3" fill="${pal.base}"/>`, T('Baseline')]);
  if (PS.lines && D.status != null) items.push([`<line x1="9" x2="9" y1="-6" y2="6" stroke="#d98200" stroke-width="1.2" stroke-dasharray="3 2"/>`, T('Status date')]);
  let x = 0, out = `<line x1="0" x2="${w}" y1="${y - 10}" y2="${y - 10}" stroke="#ccc"/>`;
  for (const [shape, label] of items) { out += `<g transform="translate(${x},${y})">${shape}<text x="23" y="3.5" font-size="9" fill="#333">${esc(label)}</text></g>`; x += 34 + label.length * 5.2; }
  return out;
}

/* ---------- other views: cut the view into pages ---------- */
function htmlPages(box) {
  const html = { wbs: wbsView, network, usage: usageView, resources: people }[PS.what]();
  const m = document.createElement('div'); m.className = 'ps-measure'; m.innerHTML = html; $('#print').appendChild(m);
  const v = m.querySelector('.view') || m, W = Math.max(1, v.scrollWidth), H = Math.max(1, v.scrollHeight); m.remove();
  let s = PS.fit === 'scale' ? PS.scale / 100 : Math.max(1, Math.round(PS.wide)) * box.cw / W;
  if (PS.fit === 'page') s = Math.min(s, Math.max(1, Math.round(PS.tall)) * box.ch / H);
  const nx = Math.max(1, Math.ceil(W * s / box.cw - 0.001)), ny = Math.max(1, Math.ceil(H * s / box.ch - 0.001)), pages = [];
  for (let j = 0; j < ny; j++) for (let i = 0; i < nx; i++)
    pages.push(`<div class="ps-tile" style="width:${W}px;height:${H}px;transform:scale(${s}) translate(${-i * box.cw / s}px,${-j * box.ch / s}px)">${html}</div>`);
  return pages;
}

/* ---------- printing ---------- */
function psPrint() {
  const box = psBox();
  if (PS.what === 'report') {
    const m = Math.max(PS.margin, box.hh || box.fh ? 12 : 0);
    const cssv = s => {
      // {page} and {pages} become the printer's own page counters.
      const parts = String(psTokens(s, '\u0001', '\u0002')).split(/([\u0001\u0002])/).filter(x => x !== '');
      return parts.length ? parts.map(p => p === '\u0001' ? 'counter(page)' : p === '\u0002' ? 'counter(pages)' : `"${cssStr(p)}"`).join(' ') : '""';
    };
    const boxes = ['left', 'center', 'right'].map((pos, i) => (PS.head[i] ? `@top-${pos}{content:${cssv(PS.head[i])};font-size:8pt;color:#555}` : '') + (PS.foot[i] ? `@bottom-${pos}{content:${cssv(PS.foot[i])};font-size:8pt;color:#555}` : '')).join('');
    $('#page-setup').textContent = `@page{size:${box.w}mm ${box.h}mm;margin:${m}mm ${box.m}mm;${boxes}}`;
  } else {
    $('#page-setup').textContent = `@page{size:${box.w}mm ${box.h}mm;margin:0}`;
  }
  document.body.classList.add('printing-studio');
  setTimeout(() => print(), 30);
}
addEventListener('afterprint', () => document.body.classList.remove('printing-studio'));
