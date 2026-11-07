"use strict";
/* rplan — printable reports (summary, who does what, cash flow, earned value, task list, problems). */

let report = { kind: 'summary', paper: store('paper') || (LANG === 'es' ? 'A4' : 'Letter'), orient: store('orient') || 'landscape' };
const REPORTS = () => [['summary', T('Project summary')], ['who', T('Who does what')], ['cash', T('Cash flow')], ['ev', T('Earned value')], ['tasks', T('Task list')], ['problems', T('Problems & late tasks')]];

function openReport(kind) {
  if (kind) report.kind = kind;
  if (!usage) loadUsage().then(() => { if (!$('#report').hidden) renderReport(); });
  $('#report').hidden = false;
  renderReport();
}
function closeReport() { $('#report').hidden = true; $('#page-setup').textContent = ''; }
function pageSetup() {
  $('#page-setup').textContent = `@page{size:${report.paper} ${report.orient};margin:12mm 12mm 14mm;@bottom-left{content:"${cssStr(D.name)} · ${cssStr(REPORTS().find(r => r[0] === report.kind)[1])}";font-size:8pt;color:#666}@bottom-right{content:counter(page) " / " counter(pages);font-size:8pt;color:#666}}`;
}
const cssStr = s => String(s).replace(/["\\\n]/g, ' ');
function printReport() {
  pageSetup();
  document.body.classList.add('printing-report');
  setTimeout(() => { print(); }, 30);
}
addEventListener('afterprint', () => document.body.classList.remove('printing-report'));

function renderReport() {
  pageSetup();
  const r = REPORTS();
  const body = { summary: repSummary, who: repWho, cash: repCash, ev: repEv, tasks: repTasks, problems: repProblems }[report.kind]();
  $('#report').innerHTML = `<div class="rbar"><b>${T('Reports')}</b>
    <select data-rep="kind">${r.map(([k, l]) => `<option value="${k}" ${report.kind === k ? 'selected' : ''}>${l}</option>`).join('')}</select>
    <span class="mut">${T('Paper')}</span><select data-rep="paper">${['A4', 'Letter', 'A3', 'Legal'].map(p => `<option ${report.paper === p ? 'selected' : ''}>${p}</option>`).join('')}</select>
    <select data-rep="orient"><option value="landscape" ${report.orient === 'landscape' ? 'selected' : ''}>${T('Landscape')}</option><option value="portrait" ${report.orient === 'portrait' ? 'selected' : ''}>${T('Portrait')}</option></select>
    <button class="btn primary" data-act="rep-print">${ic('dl')}${T('Print / save as PDF')}</button>
    <button class="btn" data-act="rep-layout" title="${T('Header, footer, margins and paper')}">${T('Page layout…')}</button>
    <span class="mut">${T('Tip: pick “Save as PDF” as the printer to share it.')}</span>
    <button class="btn" style="margin-left:auto" data-act="rep-close">${T('Close')}</button></div>
    <div class="paper ${report.orient}">${body}</div>`;
}
const head = title => `<h1>${esc(D.name)}</h1><p class="sub">${esc(title)} · ${T('generated {0}', new Date().toLocaleString(LANG))}${D.status != null ? ' · ' + T('status date {0}', niceY(D.status)) : ''}</p>`;
const card = (l, v, cls = '') => `<div class="card"><span>${l}</span><b class="${cls}">${v}</b></div>`;
const tdate = (m, end) => m == null ? '—' : esc(when(m, end));

function repSummary() {
  const leaves = D.tasks.filter(t => !t.summary && !t.inactive), done = leaves.filter(t => t.done).length;
  const base = D.tasks.filter(t => t.baselines.some(b => b.n === 0));
  const bf = base.length ? Math.max(...base.map(t => t.baselines.find(b => b.n === 0).f)) : null;
  const fin = Math.floor((D.finish - 1) / 1440);
  let slip = null;
  if (bf != null) { const a = Math.floor((bf - 1) / 1440); slip = a <= fin ? workDays(a + 1, fin) : -workDays(fin + 1, a); }
  const spi = D.ev.pv ? D.ev.ev / D.ev.pv : null, cpi = D.ev.ac ? D.ev.ev / D.ev.ac : null;
  const ms = D.tasks.filter(t => !t.summary && t.edur === 0 && !t.inactive);
  const upcoming = leaves.filter(t => t.crit && !t.done).sort((a, b) => a.es - b.es).slice(0, 12);
  const iss = issues().filter(i => i.sev >= 1).slice(0, 12);
  return head(T('Project summary')) + `<div class="cards">
    ${card(T('Start'), niceY(0))}${card(T('Finish'), niceY(fin), slip > 0 ? 'bad' : '')}${card(T('Duration'), fmtDays(D.span))}${card(T('Progress'), (+D.progress).toFixed(0) + '%')}
    ${card(T('Tasks done'), `${done} / ${leaves.length}`)}${card(T('Cost'), money(D.cost))}${card(T('Work'), fmtHours(D.work))}
    ${slip != null ? card(T('vs baseline'), slip > 0 ? T('+{0}d late', slip) : slip < 0 ? T('{0}d early', -slip) : T('on time'), slip > 0 ? 'bad' : 'good') : ''}
    ${D.status != null ? card('SPI', spi == null ? '—' : spi.toFixed(2), spi < 0.95 ? 'bad' : 'good') + card('CPI', cpi == null ? '—' : cpi.toFixed(2), cpi < 0.95 ? 'bad' : 'good') : ''}</div>
    ${ms.length ? `<h2>${T('Milestones')}</h2><table><tr><th>${T('Milestone')}</th><th>${T('Date')}</th><th>${T('Baseline')}</th><th>${T('Status')}</th></tr>${ms.map(t => { const b = t.baselines.find(x => x.n === 0); return `<tr><td>${esc(t.name)}</td><td>${tdate(t.es)}</td><td>${b ? tdate(b.s) : '—'}</td><td class="${t.late ? 'bad' : t.done ? 'good' : ''}">${t.done ? T('Done') : t.late ? T('Late') : T('Planned')}</td></tr>`; }).join('')}</table>` : ''}
    ${upcoming.length ? `<h2>${T('Next critical tasks')}</h2><table><tr><th>#</th><th>${T('Task')}</th><th>${T('Start')}</th><th>${T('Finish')}</th><th>${T('Who')}</th><th class="n">${T('Done')}</th></tr>${upcoming.map(t => `<tr><td>${t.id}</td><td>${esc(t.name)}</td><td>${tdate(t.es)}</td><td>${tdate(t.ef, true)}</td><td>${esc(whoStr(t))}</td><td class="n">${t.pct}%</td></tr>`).join('')}</table>` : ''}
    ${iss.length ? `<h2>${T('Problems')}</h2><table>${iss.map(i => `<tr><td>${i.icon}</td><td>${esc(i.text)}</td><td>${esc(i.sub || '')}</td></tr>`).join('')}</table>` : ''}
    ${D.loads.length ? `<h2>${T('People')}</h2><table><tr><th>${T('Name')}</th><th class="n">${T('Tasks')}</th><th class="n">${T('Hours')}</th><th class="n">${T('Cost')}</th><th class="n">${T('Days over capacity')}</th></tr>${D.loads.map(l => `<tr><td>${esc(l.name)}</td><td class="n">${l.tasks}</td><td class="n">${fmtHours(l.work)}</td><td class="n">${money(l.cost)}</td><td class="n ${l.over ? 'bad' : ''}">${l.over || '—'}</td></tr>`).join('')}</table>` : ''}`;
}
function repWho() {
  const people = D.resources.filter(r => D.tasks.some(t => !t.summary && t.who.some(a => a.name === r.name)));
  return head(T('Who does what')) + people.map(r => {
    const ts = D.tasks.filter(t => !t.summary && !t.inactive && t.who.some(a => a.name === r.name)).sort((a, b) => a.es - b.es);
    let hours = 0, cost = 0;
    const rows = ts.map(t => {
      const a = t.who.find(x => x.name === r.name);
      const h = r.kind === 'work' ? t.edur * a.units / 100 + t.ot : 0;
      const c = r.kind === 'work' ? r.rate * h / 60 + r.perUse : r.kind === 'material' ? r.rate * a.amount + r.perUse : a.amount;
      hours += h; cost += c;
      const amt = r.kind === 'work' ? a.units + '%' : r.kind === 'material' ? `${num(a.amount)} ${esc(r.unit)}` : money(a.amount);
      return `<tr><td>${t.wbs}</td><td>${esc(t.name)}</td><td>${tdate(t.es)}</td><td>${tdate(t.ef, true)}</td><td class="n">${amt}</td><td class="n">${r.kind === 'work' ? fmtHours(h) : '—'}</td><td class="n">${money(c)}</td><td class="n">${t.pct}%</td></tr>`;
    }).join('');
    return `<h2>${esc(r.name)} <span style="font-weight:400;color:#555">· ${r.kind === 'work' ? fmtHours(hours) + ' · ' : ''}${money(cost)}</span></h2><table><tr><th>WBS</th><th>${T('Task')}</th><th>${T('Start')}</th><th>${T('Finish')}</th><th class="n">${T('Units')}</th><th class="n">${T('Hours')}</th><th class="n">${T('Cost')}</th><th class="n">${T('Done')}</th></tr>${rows}</table>`;
  }).join('') || `<p>${T('Nobody is assigned yet.')}</p>`;
}
/* Cost spread over each task's working days (weighted by its usage when we have it). */
function costByDay(useBaseline) {
  const out = new Map(), add = (d, v) => out.set(d, (out.get(d) || 0) + v);
  // The server knows when each cost is spent (rates by date, start/end accrual).
  if (!useBaseline && usage?.costs) { for (const [, days] of usage.costs) for (const [d, c] of days) add(d, c); return out; }
  const byTask = usage ? new Map(usage.tasks.map(([id, days]) => [id, days])) : new Map();
  for (const t of D.tasks) {
    if (t.summary || t.inactive) continue;
    if (useBaseline) {
      const b = t.baselines.find(x => x.n === 0);
      // 'pv' mode: tasks without a baseline cost count at their current plan (like the totals).
      if (!b || !b.cost) { if (useBaseline !== 'pv') continue; } else {
      const a = Math.floor(b.s / 1440), z = Math.max(a, Math.floor((b.f - 1) / 1440)), days = [];
      for (let d = a; d <= z; d++) if (!isOff(d)) days.push(d);
      if (!days.length) days.push(a);
      days.forEach(d => add(d, b.cost / days.length));
      continue;
    } }
    if (!t.cost) continue;
    const days = byTask.get(t.id);
    if (days && days.length) { const tot = days.reduce((a, [, m]) => a + m, 0) || 1; days.forEach(([d, m]) => add(d, t.cost * m / tot)); }
    else add(sDay(t), t.cost);
  }
  return out;
}
// Report chart colors for each style (reports are on white paper, whatever the mode).
const CHART_COLORS = { modern: ['#3b7cff', '#16834f', '#c62839', '#999'], corporate: ['#2f6db5', '#2e7d4f', '#c0392b', '#8a9bb0'], classic: ['#4472c4', '#70ad47', '#c00000', '#a5a5a5'], executive: ['#566573', '#4d7c6a', '#a93226', '#aab4be'], mono: ['#111', '#111', '#111', '#888'] };
const chartColor = k => (CHART_COLORS[document.documentElement.dataset.style] || CHART_COLORS.modern)[k];
// In black & white the lines are told apart by their dash pattern instead.
const chartDash = k => document.documentElement.dataset.style === 'mono' ? ['', '7 4', '2 3', '4 4'][k] : '';
function lineChart(series, labels, fmt) {
  const W = 760, H = 220, P = 40, max = Math.max(1, ...series.flatMap(s => s.values.filter(v => v != null)));
  const x = k => P + k * (W - P - 10) / Math.max(1, labels.length - 1), y = v => H - 24 - v / max * (H - 40);
  const lines = series.map(s => `<polyline fill="none" stroke="${s.color}" stroke-width="2.5" ${s.dash ? `stroke-dasharray="${s.dash === 1 ? '6 4' : s.dash}"` : ''} points="${s.values.map((v, k) => v == null ? '' : `${x(k)},${y(v)}`).join(' ')}"/>`).join('');
  const ticks = labels.map((l, k) => k % Math.ceil(labels.length / 10) ? '' : `<text x="${x(k)}" y="${H - 6}" font-size="10" text-anchor="middle" fill="#666">${esc(l)}</text>`).join('');
  const legend = series.map((s, k) => `<line x1="${P + k * 170}" x2="${P + k * 170 + 14}" y1="6" y2="6" stroke="${s.color}" stroke-width="3" ${s.dash ? `stroke-dasharray="${s.dash === 1 ? '6 4' : s.dash}"` : ''}/><text x="${P + 18 + k * 170}" y="10" font-size="11" fill="#333">${esc(s.name)}</text>`).join('');
  const grid = [0, 0.25, 0.5, 0.75, 1].map(f => `<line x1="${P}" x2="${W - 10}" y1="${y(max * f)}" y2="${y(max * f)}" stroke="#eee"/><text x="${P - 4}" y="${y(max * f) + 3}" font-size="9" text-anchor="end" fill="#888">${esc(fmt(max * f))}</text>`).join('');
  return `<svg viewBox="0 0 ${W} ${H}" width="100%" style="max-width:${W}px">${grid}${lines}${ticks}${legend}</svg>`;
}
function repCash() {
  const unit = (D.finish / 1440) > 120 ? 'month' : 'week', ps = periods(unit);
  const lab = ([a]) => unit === 'month' ? FMT.month.format(dateOf(a)) : mmdd(a);
  const sum = map => ps.map(([a, b]) => { let v = 0; for (const [d, c] of map) if (d >= a && d < b) v += c; return v; });
  const cur = sum(costByDay(false)), hasBase = D.tasks.some(t => t.baselines.some(b => b.n === 0 && b.cost));
  const base = hasBase ? sum(costByDay(true)) : null;
  const cum = arr => { let s = 0; return arr.map(v => s += v); };
  const series = [{ name: T('Planned (cumulative)'), color: chartColor(0), values: cum(cur) }];
  if (base) series.push({ name: T('Baseline (cumulative)'), color: chartColor(3), dash: 1, values: cum(base) });
  return head(T('Cash flow')) + (usage ? '' : `<p class="sub">${T('Loading…')}</p>`) + lineChart(series, ps.map(lab), money) +
    `<table><tr><th>${unit === 'month' ? T('Month') : T('Week of')}</th><th class="n">${T('Cost')}</th><th class="n">${T('Cumulative')}</th>${base ? `<th class="n">${T('Baseline')}</th><th class="n">${T('Baseline cumulative')}</th>` : ''}</tr>
    ${ps.map((p, k) => `<tr><td>${esc(lab(p))}</td><td class="n">${money(cur[k])}</td><td class="n">${money(series[0].values[k])}</td>${base ? `<td class="n">${money(base[k])}</td><td class="n">${money(series[1].values[k])}</td>` : ''}</tr>`).join('')}
    <tr><td><b>${T('Total')}</b></td><td class="n"><b>${money(D.cost)}</b></td><td></td>${base ? `<td class="n"><b>${money(base.reduce((a, b) => a + b, 0))}</b></td><td></td>` : ''}</tr></table>`;
}
function repEv() {
  if (D.status == null) return head(T('Earned value')) + `<p>${T('Set a status date (at the top of the window) to measure planned value, earned value and actual cost as of that day. Saving a baseline first gives the most meaningful numbers.')}</p>`;
  const e = D.ev, spi = e.pv ? e.ev / e.pv : null, cpi = e.ac ? e.ev / e.ac : null, eac = cpi ? e.bac / cpi : null;
  const unit = (D.finish / 1440) > 120 ? 'month' : 'week', ps = periods(unit), lab = ([a]) => unit === 'month' ? FMT.month.format(dateOf(a)) : mmdd(a);
  const hasBase = D.tasks.some(t => t.baselines.some(b => b.n === 0 && b.cost));
  const pvMap = costByDay('pv'), st = D.status;
  let s = 0;
  const pv = ps.map(([a, b]) => { for (const [d, c] of pvMap) if (d >= a && d < b) s += c; return s; });
  const k = ps.findIndex(([a, b]) => st >= a && st < b);
  const evS = ps.map((p, j) => j === k ? e.ev : j < k ? null : null), acS = ps.map((p, j) => j === k ? e.ac : null);
  if (k > 0) { evS[0] = 0; acS[0] = 0; for (let j = 1; j < k; j++) { evS[j] = e.ev * j / k; acS[j] = e.ac * j / k; } }
  const chart = lineChart([{ name: T('Planned value (PV)'), color: chartColor(0), dash: chartDash(0), values: pv }, { name: T('Earned value (EV)'), color: chartColor(1), dash: chartDash(1), values: evS }, { name: T('Actual cost (AC)'), color: chartColor(2), dash: chartDash(2), values: acS }], ps.map(lab), money);
  const rows = D.tasks.filter(t => !t.inactive && (t.bac || t.summary)).map(t => {
    const sv = t.ev - t.pv, cv = t.ev - t.acv;
    return `<tr><td>${t.wbs}</td><td>${t.summary ? '<b>' : ''}${'&nbsp;'.repeat(t.level * 3)}${esc(t.name)}${t.summary ? '</b>' : ''}</td><td class="n">${money(t.bac)}</td><td class="n">${money(t.pv)}</td><td class="n">${money(t.ev)}</td><td class="n">${money(t.acv)}</td><td class="n ${sv < 0 ? 'bad' : ''}">${money(sv)}</td><td class="n ${cv < 0 ? 'bad' : ''}">${money(cv)}</td><td class="n">${t.pv ? (t.ev / t.pv).toFixed(2) : '—'}</td><td class="n">${t.acv ? (t.ev / t.acv).toFixed(2) : '—'}</td></tr>`;
  }).join('');
  return head(T('Earned value')) + `<div class="cards">${card('BAC', money(e.bac))}${card('PV', money(e.pv))}${card('EV', money(e.ev))}${card('AC', money(e.ac))}
    ${card('SV', money(e.ev - e.pv), e.ev < e.pv ? 'bad' : 'good')}${card('CV', money(e.ev - e.ac), e.ev < e.ac ? 'bad' : 'good')}${card('SPI', spi == null ? '—' : spi.toFixed(2), spi < 0.95 ? 'bad' : 'good')}${card('CPI', cpi == null ? '—' : cpi.toFixed(2), cpi < 0.95 ? 'bad' : 'good')}
    ${card(T('Estimate at completion'), eac == null ? '—' : money(eac), eac > e.bac ? 'bad' : '')}${card(T('Variance at completion'), eac == null ? '—' : money(e.bac - eac), eac > e.bac ? 'bad' : 'good')}</div>
    <p class="sub">${T('SPI below 1 means behind schedule; CPI below 1 means over budget.')} ${hasBase ? '' : T('No baseline with costs yet — planned value uses the current plan.')}</p>${chart}
    <h2>${T('By task')}</h2><table><tr><th>WBS</th><th>${T('Task')}</th><th class="n">BAC</th><th class="n">PV</th><th class="n">EV</th><th class="n">AC</th><th class="n">SV</th><th class="n">CV</th><th class="n">SPI</th><th class="n">CPI</th></tr>${rows}</table>`;
}
function repTasks() {
  return head(T('Task list')) + `<table><tr><th>WBS</th><th>${T('Task')}</th><th>${T('Start')}</th><th>${T('Finish')}</th><th class="n">${T('Dur')}</th><th>${T('Who')}</th><th class="n">${T('Done')}</th><th class="n">${T('Cost')}</th><th class="n">${T('Slack')}</th></tr>
    ${D.tasks.filter(t => !t.inactive).map(t => `<tr><td>${t.wbs}</td><td>${t.summary ? '<b>' : ''}${'&nbsp;'.repeat(t.level * 3)}${esc(t.name)}${t.summary ? '</b>' : ''}${t.crit && !t.summary ? ' <span class="bad">●</span>' : ''}</td><td>${tdate(t.es)}</td><td>${tdate(t.edur || t.summary ? t.ef : t.es, !!(t.edur || t.summary))}</td><td class="n">${t.summary ? fmtDays(t.span) : fmtDur(t.edur, t.elapsed)}</td><td>${esc(whoStr(t))}</td><td class="n">${Math.round(t.summary ? t.pctR : t.pct)}%</td><td class="n">${t.cost ? money(t.cost) : ''}</td><td class="n">${t.done || t.summary ? '' : fmtDays(t.slack)}</td></tr>`).join('')}</table>
    <p class="sub"><span class="bad">●</span> ${T('critical path')}</p>`;
}
function repProblems() {
  const iss = issues();
  const late = D.tasks.filter(t => t.late || t.slip > 0).sort((a, b) => (b.slip || 0) - (a.slip || 0));
  return head(T('Problems & late tasks')) + (iss.length ? `<table><tr><th></th><th>${T('Problem')}</th><th>${T('Details')}</th></tr>${iss.map(i => `<tr><td>${i.icon}</td><td>${esc(i.text)}</td><td>${esc(i.sub || '')}</td></tr>`).join('')}</table>` : `<p class="good">✓ ${T('No problems found')}</p>`)
    + (late.length ? `<h2>${T('Late or slipping tasks')}</h2><table><tr><th>#</th><th>${T('Task')}</th><th>${T('Finish')}</th><th>${T('Deadline')}</th><th class="n">${T('Slip vs baseline')}</th></tr>${late.map(t => `<tr><td>${t.id}</td><td>${esc(t.name)}</td><td>${tdate(t.ef, true)}</td><td>${t.deadline != null ? niceY(t.deadline) : '—'}</td><td class="n bad">${t.slip ? '+' + fmtDays(t.slip) : '—'}</td></tr>`).join('')}</table>` : '');
}
