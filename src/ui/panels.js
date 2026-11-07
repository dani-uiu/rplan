"use strict";
/* rplan — details panel and dialogs (calendar & project settings, projects, history, recurring tasks, personal hours). */

const dtLocal = m => m == null ? '' : dtStr(m);
const fromLocal = v => { const [d, t = '00:00'] = v.split('T'); const [h, mi] = t.split(':').map(Number); return offOf(d) * 1440 + h * 60 + (mi || 0); };

function inspector() {
  const el = $('#insp'); el.hidden = !inspW(); inspTask = null; if (el.hidden) return;
  const head = (title) => `<div class="ih">${title}<button class="x" data-act="details" title="${T('Close')}">×</button></div>`;
  if (sel.size > 1) {
    el.innerHTML = head(`<b>${T('{0} tasks selected', sel.size)}</b>`) + `
      <div class="isec"><div class="btnrow"><button class="btn sm" data-act="outdent">${ic('out')}${T('Outdent')}</button><button class="btn sm" data-act="indent">${ic('ind')}${T('Indent')}</button><button class="btn sm" data-act="link-chain">${T('Link in order')}</button><button class="btn sm danger" data-act="del-sel">${ic('trash')}${T('Delete')}</button></div></div>
      <div class="isec"><h5>${T('Assign to all selected')}</h5><input class="wide" data-insp="bulk-who" list="dl-people" placeholder="${T('e.g. Ana, Bo[50%] — Enter to apply')}"></div>
      <div class="isec"><h5>${T('Set % done for all')}</h5><input class="wide" data-insp="bulk-pct" placeholder="0–100"></div>
      <div class="isec"><h5>${T('Priority for all')}</h5><input class="wide" data-insp="bulk-priority" placeholder="0–1000"></div>
      <div class="isec"><span class="hint">${T('Tip: Ctrl+C copies the selected rows (paste into a spreadsheet or back here). Shift+click selects a range.')}</span></div>`;
    return;
  }
  const t = primary != null ? task(primary) : null;
  if (!t) { el.hidden = true; return; }
  inspTask = t.id;
  const i = tIdx(t.id), rel = relatives(i), others = D.tasks.filter(x => !rel.has(x.id)), leaf = !t.summary;
  const chips = [
    t.summary ? `<span class="chip n">${T('Summary')}</span>` : t.edur === 0 ? `<span class="chip y">${T('Milestone')}</span>` : `<span class="chip n">${typeLabel(t)}</span>`,
    t.done ? `<span class="chip g">${T('Completed')}</span>` : t.crit ? `<span class="chip r">${T('Critical')}</span>` : `<span class="chip">${T('{0} slack', fmtDays(t.slack))}</span>`,
    t.late ? `<span class="chip r">${T('Past deadline')}</span>` : '', t.conflict ? `<span class="chip y">${T('Constraint conflict')}</span>` : '',
    t.delay ? `<span class="chip y">${T('Leveled')} +${fmtDur(t.delay)}</span>` : '', behindOf(t, nowRel()) ? `<span class="chip y">${T('Behind')}</span>` : '',
    t.inactive ? `<span class="chip n">${T('Inactive')}</span>` : '',
  ].join('');
  const K = KIND();
  const deps = t.deps.map((d, k) => `<div class="deprow">
      <select data-insp="dep-id" data-k="${k}">${(others.some(o => o.id === d.id) ? others : [task(d.id) || { id: d.id, name: '?' }, ...others]).map(o => `<option value="${o.id}" ${o.id === d.id ? 'selected' : ''}>#${o.id} ${esc(o.name)}</option>`).join('')}</select>
      <select data-insp="dep-kind" data-k="${k}" title="${esc(K[d.kind])}">${Object.keys(K).map(kd => `<option value="${kd}" ${kd === d.kind ? 'selected' : ''} title="${esc(K[kd])}">${kd}</option>`).join('')}</select>
      <input data-insp="dep-lag" data-k="${k}" value="${lagStr(d)}" placeholder="${T('lag')}" title="${esc(T('Lag (+2d), lead (-4h), calendar time (+12eh) or a share of the predecessor (+50%)'))}">
      <button class="rm" data-act="dep-rm" data-k="${k}" title="${T('Remove link')}">×</button></div>`).join('');
  const xdeps = t.xdeps.map((x, k) => `<div class="deprow" style="grid-template-columns:1fr 26px"><span class="mut" title="${esc(t.ext[k]?.label || '')}">🔗 ${esc(x.file)} #${x.id}${x.kind !== 'FS' ? ' ' + x.kind : ''}${x.lag ? ' ' + (x.lag > 0 ? '+' : '-') + fmtDur(Math.abs(x.lag)) : ''}${t.ext[k] ? ' · ' + esc(when(t.ext[k].ef, true)) : ' · ⚠'}</span><button class="rm" data-act="xdep-rm" data-k="${k}">×</button></div>`).join('');
  const who = t.who.map((a, k) => {
    const r = resOf(a.name), kind = r?.kind || 'work';
    const val = kind === 'material' ? num(a.amount) : kind === 'cost' ? num(a.amount) : a.units + '%';
    const unit = kind === 'material' ? (r.unit || T('units')) : kind === 'cost' ? D.currency : '';
    const more = kind === 'work' ? `<div class="whomore">
      <label title="${esc(T('Working time after the task starts before they start'))}">${T('Starts after')}<input data-insp="who-delay" data-k="${k}" value="${a.delay ? fmtDur(a.delay) : ''}" placeholder="0"></label>
      <label title="${esc(T('Their own hours on this task (their part lasts hours ÷ units)'))}">${T('Own work')}<input data-insp="who-work" data-k="${k}" value="${a.work != null ? fmtHours(a.work) : ''}" placeholder="${T('as the task')}"></label>
      <label title="${esc(T('How their hours are spread over their part'))}">${T('Contour')}<select data-insp="who-contour" data-k="${k}">${CONTOURS.map(c => `<option value="${c}" ${(a.contour || 'flat') === c ? 'selected' : ''}>${CONTOUR_LABEL()[c]}</option>`).join('')}</select></label></div>` : '';
    return `<div class="whorow"><input data-insp="who-name" data-k="${k}" value="${esc(a.name)}" list="dl-people"><input data-insp="who-amt" data-k="${k}" value="${esc(val)}" title="${esc(kind === 'work' ? T('Share of their time') : kind === 'material' ? T('Quantity ({0})', unit) : T('Amount ({0})', unit))}"><button class="rm" data-act="who-rm" data-k="${k}" title="${T('Unassign')}">×</button></div>${more}`;
  }).join('');
  const ctype = t.constraint ? t.constraint.type : 'asap';
  const cons = [['asap', T('As soon as possible')], ['alap', T('As late as possible')], ['snet', T('Start no earlier than')], ['snlt', T('Start no later than')], ['fnet', T('Finish no earlier than')], ['fnlt', T('Finish no later than')], ['mso', T('Must start on')], ['mfo', T('Must finish on')]];
  const splits = t.splits.map(([a, b], k) => `<div class="splitrow"><input type="date" data-insp="split-from" data-k="${k}" value="${dateStr(a)}"><span class="mut">→</span><input type="date" data-insp="split-to" data-k="${k}" value="${dateStr(b)}"><button class="rm" data-act="split-rm" data-k="${k}">×</button></div>`).join('');
  const ev = D.status != null && t.bac ? (() => {
    const sv = t.ev - t.pv, cv = t.ev - t.acv, spi = t.pv ? t.ev / t.pv : null, cpi = t.acv ? t.ev / t.acv : null;
    return `<div class="isec"><h5>${T('Earned value')} <span class="mut" style="text-transform:none;letter-spacing:0">${T('as of {0}', nice(D.status))}</span></h5><div class="g3">
      <div class="ro2"><span title="${esc(T('Budget at completion'))}">BAC</span><b>${money(t.bac)}</b></div><div class="ro2"><span title="${esc(T('Planned value: what should be done by now'))}">PV</span><b>${money(t.pv)}</b></div><div class="ro2"><span title="${esc(T('Earned value: budget of the work done'))}">EV</span><b>${money(t.ev)}</b></div>
      <div class="ro2"><span title="${esc(T('Actual cost so far'))}">AC</span><b>${money(t.acv)}</b></div><div class="ro2"><span>SPI</span><b class="${spi != null && spi < 0.95 ? 'r' : 'g'}">${spi == null ? '—' : spi.toFixed(2)}</b></div><div class="ro2"><span>CPI</span><b class="${cpi != null && cpi < 0.95 ? 'r' : 'g'}">${cpi == null ? '—' : cpi.toFixed(2)}</b></div>
      <div class="ro2"><span title="${esc(T('Schedule variance = EV − PV'))}">SV</span><b class="${sv < 0 ? 'r' : ''}">${money(sv)}</b></div><div class="ro2"><span title="${esc(T('Cost variance = EV − AC'))}">CV</span><b class="${cv < 0 ? 'r' : ''}">${money(cv)}</b></div></div></div>`;
  })() : '';
  const bases = t.baselines.map(b => `<div class="ro2" style="margin-bottom:6px"><span>${b.n ? T('Baseline {0}', b.n) : T('Baseline')}</span><b>${esc(when(b.s))} → ${esc(when(b.f, true))} · ${money(b.cost)}${b.n === 0 && t.slip ? ` · <span style="color:${t.slip > 0 ? 'var(--cr1)' : 'var(--good)'}">${t.slip > 0 ? '+' : ''}${fmtDays(t.slip)}</span>` : ''}</b></div>`).join('');
  const subOpts = (files?.files || []).filter(f => f.file !== files.current).map(f => f.file);
  if (t.sub && !subOpts.includes(t.sub)) subOpts.unshift(t.sub);
  el.innerHTML = head(`<span class="dot ${t.summary ? 's' : t.edur === 0 ? 'm' : t.done ? 'd' : t.crit ? 'c' : ''}"></span><small>#${t.id} · WBS ${t.wbs}</small>`) + `
    <input class="iname" data-insp="name" value="${esc(t.name)}" aria-label="${T('Task name')}">
    <div class="chips">${chips}</div>
    <div class="isec"><div class="g2">
      <label class="fl">${T('Duration')}<input data-insp="dur" value="${t.summary ? fmtDays(t.span) : fmtDur(t.edur, t.elapsed)}" ${t.summary || t.sub || (t.work != null && !t.fixedUnits) ? 'disabled' : ''} title="3d, 4h, 30m, 2w, 3ed — 0 = ${T('milestone')}"></label>
      <label class="fl">${T('Work (effort)')}<input data-insp="work" value="${t.work != null ? fmtHours(t.work) : ''}" placeholder="${t.workT ? fmtHours(t.workT) : T('fixed duration')}" ${leaf && !t.sub ? '' : 'disabled'} title="${esc(T('Set total effort (e.g. 40h) to make the task effort-driven: more people → shorter. Empty = fixed duration.'))}"></label>
      <label class="fl">${T('% done')}<input data-insp="pct" value="${Math.round(t.summary ? t.pctR : t.pct)}%" ${t.summary || t.sub ? 'disabled' : ''}></label>
      ${leaf && !t.sub ? `<label class="fl">${T('Task type')}<select data-insp="ttype" title="${esc(T('What stays fixed when you change the duration, the work or the people'))}">${[['duration', T('Fixed duration')], ['units', T('Fixed units')], ['work', T('Fixed work (effort-driven)')]].map(([v, l]) => `<option value="${v}" ${(t.work == null ? 'duration' : t.fixedUnits ? 'units' : 'work') === v ? 'selected' : ''}>${l}</option>`).join('')}</select></label>` : '<div></div>'}
      <label class="fl chk"><input type="checkbox" data-insp="elapsed" ${t.elapsed ? 'checked' : ''} ${leaf ? '' : 'disabled'}> ${T('Calendar time (nights & weekends count)')}</label>
      <div class="ro2"><span>${T('Start')}</span><b>${esc(when(t.es))}</b></div><div class="ro2"><span>${T('Finish')}</span><b class="${t.late ? 'r' : ''}">${esc(t.ef > t.es ? when(t.ef, true) : when(t.es))}</b></div>
      <div class="ro2"><span>${T('Latest start')}</span><b>${esc(when(t.ls))}</b></div><div class="ro2"><span>${T('Slack / free slack')}</span><b class="${t.slack <= 0 && !t.done ? 'r' : ''}">${t.done ? '—' : fmtDays(t.slack) + ' / ' + fmtDays(t.free)}</b></div>
    </div></div>
    ${leaf && !t.sub ? `<div class="isec"><h5>${T('Actual progress')}</h5><div class="g2">
      <label class="fl">${T('Actual start')}<input type="datetime-local" data-insp="as" value="${dtLocal(t.as)}"></label>
      <label class="fl">${T('Actual finish')}<input type="datetime-local" data-insp="af" value="${dtLocal(t.af)}"></label>
      <label class="fl">${T('Actual cost')} (${esc(D.currency)})<input data-insp="ac" value="${t.ac ?? ''}" placeholder="${t.started ? money(t.cost * t.pct / 100) : '—'}"></label>
      <div class="ro2"><span>${T('Remaining')}</span><b>${t.done ? '0' : fmtDur(Math.round(t.edur * (100 - t.pct) / 100), t.elapsed)}</b></div></div>
      <div class="btnrow"><button class="btn sm" data-act="act-start">${T('Started today')}</button><button class="btn sm" data-act="act-done">✓ ${T('Mark complete')}</button>${t.as != null || t.af != null || t.ac != null ? `<button class="btn sm" data-act="act-clear">${T('Clear actuals')}</button>` : ''}</div>
      ${D.status == null ? `<span class="hint">${T('Set a status date at the top to measure progress and earned value as of that day.')}</span>` : ''}</div>` : ''}
    <div class="isec"><h5>${T('Predecessors')} <button class="btn sm" data-act="dep-add">+ ${T('Add')}</button></h5>${deps || `<span class="hint">${T('None — this task can start right away.')}</span>`}
      <span class="hint">${T('FS: start after it finishes · SS: start together · FF: finish together · SF: finish once it starts. Lag delays, lead (−) overlaps.')}</span>
      ${xdeps}<div class="deprow" style="grid-template-columns:1fr"><input data-insp="xdep-add" placeholder="${esc(T('Link to another project: Other.plan#5FS+1d'))}" list="dl-files"></div></div>
    ${leaf ? `<div class="isec"><h5>${T('People & resources')} <button class="btn sm" data-act="who-add">+ ${T('Assign')}</button></h5>${who || `<span class="hint">${T('Nobody assigned.')}</span>`}
      <div class="g2" style="margin-top:6px"><label class="fl">${T('Overtime per person')}<input data-insp="ot" value="${t.ot ? fmtHours(t.ot) : ''}" placeholder="0h"></label><div class="ro2"><span>${T('Work')}</span><b>${fmtHours(t.workT)}</b></div></div></div>` : ''}
    <div class="isec"><h5>${T('Scheduling')}</h5><div class="g2">
      <label class="fl">${T('Constraint')}<select data-insp="c-type">${cons.map(([v, l]) => `<option value="${v}" ${ctype === v ? 'selected' : ''}>${l}</option>`).join('')}</select></label>
      <label class="fl">${T('Date')}<input type="date" data-insp="c-date" value="${t.constraint && t.constraint.at != null ? dateStr(Math.floor(t.constraint.at / 1440)) : ''}" ${t.constraint && t.constraint.type !== 'alap' ? '' : 'disabled'}></label>
      <label class="fl">${T('Deadline')}<input type="date" data-insp="deadline" value="${t.deadline != null ? dateStr(t.deadline) : ''}"></label>
      <label class="fl">${T('Priority')}<input data-insp="priority" value="${t.priority}" title="${esc(T('Leveling priority 0–1000 (higher keeps its dates)'))}"></label>
      <label class="fl">${T('Calendar')}<select data-insp="cal" ${leaf ? '' : 'disabled'}><option value="">${T('Project calendar')}</option>${D.calendars.map(c => `<option ${t.cal === c.name ? 'selected' : ''}>${esc(c.name)}</option>`).join('')}</select></label>
      <div class="ro2"><span>${T('Leveling delay')}</span><b>${t.delay ? fmtDur(t.delay) + ` <button class="btn sm" data-act="clear-delay">${T('Clear')}</button>` : '—'}</b></div>
      <label class="fl chk"><input type="checkbox" data-insp="manual" ${t.manual != null ? 'checked' : ''} ${leaf ? '' : 'disabled'}> ${T('Manually scheduled')}</label>
      ${t.manual != null ? `<label class="fl">${T('Manual start')}<input type="datetime-local" data-insp="manual-at" value="${dtLocal(t.manual)}"></label>` : '<div></div>'}
      <label class="fl chk"><input type="checkbox" data-insp="inactive" ${t.inactive ? 'checked' : ''}> ${T('Inactive (keep it, but ignore it)')}</label>
    </div>${t.conflict ? `<div class="warnbox">${T('This date constraint can\'t be met with the current links.')}</div>` : ''}${t.late ? `<div class="warnbox">${T('Finishes {0}, after the deadline ({1}).', esc(when(t.ef, true)), nice(t.deadline))}</div>` : ''}</div>
    ${D.fields.length ? `<div class="isec"><h5>${T('Custom fields')}</h5><div class="g2">${D.fields.map(f => {
      const v = t.x[f.name] || '', a = `data-insp="xf" data-name="${esc(f.name)}"`;
      const input = f.kind === 'flag' ? `<label class="fl chk"><input type="checkbox" ${a} ${v === 'yes' ? 'checked' : ''}> ${esc(f.name)}</label>`
        : f.kind === 'date' ? `<label class="fl">${esc(f.name)}<input type="date" ${a} value="${esc(v)}"></label>`
        : f.values.length ? `<label class="fl">${esc(f.name)}<select ${a}><option value=""></option>${f.values.map(x => `<option ${x === v ? 'selected' : ''} value="${esc(x)}">${esc('\u00a0\u00a0'.repeat((x.match(/\./g) || []).length) + x)}</option>`).join('')}</select></label>`
        : `<label class="fl">${esc(f.name)}<input ${a} value="${esc(v)}" ${f.kind === 'number' ? 'inputmode="decimal"' : ''}></label>`;
      return input;
    }).join('')}</div></div>` : ''}
    ${leaf ? `<div class="isec"><h5>${T('Pauses')} <button class="btn sm" data-act="split-add">+ ${T('Pause')}</button></h5>${splits || `<span class="hint">${T('Split the task around days it should not be worked on.')}</span>`}${t.lsplits.length ? `<span class="hint">⏸ ${T('Paused by leveling: {0}', t.lsplits.map(([a, b]) => a === b ? nice(a) : nice(a) + ' – ' + nice(b)).join(', '))}</span>` : ''}</div>` : ''}
    ${leaf ? `<div class="isec"><h5>${T('Sub-project')}</h5><label class="fl">${T('This task stands for another project file')}<select data-insp="sub"><option value="">${T('— none —')}</option>${subOpts.map(f => `<option ${t.sub === f ? 'selected' : ''}>${esc(f)}</option>`).join('')}</select></label>
      ${t.subInfo ? `<span class="hint">📁 ${esc(t.subInfo.name)} · ${T('{0} task(s)', t.subInfo.tasks)} · ${fmtDays(t.subInfo.span)} · ${money(t.subInfo.cost)} · ${Math.round(t.subInfo.pct)}%</span><div class="btnrow"><button class="btn sm" data-act="open-sub">${T('Open it')}</button></div>` : t.sub ? `<div class="warnbox">${T('Couldn\'t read that file.')}</div>` : ''}</div>` : ''}
    <div class="isec"><h5>${T('Cost')}</h5><div class="g2"><label class="fl">${T('Fixed cost')} (${esc(D.currency)})<input data-insp="fixed" value="${t.fixed || ''}" placeholder="0"></label><div class="ro2"><span>${T('Total (with resources)')}</span><b>${money(t.cost)}</b></div></div></div>
    ${ev}
    ${bases ? `<div class="isec"><h5>${T('Baselines')}</h5>${bases}</div>` : ''}
    <div class="isec"><h5>${T('Notes')}</h5><textarea data-insp="note" placeholder="${T('Anything worth remembering…')}">${esc(t.note)}</textarea></div>`;
  if (!files) api('GET', '/api/files').then(j => { if (j.ok) { files = j; if (inspW() && !document.activeElement?.closest('#insp')) inspector(); } });
}

/* ---------- dialogs ---------- */
function openDialog(kind, arg) {
  dlgKind = kind; dlgArg = arg;
  if (kind === 'open') { files = null; api('GET', '/api/files').then(j => { files = j; if (dlgKind === 'open') renderDialog(); }); }
  if (kind === 'history') { histList = null; histDiff = null; api('GET', '/api/history').then(j => { histList = j.ok ? j.entries.reverse() : []; if (dlgKind === 'history') renderDialog(); }); }
  if (kind === 'calendar' && !holidaysList) api('GET', '/api/holidays').then(j => { holidaysList = j.countries || []; if (dlgKind === 'calendar') renderDialog(); });
  if (kind === 'calendar' && !files) api('GET', '/api/files').then(j => { if (j.ok) { files = j; if (dlgKind === 'calendar') renderDialog(); } });
  renderDialog(); if (!$('#dlg').open) $('#dlg').showModal();
  if (kind === 'open') setTimeout(() => $('#new-file-name')?.focus(), 0);
}
let dlgArg = null, histList = null, histDiff = null;
function ago(ms) { const s = (Date.now() - ms) / 1000; return s < 60 ? T('just now') : s < 3600 ? T('{0} min ago', Math.round(s / 60)) : s < 86400 ? T('{0} h ago', Math.round(s / 3600)) : new Date(ms).toLocaleDateString(LANG); }
const dlgHead = (title, extra = '') => `<div class="dh"><h3>${title}</h3>${extra}<button class="x" data-act="dlg-close">×</button></div>`;
function weekEditor(week, prefix, k = '') {
  return `<div class="wk">${ORDER.map(i => `<div class="wkrow"><label class="sw"><input type="checkbox" data-cal="${prefix}-on" data-d="${i}" ${k !== '' ? `data-k="${k}"` : ''} ${week[i].length ? 'checked' : ''}>${wdName(i)}</label><input type="text" data-cal="${prefix}-h" data-d="${i}" ${k !== '' ? `data-k="${k}"` : ''} value="${hoursStr(week[i])}" ${week[i].length ? '' : 'disabled'}><span class="mut">${+(hoursLen(week[i]) / 60).toFixed(2)}h</span></div>`).join('')}</div>`;
}
function renderDialog() {
  const d = $('#dlg');
  if (dlgKind === 'help') {
    d.innerHTML = dlgHead(T('Keyboard & mouse')) + `<div class="db"><div class="keys" style="margin-top:12px">
      ${[['N', T('New task (after the selected one)')], ['Enter', T('Open the details panel')], ['Tab / Shift+Tab', T('Indent / outdent (make or unmake subtasks)')], ['Alt+↑ / Alt+↓', T('Move the task up / down')], ['↑ / ↓', T('Select previous / next task')], ['Shift+click, Ctrl+click', T('Select several tasks')], ['Ctrl+A', T('Select all tasks')], ['Delete', T('Delete selected tasks')], ['Ctrl+C / Ctrl+V', T('Copy rows / paste rows (also from a spreadsheet)')], ['Ctrl+Z / Ctrl+Y', T('Undo / redo')], ['Ctrl+F', T('Search tasks')], ['Ctrl+O', T('Open or create a project')], ['1 2 3 4 5', T('Gantt / Network / WBS / Usage / People')], ['+ / − / F', T('Zoom in / out / fit')], ['C', T('Focus the critical path')], ['Ctrl+P', T('Print or save as PDF')], [T('Drag a bar'), T('Set "start no earlier than"')], [T('Drag a bar’s right edge'), T('Change the duration')], [T('Drag the ○ to another bar'), T('Link them (finish → start)')], [T('Drag ⋮⋮ in the # column'), T('Reorder rows')], [T('Click a date in the timeline'), T('Toggle holiday / working day')]].map(([k, v]) => `<div><kbd>${k}</kbd></div><div>${v}</div>`).join('')}
    </div></div>`;
    return;
  }
  if (dlgKind === 'open') {
    const list = !files ? `<div class="mut">${T('Loading…')}</div>` : files.files.map(f => `<button class="fileit ${f.file === files.current ? 'cur' : ''}" data-act="open-file" data-file="${esc(f.file)}" ${f.file === files.current ? 'disabled' : ''}>
        <img src="/favicon.svg" alt=""><span><b>${esc(f.name || f.file.replace(/\.plan$/i, ''))}</b><small>${esc(f.file)}</small></span><small>${f.file === files.current ? T('open now') : ago(f.modified)}</small></button>`).join('') || `<div class="mut">${T('No project files yet.')}</div>`;
    d.innerHTML = dlgHead(T('Projects'), `<span class="mut">${esc(files?.dir || '')}</span>`) + `<div class="db">
      <h4>${T('New project')}</h4><div class="newrow"><input id="new-file-name" placeholder="${T('Name, e.g. Office move')}" maxlength="100"><button class="btn primary" data-act="create-file">${ic('plus')}${T('Create')}</button></div>
      <h4>${T('Open')}</h4><div class="files">${list}</div></div>`;
    return;
  }
  if (dlgKind === 'history') {
    const list = !histList ? `<div class="mut">${T('Loading…')}</div>` : histList.length ? histList.map(e => `<button class="fileit ${histDiff?.n === e.n ? 'cur' : ''}" data-act="hist-pick" data-k="${e.n}">${ic('hist')}<span><b>${esc(new Date(e.ms).toLocaleString(LANG))}</b><small>${esc(T('Before: {0}', tr(e.note) || T('Edit')))} · ${T('{0} task(s)', e.tasks)}</small></span><small>${ago(e.ms)}</small></button>`).join('') : `<div class="mut">${T('No earlier versions yet — they appear here as you work.')}</div>`;
    const diff = histDiff ? (histDiff.loading ? `<div class="mut">${T('Comparing…')}</div>` : `<div class="diffbox">
      <b>${T('Restoring this version will:')}</b>
      ${histDiff.removed.length ? `<div>↩ ${T('bring back')}: ${histDiff.removed.map(esc).join(', ')}</div>` : ''}
      ${histDiff.added.length ? `<div>✕ ${T('remove')}: ${histDiff.added.map(esc).join(', ')}</div>` : ''}
      ${histDiff.changed.length ? `<div>✎ ${T('change')}: ${histDiff.changed.map(c => `${esc(c.task)} <span class="mut">(${c.fields.map(f => T(f)).join(', ')})</span>`).join('; ')}</div>` : ''}
      ${histDiff.settings ? `<div>⚙ ${T('change project settings (calendar, people…)')}</div>` : ''}
      ${!histDiff.removed.length && !histDiff.added.length && !histDiff.changed.length && !histDiff.settings ? `<div class="mut">${T('Same as now.')}</div>` : ''}
      <div class="btnrow"><button class="btn primary" data-act="hist-restore" data-k="${histDiff.n}">${ic('hist')}${T('Restore this version')}</button></div></div>`) : `<div class="mut" style="margin-top:8px">${T('Pick a version to see what restoring it would change.')}</div>`;
    d.innerHTML = dlgHead(T('Version history'), `<span class="mut">${T('Saved automatically in .rplan-history next to the file')}</span>`) + `<div class="db" style="display:grid;grid-template-columns:1fr 1fr;gap:16px"><div><h4>${T('Earlier versions')}</h4><div class="files">${list}</div></div><div><h4>${T('Preview')}</h4>${diff}</div></div>`;
    return;
  }
  if (dlgKind === 'recurring') {
    d.innerHTML = dlgHead(T('Add a recurring task')) + `<div class="db"><div class="g2" style="margin-top:12px">
      <label class="fl">${T('Name')}<input id="rec-name" class="in" value="${T('Weekly meeting')}"></label>
      <label class="fl">${T('Each occurrence lasts')}<input id="rec-dur" class="in" value="1h"></label>
      <label class="fl">${T('Repeats')}<select id="rec-every" class="in"><option value="week">${T('Weekly')}</option><option value="day">${T('Every working day')}</option><option value="month">${T('Monthly (same day)')}</option></select></label>
      <label class="fl">${T('On')}<span>${ORDER.map(i => `<label class="sw" style="display:inline-flex;margin-right:6px;font-weight:400"><input type="checkbox" class="rec-wd" value="${i}" ${i === 1 ? 'checked' : ''}>${wdName(i)}</label>`).join('')}</span></label>
      <label class="fl">${T('First date')}<input id="rec-from" class="in" type="date" value="${dateStr(Math.max(0, todayOff()))}"></label>
      <label class="fl">${T('Occurrences')}<input id="rec-count" class="in" value="10"></label>
      <label class="fl">${T('Who')}<input id="rec-who" class="in" list="dl-people" placeholder="Ana, Bo"></label>
      <label class="fl">${T('Time of day')}<input id="rec-time" class="in" type="time" value="${hm((D.week.find(h => h.length) || STD)[0][0])}"></label>
    </div><span class="hint">${T('Creates a summary task with one subtask per occurrence, each pinned with “start no earlier than”.')}</span>
    <div class="btnrow"><button class="btn primary" data-act="rec-create">${ic('repeat')}${T('Create')}</button></div></div>`;
    return;
  }
  if (dlgKind === 'reshours') {
    const r = resOf(dlgArg); if (!r) { d.close(); return; }
    const w = r.week || D.week, work = r.kind === 'work', nm = esc(r.name);
    const tbl = (rows, kind, unit) => rows.map(([day, v], k) => `<div class="exrow" style="grid-template-columns:auto 150px 120px 26px"><span class="mut">${T('From')}</span><input type="date" data-res="${kind}-at" data-k="${k}" data-name="${nm}" value="${dateStr(day)}"><input data-res="${kind}-v" data-k="${k}" data-name="${nm}" value="${kind === 'cap' ? v + '%' : num(v)}" title="${esc(unit)}"><button class="rm" data-act="${kind}-rm" data-k="${k}">×</button></div>`).join('');
    d.innerHTML = dlgHead(T('{0}: details', nm)) + `<div class="db">
      <h4>${T('About')}</h4><div class="inl">
        <label>${T('Group')} <input data-res="group" data-name="${nm}" value="${esc(r.group)}" placeholder="${T('e.g. Engineering/Backend')}" style="width:190px!important"></label>
        <label>${T('Initials')} <input data-res="initials" data-name="${nm}" value="${esc(r.initials)}" style="width:60px!important"></label>
        <label>${T('Email')} <input data-res="email" data-name="${nm}" value="${esc(r.email)}" type="email" style="width:210px!important"></label></div>
      <label class="fl" style="margin-top:6px">${T('Notes')}<input class="wide" data-res="note" data-name="${nm}" value="${esc(r.note)}"></label>
      <span class="hint">${T('Groups make the resource breakdown (RBS) chart in People; use / for sub-groups.')}</span>
      <h4>${T('Cost')}</h4><div class="inl"><label>${T('Cost counts')} <select data-res="accrue" data-name="${nm}">${[['prorated', T('as the work is done')], ['start', T('at the start')], ['end', T('at the end')]].map(([v, l]) => `<option value="${v}" ${r.accrue === v ? 'selected' : ''}>${l}</option>`).join('')}</select></label></div>
      <h4>${T('Rate changes')} <button class="btn sm" data-act="rate-add">+ ${T('New rate from a date')}</button></h4>
      ${tbl(r.rates, 'rate', work ? T('Cost per hour ({0})', D.currency) : T('Cost per unit')) || `<div class="mut">${T('{0} per hour until a change is added here.', money(r.rate))}</div>`}
      ${work ? `<h4>${T('Capacity changes')} <button class="btn sm" data-act="cap-add">+ ${T('New capacity from a date')}</button></h4>
      ${tbl(r.caps, 'cap', '%') || `<div class="mut">${T('{0}% all the time.', r.capacity)}</div>`}
      <h4>${T('Personal working week')}</h4>${weekEditor(w, 'rh')}
      <div class="presets"><button class="btn sm" data-act="rh-reset">${T('Use the project hours')}</button><span class="mut">${r.week ? T('Custom hours') : T('Following the project calendar')}</span></div>
      <span class="hint">${T('Tasks with several people only run when all of them are working.')}</span>` : ''}</div>`;
    return;
  }
  if (dlgKind !== 'calendar') return;
  const ex = D.exceptions.map((e, k) => [e, k]).sort((a, b) => a[0].from - b[0].from).map(([e, k]) => `<div class="exrow">
      <select data-cal="ex-type" data-k="${k}"><option value="off" ${e.hours.length ? '' : 'selected'}>🏖 ${T('Day off')}</option><option value="work" ${e.hours.length ? 'selected' : ''}>🛠 ${T('Working day')}</option></select>
      <input type="date" data-cal="ex-from" data-k="${k}" value="${dateStr(e.from)}"><span class="mut">→</span><input type="date" data-cal="ex-to" data-k="${k}" value="${dateStr(e.to)}">
      <input data-cal="ex-name" data-k="${k}" value="${esc(e.name)}" placeholder="${T('Name (e.g. Christmas)')}">
      <input data-cal="ex-hours" data-k="${k}" value="${hoursStr(e.hours)}" ${e.hours.length ? '' : 'disabled'} title="${T('Working hours that day')}">
      <button class="rm" data-act="ex-rm" data-k="${k}">×</button></div>`).join('');
  const rec = D.recurring.map((r, k) => { const m = r.match(/^(\S+(?:\s+(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\s+\w{3})?)(?:\s+(observed|monday|substitute))?\s*(.*)$/); return `<div class="recrow"><input data-cal="rec-rule" data-k="${k}" value="${esc(m ? m[1] + (m[2] ? ' ' + m[2] : '') : r)}" title="${esc(T('12-25, 3rd Mon Jan, last Mon May, easter-2 — add observed / monday / substitute to move it off weekends'))}"><input data-cal="rec-name" data-k="${k}" value="${esc(m ? m[3] : '')}" placeholder="${T('Name')}"><span class="mut">${nextOccurrence(k)}</span><button class="rm" data-act="rec-rm" data-k="${k}">×</button></div>`; }).join('');
  const cals = D.calendars.map((c, k) => `<div class="calrow"><div class="head"><input class="in" data-cal="ncal-name" data-k="${k}" value="${esc(c.name)}" style="max-width:260px;background:var(--panel2);border:1px solid var(--line);border-radius:7px;padding:5px 8px"><span class="mut">${T('{0} task(s) use it', D.tasks.filter(t => t.cal === c.name).length)}</span><button class="rm" style="margin-left:auto" data-act="ncal-rm" data-k="${k}">×</button></div>${weekEditor(c.week, 'ncal', k)}</div>`).join('');
  const base = Math.max(0, Math.min(todayOff(), Math.floor(D.finish / 1440)));
  let months = '';
  const first = dateOf(base);
  for (let k = 0; k < 4; k++) {
    const y = first.getUTCFullYear(), mo = first.getUTCMonth() + k;
    const m0 = Math.round((Date.UTC(y, mo, 1) - START) / 864e5), m1 = Math.round((Date.UTC(y, mo + 1, 1) - START) / 864e5);
    const lead = (wdOf(m0) + 6) % 7;
    let g = ORDER.map(i => `<span>${wdName(i).slice(0, 2)}</span>`).join('') + '<span></span>'.repeat(lead);
    for (let dd = m0; dd < m1; dd++) { const e = EXC.get(dd); const cls = e ? (e.hours.length ? 'xw' : 'hol') : isOff(dd) ? 'off' : ''; g += `<button class="${cls} ${dd === 0 ? 'st' : ''}" data-day="${dd}" title="${esc(niceY(dd) + (e && e.name ? ' — ' + e.name : '') + '\n' + (isOff(dd) ? T('Day off') : hoursStr(dayHours(dd))) + (dd === 0 ? '\n' + T('Project start') : ''))}">${dateOf(dd).getUTCDate()}</button>`; }
    months += `<div class="mon"><h6>${FMT.month.format(dateOf(m0))}</h6><div class="g7">${g}</div></div>`;
  }
  const poolOpts = (files?.files || []).filter(f => f.file !== files.current).map(f => f.file);
  if (D.pool && !poolOpts.includes(D.pool)) poolOpts.unshift(D.pool);
  d.innerHTML = dlgHead(T('Calendar & project settings'), `<span class="mut">${T('Changes apply immediately · Ctrl+Z undoes')}</span>`) + `<div class="db">
    <h4>${T('Project')}</h4><div class="inl">
      <label>${T('Status date')} <input type="date" data-cal="status" value="${D.status != null ? dateStr(D.status) : ''}"></label>
      <label>${T('Currency')} <input data-cal="cur" value="${esc(D.currency)}" style="width:60px!important"></label>
      <label>${T('Shared team file')} <select data-cal="pool"><option value="">${T('— none —')}</option>${poolOpts.map(f => `<option ${D.pool === f ? 'selected' : ''}>${esc(f)}</option>`).join('')}</select></label></div>
    <span class="hint">${T('A team file is a .plan file with people (resource lines) that several projects share, so workloads add up across projects.')}</span>
    <h4>${T('Working week')}</h4>${weekEditor(D.week, 'wk')}
    <div class="presets"><span class="mut">${T('Presets:')}</span>
      <button class="btn sm" data-act="preset" data-p="std">${T('Mon–Fri 8–5')}</button><button class="btn sm" data-act="preset" data-p="96">${T('Mon–Fri 9–6')}</button>
      <button class="btn sm" data-act="preset" data-p="sat">${T('Mon–Sat')}</button><button class="btn sm" data-act="preset" data-p="4x10">4×10 h</button><button class="btn sm" data-act="preset" data-p="24">24/7</button></div>
    <div class="inl"><label>${T('Hours in one "day" of duration')} <input data-cal="dh" value="${+(D.dayMinutes / 60).toFixed(2)}"></label>
      <span class="mut">${T('A 3d task = 3 × {0} working hours.', +(D.dayMinutes / 60).toFixed(2))}</span></div>
    <h4>${T('Repeating holidays')}
      <select id="country" class="in" style="width:auto">${(holidaysList || []).map(c => `<option value="${c.code}" ${LANG === 'es' && c.code === 'CO' ? 'selected' : ''}>${esc(c.name)}</option>`).join('')}</select>
      <button class="btn sm" data-act="add-country">+ ${T('Add public holidays')}</button><button class="btn sm" data-act="rec-add">+ ${T('Repeating date')}</button></h4>
    ${rec || `<div class="mut">${T('None yet. Add your country\'s public holidays, or a date like 12-25 that repeats every year.')}</div>`}
    <h4>${T('Holidays & exceptions')}<button class="btn sm" data-act="ex-add" data-p="off">+ ${T('Day off / holiday')}</button><button class="btn sm" data-act="ex-add" data-p="work">+ ${T('Extra working day')}</button></h4>
    ${ex || `<div class="mut">${T('No dated exceptions.')}</div>`}
    <h4>${T('Custom fields & outline codes')}<button class="btn sm" data-act="fld-add">+ ${T('Field')}</button></h4>
    ${D.fields.map((f, k) => `<div class="exrow" style="grid-template-columns:170px 120px 1fr 26px"><input data-cal="fld-name" data-k="${k}" value="${esc(f.name)}" placeholder="${T('Name')}"><select data-cal="fld-kind" data-k="${k}">${[['text', T('Text')], ['number', T('Number')], ['date', T('Date')], ['flag', T('Yes / no')], ['code', T('Outline code')]].map(([v, l]) => `<option value="${v}" ${f.kind === v ? 'selected' : ''}>${l}</option>`).join('')}</select><input data-cal="fld-values" data-k="${k}" value="${esc(f.values.join(', '))}" placeholder="${f.kind === 'code' ? T('Codes: Eng, Eng.Backend, Sales') : T('Allowed values (optional)')}" ${f.kind === 'flag' || f.kind === 'date' ? 'disabled' : ''}><button class="rm" data-act="fld-rm" data-k="${k}">×</button></div>`).join('') || `<div class="mut">${T('Add your own columns, like a region, a budget code or an approval flag. Outline codes are levels separated by dots (Eng.Backend).')}</div>`}
    <h4>${T('Other calendars')}<button class="btn sm" data-act="ncal-add">+ ${T('Calendar')}</button></h4>
    ${cals || `<div class="mut">${T('Make a calendar (e.g. a night shift) and pick it in a task\'s details.')}</div>`}
    <h4>${T('Preview')} <span style="text-transform:none;letter-spacing:0;font-weight:400">— ${T('click a day to toggle it')}</span></h4><div class="months4">${months}</div>
  </div>`;
}
function nextOccurrence(k) {
  const name = D.recurring[k].split(/\s+/).slice(-1)[0];
  const t = todayOff();
  const hit = D.holidays.filter(([d]) => d >= t).find(([, n]) => D.recurring[k].endsWith(n));
  return hit ? T('next: {0}', niceY(hit[0])) : '';
}
