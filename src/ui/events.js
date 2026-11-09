"use strict";
/* rplan — actions, editing, keyboard, mouse, clipboard and start-up. */

/* ---------- structural edits ---------- */
function selectedTop(m) { return m.tasks.map((t, i) => i).filter(i => sel.has(m.tasks[i].id)); }
function shiftLevels(m, dir) {
  if (isFlat()) throw new Error(T('Turn off sorting and grouping to indent or move tasks'));
  let changed = 0, skip = -1;
  for (const i of selectedTop(m)) {
    if (i < skip) continue;
    const t = m.tasks[i], end = subtreeEnd(m.tasks, i);
    if (dir > 0 ? (i === 0 || m.tasks[i - 1].level < t.level) : t.level === 0) continue;
    for (let k = i; k < end; k++) m.tasks[k].level += dir;
    skip = end; changed++;
  }
  if (!changed) throw new Error(dir > 0 ? T("Can't indent — there's no task above it at the same level") : T('Already at the top level'));
}
function fixFamilyLinks(m) {
  const par = parentsOf(m.tasks), idx = new Map(m.tasks.map((t, i) => [t.id, i]));
  const isAnc = (a, b) => { for (let p = par[b]; p >= 0; p = par[p]) if (p === a) return true; return false; };
  let n = 0;
  m.tasks.forEach((t, i) => { const before = t.deps.length; t.deps = t.deps.filter(d => { const j = idx.get(d.id); return j == null || !(isAnc(j, i) || isAnc(i, j)); }); n += before - t.deps.length; });
  return n;
}
function restructure(fn, msg) {
  let dropped = 0;
  return commit(m => { const r = fn(m); if (r === false) return false; dropped = fixFamilyLinks(m); }, j => [typeof msg === 'function' ? msg(j) : msg, dropped ? T('removed {0} link(s) between a task and its own summary', dropped) : ''].filter(Boolean).join(' — '));
}
const indentSel = dir => restructure(m => shiftLevels(m, dir));
function removeTasks(m, ids) {
  const gone = new Set();
  for (let i = 0; i < m.tasks.length; i++) if (ids.has(m.tasks[i].id)) { const end = subtreeEnd(m.tasks, i); for (let k = i; k < end; k++) gone.add(m.tasks[k].id); i = end - 1; }
  m.tasks = m.tasks.filter(t => !gone.has(t.id));
  m.tasks.forEach(t => t.deps = t.deps.filter(d => !gone.has(d.id)));
  return gone.size;
}
function moveBlock(m, id, targetId, after) {
  const i = m.tasks.findIndex(t => t.id === id), end = subtreeEnd(m.tasks, i), block = m.tasks.slice(i, end);
  if (block.some(t => t.id === targetId)) return false;
  m.tasks.splice(i, end - i);
  const j = m.tasks.findIndex(t => t.id === targetId), at = after ? subtreeEnd(m.tasks, j) : j, delta = m.tasks[j].level - block[0].level;
  block.forEach(t => t.level = Math.max(0, t.level + delta));
  m.tasks.splice(at, 0, ...block);
}
function moveUpDown(m, id, dir) {
  if (isFlat()) throw new Error(T('Turn off sorting and grouping to indent or move tasks'));
  const i = m.tasks.findIndex(t => t.id === id), t = m.tasks[i], end = subtreeEnd(m.tasks, i);
  if (dir < 0) { let j = i - 1; while (j >= 0 && m.tasks[j].level > t.level) j--; if (j < 0 || m.tasks[j].level < t.level) return false; return moveBlock(m, id, m.tasks[j].id, false); }
  if (end >= m.tasks.length || m.tasks[end].level !== t.level) return false;
  return moveBlock(m, id, m.tasks[end].id, true);
}
const newTask = (id, name, o = {}) => ({ id, name, level: 0, dur: dmin(), elapsed: false, work: null, deps: [], who: [], pct: 0, constraint: null, deadline: null, fixed: 0, delay: 0, baselines: [], note: '', as: null, af: null, ac: null, ot: 0, splits: [], manual: null, inactive: false, priority: 500, cal: null, sub: null, xdeps: [], ...o });
function insertAt(m) {
  let at = m.tasks.length, lvl = 0;
  if (primary != null) { const i = m.tasks.findIndex(t => t.id === primary); if (i >= 0) { at = subtreeEnd(m.tasks, i); lvl = m.tasks[i].level; } }
  return [at, lvl];
}
function addTask(name) {
  const after = primary != null ? task(primary) : null;
  let newId;
  return commit(m => {
    newId = Math.max(0, ...m.tasks.map(t => t.id)) + 1;
    const [at, lvl] = insertAt(m);
    m.tasks.splice(at, 0, newTask(newId, name, { level: lvl, deps: after && !after.summary && !isFlat() ? [{ id: after.id, kind: 'FS', lag: 0, elapsed: false }] : [] }));
  }, after && !after.summary ? T('Added after #{0}', after.id) : null).then(ok => { if (ok) { sel = new Set([newId]); primary = anchor = newId; render(); setTimeout(() => $('[data-f="new"]')?.focus(), 0); } return ok; });
}
function pasteRows(txt) {
  let lines = txt.replace(/\r/g, '').split('\n').filter(l => l.trim());
  if (lines.length && /^(task( name)?|name|tarea|nombre( de tarea)?)$/i.test(lines[0].split('\t')[0].trim())) lines.shift();
  if (!lines.length) return;
  const rows = lines.map(l => {
    let c = l.split('\t');
    if (c.length > 1 && /^\s*\d+\s*$/.test(c[0]) && !/^\s*\d+\s*$/.test(c[1])) c = c.slice(1);
    const lead = c[0].match(/^\s*/)[0].replace(/\t/g, '  ').length;
    let dur = dmin(), who = [], pct = 0;
    try { if (c[1] && c[1].trim()) dur = parseDur(c[1]); } catch (e) {}
    try { if (c[3]) who = parseWho(c[3]); } catch (e) {}
    if (c[4]) pct = Math.max(0, Math.min(100, parseInt(c[4], 10) || 0));
    return { name: cl(c[0]) || T('Pasted task'), lvl: Math.floor(lead / 2), dur, who, pct };
  });
  const min = Math.min(...rows.map(r => r.lvl));
  commit(m => {
    let [at, base] = insertAt(m), id = Math.max(0, ...m.tasks.map(t => t.id)) + 1;
    rows.forEach((r, k) => m.tasks.splice(at + k, 0, newTask(id++, r.name, { level: base + r.lvl - min, dur: r.dur, who: r.who, pct: r.pct })));
  }, T('Pasted {0} task(s)', rows.length) + (lines.some(l => l.split('\t')[2]?.trim()) ? ' ' + T('(links are not pasted)') : ''));
}
function toggleDay(d) {
  commit(m => {
    const k = m.exceptions.findIndex(e => e.from <= d && d <= e.to);
    if (k >= 0) {
      const e = m.exceptions[k];
      m.exceptions.splice(k, 1);
      if (e.from < d) m.exceptions.push({ ...e, to: d - 1 });
      if (d < e.to) m.exceptions.push({ ...e, from: d + 1 });
    } else if (isOff(d)) m.exceptions.push({ from: d, to: d, name: T('Extra working day'), hours: (m.week.find(h => h.length) || STD).map(x => [...x]) });
    else m.exceptions.push({ from: d, to: d, name: T('Holiday'), hours: [] });
    m.exceptions.sort((a, b) => a.from - b.from);
  }, () => isOff(d) ? T('{0} is now a day off — tasks rescheduled', nice(d)) : T('{0} is now a working day — tasks rescheduled', nice(d)));
}
function createRecurring() {
  const name = cl($('#rec-name').value) || T('Recurring task'), [dur] = parseDurE($('#rec-dur').value || '1h');
  const every = $('#rec-every').value, from = offOf($('#rec-from').value || dateStr(0)), count = Math.max(1, Math.min(200, parseInt($('#rec-count').value, 10) || 1));
  const wds = $$('.rec-wd').filter(x => x.checked).map(x => +x.value), [th, tm] = ($('#rec-time').value || '08:00').split(':').map(Number);
  const who = parseWho($('#rec-who').value || '');
  const dates = [];
  if (every === 'month') { const f = dateOf(from); for (let k = 0; dates.length < count && k < 400; k++) dates.push(Math.round((Date.UTC(f.getUTCFullYear(), f.getUTCMonth() + k, f.getUTCDate()) - START) / 864e5)); }
  else for (let d = from; dates.length < count && d < from + 3650; d++) { if (every === 'day' ? !isOff(d) : wds.includes(wdOf(d))) dates.push(d); }
  if (!dates.length) throw new Error(T('Pick at least one weekday'));
  let sid;
  return commit(m => {
    sid = Math.max(0, ...m.tasks.map(t => t.id)) + 1;
    const [at, lvl] = insertAt(m);
    const items = [newTask(sid, name, { level: lvl })];
    dates.forEach((d, k) => items.push(newTask(sid + 1 + k, `${name} ${k + 1}`, { level: lvl + 1, dur, who: who.map(a => ({ ...a })), constraint: { type: 'snet', at: d * 1440 + th * 60 + (tm || 0) } })));
    m.tasks.splice(at, 0, ...items);
  }, T('Added “{0}” with {1} occurrences', name, dates.length)).then(ok => { if (ok) { $('#dlg').close(); sel = new Set([sid]); primary = anchor = sid; render(); } });
}

/* ---------- selection ---------- */
function selectClick(id, e) {
  const order = G ? G.rows.filter(r => r.kind === 'task').map(r => D.tasks[r.i].id) : D.tasks.map(t => t.id);
  if (e.shiftKey && anchor != null) { const a = order.indexOf(anchor), b = order.indexOf(id); if (a >= 0 && b >= 0) sel = new Set(order.slice(Math.min(a, b), Math.max(a, b) + 1)); primary = id; }
  else if (e.ctrlKey || e.metaKey) { sel.has(id) ? sel.delete(id) : sel.add(id); primary = sel.has(id) ? id : ([...sel].pop() ?? null); anchor = id; }
  else { sel = new Set([id]); primary = anchor = id; }
}
function setCollapsed(id, on) { on ? collapsed.add(id) : collapsed.delete(id); store('collapsed:' + D.path, JSON.stringify([...collapsed])); render(); }
function openDetails(id) {
  sel = new Set([id]); primary = anchor = id; inspOpen = true; store('insp', '1');
  if (view !== 'gantt') setView('gantt'); else render();
  setTimeout(() => { const el = $('[data-insp="name"]'); el?.focus(); el?.select(); scrollToTask(id); }, 0);
}
function switchProject(url, body) {
  return enqueue(async () => {
    const j = await api('POST', url, body);
    if (!j.ok) { if (!j.handled) toast(tr(j.error), 1); return; }
    undo = []; redo = []; sel = new Set(); primary = anchor = null; files = null;
    if ($('#dlg').open) $('#dlg').close();
    dlgKind = null; accept(j); fit(); saved();
    toast(url.endsWith('create') ? T('Created “{0}”', j.name) : T('Opened “{0}”', j.name));
  });
}

/* ---------- actions ---------- */
async function act(a, el) {
  closeMenu();
  const id = el?.dataset.id != null ? +el.dataset.id : null, k = el?.dataset.k != null ? +el.dataset.k : null;
  const onTask = (fn, msg) => commit(m => { const t = mt(m, primary); if (!t) return false; return fn(t, m); }, msg);
  const [verb, arg] = a.includes(':') ? [a.slice(0, a.indexOf(':')), a.slice(a.indexOf(':') + 1)] : [a, null];
  switch (verb) {
    case 'add': if (view !== 'gantt') setView('gantt'); setTimeout(() => { $('#main .view') && ($('#main .view').scrollTop = 1e9); paintWindow(); $('[data-f="new"]')?.focus(); }, 0); break;
    case 'undo': doUndo(); break;
    case 'redo': doRedo(); break;
    case 'zin': zoom(1.25); break;
    case 'zout': zoom(0.8); break;
    case 'fit': fit(); break;
    case 'scale': dw = +el.dataset.v; store('dw', dw); if (view === 'network' || view === 'wbs') setView('gantt'); else render(); break;
    case 'crit': focusCrit = !focusCrit; render(); break;
    case 'details': inspOpen = !inspOpen; store('insp', inspOpen ? '1' : '0'); render(); break;
    case 'help': openDialog('help'); break;
    case 'calendar': openDialog('calendar'); break;
    case 'dlg-close': $('#dlg').close(); dlgKind = null; break;
    case 'lang': LANG = LANG === 'es' ? 'en' : 'es'; store('lang', LANG); setupFormats(); render(); if (!$('#report').hidden) renderReport(); break;
    case 'theme': { const t = arg || (document.documentElement.dataset.theme === 'light' ? 'dark' : 'light'); applyTheme(t); store('theme', t); render(); if (!$('#report').hidden) renderReport(); break; }
    case 'style': applyStyle(arg); store('style', arg); render(); if (!$('#report').hidden) renderReport(); break;
    case 'uiz': setUiZoom(+arg); break;
    case 'uiz-in': uiZoomStep(1); break;
    case 'uiz-out': uiZoomStep(-1); break;
    case 'status-today': commit(m => { m.status = todayOff(); }, T('Status date set to today')); break;
    case 'indent': indentSel(1); break;
    case 'outdent': indentSel(-1); break;
    case 'toggle': setCollapsed(id, !collapsed.has(id)); break;
    case 'del': { const t = task(id); commit(m => { removeTasks(m, new Set([id])); }, t.summary ? T('Deleted “{0}” and its subtasks — Ctrl+Z to undo', t.name) : T('Deleted “{0}” — Ctrl+Z to undo', t.name)); break; }
    case 'del-sel': { const n = sel.size; commit(m => { removeTasks(m, new Set(sel)); }, T('Deleted {0} task(s) — Ctrl+Z to undo', n)); break; }
    case 'link-chain': commit(m => { const ts = m.tasks.filter(t => sel.has(t.id) && !D.tasks.find(x => x.id === t.id).summary); for (let i = 1; i < ts.length; i++) if (!ts[i].deps.some(d => d.id === ts[i - 1].id)) ts[i].deps.push({ id: ts[i - 1].id, kind: 'FS', lag: 0, elapsed: false }); }, T('Linked the selected tasks in order')); break;
    case 'level': action('/api/level'); break;
    case 'lvl-opt': commit(m => { m.leveling[arg] = !m.leveling[arg]; }, T('Leveling options saved — level again to apply them')); break;
    case 'lvl-order': commit(m => { m.leveling.order = arg; }, T('Leveling options saved — level again to apply them')); break;
    case 'update-progress': if (D.status == null) { toast(T('Set a status date first (at the top of the window)'), 1); break; } action('/api/progress'); break;
    case 'tracking': store('tracking', store('tracking') === '1' ? '0' : '1'); if (view !== 'gantt') setView('gantt'); else render(); break;
    case 'people-mode': store('peopleMode', el.dataset.v); render(); break;
    case 'rate-add': commit(m => { const r = ensureRes(m, dlgArg); r.rates.push([Math.max(0, todayOff()), r.rates.length ? r.rates[r.rates.length - 1][1] : r.rate]); r.rates.sort((a, b) => a[0] - b[0]); }); break;
    case 'rate-rm': commit(m => { ensureRes(m, dlgArg).rates.splice(k, 1); }); break;
    case 'cap-add': commit(m => { const r = ensureRes(m, dlgArg); r.caps.push([Math.max(0, todayOff()), 50]); r.caps.sort((a, b) => a[0] - b[0]); }); break;
    case 'cap-rm': commit(m => { ensureRes(m, dlgArg).caps.splice(k, 1); }); break;
    case 'fld-add': commit(m => { let n = 1; while (m.fields.some(f => f.name === T('Field {0}', n))) n++; m.fields.push({ name: T('Field {0}', n), kind: 'text', values: [] }); }); break;
    case 'fld-rm': commit(m => { const name = m.fields[k].name; m.fields.splice(k, 1); m.tasks.forEach(t => { delete t.x[name]; }); }); break;
    case 'unlevel': action('/api/unlevel'); break;
    case 'baseline': action('/api/baseline?n=' + arg, D.tasks.some(t => t.baselines.some(b => b.n === +arg)) ? T('Replace the saved baseline with the current dates?') : null); break;
    case 'baseline-clear': action('/api/baseline/clear?n=' + arg); break;
    case 'go-people': setView('resources'); break;
    case 'import': $('#file').click(); break;
    case 'exp-xml': location.href = '/api/export.xml?token=' + TOKEN; break;
    case 'exp-html': location.href = '/api/export.html?token=' + TOKEN; break;
    case 'exp-plan': location.href = '/api/export.plan?token=' + TOKEN; break;
    case 'print': openPrint(['gantt', 'wbs', 'network', 'usage', 'resources'].includes(view) ? view : 'gantt'); break;
    case 'rep-layout': openPrint('report'); break;
    case 'reports': openReport(); break;
    case 'report': openReport(el.dataset.r); break;
    case 'rep-print': printReport(); break;
    case 'rep-close': closeReport(); break;
    case 'filter': filter = arg; store('filter', filter); if (view !== 'gantt') setView('gantt'); else render(); break;
    case 'filter-to': filter = el.dataset.v; store('filter', filter); setView('gantt'); break;
    case 'sort': sortBy = arg; store('sort', sortBy); render(); break;
    case 'group': groupBy = arg; store('group', groupBy); render(); break;
    case 'clear-filters': filter = 'all'; search = ''; sortBy = 'file'; groupBy = 'none'; ['filter', 'sort', 'group'].forEach(x => store(x, { filter: 'all', sort: 'file', group: 'none' }[x])); render(); break;
    case 'cols-reset': store('cols', 'null'); closeMenu(); render(); break;
    case 'issue': {
      const x = issues()[k]; if (!x) break;
      if (x.task != null) { sel = new Set([x.task]); primary = anchor = x.task; if (filter !== 'all' || search) { filter = 'all'; search = ''; } setView('gantt'); setTimeout(() => scrollToTask(x.task), 0); }
      else if (x.person) setView('resources');
      else if (x.filter) { filter = x.filter; setView('gantt'); }
      break;
    }
    case 'open': openDialog('open'); break;
    case 'open-file': switchProject('/api/open', el.dataset.file); break;
    case 'create-file': { const n = $('#new-file-name')?.value.trim(); if (!n) { toast(T('Type a name for the new project'), 1); $('#new-file-name')?.focus(); } else switchProject('/api/create', n); break; }
    case 'history': openDialog('history'); break;
    case 'hist-pick': {
      histDiff = { n: k, loading: true }; renderDialog();
      const j = await api('GET', '/api/history/diff?n=' + k);
      histDiff = j.ok ? { ...j, n: k } : null; if (!j.ok) toast(tr(j.error), 1); renderDialog(); break;
    }
    case 'hist-restore': enqueue(async () => {
      const prev = D.text, j = await api('POST', '/api/history/restore?n=' + k);
      if (!j.ok) { if (!j.handled) toast(tr(j.error), 1); return; }
      remember(prev); $('#dlg').close(); dlgKind = null; accept(j); render(); saved(); toast(T('Restored the earlier version — Ctrl+Z goes back'));
    }); break;
    case 'recurring': openDialog('recurring'); break;
    case 'rec-create': try { createRecurring(); } catch (e) { toast(e.message, 1); } break;
    case 'new-project': if (!D.tasks.length || confirm(T('Clear “{0}” and start over in this file? (Ctrl+Z brings it back.)', D.name))) { sel = new Set(); primary = null; await action('/api/new'); fit(); } break;
    case 'sample': if (!D.tasks.length || confirm(T('Replace “{0}” with the sample project? (Ctrl+Z brings it back.)', D.name))) { sel = new Set(); primary = null; await action('/api/sample'); fit(); } break;
    case 'dep-add': onTask(t => { const i = tIdx(t.id), rel = relatives(i); const ok = x => !rel.has(x.id) && !t.deps.some(d => d.id === x.id); const cand = D.tasks.slice(0, i).reverse().find(ok) || D.tasks.find(ok); if (!cand) throw new Error(T('There are no other tasks to link to')); t.deps.push({ id: cand.id, kind: 'FS', lag: 0, elapsed: false }); }); break;
    case 'dep-rm': onTask(t => { t.deps.splice(k, 1); }); break;
    case 'xdep-rm': onTask(t => { t.xdeps.splice(k, 1); }); break;
    case 'who-add': onTask(t => { const before = unitsSum(t); const r = D.resources.find(r => !t.who.some(a => a.name === r.name)); t.who.push(r ? (r.kind === 'work' ? { name: r.name, units: 100, amount: 100, form: 'pct' } : r.kind === 'material' ? { name: r.name, units: 100, amount: 1, form: 'qty' } : { name: r.name, units: 100, amount: 0, form: 'money' }) : { name: T('New person'), units: 100, amount: 100, form: 'pct' }); keepDuration(t, before); }); break;
    case 'who-rm': onTask(t => { const before = unitsSum(t); t.who.splice(k, 1); keepDuration(t, before); }); break;
    case 'clear-delay': onTask(t => { t.delay = 0; }); break;
    case 'act-start': onTask(t => { const d = Math.max(sDay(task(t.id)), todayOff()); t.as = d * 1440 + ((dayHours(d)[0] || STD[0])[0]); if (!t.pct) t.pct = 1; }, T('Marked as started')); break;
    case 'act-done': onTask(t => { const cur = task(t.id); t.as = t.as ?? cur.es; t.af = Math.max(t.as, Math.min(cur.ef, (todayOff() + 1) * 1440)); t.pct = 100; }, T('Marked complete ✓')); break;
    case 'act-clear': onTask(t => { t.as = null; t.af = null; t.ac = null; }); break;
    case 'split-add': onTask(t => { const cur = task(t.id), d = Math.max(sDay(cur) + 1, todayOff()); t.splits.push([d, d]); }); break;
    case 'split-rm': onTask(t => { t.splits.splice(k, 1); }); break;
    case 'open-sub': { const t = task(primary); if (t?.sub) switchProject('/api/open', t.sub); break; }
    case 'res-rm': {
      const name = el.dataset.name, n = D.tasks.filter(t => t.who.some(a => a.name === name)).length;
      if (n && !confirm(T('Remove {0}? They\'re assigned to {1} task(s); those assignments will be removed too.', name, n))) return;
      commit(m => { m.resources = m.resources.filter(r => r.name !== name); m.tasks.forEach(t => t.who = t.who.filter(a => a.name !== name)); }, T('Removed {0}', name)); break;
    }
    case 'res-hours': openDialog('reshours', el.dataset.name); break;
    case 'rh-reset': commit(m => { ensureRes(m, dlgArg).week = null; }); break;
    case 'preset': {
      const p = el.dataset.p;
      commit(m => {
        const day = { std: STD, 96: [[540, 780], [840, 1080]], sat: m.week[1].length ? m.week[1] : STD, '4x10': [[420, 720], [750, 1050]], 24: [[0, 1440]] }[p];
        const on = { std: [1, 2, 3, 4, 5], 96: [1, 2, 3, 4, 5], sat: [1, 2, 3, 4, 5, 6], '4x10': [1, 2, 3, 4], 24: [0, 1, 2, 3, 4, 5, 6] }[p];
        m.week = m.week.map((h, i) => on.includes(i) ? day.map(x => [...x]) : []);
        m.dayMinutes = p === '24' ? 1440 : p === '4x10' ? 600 : 480;
      }, T('Working week updated — tasks rescheduled')); break;
    }
    case 'ex-add': {
      const work = el.dataset.p === 'work';
      let d = Math.max(0, todayOff()); for (let n = 0; n < 30 && (work ? !isOff(d) : isOff(d)); n++) d++;
      commit(m => { m.exceptions.push({ from: d, to: d, name: work ? T('Extra working day') : T('Holiday'), hours: work ? (m.week.find(h => h.length) || STD).map(x => [...x]) : [] }); }); break;
    }
    case 'ex-rm': commit(m => { m.exceptions.splice(k, 1); }); break;
    case 'rec-add': commit(m => { m.recurring.push(`12-25 ${T('Holiday')}`); }); break;
    case 'rec-rm': commit(m => { m.recurring.splice(k, 1); }); break;
    case 'add-country': {
      const code = $('#country')?.value, c = (holidaysList || []).find(x => x.code === code); if (!c) break;
      let added = 0;
      commit(m => { for (const r of c.rules) if (!m.recurring.includes(r)) { m.recurring.push(r); added++; } if (!added) { toast(T('Those holidays are already in the calendar')); return false; } }, () => T('Added {0} public holidays for {1} — they repeat every year', added, c.name));
      break;
    }
    case 'ncal-add': commit(m => { let n = 1; while (m.calendars.some(c => c.name === T('Calendar {0}', n))) n++; m.calendars.push({ name: T('Calendar {0}', n), week: m.week.map(h => h.map(x => [...x])) }); }); break;
    case 'ncal-rm': commit(m => { const name = m.calendars[k].name; m.calendars.splice(k, 1); m.tasks.forEach(t => { if (t.cal === name) t.cal = null; }); }); break;
    case 'usage-mode': store('usageMode', el.dataset.v); render(); break;
    case 'usage-period': store('usagePeriod', el.dataset.v); render(); break;
  }
}

/* ---------- clicks ---------- */
document.addEventListener('click', e => {
  if (suppressClick) { suppressClick = false; return; }
  const mb = e.target.closest('[data-menu]');
  if (mb) { if ($('#menu')?.dataset.for === mb.dataset.menu) closeMenu(); else openMenu(mb.dataset.menu, mb); return; }
  if (!e.target.closest('#menu')) closeMenu();
  else if (e.target.closest('label')) return; // column checkboxes keep the menu open
  const b = e.target.closest('[data-act]'); if (b && !b.disabled) return act(b.dataset.act, b);
  const tab = e.target.closest('#tabs button'); if (tab) return setView(tab.dataset.v);
  const day = e.target.closest('[data-day]'); if (day) return toggleDay(+day.dataset.day);
  if (e.target === $('#dlg')) { $('#dlg').close(); dlgKind = null; return; }
  const node = e.target.closest('[data-node]'), bar = e.target.closest('[data-bar]'), row = e.target.closest('[data-row]');
  const id = node ? +node.dataset.node : bar ? +bar.dataset.bar : row ? +row.dataset.row : null;
  if (id == null) return;
  if (e.detail === 2 && (bar || node)) return openDetails(id);
  const inInput = e.target.matches('input,select,textarea');
  if (!inInput && !e.shiftKey && !e.ctrlKey && !e.metaKey && sel.size === 1 && sel.has(id)) return;
  if (inInput && sel.has(id) && !e.shiftKey && !e.ctrlKey && !e.metaKey) { if (primary !== id) { primary = anchor = id; inspector(); } return; }
  selectClick(id, e);
  if (inInput && $('#insp').hidden === !inspW()) { $$('.row[data-row]').forEach(r => r.classList.toggle('sel', sel.has(+r.dataset.row))); renderToolbar(); inspector(); }
  else render();
});

/* ---------- edits ---------- */
let searchT;
document.addEventListener('input', e => {
  if (e.target.id === 'q') { clearTimeout(searchT); searchT = setTimeout(() => { search = e.target.value; render(); }, 140); }
});
document.addEventListener('change', e => {
  const el = e.target, v = (el.value || '').trim();
  if (el.id === 'pname') return commit(m => { m.name = v || m.name; }, T('Renamed project'));
  if (el.id === 'pstart') return v && commit(m => { m.start = v; }, T('Project now starts {0}', v));
  if (el.id === 'pstatus') return commit(m => { m.status = v ? offOf(v) : null; }, v ? T('Status date: {0}', v) : T('Status date removed'));
  if (el.id === 'file') return importFile(el);
  if (el.dataset.c) { const cur = new Set(visibleCols()); el.checked ? cur.add(el.dataset.c) : cur.delete(el.dataset.c); store('cols', JSON.stringify([...Object.keys(COLUMNS), ...D.fields.map(f => 'x:' + f.name)].filter(c => cur.has(c)))); return render(); }
  if (el.dataset.rep) { report[el.dataset.rep] = el.value; if (el.dataset.rep !== 'kind') store(el.dataset.rep, el.value); return renderReport(); }
  if (el.dataset.f && el.dataset.f !== 'new') {
    const id = +el.dataset.id, f = el.dataset.f;
    return commit(m => {
      const t = mt(m, id); if (!t) return false;
      if (f === 'name') { if (!v) return false; t.name = v; }
      if (f === 'dur') { if (t.fixedUnits && t.work != null) t.work = Math.round(parseDurE(v)[0] * (unitsSum(t) || 1)); else [t.dur, t.elapsed] = parseDurE(v); }
      if (f === 'work') t.work = parseEffort(v);
      if (f === 'pct') { const n = parseInt(v, 10); if (!(n >= 0)) throw new Error(T('% done must be 0–100')); t.pct = Math.min(100, n); if (n < 100) t.af = null; }
      if (f === 'priority') { const n = parseInt(v, 10); if (!(n >= 0 && n <= 1000)) throw new Error(T('Priority must be 0–1000')); t.priority = n; }
      if (f === 'who') t.who = parseWho(v);
      if (f === 'deps') { t.deps = parseDeps(v); if (t.deps.some(d => d.id === id)) throw new Error(T("A task can't depend on itself")); }
    });
  }
  if (el.dataset.insp) return inspChange(el, v);
  if (el.dataset.cal) return calChange(el, v);
  if (el.dataset.res) return resChange(el, v);
});
function inspChange(el, v) {
  const f = el.dataset.insp, k = +el.dataset.k;
  if (f.startsWith('bulk-')) {
    const ids = new Set(sel), sums = new Set(D.tasks.filter(t => t.summary).map(t => t.id));
    return commit(m => {
      const who = f === 'bulk-who' ? parseWho(v) : null, n = parseInt(v, 10);
      m.tasks.filter(t => ids.has(t.id) && (f === 'bulk-priority' || !sums.has(t.id))).forEach(t => {
        if (who) t.who = who.map(a => ({ ...a })); else if (f === 'bulk-pct') t.pct = Math.max(0, Math.min(100, n || 0)); else t.priority = Math.max(0, Math.min(1000, n || 0));
      });
    }, T('Updated {0} task(s)', ids.size));
  }
  commit(m => {
    const t = mt(m, primary); if (!t) return false;
    const cur = task(primary), before = unitsSum(t);
    switch (f) {
      case 'name': if (!v) return false; t.name = v; break;
      case 'note': t.note = v; break;
      case 'dur': if (t.fixedUnits && t.work != null) t.work = Math.round(parseDurE(v)[0] * (before || 1)); else [t.dur, t.elapsed] = parseDurE(v); break;
      case 'ttype':
        if (v === 'duration') { if (t.work != null) t.dur = cur.edur; t.work = null; t.fixedUnits = false; }
        else { if (t.work == null) t.work = Math.round(cur.edur * (before || 1)); t.fixedUnits = v === 'units'; }
        break;
      case 'who-delay': t.who[k].delay = v ? Math.max(0, parseDur(v)) : 0; break;
      case 'who-work': t.who[k].work = parseEffort(v); break;
      case 'who-contour': t.who[k].contour = v; break;
      case 'xf': {
        const name = el.dataset.name, fd = D.fields.find(x => x.name === name), val = fd?.kind === 'flag' ? (el.checked ? 'yes' : '') : cl(v);
        if (fd?.kind === 'number' && val && !isFinite(+val.replace(',', '.'))) throw new Error(T('That isn\'t a number'));
        if (val) t.x[name] = val; else delete t.x[name];
        break;
      }
      case 'work': t.work = parseEffort(v); break;
      case 'elapsed': { const on = el.checked; if (on !== t.elapsed) { t.dur = on ? Math.round(t.dur / dmin() * 1440) : Math.round(t.dur / 1440 * dmin()); t.elapsed = on; } break; }
      case 'pct': { const n = parseInt(v, 10); if (!(n >= 0)) throw new Error(T('% done must be 0–100')); t.pct = Math.min(100, n); if (n < 100) t.af = null; break; }
      case 'as': t.as = v ? fromLocal(v) : null; break;
      case 'af': t.af = v ? fromLocal(v) : null; if (t.af != null) { t.pct = 100; if (t.as == null) t.as = cur.es; } break;
      case 'ac': t.ac = v === '' ? null : +v.replace(',', '.'); if (t.ac != null && !isFinite(t.ac)) throw new Error(T('Cost must be a number')); break;
      case 'dep-id': t.deps[k].id = +v; break;
      case 'dep-kind': t.deps[k].kind = v; break;
      case 'dep-lag': [t.deps[k].lag, t.deps[k].elapsed, t.deps[k].pct] = parseLagE(v); break;
      case 'xdep-add': {
        if (!v) return false;
        const mm = v.match(/^(.+?\.plan)\s*#\s*(\d+)\s*(FS|SS|FF|SF)?\s*(?:([+-])\s*(.+))?$/i);
        if (!mm) throw new Error(T('Use the form Other.plan#5 or Other.plan#5SS+1d'));
        const lag = mm[4] ? (mm[4] === '-' ? -1 : 1) * parseDur(mm[5]) : 0;
        t.xdeps.push({ file: mm[1].trim(), id: +mm[2], kind: (mm[3] || 'FS').toUpperCase(), lag }); break;
      }
      case 'who-name': { const n = rn(v); if (!n) t.who.splice(k, 1); else if (t.who.some((a, j) => j !== k && a.name === n)) throw new Error(T('{0} is already assigned', n)); else { t.who[k].name = n; const r = resOf(n); if (r?.kind === 'material') Object.assign(t.who[k], { form: 'qty', amount: 1, units: 100 }); else if (r?.kind === 'cost') Object.assign(t.who[k], { form: 'money', amount: 0, units: 100 }); } break; }
      case 'who-amt': {
        const r = resOf(t.who[k].name), kind = r?.kind || 'work', n = +v.replace('%', '').replace(',', '.').replace('$', '');
        if (!isFinite(n) || n < 0) throw new Error(T('That isn\'t a number'));
        if (kind === 'work') { if (!(n >= 1 && n <= 1000)) throw new Error(T('Units must be 1–1000%')); Object.assign(t.who[k], { units: Math.round(n), amount: Math.round(n), form: 'pct' }); }
        else Object.assign(t.who[k], { amount: n, form: kind === 'material' ? 'qty' : 'money' });
        break;
      }
      case 'ot': t.ot = parseEffort(v) || 0; break;
      case 'c-type': {
        if (v === 'asap') t.constraint = null;
        else if (v === 'alap') t.constraint = { type: 'alap', at: null };
        else t.constraint = { type: v, at: t.constraint?.at ?? (v.startsWith('f') || v === 'mfo' ? fDay(cur) : sDay(cur)) * 1440 };
        break;
      }
      case 'c-date': if (!v || !t.constraint) return false; t.constraint.at = offOf(v) * 1440; break;
      case 'deadline': t.deadline = v ? offOf(v) : null; break;
      case 'priority': { const n = parseInt(v, 10); if (!(n >= 0 && n <= 1000)) throw new Error(T('Priority must be 0–1000')); t.priority = n; break; }
      case 'cal': t.cal = v || null; break;
      case 'manual': t.manual = el.checked ? cur.es : null; break;
      case 'manual-at': if (!v) return false; t.manual = fromLocal(v); break;
      case 'inactive': t.inactive = el.checked; break;
      case 'split-from': { const a = offOf(v); t.splits[k][0] = a; if (t.splits[k][1] < a) t.splits[k][1] = a; break; }
      case 'split-to': { const b = offOf(v); if (b < t.splits[k][0]) throw new Error(T("The end date can't be before the start")); t.splits[k][1] = b; break; }
      case 'sub': t.sub = v || null; break;
      case 'fixed': { const n = v === '' ? 0 : +v.replace(/[^\d.\-]/g, ''); if (!isFinite(n)) throw new Error(T('Cost must be a number')); t.fixed = n; break; }
    }
    if (f === 'who-amt' || f === 'who-name') keepDuration(t, before);
  });
}
// Hours per day of the people on a task (100% = one full-time person).
function unitsSum(t) { return t.who.filter(a => (resOf(a.name)?.kind || 'work') === 'work').reduce((s, a) => s + a.units, 0) / 100; }
// Fixed-units tasks keep their duration when people change: the work follows.
function keepDuration(t, before) { if (t.fixedUnits && t.work != null && before > 0) t.work = Math.round(t.work / before * (unitsSum(t) || before)); }
function calChange(el, v) {
  const f = el.dataset.cal, d = +el.dataset.d, k = +el.dataset.k;
  commit(m => {
    const weekOf = p => p === 'wk' ? m.week : p === 'ncal' ? m.calendars[k].week : (ensureRes(m, dlgArg).week ||= m.week.map(h => h.map(x => [...x])));
    const [pre, what] = f.includes('-') ? [f.slice(0, f.indexOf('-')), f.slice(f.indexOf('-') + 1)] : [f, ''];
    if (['wk', 'ncal', 'rh'].includes(pre) && (what === 'on' || what === 'h')) {
      const w = weekOf(pre);
      w[d] = what === 'on' ? (el.checked ? (w.find(h => h.length) || STD).map(x => [...x]) : []) : parseHours(v);
      if (w.every(h => !h.length)) throw new Error(T('At least one weekday needs working hours'));
      return;
    }
    switch (f) {
      case 'dh': { const h = +v.replace(',', '.'); if (!(h >= 0.25 && h <= 24)) throw new Error(T('Hours per day must be between 0.25 and 24')); m.dayMinutes = Math.round(h * 60); break; }
      case 'cur': m.currency = cl(v) || '$'; break;
      case 'status': m.status = v ? offOf(v) : null; break;
      case 'pool': m.pool = v || null; break;
      case 'ex-type': m.exceptions[k].hours = v === 'off' ? [] : (m.week.find(h => h.length) || STD).map(x => [...x]); break;
      case 'ex-from': { const a = offOf(v); m.exceptions[k].from = a; if (m.exceptions[k].to < a) m.exceptions[k].to = a; break; }
      case 'ex-to': { const b = offOf(v); if (b < m.exceptions[k].from) throw new Error(T("The end date can't be before the start")); m.exceptions[k].to = b; break; }
      case 'ex-name': m.exceptions[k].name = cl(v); break;
      case 'ex-hours': { const h = parseHours(v); if (!h.length) throw new Error(T('A working day needs hours (or switch it to "Day off")')); m.exceptions[k].hours = h; break; }
      case 'rec-rule': case 'rec-name': { const row = el.closest('.recrow'); m.recurring[k] = `${row.querySelector('[data-cal="rec-rule"]').value.trim()} ${cl(row.querySelector('[data-cal="rec-name"]').value)}`.trim(); break; }
      case 'fld-name': { const old = m.fields[k].name, n = fieldName(v); if (!n || n === old) return false; if (m.fields.some((x, j) => j !== k && x.name === n)) throw new Error(T('There\'s already a field called {0}', n)); m.fields[k].name = n; m.tasks.forEach(t => { if (old in t.x) { t.x[n] = t.x[old]; delete t.x[old]; } }); break; }
      case 'fld-kind': m.fields[k].kind = v; if (v === 'flag' || v === 'date') m.fields[k].values = []; break;
      case 'fld-values': m.fields[k].values = v.split(',').map(x => cl(x)).filter(Boolean); break;
      case 'ncal-name': { const old = m.calendars[k].name, n = cl(v); if (!n) return false; if (m.calendars.some((c, j) => j !== k && c.name === n)) throw new Error(T('There\'s already a calendar called {0}', n)); m.calendars[k].name = n; m.tasks.forEach(t => { if (t.cal === old) t.cal = n; }); break; }
    }
  });
}
function resChange(el, v) {
  const f = el.dataset.res, name = el.dataset.name;
  commit(m => {
    if (f === 'new') { const n = rn(v); if (!n) return false; if (D.resources.some(r => r.name === n)) throw new Error(T('{0} is already on the team', n)); ensureRes(m, n); return; }
    const r = ensureRes(m, name), num2 = s => { const n = s === '' ? 0 : +s.replace(/[^\d.,\-]/g, '').replace(',', '.'); if (!isFinite(n)) throw new Error(T('That isn\'t a number')); return n; };
    if (f === 'name') { const n = rn(v); if (!n || n === name) return false; if (D.resources.some(x => x.name === n)) throw new Error(T('{0} already exists', n)); r.name = n; m.tasks.forEach(t => t.who.forEach(a => { if (a.name === name) a.name = n; })); }
    if (f === 'kind') {
      r.kind = v;
      m.tasks.forEach(t => t.who.forEach(a => { if (a.name !== name) return; if (v === 'work') Object.assign(a, { form: 'pct', units: 100, amount: 100 }); else if (v === 'material') Object.assign(a, { form: 'qty', units: 100, amount: a.form === 'qty' ? a.amount : 1 }); else Object.assign(a, { form: 'money', units: 100, amount: a.form === 'money' ? a.amount : 0 }); }));
    }
    if (f === 'rate') r.rate = num2(v);
    if (f === 'perUse') r.perUse = num2(v);
    if (f === 'overtime') r.overtime = num2(v);
    if (f === 'capacity') { if (r.kind === 'work') { const n = parseInt(v, 10); if (!(n >= 1 && n <= 1000)) throw new Error(T('Capacity must be 1–1000%')); r.capacity = n; } else r.unit = rn(v); }
    if (f === 'off') r.off = parseRanges(v);
    if (['group', 'initials', 'email', 'note'].includes(f)) r[f] = cl(v);
    if (f === 'accrue') r.accrue = v;
    const k = +el.dataset.k;
    if (f === 'rate-at' || f === 'cap-at') { if (!v) return false; (f === 'rate-at' ? r.rates : r.caps)[k][0] = offOf(v); (f === 'rate-at' ? r.rates : r.caps).sort((a, b) => a[0] - b[0]); }
    if (f === 'rate-v') r.rates[k][1] = num2(v);
    if (f === 'cap-v') { const n = parseInt(v, 10); if (!(n >= 1 && n <= 1000)) throw new Error(T('Capacity must be 1–1000%')); r.caps[k][1] = n; }
  }, f === 'new' ? T('Added {0}', rn(v)) : null);
}
async function importFile(input) {
  const f = input.files[0]; input.value = ''; if (!f) return;
  const xml = await f.text();
  if (D.tasks.length && !confirm(T('Replace “{0}” with “{1}”?\nYou can undo this with Ctrl+Z.', D.name, f.name))) return;
  return enqueue(async () => {
    const prev = D.text, j = await api('POST', '/api/import', xml);
    if (!j.ok) { if (!j.handled) toast(T('Import failed: {0}', tr(j.error)), 1); return; }
    remember(prev); sel = new Set(); primary = null;
    accept(j); fit(); saved(); toast(T('Imported {0} tasks and {1} people from {2}', j.tasks.length, j.resources.length, f.name));
  });
}

/* ---------- keyboard ---------- */
document.addEventListener('keydown', e => {
  const el = e.target, inInput = el.matches && el.matches('input,select,textarea'), k = e.key.toLowerCase(), mod = e.ctrlKey || e.metaKey;
  if (e.key === 'Escape' && $('#menu')) { closeMenu(); return; }
  if (e.key === 'Escape' && !$('#report').hidden) { closeReport(); return; }
  if (mod && !e.altKey && (k === '+' || k === '=' || k === '-' || k === '0' || e.code === 'NumpadAdd' || e.code === 'NumpadSubtract' || e.code === 'Numpad0')) {
    e.preventDefault(); uiZoomStep(k === '0' || e.code === 'Numpad0' ? 0 : k === '-' || e.code === 'NumpadSubtract' ? -1 : 1); return;
  }
  if (printOpen()) { if (e.key === 'Escape') closePrint(); if (mod && k === 'p') { e.preventDefault(); psPrint(); } return; }
  if (mod && k === 'p' && !e.shiftKey) { e.preventDefault(); act('print'); return; }
  if (mod && k === 'o') { e.preventDefault(); act('open'); return; }
  if (mod && k === 'f') { e.preventDefault(); if (view !== 'gantt') setView('gantt'); $('#q')?.focus(); $('#q')?.select(); return; }
  if ($('#dlg').open) { if (e.key === 'Enter' && el.id === 'new-file-name') act('create-file'); return; }
  if (!$('#report').hidden) return;
  if (inInput) {
    if (el.id === 'q') { if (e.key === 'Escape') { search = ''; el.value = ''; render(); el.blur(); } if (e.key === 'Enter') { clearTimeout(searchT); search = el.value; render(); } return; }
    if (e.key === 'Enter' && el.dataset.f === 'new') { const name = el.value.trim(); if (name) addTask(name); return; }
    if (e.key === 'Enter' && el.tagName !== 'TEXTAREA' && el.tagName !== 'SELECT') { el.blur(); return; }
    if (e.key === 'Escape') { render(); el.blur(); return; }
    if (mod && k === 'z' && !el.value) { e.preventDefault(); doUndo(); }
    return;
  }
  if (mod && k === 'z') { e.preventDefault(); e.shiftKey ? doRedo() : doUndo(); return; }
  if (mod && k === 'y') { e.preventDefault(); doRedo(); return; }
  if (mod && k === 'a' && view === 'gantt') { e.preventDefault(); sel = new Set(G.rows.filter(r => r.kind === 'task').map(r => D.tasks[r.i].id)); render(); return; }
  if (mod) return;
  if (k === 'tab' && sel.size) { e.preventDefault(); indentSel(e.shiftKey ? -1 : 1); return; }
  if (e.altKey && (k === 'arrowright' || k === 'arrowleft') && sel.size) { e.preventDefault(); indentSel(k === 'arrowright' ? 1 : -1); return; }
  const views = { 1: 'gantt', 2: 'network', 3: 'wbs', 4: 'usage', 5: 'resources' };
  if (views[k]) setView(views[k]);
  if (k === 'n') { e.preventDefault(); act('add'); }
  if (k === '+' || k === '=') zoom(1.25);
  if (k === '-') zoom(0.8);
  if (k === 'f') fit();
  if (k === 'c') { focusCrit = !focusCrit; render(); }
  if (k === '?') openDialog('help');
  if (k === 'enter' && primary != null) { inspOpen = true; store('insp', '1'); render(); setTimeout(() => $('[data-insp="name"]')?.focus(), 0); }
  if ((k === 'delete' || k === 'backspace') && sel.size) { const n = sel.size; commit(m => { removeTasks(m, new Set(sel)); }, T('Deleted {0} task(s) — Ctrl+Z to undo', n)); }
  if (k === 'escape') { sel = new Set(); primary = null; render(); }
  if ((k === 'arrowup' || k === 'arrowdown') && D.tasks.length && view === 'gantt') {
    e.preventDefault();
    const dir = k === 'arrowup' ? -1 : 1;
    if (e.altKey && primary != null) { restructure(m => moveUpDown(m, primary, dir)); return; }
    const vis = G.rows.filter(r => r.kind === 'task').map(r => D.tasks[r.i].id), i = vis.indexOf(primary);
    const j = i < 0 ? 0 : Math.max(0, Math.min(vis.length - 1, i + dir));
    if (e.shiftKey && primary != null) { sel.add(vis[j]); primary = vis[j]; } else { sel = new Set([vis[j]]); primary = anchor = vis[j]; }
    render(); scrollToTask(primary);
  }
});

/* ---------- clipboard ---------- */
document.addEventListener('copy', e => {
  if (e.target.matches?.('input,textarea') && e.target.selectionStart !== e.target.selectionEnd) return;
  if (!sel.size || !D) return;
  const rows = D.tasks.filter(t => sel.has(t.id)), min = Math.min(...rows.map(t => t.level));
  e.clipboardData.setData('text/plain', rows.map(t => ['  '.repeat(t.level - min) + t.name, t.summary ? '' : fmtDur(t.edur, t.elapsed), depsStr(t), whoStr(t), t.pct].join('\t')).join('\n'));
  e.preventDefault(); toast(T('Copied {0} task(s) — paste here or into a spreadsheet', rows.length));
});
document.addEventListener('paste', e => {
  const txt = e.clipboardData.getData('text/plain'), inNew = e.target.dataset?.f === 'new';
  if (e.target.matches?.('input,textarea') && !(inNew && /[\t\n]/.test(txt.trim()))) return;
  if (view !== 'gantt' || !txt.trim()) return;
  e.preventDefault(); pasteRows(txt);
});

/* ---------- dragging rows ---------- */
const clearDrop = () => $$('.drop-before,.drop-after').forEach(r => r.classList.remove('drop-before', 'drop-after'));
document.addEventListener('dragstart', e => { const h = e.target.closest?.('[data-drag]'); if (!h) return; dragRow = +h.dataset.drag; e.dataTransfer.setData('text/plain', ''); e.dataTransfer.effectAllowed = 'move'; });
document.addEventListener('dragover', e => {
  if (dragRow == null) return; const row = e.target.closest('[data-row]'); clearDrop();
  if (!row) return; e.preventDefault(); const r = row.getBoundingClientRect(); row.classList.add(e.clientY > r.top + r.height / 2 ? 'drop-after' : 'drop-before');
});
document.addEventListener('drop', e => {
  if (dragRow == null) return; const row = e.target.closest('[data-row]'); e.preventDefault();
  const id = dragRow, after = row?.classList.contains('drop-after'); dragRow = null; clearDrop();
  if (!row || +row.dataset.row === id) return;
  restructure(m => moveBlock(m, id, +row.dataset.row, after), T('Moved #{0}', id));
});
document.addEventListener('dragend', () => { dragRow = null; clearDrop(); });

/* ---------- dragging bars: move, resize, link ---------- */
document.addEventListener('mousedown', e => {
  if (e.button !== 0) return;
  const r = e.target.closest('[data-resize]'), l = e.target.closest('[data-link]'), mv = !r && !l && e.target.closest('[data-move]');
  if (!r && !l && !mv) return;
  hideTip();
  if (r) { e.preventDefault(); const bar = r.parentElement; drag = { kind: 'resize', id: +r.dataset.resize, bar, x0: e.clientX, w0: bar.offsetWidth }; document.body.style.cursor = 'ew-resize'; }
  else if (l) {
    e.preventDefault();
    const svg = $('#links'), box = svg.getBoundingClientRect(), src = l.parentElement.getBoundingClientRect();
    const p = document.createElementNS('http://www.w3.org/2000/svg', 'path'); p.setAttribute('class', 'tmp'); svg.appendChild(p);
    drag = { kind: 'link', id: +l.dataset.link, path: p, box, sx: (src.right - box.left) / Z(), sy: (src.top + src.height / 2 - box.top) / Z() }; document.body.style.cursor = 'crosshair';
  } else { drag = { kind: 'move', id: +mv.dataset.move, bar: mv, x0: e.clientX, l0: mv.offsetLeft, moved: false }; }
});
document.addEventListener('mousemove', e => {
  if (!drag) return moveTip(e);
  const t = task(drag.id);
  if (drag.kind === 'resize') {
    const w = Math.max(dw * 0.5, drag.w0 + (e.clientX - drag.x0) / Z()); drag.bar.style.width = w + 'px';
    const endDay = Math.floor((drag.bar.offsetLeft + w - 1) / dw - PAD);
    drag.dur = t.elapsed ? Math.max(1, endDay - sDay(t) + 1) * 1440 : Math.max(1, workDays(sDay(t), endDay)) * dmin();
    showTip(e, `<h5>${esc(t.name)}</h5><b>${fmtDur(drag.dur, t.elapsed)}</b> → ${T('ends {0}', nice(Math.max(endDay, sDay(t))))}`);
  } else if (drag.kind === 'link') {
    const x = (e.clientX - drag.box.left) / Z(), y = (e.clientY - drag.box.top) / Z();
    drag.path.setAttribute('d', `M${drag.sx},${drag.sy} C${drag.sx + 60},${drag.sy} ${x - 60},${y} ${x},${y}`);
  } else {
    const dx = (e.clientX - drag.x0) / Z(); if (!drag.moved && Math.abs(dx) * Z() < 4) return;
    drag.moved = true; drag.bar.classList.add('dragging'); drag.bar.style.left = drag.l0 + dx + 'px';
    drag.day = t.edur === 0 ? Math.floor((drag.l0 + dx + 8) / dw - PAD) : Math.round((drag.l0 + dx) / dw - PAD);
    showTip(e, `<h5>${esc(t.name)}</h5>${T('Start no earlier than')} <b>${nice(drag.day)}</b>`);
  }
});
document.addEventListener('mouseup', e => {
  if (!drag) return;
  const d = drag; drag = null; document.body.style.cursor = ''; hideTip();
  const t = task(d.id);
  if (d.kind === 'resize') {
    noClick();
    if (d.dur && d.dur !== t.edur) commit(m => { mt(m, d.id).dur = d.dur; }, T('“{0}” is now {1}', t.name, fmtDur(d.dur, t.elapsed))); else render();
  } else if (d.kind === 'link') {
    d.path.remove(); noClick();
    const tgt = document.elementFromPoint(e.clientX, e.clientY)?.closest('[data-bar],[data-row],[data-node]');
    const tid = tgt ? +(tgt.dataset.bar || tgt.dataset.row || tgt.dataset.node) : null;
    if (tid && tid !== d.id) commit(m => {
      const x = mt(m, tid); if (x.deps.some(y => y.id === d.id)) { toast(T('Already linked')); return false; }
      if (relatives(tIdx(tid)).has(d.id)) throw new Error(T("A task can't link to its own summary or subtask"));
      x.deps.push({ id: d.id, kind: 'FS', lag: 0, elapsed: false });
    }, T('Linked #{0} → #{1}', d.id, tid));
  } else if (d.moved) {
    noClick();
    const day = d.day;
    commit(m => { mt(m, d.id).constraint = { type: 'snet', at: day * 1440 }; }, j => {
      const nt = j.tasks.find(x => x.id === d.id);
      return nt && Math.floor(nt.es / 1440) > day ? T('#{0} can\'t start before {1} (its predecessors or a day off) — constraint saved anyway', d.id, nice(Math.floor(nt.es / 1440))) : T('#{0} now starts no earlier than {1} (📌 remove it in Details)', d.id, nice(day));
    });
  }
});

/* ---------- tooltip ---------- */
function moveTip(e) {
  const b = e.target.closest && e.target.closest('[data-tip]');
  if (!b || e.target.closest('[data-link],[data-resize]')) return hideTip();
  const t = task(+b.dataset.tip); if (!t) return hideTip();
  showTip(e, `<h5>#${t.id} ${esc(t.name)} ${t.done ? `<span class="chip g">${T('done')}</span>` : t.crit ? `<span class="chip r">${T('critical')}</span>` : `<span class="chip">${T('{0} slack', fmtDays(t.slack))}</span>`}${t.late ? `<span class="chip r">${T('late')}</span>` : ''}</h5>
    <div class="g"><span>${T('Start')}</span><b>${esc(when(t.es))}</b><span>${T('Finish')}</span><b>${esc(t.ef > t.es ? when(t.ef, true) : when(t.es))}</b>
    <span>${T('Duration')}</span><b>${t.summary ? fmtDays(t.span) + ' (' + T('summary') + ')' : t.edur ? fmtDur(t.edur, t.elapsed) : T('Milestone')}</b>${t.workT ? `<span>${T('Work')}</span><b>${fmtHours(t.workT)}</b>` : ''}
    <span>${T('Who')}</span><b>${esc(whoStr(t) || '—')}</b><span>${T('Done')}</span><b>${Math.round(t.summary ? t.pctR : t.pct)}%</b><span>${T('After')}</span><b>${esc(depsStr(t) || '—')}</b>
    ${t.as != null ? `<span>${T('Actual start')}</span><b>${esc(when(t.as))}</b>` : ''}${t.af != null ? `<span>${T('Actual finish')}</span><b>${esc(when(t.af, true))}</b>` : ''}
    <span>${T('Latest start')}</span><b>${esc(when(t.ls))}</b>${t.cost ? `<span>${T('Cost')}</span><b>${money(t.cost)}</b>` : ''}
    ${t.constraint ? `<span>${T('Constraint')}</span><b>${esc(consLabel(t.constraint))}</b>` : ''}${t.deadline != null ? `<span>${T('Deadline')}</span><b>${niceY(t.deadline)}</b>` : ''}
    ${t.baselines.find(b => b.n === 0) ? `<span>${T('Baseline')}</span><b>${esc(when(t.baselines.find(b => b.n === 0).s))} → ${esc(when(t.baselines.find(b => b.n === 0).f, true))}</b>` : ''}
    ${t.subInfo ? `<span>${T('Sub-project')}</span><b>${esc(t.subInfo.name)}</b>` : ''}</div>`);
}
document.addEventListener('mouseleave', hideTip);

/* ---------- start-up ---------- */
applyTheme(store('theme') || (matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'));
applyStyle(store('style'));
applyUiZoom();
// A late 'close' event must not reset a dialog that was opened again in the meantime.
$('#dlg').addEventListener('close', () => { if (!$('#dlg').open) dlgKind = null; });
document.addEventListener('wheel', e => {
  if (!e.ctrlKey) return;
  e.preventDefault();
  const timeline = !e.shiftKey && (view === 'gantt' || view === 'usage') && e.target.closest?.('.view') && !e.target.closest('.gridl,.corner');
  if (timeline) zoom(e.deltaY < 0 ? 1.1 : 0.9);
  else { clearTimeout(wheelZ); wheelZ = setTimeout(() => uiZoomStep(e.deltaY < 0 ? 1 : -1), 40); }
}, { passive: false });
let wheelZ;
let rt; addEventListener('resize', () => { clearTimeout(rt); rt = setTimeout(render, 150); });
addEventListener('beforeprint', () => {
  document.documentElement.style.zoom = ''; document.documentElement.style.setProperty('--zf', 1);
  if ($('#report').hidden && !printOpen() && view !== 'gantt') { view = 'gantt'; render(); }
});
addEventListener('afterprint', applyUiZoom);
// Tell rplan the window is still open (it quits a while after the last window closes).
setInterval(() => { fetch('/api/ping', { headers: { 'X-Rplan-Token': TOKEN } }).catch(() => {}); }, 15000);
// Live reload when the .plan file changes elsewhere (text editor, CLI, git pull…).
setInterval(async () => {
  const a = document.activeElement, typing = a?.matches('input,textarea') && a.value !== a.defaultValue;
  if (!D || drag || typing || $('#dlg').open) return;
  try {
    const r = await (await fetch('/api/rev', { headers: { 'X-Rplan-Token': TOKEN } })).json();
    if (r.rev && r.rev !== D.rev) { await chain; const old = D.text; await load(); if (D.text !== old) { undo.push(old); toast(T('Reloaded — the file changed on disk')); } }
  } catch (e) {}
}, 1500);
load();
