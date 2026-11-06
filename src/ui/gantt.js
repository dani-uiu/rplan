"use strict";
/* rplan — Gantt view: columns, search/filter/sort/group, timescales, virtualized rows. */

const COLUMNS = {
  wbs: { label: () => 'WBS', w: 44 },
  name: { label: () => T('Task'), w: 240 },
  dur: { label: () => T('Dur'), w: 60 },
  work: { label: () => T('Work'), w: 66 },
  start: { label: () => T('Start'), w: 96 },
  finish: { label: () => T('Finish'), w: 96 },
  deps: { label: () => T('After'), w: 80 },
  who: { label: () => T('Who'), w: 104 },
  pct: { label: () => T('Done'), w: 54 },
  cost: { label: () => T('Cost'), w: 76 },
  slack: { label: () => T('Slack'), w: 56 },
  free: { label: () => T('Free slack'), w: 64 },
  astart: { label: () => T('Actual start'), w: 96 },
  afinish: { label: () => T('Actual finish'), w: 96 },
  bstart: { label: () => T('Baseline start'), w: 96 },
  bfinish: { label: () => T('Baseline finish'), w: 96 },
  slip: { label: () => T('Slip'), w: 56 },
  constraint: { label: () => T('Constraint'), w: 130 },
  deadline: { label: () => T('Deadline'), w: 90 },
  priority: { label: () => T('Priority'), w: 60 },
  type: { label: () => T('Type'), w: 100 },
  ev: { label: () => T('Earned value'), w: 84 },
  cpi: { label: () => 'CPI', w: 50 },
  note: { label: () => T('Notes'), w: 160 },
};
const DEFAULT_COLS = ['wbs', 'dur', 'start', 'finish', 'deps', 'who', 'pct'];
function visibleCols() {
  try { const c = JSON.parse(store('cols') || 'null'); if (Array.isArray(c)) return c.filter(k => COLUMNS[k]); } catch (e) {}
  const c = [...DEFAULT_COLS];
  if (D && D.tasks.some(t => t.cost > 0)) c.push('cost');
  if (D && D.tasks.some(t => t.baselines.length)) c.push('slip');
  return c;
}
function colsDef() {
  const vis = new Set(visibleCols());
  const out = [['num', '#', 50]];
  for (const k of Object.keys(COLUMNS)) if (k === 'name' || vis.has(k)) out.push([k, COLUMNS[k].label(), COLUMNS[k].w]);
  out.push(['del', '', 30]);
  return out;
}

/* ---------- which rows to show ---------- */
const norm = s => String(s || '').normalize('NFD').replace(/[\u0300-\u036f]/g, '').toLowerCase();
function matchesSearch(t) {
  if (!search) return true;
  const q = norm(search).trim();
  if (/^#?\d+$/.test(q)) return String(t.id) === q.replace('#', '');
  return [t.name, t.note, t.wbs, ...t.who.map(a => a.name)].some(x => norm(x).includes(q));
}
function matchesFilter(t) {
  const now = nowRel();
  if (filter.startsWith('person:')) return t.who.some(a => a.name === filter.slice(7));
  switch (filter) {
    case 'critical': return t.crit;
    case 'late': return t.late;
    case 'behind': return behindOf(t, now);
    case 'notstarted': return !t.summary && !t.started;
    case 'inprogress': return !t.summary && t.started && !t.done;
    case 'done': return t.done;
    case 'milestones': return !t.summary && t.edur === 0;
    case 'summaries': return t.summary;
    case 'unassigned': return !t.summary && t.edur > 0 && !t.who.length;
    case 'inactive': return t.inactive;
    default: return true;
  }
}
const isFlat = () => sortBy !== 'file' || groupBy !== 'none';
function rowsModel() {
  const ts = D.tasks, n = ts.length, par = parentsOf(ts), rows = [];
  const filtering = filter !== 'all' || !!search;
  const match = ts.map(t => matchesFilter(t) && matchesSearch(t));
  if (!isFlat()) {
    const keep = new Array(n).fill(!filtering);
    if (filtering) ts.forEach((t, i) => { if (match[i]) { keep[i] = true; for (let p = par[i]; p >= 0; p = par[p]) keep[p] = true; } });
    const hidden = [];
    ts.forEach((t, i) => { const p = par[i]; hidden[i] = p >= 0 && (hidden[p] || (!search && collapsed.has(ts[p].id))); });
    ts.forEach((t, i) => { if (keep[i] && !hidden[i]) rows.push({ kind: 'task', i, dim: filtering && !match[i] }); });
    return rows;
  }
  let idx = ts.map((t, i) => i).filter(i => match[i] && (filter === 'summaries' || !ts[i].summary));
  const key = {
    start: i => ts[i].es, finish: i => ts[i].ef, dur: i => -ts[i].edur, slack: i => ts[i].slack, cost: i => -ts[i].cost, priority: i => -ts[i].priority,
  }[sortBy];
  if (sortBy === 'name') idx.sort((a, b) => ts[a].name.localeCompare(ts[b].name, LANG));
  else if (key) idx.sort((a, b) => key(a) - key(b) || a - b);
  if (groupBy === 'none') return idx.map(i => ({ kind: 'task', i }));
  const groups = new Map();
  const put = (g, i) => { if (!groups.has(g)) groups.set(g, []); groups.get(g).push(i); };
  for (const i of idx) {
    const t = ts[i];
    if (groupBy === 'person') { if (t.who.length) t.who.forEach(a => put(a.name, i)); else put(T('Unassigned'), i); }
    else if (groupBy === 'phase') { let a = i; while (par[a] >= 0) a = par[a]; put(a === i ? T('(no phase)') : ts[a].name, i); }
    else if (groupBy === 'status') put(t.done ? T('Completed') : t.started ? T('In progress') : t.late ? T('Past deadline') : T('Not started'), i);
    else if (groupBy === 'critical') put(t.crit ? T('Critical') : T('Not critical'), i);
  }
  for (const [g, list] of groups) { rows.push({ kind: 'group', label: g, count: list.length }); list.forEach(i => rows.push({ kind: 'task', i })); }
  return rows;
}

/* ---------- timescale ---------- */
const X = m => (m / 1440 + PAD) * dw;
function spanDays(gw) {
  const last = Math.max(D.finish, ...D.tasks.map(t => Math.max(t.lf, t.baselines.reduce((a, b) => Math.max(a, b.f), 0), t.deadline != null ? (t.deadline + 1) * 1440 : 0)), D.status != null ? (D.status + 2) * 1440 : 0);
  return Math.max(Math.ceil(last / 1440) + Math.ceil(40 / Math.max(dw, 0.5)), Math.ceil((innerWidth - gw - inspW()) / dw) - PAD);
}
function unitStart(unit, d) {
  const x = dateOf(d), y = x.getUTCFullYear(), m = x.getUTCMonth();
  const off = ms => Math.round((ms - START) / 864e5);
  if (unit === 'day') return d;
  if (unit === 'week') return d - ((wdOf(d) + 6) % 7);
  if (unit === 'month') return off(Date.UTC(y, m, 1));
  if (unit === 'quarter') return off(Date.UTC(y, m - m % 3, 1));
  return off(Date.UTC(y, 0, 1));
}
function unitNext(unit, d) {
  const x = dateOf(d), y = x.getUTCFullYear(), m = x.getUTCMonth();
  const off = ms => Math.round((ms - START) / 864e5);
  if (unit === 'day') return d + 1;
  if (unit === 'week') return d + 7;
  if (unit === 'month') return off(Date.UTC(y, m + 1, 1));
  if (unit === 'quarter') return off(Date.UTC(y, m + 3, 1));
  return off(Date.UTC(y + 1, 0, 1));
}
function unitLabel(unit, d, top) {
  const x = dateOf(d);
  if (unit === 'day') return x.getUTCDate();
  if (unit === 'week') return mmdd(d);
  if (unit === 'month') return top ? FMT.month.format(x) : FMT.monthOnly.format(x);
  if (unit === 'quarter') return `${T('Q')}${Math.floor(x.getUTCMonth() / 3) + 1}${top ? ' ' + x.getUTCFullYear() : ''}`;
  return x.getUTCFullYear();
}
function scaleUnits() {
  if (dw >= 16) return ['month', 'day'];
  if (dw >= 4.5) return ['month', 'week'];
  if (dw >= 1.1) return ['year', 'month'];
  return ['year', 'quarter'];
}
function timeHeader(days) {
  const [topU, botU] = scaleUnits(), td = todayOff();
  const tier = (unit, top) => {
    let html = '', d = unitStart(unit, -PAD);
    const end = days;
    while (d < end) {
      const nx = unitNext(unit, d), a = Math.max(d, -PAD), w = (Math.min(nx, end) - a) * dw;
      let cls = '', title = '';
      if (!top && unit === 'day') {
        const off = isOff(d), ex = EXC.get(d);
        cls = [off && !ex ? 'we' : '', ex && !ex.hours.length ? 'hol' : '', ex && ex.hours.length ? 'xw' : '', d === td ? 'today' : '', 'click'].join(' ');
        title = `${niceY(d)}${ex ? ' — ' + (ex.name || (ex.hours.length ? T('working day') : T('holiday'))) : ''}\n${off ? T('Day off') : hoursStr(dayHours(d))}\n${off ? T('Click to make it a working day') : T('Click to mark it as a holiday')}`;
        html += `<div class="${cls}" style="width:${w}px" data-day="${d}" title="${esc(title)}">${dw >= 18 ? unitLabel(unit, d) : ''}</div>`;
      } else {
        html += `<div style="width:${w}px" title="${esc(niceY(a))}">${w > 26 ? unitLabel(unit, d, top) : ''}</div>`;
      }
      d = nx;
    }
    return `<div class="tier ${top ? '' : 'b'}">${html}</div>`;
  };
  return tier(topU, true) + tier(botU, false);
}
function dayStripes(days) {
  let bg = '';
  if (dw >= 4) for (let o = -PAD; o < days; o++) if (isOff(o)) bg += `<div class="stripe ${EXC.has(o) ? 'hol' : ''}" style="left:${X(o * 1440)}px;width:${dw}px"></div>`;
  const td = todayOff(); if (td >= -PAD && td < days) bg += `<div class="todayl" style="left:${X(nowRel())}px" title="${T('Now')}"></div>`;
  if (D.status != null) bg += `<div class="statusl" style="left:${X((D.status + 1) * 1440)}px" title="${esc(T('Status date: {0}', niceY(D.status)))}"></div>`;
  return bg;
}

/* ---------- cells ---------- */
function dateCell(m, isEnd) {
  if (m == null) return `<div class="ro">—</div>`;
  const d = Math.floor((isEnd ? m - 1 : m) / 1440), full = when(m, isEnd), time = /\d\d:\d\d$/.test(full) ? full.slice(-5) : '';
  return `<div class="ro" title="${esc(niceY(d))}${time ? ' ' + time : ''}">${mmdd(d)} <i>${wdName(wdOf(d))}${time ? ' ' + time : ''}</i></div>`;
}
const inp = (t, f, v, extra = '') => `<div><input data-id="${t.id}" data-f="${f}" value="${esc(v)}" ${extra}></div>`;
function hl(s) {
  const e = esc(s); if (!search || /^#?\d+$/.test(search.trim())) return e;
  const q = norm(search).trim(), i = norm(s).indexOf(q);
  return i < 0 ? e : esc(s.slice(0, i)) + '<mark>' + esc(s.slice(i, i + q.length)) + '</mark>' + esc(s.slice(i + q.length));
}
function typeLabel(t) {
  if (t.summary) return T('Summary');
  if (t.sub) return T('Sub-project');
  if (t.edur === 0) return T('Milestone');
  if (t.manual != null) return T('Manual');
  if (t.work != null) return T('Fixed work');
  return t.elapsed ? T('Calendar time') : T('Fixed duration');
}
function cell(k, t, flat) {
  const ms = !t.summary && t.edur === 0, base = t.baselines.find(b => b.n === 0);
  switch (k) {
    case 'num': return `<div class="num" ${flat ? '' : `draggable="true" data-drag="${t.id}"`} title="${flat ? '' : T('Drag to reorder')}">${t.id}</div>`;
    case 'wbs': return `<div class="ro">${t.wbs}</div>`;
    case 'name': {
      const badges = [
        t.constraint ? `<span class="badge" title="${esc(consLabel(t.constraint))}">📌</span>` : '',
        t.deadline != null ? `<span class="badge" title="${esc(T('Deadline {0}', niceY(t.deadline)))}">${t.late ? '⚠️' : '⏰'}</span>` : '',
        t.delay > 0 ? `<span class="badge" title="${esc(T('Delayed {0} by leveling', fmtDur(t.delay)))}">⏸</span>` : '',
        t.conflict ? `<span class="badge" title="${esc(T('Date constraint can\'t be met'))}">⚠️</span>` : '',
        t.manual != null ? `<span class="badge" title="${T('Manually scheduled')}">✋</span>` : '',
        t.sub ? `<span class="badge" title="${esc(T('Sub-project: {0}', t.sub))}">📁</span>` : '',
        t.ext.length ? `<span class="badge" title="${esc(T('Waits for: {0}', t.ext.map(x => x.label).join(', ')))}">🔗</span>` : '',
        t.done && !t.summary ? `<span class="badge" title="${T('Completed')}">✅</span>` : '',
        t.note ? `<span class="badge" title="${esc(t.note)}">📝</span>` : '',
      ].join('');
      const dot = t.inactive ? 'x' : t.summary ? 's' : ms ? 'm' : t.done ? 'd' : t.crit ? 'c' : '';
      const chev = !flat && t.summary ? `<button class="chev ${collapsed.has(t.id) ? 'closed' : ''}" data-act="toggle" data-id="${t.id}" title="${T('Collapse / expand')}">${ic('chev')}</button>` : '<span class="chev-sp"></span>';
      return `<div class="nm" style="padding-left:${flat ? 0 : t.level * 16}px">${chev}<span class="dot ${dot}"></span><input data-id="${t.id}" data-f="name" value="${esc(t.name)}" spellcheck="false" aria-label="${T('Task name')}" ${search ? `title="${esc(t.name)}"` : ''}>${badges}</div>`;
    }
    case 'dur': return inp(t, 'dur', t.summary ? fmtDays(t.span) : fmtDur(t.edur, t.elapsed), t.summary || t.sub ? 'disabled' : t.work != null ? `disabled title="${esc(T('Set by the work and the people on it'))}"` : `title="3d, 4h, 30m, 2w, 3ed — 0 = ${T('milestone')}"`);
    case 'work': return inp(t, 'work', t.work != null ? fmtHours(t.work) : '', t.summary ? `disabled placeholder="${fmtHours(t.workT)}"` : `placeholder="${t.workT ? fmtHours(t.workT) : '—'}" title="${esc(T('Total effort (e.g. 40h). Set it to make the task effort-driven: more people → shorter.'))}"`);
    case 'start': return dateCell(t.es, false);
    case 'finish': return dateCell(t.edur > 0 || t.summary ? t.ef : t.es, t.edur > 0 || t.summary);
    case 'deps': return inp(t, 'deps', depsStr(t), `placeholder="—" title="${esc(T('Predecessors: 2, 3SS, 4FF+1d'))}"`);
    case 'who': return inp(t, 'who', whoStr(t), t.summary ? 'disabled' : `placeholder="—" list="dl-people" title="${esc(T('People, e.g. Ana, Bo[50%]'))}"`);
    case 'pct': return `<div class="pct"><input data-id="${t.id}" data-f="pct" value="${Math.round(t.summary ? t.pctR : t.pct)}%" ${t.summary || t.sub ? 'disabled' : ''}><div class="mini"><div style="width:${t.summary ? t.pctR : t.pct}%"></div></div></div>`;
    case 'cost': return `<div class="ro">${t.cost ? money(t.cost) : '—'}</div>`;
    case 'slack': return `<div class="ro ${t.slack < 0 ? 'bad' : ''}">${t.done ? '—' : fmtDays(t.slack)}</div>`;
    case 'free': return `<div class="ro">${t.done ? '—' : fmtDays(t.free)}</div>`;
    case 'astart': return t.as != null ? dateCell(t.as) : `<div class="ro">—</div>`;
    case 'afinish': return t.af != null ? dateCell(t.af, true) : `<div class="ro">—</div>`;
    case 'bstart': return base ? dateCell(base.s) : `<div class="ro">—</div>`;
    case 'bfinish': return base ? dateCell(base.f, true) : `<div class="ro">—</div>`;
    case 'slip': return `<div class="ro ${t.slip > 0 ? 'bad' : t.slip < 0 ? 'goodc' : ''}">${t.slip == null ? '—' : t.slip === 0 ? '0' : (t.slip > 0 ? '+' : '') + fmtDays(t.slip)}</div>`;
    case 'constraint': return `<div class="ro" title="${esc(consLabel(t.constraint))}">${t.constraint ? esc(consLabel(t.constraint)) : '—'}</div>`;
    case 'deadline': return `<div class="ro ${t.late ? 'bad' : ''}">${t.deadline != null ? mmdd(t.deadline) : '—'}</div>`;
    case 'priority': return inp(t, 'priority', t.priority, `title="${esc(T('Leveling priority 0–1000 (higher keeps its dates)'))}"`);
    case 'type': return `<div class="ro">${typeLabel(t)}</div>`;
    case 'ev': return `<div class="ro">${D.status != null ? money(t.ev) : '—'}</div>`;
    case 'cpi': return `<div class="ro">${t.acv > 0 ? (t.ev / t.acv).toFixed(2) : '—'}</div>`;
    case 'note': return `<div class="ro" title="${esc(t.note)}">${esc(t.note) || '—'}</div>`;
    case 'del': return `<div><button class="del" data-act="del" data-id="${t.id}" title="${T('Delete task')}">${ic('trash')}</button></div>`;
  }
  return '<div></div>';
}

/* ---------- the view ---------- */
let G = null; // what the current Gantt paint needs
function gantt() {
  const cols = colsDef(), GW = cols.reduce((a, c) => a + c[2], 0), tmpl = cols.map(c => c[2] + 'px').join(' ');
  const days = spanDays(GW), W = (days + PAD) * dw, rows = rowsModel(), H = (rows.length + 1) * RH;
  const rowOf = new Map();
  rows.forEach((r, k) => { if (r.kind === 'task' && !rowOf.has(D.tasks[r.i].id)) rowOf.set(D.tasks[r.i].id, k); });
  G = { cols, tmpl, rows, rowOf, flat: isFlat(), painted: null, W, H };
  const shown = rows.filter(r => r.kind === 'task').length;
  const welcome = !D.tasks.length ? `<div class="welcome"><h3>${T('Start planning “{0}”', esc(D.name))}</h3>
    <p>${T('Type your first task in the <b>Add a task…</b> row on the left and press Enter. Tasks are scheduled from <b>{0}</b>; change the start date at the top.', nice(0))}</p>
    <div class="acts"><button class="btn primary" data-act="add">${ic('plus')}${T('Add the first task')}</button><button class="btn" data-act="import">${ic('up')}${T('Import from ProjectLibre')}</button><button class="btn" data-act="calendar">${ic('cal')}${T('Set working hours')}</button><button class="btn" data-act="sample">${T('Try the sample project')}</button></div></div>` : '';
  return `<div class="view ${focusCrit ? 'dim' : ''}" data-v="gantt"><div class="gt ${G.flat ? 'flat' : ''}" style="--gw:${GW}px;--cols:${tmpl};--rh:${RH}px">
    <div class="corner"><div class="cols">${cols.map(c => `<div title="${esc(c[1])}">${esc(c[1])}</div>`).join('')}</div></div>
    <div class="thead" style="width:${W}px">${timeHeader(days)}</div>
    <div class="gridl" style="height:${H}px"><div class="rows" id="rows" style="height:${H}px"></div></div>
    <div class="tbody" id="tbody" style="width:${W}px;height:${H}px;background:repeating-linear-gradient(to bottom,transparent 0 ${RH - 1}px,var(--line2) ${RH - 1}px ${RH}px)">
      ${dayStripes(days)}<div id="selbands"></div>
      <svg class="links" id="links" width="${W}" height="${H}"><defs>
        <marker id="ar" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0 L8,4 L0,8z" fill="var(--dim)"/></marker>
        <marker id="arc" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0 L8,4 L0,8z" fill="var(--cr1)"/></marker>
      </defs><g id="lnks"></g></svg>
      <div id="bars"></div>${welcome}
    </div></div></div>`.replace('__SHOWN__', shown);
}
function geoOf(t, r) {
  const y = r * RH, x1 = X(t.es), x2 = Math.max(X(t.ef), x1 + 3);
  if (t.summary) return { x1, x2, y: y + 15 };
  if (t.edur === 0) return { x1: x1 - 9, x2: x1 + 9, y: y + 18 };
  return { x1, x2, y: y + 17 };
}
function barHtml(t, r) {
  const y = r * RH, ms = !t.summary && t.edur === 0, c = t.crit ? 'c' : '', tip = `data-tip="${t.id}"`, x1 = X(t.es), x2 = Math.max(X(t.ef), x1 + 3);
  let h = '';
  if (t.summary) {
    h += `<div class="sbar ${c}" style="left:${x1}px;top:${y + 11}px;width:${x2 - x1}px" data-bar="${t.id}" ${tip}><div class="sprog" style="width:${t.pctR}%"></div><span class="lab">${Math.round(t.pctR)}%${t.cost ? ' · ' + money(t.cost) : ''}</span><span class="hl" data-link="${t.id}"></span></div>`;
  } else if (ms) {
    h += `<div class="ms ${c} ${t.done ? 'done' : ''}" style="left:${x1 - 8}px;top:${y + 10}px" data-bar="${t.id}" data-move="${t.id}" ${tip}><span class="lab">${esc(t.name)} · ${nice(Math.floor(t.es / 1440))}</span><span class="hl" data-link="${t.id}"></span></div>`;
  } else {
    const w = x2 - x1, cls = [c, t.late ? 'late' : '', t.done ? 'done' : '', t.manual != null ? 'man' : '', t.inactive ? 'off' : '', t.sub ? 'sub' : ''].join(' ');
    const warn = t.late ? ` · <span class="w">⚠ ${T('past deadline')}</span>` : t.delay ? ` · <span class="w">⏸ ${T('leveled')} +${fmtDur(t.delay)}</span>` : '';
    const gaps = t.splits.filter(([a, b]) => (b + 1) * 1440 > t.es && a * 1440 < t.ef).map(([a, b]) => `<span class="gap" style="left:${Math.max(0, X(a * 1440) - x1)}px;width:${Math.min(w, X((b + 1) * 1440) - x1) - Math.max(0, X(a * 1440) - x1)}px"></span>`).join('');
    h += `<div class="bar ${cls}" style="left:${x1}px;top:${y + 7}px;width:${w}px" data-bar="${t.id}" ${t.manual != null || t.af != null ? '' : `data-move="${t.id}"`} ${tip}>
      ${t.constraint ? '<span class="pin">📌</span>' : ''}<div class="prog ${t.pct >= 100 ? 'full' : ''}" style="width:${t.pct}%"></div>${gaps}
      ${w > 70 ? `<span class="in">${esc(t.name)}</span>` : ''}
      <span class="lab">${t.who.length ? `<b>${esc(whoStr(t))}</b> · ` : ''}${t.pct}%${t.slack > 0 && !t.done ? ` · ${T('{0} slack', fmtDays(t.slack))}` : ''}${warn}</span>
      ${t.sub || t.work != null ? '' : `<span class="hr" data-resize="${t.id}"></span>`}<span class="hl" data-link="${t.id}"></span></div>`;
    if (t.slack > 0 && !t.done && X(t.lf) > x2 + 4) h += `<div class="float" style="left:${x2 + 3}px;top:${y + 32}px;width:${X(t.lf) - x2 - 3}px" title="${T('Slack')}"></div>`;
  }
  const b = t.baselines.find(x => x.n === 0);
  if (b) h += `<div class="base" style="left:${X(b.s)}px;top:${y + 29}px;width:${Math.max(X(b.f) - X(b.s), 2)}px" title="${esc(T('Baseline: {0} → {1}', when(b.s), when(b.f, true)))}"></div>`;
  if (t.deadline != null) h += `<div class="dl ${t.late ? '' : 'ok'}" style="left:${X((t.deadline + 1) * 1440) - 6}px;top:${y + 1}px" title="${esc(T('Deadline: {0}', niceY(t.deadline)))}"></div>`;
  return h;
}
function linkPath(a, b, kind) {
  const fromEnd = kind === 'FS' || kind === 'FF', toStart = kind === 'FS' || kind === 'SS';
  const x1 = fromEnd ? a.x2 : a.x1, x2 = toStart ? b.x1 : b.x2, ax = x1 + (fromEnd ? 8 : -8), end = x2 + (toStart ? -1 : 1);
  if ((toStart && ax <= x2 - 4) || (!toStart && ax >= x2 + 4)) return `M${x1},${a.y} H${ax} V${b.y} H${end}`;
  const my = a.y + (b.y > a.y ? RH / 2 : -RH / 2), bx = x2 + (toStart ? -10 : 10);
  return `M${x1},${a.y} H${ax} V${my} H${bx} V${b.y} H${end}`;
}
function paintWindow(force) {
  const v = $('#main .view'); if (!v || !G) return;
  const top = Math.max(0, v.scrollTop - 52), first = Math.max(0, Math.floor(top / RH) - 8), last = Math.min(G.rows.length + 1, Math.floor(top / RH) + Math.ceil(v.clientHeight / RH) + 8);
  if (!force && G.painted && first >= G.painted[0] && last <= G.painted[1]) return;
  const from = first, to = last;
  G.painted = [from, to];
  let rows = '', bars = '', bands = '';
  const total = D.tasks.filter(t => !t.summary).length, shown = G.rows.filter(r => r.kind === 'task').length;
  for (let k = from; k < to; k++) {
    const r = G.rows[k], y = k * RH;
    if (!r) {
      const after = primary != null && task(primary) ? task(primary) : null, filtered = filter !== 'all' || search;
      rows += `<div class="row cols add" style="top:${y}px;--cols:${G.cols[0][2]}px 1fr"><div class="num">+</div><div class="nm"><input data-id="0" data-f="new" placeholder="${esc(filtered ? T('Showing {0} of {1} tasks — add a task…', shown, total) : after ? T('Add a task… (goes after #{0}{1}) — or paste rows from Excel', after.id, after.summary ? '' : T(', linked to it')) : T('Add a task… type a name and press Enter — or paste rows from Excel'))}"></div></div>`;
      continue;
    }
    if (r.kind === 'group') { rows += `<div class="row grp" style="top:${y}px">${esc(r.label)} <small>${T('{0} task(s)', r.count)}</small></div>`; continue; }
    const t = D.tasks[r.i];
    if (sel.has(t.id)) bands += `<div class="selband" style="top:${y}px"></div>`;
    rows += `<div class="row cols ${sel.has(t.id) ? 'sel' : ''} ${t.crit ? 'crit' : ''} ${t.summary ? 'sum' : ''} ${r.dim ? 'dimmed' : ''} ${t.inactive ? 'off' : ''}" style="top:${y}px" data-row="${t.id}">${G.cols.map(c => cell(c[0], t, G.flat)).join('')}</div>`;
    bars += barHtml(t, k);
  }
  let links = '';
  for (const t of D.tasks) {
    const rb = G.rowOf.get(t.id); if (rb == null) continue;
    for (const d of t.deps) {
      const ra = G.rowOf.get(d.id); if (ra == null) continue;
      if ((ra < from || ra >= to) && (rb < from || rb >= to) && !((ra < from && rb >= to) || (rb < from && ra >= to))) continue;
      const pa = task(d.id), c = pa.crit && t.crit ? 'c' : '';
      links += `<path class="lnk ${c}" d="${linkPath(geoOf(pa, ra), geoOf(t, rb), d.kind)}" marker-end="url(#ar${c})"><title>#${d.id} → #${t.id} (${KIND()[d.kind]}${d.lag ? ', ' + lagStr(d) : ''})</title></path>`;
    }
  }
  $('#rows').innerHTML = rows; $('#bars').innerHTML = bars; $('#lnks').innerHTML = links; $('#selbands').innerHTML = bands;
}
function ganttMounted() {
  const v = $('#main .view');
  paintWindow(true);
  let raf = 0;
  v.addEventListener('scroll', () => { if (!raf) raf = requestAnimationFrame(() => { raf = 0; paintWindow(); }); }, { passive: true });
}
function scrollToTask(id) {
  const v = $('#main .view'); if (!v || !G) return;
  const r = G.rowOf.get(id); if (r == null) return;
  const y = r * RH + 52;
  if (y < v.scrollTop + 52 || y > v.scrollTop + v.clientHeight - RH) v.scrollTop = y - v.clientHeight / 2;
  const t = task(id); if (t && (X(t.es) < v.scrollLeft || X(t.es) > v.scrollLeft + v.clientWidth - 200)) v.scrollLeft = Math.max(0, X(t.es) - 200);
  paintWindow();
}
