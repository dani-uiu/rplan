"use strict";
/* rplan — Network, WBS chart, Usage (tables + workload chart) and People views. */

function network() {
  const par = parentsOf(D.tasks), byId = new Map(D.tasks.map((t, i) => [t.id, i]));
  const leavesOf = i => { const out = []; for (let j = i; j < subtreeEnd(D.tasks, i); j++) if (!D.tasks[j].summary) out.push(j); return out; };
  const edges = [];
  D.tasks.forEach((t, i) => {
    if (t.summary || t.inactive) return;
    const deps = []; for (let a = i; a >= 0; a = par[a]) deps.push(...D.tasks[a].deps);
    for (const d of deps) {
      const pi = byId.get(d.id); if (pi == null || D.tasks[pi].inactive) continue;
      let srcs = D.tasks[pi].summary ? leavesOf(pi) : [pi];
      if (D.tasks[pi].summary && srcs.length) { const st = d.kind === 'SS' || d.kind === 'SF'; const key = j => st ? D.tasks[j].es : D.tasks[j].ef; const best = st ? Math.min(...srcs.map(key)) : Math.max(...srcs.map(key)); srcs = [srcs.find(j => key(j) === best)]; }
      for (const s of srcs) edges.push({ from: s, to: i, d });
    }
  });
  const leaves = D.tasks.map((t, i) => i).filter(i => !D.tasks[i].summary && !D.tasks[i].inactive && matchesSearch(D.tasks[i]) && matchesFilter(D.tasks[i])), lvl = new Map();
  const leafSet = new Set(leaves);
  const into = new Map(leaves.map(i => [i, []])); edges.forEach(e => { if (leafSet.has(e.from)) into.get(e.to)?.push(e.from); });
  const L = i => { if (lvl.has(i)) return lvl.get(i); lvl.set(i, 0); const v = into.get(i).length ? 1 + Math.max(...into.get(i).map(L)) : 0; lvl.set(i, v); return v; };
  leaves.forEach(L);
  const cols = new Map(), pos = new Map(); let W = 0, H = 0;
  for (const i of leaves) { const l = lvl.get(i); if (!cols.has(l)) cols.set(l, []); cols.get(l).push(i); }
  for (const [l, is] of cols) is.forEach((i, r) => { const x = 40 + l * 270, y = 40 + r * 168; pos.set(i, { x, y }); W = Math.max(W, x + 260); H = Math.max(H, y + 168); });
  let nodes = '', es = '';
  for (const i of leaves) {
    const t = D.tasks[i], { x, y } = pos.get(i), grp = par[i] >= 0 ? D.tasks[par[i]].name : '';
    nodes += `<div class="node ${t.crit ? 'c' : ''} ${t.done ? 'd' : ''} ${sel.has(t.id) ? 'sel' : ''}" style="left:${x}px;top:${y}px" data-node="${t.id}">
      <h4><span class="dot ${t.edur === 0 ? 'm' : t.done ? 'd' : t.crit ? 'c' : ''}"></span><small>#${t.id}</small>${esc(t.name)}</h4>${grp ? `<div class="grp">${esc(grp)}</div>` : ''}
      <div class="g"><span>${T('Start')}</span><span>${T('Finish')}</span><b>${nice(sDay(t))}</b><b>${nice(fDay(t))}</b>
      <span>${T('Duration')}</span><span>${T('Slack')}</span><b>${t.edur ? fmtDur(t.edur, t.elapsed) : T('milestone')}</b><b style="color:${t.slack > 0 || t.done ? 'var(--fg)' : 'var(--cr1)'}">${t.done ? '✓' : fmtDays(t.slack)}</b></div>
      <div class="g" style="margin-top:4px"><span>${esc(whoStr(t) || T('Unassigned'))}</span><span style="text-align:right">${t.pct}%</span></div>
      <div class="pb"><div style="width:${t.pct}%"></div></div></div>`;
  }
  for (const e of edges) {
    const a = pos.get(e.from), b = pos.get(e.to); if (!a || !b) continue;
    const s = D.tasks[e.from], t = D.tasks[e.to], c = s.crit && t.crit ? 'c' : '';
    const x1 = a.x + 210, y1 = a.y + 55, x2 = b.x - 4, y2 = b.y + 55, mx = (x1 + x2) / 2;
    es += `<path class="nedge ${c}" d="M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}" marker-end="url(#n${c})"/>`;
    const lab = (e.d.kind !== 'FS' ? e.d.kind : '') + lagStr(e.d);
    if (lab) es += `<text x="${mx}" y="${(y1 + y2) / 2 - 6}" fill="var(--mut)" font-size="11" text-anchor="middle">${lab}</text>`;
  }
  return `<div class="view" data-v="network"><div class="net" style="width:${W + 40}px;height:${H + 40}px">
    <svg width="${W + 40}" height="${H + 40}" style="position:absolute;left:0;top:0"><defs>
      <marker id="n" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" orient="auto"><path d="M0,0 L8,4 L0,8z" fill="var(--dim)"/></marker>
      <marker id="nc" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" orient="auto"><path d="M0,0 L8,4 L0,8z" fill="var(--cr1)"/></marker>
    </defs>${es}</svg>${nodes || `<div class="empty">${T('No tasks yet.')}</div>`}</div></div>`;
}

/* WBS: the phase structure as a top-down tree, one color per outline level. */
const LEVEL_COLORS = 6;
function wbsView() {
  const ts = D.tasks, par = parentsOf(ts), kids = ts.map(() => []), roots = [];
  ts.forEach((t, i) => (par[i] >= 0 ? kids[par[i]] : roots).push(i));
  const NW = 170, GAP = 18, LH = 112, pos = new Map();
  let cursor = 30, maxDepth = 0;
  const place = (i, depth) => {
    maxDepth = Math.max(maxDepth, depth);
    const ch = i < 0 ? roots : (collapsed.has(ts[i].id) ? [] : kids[i]);
    let x;
    if (!ch.length) { x = cursor; cursor += NW + GAP; }
    else { const xs = ch.map(c => place(c, depth + 1)); x = (xs[0] + xs[xs.length - 1]) / 2; }
    pos.set(i, { x, y: 20 + depth * LH });
    return x;
  };
  place(-1, 0);
  // The project card is a header, not part of the tree: top-level items aren't wired to it.
  pos.set(-1, { x: 30, y: 20 });
  let nodes = '', edges = '';
  const node = (i) => {
    const { x, y } = pos.get(i);
    if (i < 0) return `<div class="wnode s" style="left:${x}px;top:${y}px"><b>${esc(D.name)}</b><small>${niceY(0)} → ${niceY(Math.floor((D.finish - 1) / 1440))}</small><small>${money(D.cost)} · ${(+D.progress).toFixed(0)}%</small><div class="pb"><div style="width:${D.progress}%"></div></div></div>`;
    const t = ts[i];
    return `<div class="wnode lv${t.level % LEVEL_COLORS} ${t.summary ? 's' : ''} ${t.crit ? 'c' : ''} ${sel.has(t.id) ? 'sel' : ''}" style="left:${x}px;top:${y}px" data-node="${t.id}" data-level="${t.level}" title="${esc(t.name)}">
      <small class="wnum">${t.wbs}${t.summary && collapsed.has(t.id) ? ' · ' + T('{0} hidden', subtreeEnd(ts, i) - i - 1) : ''}${t.crit ? ` <span class="wcrit" title="${T('Critical')}">●</span>` : ''}</small><b>${esc(t.name)}</b>
      <small>${mmdd(sDay(t))} → ${mmdd(fDay(t))} · ${t.summary ? fmtDays(t.span) : fmtDur(t.edur, t.elapsed)}</small><small>${t.cost ? money(t.cost) + ' · ' : ''}${Math.round(t.summary ? t.pctR : t.pct)}%</small>
      <div class="pb"><div style="width:${t.summary ? t.pctR : t.pct}%"></div></div></div>`;
  };
  for (const [i, p] of pos) {
    nodes += node(i);
    const pi = i < 0 ? -1 : par[i];
    if (pi >= 0) {
      const q = pos.get(pi);
      const x1 = q.x + NW / 2, y1 = q.y + 78, x2 = p.x + NW / 2, y2 = p.y, my = (y1 + y2) / 2;
      edges += `<path class="wedge lv${ts[i].level % LEVEL_COLORS}" d="M${x1},${y1} V${my} H${x2} V${y2}"/>`;
    }
  }
  const W = cursor + 40, H = 20 + (maxDepth + 1) * LH + 20;
  // Legend: one chip per level that appears in the chart.
  const levels = [...new Set(ts.map(t => t.level))].sort((a, b) => a - b);
  const legend = levels.length ? `<div class="wlegend">${levels.map(l => `<span class="lv${l % LEVEL_COLORS}"><i></i>${T('Level {0}', l + 1)}</span>`).join('')}</div>` : '';
  return `<div class="view" data-v="wbs"><div class="net" style="width:${W}px;height:${H}px">${legend}<svg width="${W}" height="${H}" style="position:absolute;left:0;top:0">${edges}</svg>${nodes}</div></div>`;
}

/* ---------- usage ---------- */
function personMinutes(r, d) {
  if (r.off.some(([a, b]) => a <= d && d <= b)) return 0;
  const h = EXC.has(d) ? EXC.get(d).hours : (r.week || D.week)[wdOf(d)];
  return hoursLen(h) * (r.capacity || 100) / 100;
}
function periods(unit) {
  const lastDay = Math.ceil(D.finish / 1440) + 1, out = [];
  let d = unitStart(unit, 0);
  while (d <= lastDay && out.length < 400) { const n = unitNext(unit, d); out.push([d, n]); d = n; }
  return out;
}
function bucket(days, ps) {
  const out = new Array(ps.length).fill(0);
  let k = 0;
  for (const [d, m] of days) { while (k < ps.length && d >= ps[k][1]) k++; if (k < ps.length && d >= ps[k][0]) out[k] += m; }
  return out;
}
function usageView() {
  if (!usage) { loadUsage(); return `<div class="view" data-v="usage"><div class="empty">${T('Loading…')}</div></div>`; }
  const mode = store('usageMode') || 'person', unit = store('usagePeriod') || 'week', ps = periods(unit);
  const label = ([a]) => unit === 'day' ? `${wdName(wdOf(a))} ${mmdd(a)}` : unit === 'week' ? mmdd(a) : unit === 'month' ? FMT.month.format(dateOf(a)) : unitLabel(unit, a, true);
  const h = m => m ? FMT.num.format(m / 60) : '·';
  const bar = `<div class="subbar"><div class="seg">${[['person', T('By person')], ['task', T('By task')], ['chart', T('Workload chart')]].map(([v, l]) => `<button data-act="usage-mode" data-v="${v}" class="${mode === v ? 'on' : ''}">${l}</button>`).join('')}</div>
    <div class="seg">${[['day', T('Days')], ['week', T('Weeks')], ['month', T('Months')]].map(([v, l]) => `<button data-act="usage-period" data-v="${v}" class="${unit === v ? 'on' : ''}">${l}</button>`).join('')}</div>
    <span class="mut">${T('Hours of work per {0}', { day: T('day'), week: T('week'), month: T('month') }[unit])}</span></div>`;
  const byTask = new Map(usage.tasks.map(([id, days]) => [id, bucket(days, ps)]));
  if (mode === 'chart') {
    const rows = usage.people.map(p => {
      const r = resOf(p.name) || { off: [], week: null, capacity: 100 };
      const tot = new Array(ps.length).fill(0);
      for (const [, days] of p.tasks) bucket(days, ps).forEach((m, k) => tot[k] += m);
      const cap = ps.map(([a, b]) => { let m = 0; for (let d = a; d < b; d++) m += personMinutes(r, d); return m; });
      const other = new Array(ps.length).fill(0);
      const ol = D.otherLoads.find(o => o.name === p.name);
      if (ol) bucket(ol.days.map(([d, , m]) => [d, m]), ps).forEach((m, k) => other[k] = m);
      const max = Math.max(1, ...tot.map((m, k) => m + other[k]), ...cap), BW = 28, HH = 70;
      const bars = ps.map((pp, k) => {
        const x = k * BW + 4, hh = tot[k] / max * HH, ho = other[k] / max * HH, hc = cap[k] / max * HH, over = tot[k] + other[k] > cap[k] + 1;
        return `<rect x="${x}" y="${HH - hh}" width="${BW - 8}" height="${hh}" rx="3" fill="${over ? 'var(--cr1)' : 'var(--ok1)'}"><title>${esc(label(pp))}: ${h(tot[k])}h / ${h(cap[k])}h</title></rect>`
          + (ho ? `<rect x="${x}" y="${HH - hh - ho}" width="${BW - 8}" height="${ho}" rx="3" fill="var(--dim)"><title>${T('Other projects')}: ${h(other[k])}h</title></rect>` : '')
          + `<line x1="${x - 2}" x2="${x + BW - 6}" y1="${HH - hc}" y2="${HH - hc}" stroke="var(--warn)" stroke-width="2"/>`;
      }).join('');
      return `<div class="chartrow"><div><b>${esc(p.name)}</b><br><small class="mut">${T('{0}h planned', h(tot.reduce((a, b) => a + b, 0)))}</small></div><svg width="${ps.length * BW + 8}" height="${HH + 22}">${bars}${ps.map((pp, k) => k % Math.ceil(60 / BW) ? '' : `<text x="${k * BW + 4}" y="${HH + 16}" font-size="10" fill="var(--mut)">${esc(label(pp))}</text>`).join('')}</svg></div>`;
    }).join('');
    return `<div class="view" data-v="usage">${bar}<div style="padding:8px 0">${rows || `<div class="empty">${T('Assign people to tasks to see their workload.')}</div>`}<div class="mut" style="padding:8px 14px">${T('Bars: planned hours (red = more than they can work). Orange line: available hours. Grey: other projects sharing the team file.')}</div></div></div>`;
  }
  const head = `<tr><th>${mode === 'person' ? T('Person / task') : T('Task')}</th><th>${T('Total')}</th>${ps.map(p => `<th>${esc(label(p))}</th>`).join('')}</tr>`;
  let body = '';
  const rowHtml = (name, vals, cls = '', capv = null) => `<tr class="${cls}"><td title="${esc(name)}">${name}</td><td>${h(vals.reduce((a, b) => a + b, 0))}</td>${vals.map((m, k) => `<td class="${!m ? 'z' : capv && m > capv[k] + 1 ? 'over' : ''}">${h(m)}</td>`).join('')}</tr>`;
  if (mode === 'person') {
    const tot = new Array(ps.length).fill(0);
    for (const p of usage.people) {
      const r = resOf(p.name) || { off: [], week: null, capacity: 100 };
      const sum = new Array(ps.length).fill(0), lines = [];
      for (const [id, days] of p.tasks) { const v = bucket(days, ps); v.forEach((m, k) => sum[k] += m); lines.push(rowHtml(`&nbsp;&nbsp;&nbsp;#${id} ${esc(task(id)?.name || '')}`, v)); }
      const cap = ps.map(([a, b]) => { let m = 0; for (let d = a; d < b; d++) m += personMinutes(r, d); return m; });
      sum.forEach((m, k) => tot[k] += m);
      body += rowHtml(`<b>${esc(p.name)}</b>`, sum, 'p', cap) + lines.join('');
    }
    body += rowHtml(T('Team total'), tot, 'tot');
  } else {
    const ts = D.tasks, par = parentsOf(ts), sums = ts.map(() => new Array(ps.length).fill(0));
    ts.forEach((t, i) => { const v = byTask.get(t.id); if (v) for (let a = i; a >= 0; a = par[a]) v.forEach((m, k) => sums[a][k] += m); });
    ts.forEach((t, i) => { if (sums[i].some(Boolean) && matchesSearch(t)) body += rowHtml(`${'&nbsp;'.repeat(t.level * 3)}${t.summary ? '<b>' : ''}#${t.id} ${esc(t.name)}${t.summary ? '</b>' : ''}`, sums[i], t.summary ? 'p' : ''); });
    const tot = new Array(ps.length).fill(0); ts.forEach((t, i) => { if (par[i] < 0) sums[i].forEach((m, k) => tot[k] += m); });
    body += rowHtml(T('Total'), tot, 'tot');
  }
  return `<div class="view" data-v="usage">${bar}<table class="usage"><thead>${head}</thead><tbody>${body}</tbody></table></div>`;
}

/* ---------- people ---------- */
function weekSummary(w) {
  if (!w) return T('Project hours');
  const days = ORDER.filter(i => w[i].length);
  return days.length ? `${days.map(wdName).join(' ')} · ${FMT.num.format(days.reduce((a, i) => a + hoursLen(w[i]), 0) / 60)}h/${T('wk')}` : T('Never');
}
function people() {
  const GW = 1000, days = spanDays(GW), W = (days + PAD) * dw, loads = new Map(D.loads.map(l => [l.name, l])), others = new Map(D.otherLoads.map(o => [o.name, o]));
  const ppl = D.resources, bad = D.loads.filter(l => l.over > 0), leveled = D.tasks.some(t => t.delay > 0);
  let rows = '', cells = '', bg = dayStripes(days);
  const tmpl = '186px 96px 70px 64px 64px 64px 116px 1fr 150px 30px';
  ppl.forEach((p, i) => {
    const l = loads.get(p.name) || { work: 0, cost: 0, tasks: 0, over: 0, days: [] }, h = [...p.name].reduce((a, c) => (a * 31 + c.charCodeAt(0)) % 360, 0), y = i * RRH;
    const dis = p.pooled ? `disabled title="${esc(T('Comes from the team file {0} — edit it there', D.pool || ''))}"` : '';
    const work = p.kind === 'work', nm = esc(p.name);
    const cost = D.tasks.filter(t => !t.summary && t.who.some(a => a.name === p.name)).reduce((a, t) => {
      const asg = t.who.find(x => x.name === p.name);
      return a + (p.kind === 'work' ? p.rate * t.edur * asg.units / 100 / 60 + p.perUse + p.overtime * t.ot / 60 : p.kind === 'material' ? p.rate * asg.amount + p.perUse : asg.amount);
    }, 0);
    rows += `<div class="row cols" style="top:${y}px;--cols:${tmpl}">
      <div class="who"><div class="av" style="background:linear-gradient(135deg,hsl(${h} 70% 55%),hsl(${(h + 40) % 360} 70% 45%))">${esc(p.name.slice(0, 2).toUpperCase())}</div><input data-res="name" data-name="${nm}" value="${nm}" ${dis}>${p.pooled ? `<span class="pooled">${T('team')}</span>` : ''}</div>
      <div><select data-res="kind" data-name="${nm}" ${dis}>${[['work', T('Person')], ['material', T('Material')], ['cost', T('Cost')]].map(([v, lab]) => `<option value="${v}" ${p.kind === v ? 'selected' : ''}>${lab}</option>`).join('')}</select></div>
      <div><input data-res="rate" data-name="${nm}" value="${p.rate || ''}" placeholder="0" ${dis} title="${esc(work ? T('Cost per hour ({0})', D.currency) : p.kind === 'material' ? T('Cost per unit') : T('Not used for cost resources'))}" ${p.kind === 'cost' ? 'disabled' : ''}></div>
      <div><input data-res="capacity" data-name="${nm}" value="${work ? p.capacity + '%' : esc(p.unit)}" placeholder="${work ? '' : T('unit')}" ${dis} title="${esc(work ? T('Max units — 50% = half time') : T('Material unit, e.g. bags'))}" ${p.kind === 'cost' ? 'disabled' : ''}></div>
      <div><input data-res="perUse" data-name="${nm}" value="${p.perUse || ''}" placeholder="—" ${dis} title="${esc(T('Cost per use (each task)'))}" ${p.kind === 'cost' ? 'disabled' : ''}></div>
      <div><input data-res="overtime" data-name="${nm}" value="${p.overtime || ''}" placeholder="—" ${dis} title="${esc(T('Overtime cost per hour'))}" ${work ? '' : 'disabled'}></div>
      <div>${work ? `<button class="btn sm" data-act="res-hours" data-name="${nm}" ${p.pooled ? 'disabled' : ''} title="${esc(T('Personal working week'))}">${esc(weekSummary(p.week))}</button>` : ''}</div>
      <div><input data-res="off" data-name="${nm}" value="${esc(p.off.map(([a, b]) => rangeStr(a, b)).join(', '))}" placeholder="${T('Days off: 2026-12-24..2026-12-31')}" ${dis} ${work ? '' : 'disabled'}></div>
      <div class="meta">${work ? `<b>${fmtHours(l.work)}</b> · ` : ''}${T('{0} task(s)', l.tasks || D.tasks.filter(t => t.who.some(a => a.name === p.name)).length)} · ${money(work ? l.cost : cost)}${l.over ? `<br><span class="bad">${T('{0}d over capacity', l.over)}</span>` : ''}</div>
      <div>${p.pooled ? '' : `<button class="del" data-act="res-rm" data-name="${nm}" title="${T('Remove')}">${ic('trash')}</button>`}</div></div>`;
    if (!work) return;
    for (const [a, b] of p.off) cells += `<div class="poff" style="left:${X(a * 1440)}px;top:${y}px;width:${(b - a + 1) * dw}px" title="${esc(T('{0} is off', p.name))}"></div>`;
    for (const [d, u, mins, ids] of l.days) {
      const names = ids.map(id => '#' + id + ' ' + (task(id)?.name || '')).join('\n'), over = u > p.capacity;
      cells += `<div class="cell ${over ? 'over' : ''}" style="left:${X(d * 1440) + 1}px;width:${Math.max(dw - 2, 1)}px;top:${y + 12}px;--a:${Math.min(u / p.capacity, 1)}" title="${esc(`${p.name} · ${niceY(d)} · ${u}% / ${p.capacity}% · ${fmtHours(mins)}\n${names}`)}">${dw >= 22 ? u : ''}</div>`;
    }
    const o = others.get(p.name);
    if (o) for (const [d, u, , projects] of o.days) cells += `<div class="cell other" style="left:${X(d * 1440) + 1}px;width:${Math.max(dw - 2, 1)}px;top:${y + 42}px" title="${esc(T('Also {0}% in: {1}', u, projects.join(', ')))}"></div>`;
  });
  const addY = ppl.length * RRH;
  rows += `<div class="row cols add" style="top:${addY}px;--cols:${tmpl}"><div class="who" style="grid-column:span 4"><div class="av" style="background:var(--panel3);color:var(--mut)">+</div><input data-res="new" data-name="" placeholder="${T('Add a person, material or cost… (name, then Enter)')}"></div></div>`;
  const banner = !ppl.length ? `<div class="banner">${T('Assign people in the Gantt\'s <b>Who</b> column (or add them below) to see their workload here.')}</div>`
    : bad.length ? `<div class="banner bad">⚠ <div><b>${bad.map(l => esc(l.name)).join(', ')}</b> ${T(bad.length > 1 ? 'are booked beyond capacity.' : 'is booked beyond capacity.')}<br><small style="color:var(--mut)">${T('Leveling delays the most flexible, lowest-priority clashing tasks, choosing whatever moves the finish date least. It adds no links and can be undone.')}</small></div><div class="acts">${leveled ? `<button class="btn" data-act="unlevel">${T('Clear leveling')}</button>` : ''}<button class="btn primary" data-act="level">${ic('level')}${T('Level resources')}</button></div></div>`
    : `<div class="banner ok">✓ ${T('Nobody is booked beyond their capacity.')}${leveled ? `<div class="acts"><button class="btn" data-act="unlevel">${T('Clear leveling')}</button></div>` : ''}</div>`;
  const H = (ppl.length + 1) * RRH;
  return `<div class="view" data-v="resources">${banner}
    <div class="gt res" style="--gw:${GW}px;--rh:${RRH}px">
      <div class="corner"><div class="cols" style="--cols:${tmpl}"><div style="padding-left:46px">${T('Name')}</div><div>${T('Type')}</div><div>${T('Rate')}</div><div>${T('Capacity')}</div><div>${T('Per use')}</div><div>${T('Overtime')}</div><div>${T('Hours')}</div><div>${T('Days off')}</div><div>${T('Workload')}</div><div></div></div></div>
      <div class="thead" style="width:${W}px">${timeHeader(days)}</div>
      <div class="gridl" style="height:${H}px"><div class="rows" style="height:${H}px">${rows}</div></div>
      <div class="tbody" style="width:${W}px;height:${H}px;background:repeating-linear-gradient(to bottom,transparent 0 ${RRH - 1}px,var(--line2) ${RRH - 1}px ${RRH}px)">${bg}${cells}</div>
    </div></div>`;
}
