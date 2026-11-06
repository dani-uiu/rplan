//! Critical Path Method over working time.
//!
//! Handles summary tasks, FS/SS/FF/SF links with working or elapsed lags, all eight
//! constraint types, deadlines, effort-driven tasks, per-person and per-task
//! calendars, splits, manual and inactive tasks, actual progress against a status
//! date, costs (work, material and cost resources, overtime, per use), earned value,
//! free slack, resource leveling and up to 11 baselines.

use crate::calendar::*;
use crate::model::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default)]
pub struct Sched {
    pub es: i64,
    pub ef: i64,
    pub ls: i64,
    pub lf: i64,
    /// Total slack in working minutes (negative = behind a deadline/constraint).
    pub slack: i64,
    /// How far it can slip without delaying any successor.
    pub free_slack: i64,
    pub crit: bool,
    pub summary: bool,
    /// Finishes after its deadline.
    pub late: bool,
    /// A date constraint can't be met (or conflicts with predecessors).
    pub conflict: bool,
    pub cost: f64,
    /// Working minutes between start and finish (summaries) or the effective duration.
    pub span: i64,
    /// Effective duration (effort-driven tasks compute it from work and people).
    pub dur: i64,
    /// Work minutes of the people on it.
    pub work: i64,
    /// Percent complete (summaries: weighted by duration).
    pub pct: f64,
    pub done: bool,
    pub started: bool,
    pub inactive: bool,
    /// Earned value: budget at completion, planned value, earned value, actual cost.
    pub bac: f64,
    pub pv: f64,
    pub ev: f64,
    pub ac: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Ev {
    pub bac: f64,
    pub pv: f64,
    pub ev: f64,
    pub ac: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Schedule {
    pub tasks: Vec<Sched>,
    pub start: i64,
    pub finish: i64,
    pub cost: f64,
    pub work: i64,
    pub progress: f64,
    /// Critical leaf tasks in start order.
    pub critical: Vec<u32>,
    pub ev: Ev,
}

fn end_of_day(t: i64) -> i64 {
    if t.rem_euclid(DAY) == 0 {
        t + DAY
    } else {
        t
    }
}

// ---------- calendars ----------

/// Every calendar a schedule needs, shared between tasks.
pub struct Cals {
    pub base: Rc<Calendar>,
    pub task: Vec<Rc<Calendar>>,
}

fn is_work(p: &Project, a: &Assign) -> bool {
    p.resources.iter().find(|r| r.name == a.name).map_or(true, |r| r.kind == ResKind::Work)
}

pub fn calendars(p: &Project) -> Result<Cals, String> {
    let base = Rc::new(p.calendar()?);
    let mut named: HashMap<String, Rc<Calendar>> = HashMap::new();
    for c in &p.calendars {
        named.insert(c.name.clone(), Rc::new(p.named_calendar(&base, &c.name)?));
    }
    let personal = |start: &Calendar, r: &Resource| -> Calendar {
        let mut c = match &r.week {
            Some(w) => start.with_week(w),
            None => start.clone(),
        };
        for &(a, b) in &r.off {
            c.block_days(a, b);
        }
        c
    };
    let mut cache: HashMap<String, Rc<Calendar>> = HashMap::new();
    let mut task = Vec::with_capacity(p.tasks.len());
    for t in &p.tasks {
        let start = match &t.cal {
            Some(n) => named.get(n).cloned().ok_or(format!("task {} uses calendar '{n}', which doesn't exist", t.id))?,
            None => base.clone(),
        };
        let mut who: Vec<Resource> = t.who.iter().filter(|a| is_work(p, a)).map(|a| p.resource(&a.name)).filter(|r| r.week.is_some() || !r.off.is_empty()).collect();
        who.sort_by(|a, b| a.name.cmp(&b.name));
        if who.is_empty() && t.splits.is_empty() && t.cal.is_none() {
            task.push(base.clone());
            continue;
        }
        let key = format!("{}|{}|{:?}", t.cal.as_deref().unwrap_or(""), who.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join("\u{1}"), t.splits);
        let c = cache.entry(key).or_insert_with(|| {
            let mut c = if who.is_empty() {
                (*start).clone()
            } else {
                let each: Vec<Calendar> = who.iter().map(|r| personal(&start, r)).collect();
                let refs: Vec<&Calendar> = each.iter().collect();
                // People who never overlap: fall back to the first person's hours.
                Calendar::intersect(&refs).unwrap_or_else(|| each[0].clone())
            };
            for &(a, b) in &t.splits {
                c.block_days(a, b);
            }
            Rc::new(c)
        });
        task.push(c.clone());
    }
    Ok(Cals { base, task })
}

// ---------- the graph ----------

struct LinkE {
    from: usize,
    to: usize,
    kind: Link,
    lag: i64,
    elapsed: bool,
}

struct Graph {
    links: Vec<LinkE>,
    incoming: Vec<Vec<usize>>,
    outgoing: Vec<Vec<usize>>,
    order: Vec<usize>,
    parents: Vec<Option<usize>>,
    children: Vec<Vec<usize>>,
    summary: Vec<bool>,
}

fn graph(p: &Project) -> Result<Graph, String> {
    let n = p.tasks.len();
    let mut idx: HashMap<u32, usize> = HashMap::new();
    for (i, t) in p.tasks.iter().enumerate() {
        if idx.insert(t.id, i).is_some() {
            return Err(format!("two tasks share the id {}", t.id));
        }
    }
    let parents = p.parents();
    let summary = p.summary_flags();
    let mut children: Vec<Vec<usize>> = vec![vec![]; n];
    for (i, par) in parents.iter().enumerate() {
        if let Some(j) = par {
            children[*j].push(i);
        }
    }
    let is_ancestor = |a: usize, mut b: usize| {
        while let Some(x) = parents[b] {
            if x == a {
                return true;
            }
            b = x;
        }
        false
    };
    fn leaves(i: usize, ch: &[Vec<usize>], out: &mut Vec<usize>) {
        if ch[i].is_empty() {
            out.push(i);
        }
        for &c in &ch[i] {
            leaves(c, ch, out);
        }
    }
    let mut links: Vec<LinkE> = vec![];
    for (ti, t) in p.tasks.iter().enumerate() {
        for d in &t.deps {
            let pi = *idx.get(&d.id).ok_or(format!("task {} depends on task {}, which doesn't exist", t.id, d.id))?;
            if pi == ti {
                return Err(format!("task {} can't depend on itself", t.id));
            }
            if is_ancestor(pi, ti) || is_ancestor(ti, pi) {
                return Err(format!("task {} can't be linked to its own summary or subtask ({})", t.id, d.id));
            }
            // Inactive tasks don't hold anything up.
            if p.tasks[pi].inactive {
                continue;
            }
            let mut targets = vec![];
            leaves(ti, &children, &mut targets);
            for to in targets {
                links.push(LinkE { from: pi, to, kind: d.kind, lag: d.lag, elapsed: d.elapsed });
            }
        }
    }
    let mut incoming: Vec<Vec<usize>> = vec![vec![]; n];
    let mut outgoing: Vec<Vec<usize>> = vec![vec![]; n];
    let mut succ: Vec<Vec<usize>> = vec![vec![]; n];
    let mut indeg = vec![0usize; n];
    for (k, l) in links.iter().enumerate() {
        incoming[l.to].push(k);
        outgoing[l.from].push(k);
        succ[l.from].push(l.to);
        indeg[l.to] += 1;
    }
    for (i, par) in parents.iter().enumerate() {
        if let Some(j) = par {
            succ[i].push(*j);
            indeg[*j] += 1;
        }
    }
    let mut order = Vec::with_capacity(n);
    let mut ready: Vec<usize> = (0..n).rev().filter(|&i| indeg[i] == 0).collect();
    while let Some(i) = ready.pop() {
        order.push(i);
        for &s in succ[i].iter().rev() {
            indeg[s] -= 1;
            if indeg[s] == 0 {
                ready.push(s);
            }
        }
    }
    if order.len() != n {
        let ids: Vec<String> = (0..n).filter(|&i| indeg[i] > 0 && !summary[i]).map(|i| p.tasks[i].id.to_string()).collect();
        return Err(format!("these tasks wait on each other in a loop: {}", ids.join(", ")));
    }
    Ok(Graph { links, incoming, outgoing, order, parents, children, summary })
}

// ---------- durations and costs ----------

/// Effective duration: effort-driven tasks divide their work among the people on them.
pub fn effective_duration(p: &Project, t: &Task) -> i64 {
    if let Some(s) = &t.sub_info {
        return s.span;
    }
    let Some(work) = t.work else { return t.dur };
    let people: Vec<&Assign> = t.who.iter().filter(|a| is_work(p, a)).collect();
    let units: f64 = people.iter().map(|a| a.units as f64 / 100.0).sum();
    let regular = (work - t.overtime * people.len() as i64).max(0);
    if units > 0.0 {
        (regular as f64 / units).round() as i64
    } else {
        work
    }
}

/// (cost, work minutes) of a leaf task with the given effective duration.
pub fn task_cost(p: &Project, t: &Task, dur: i64) -> (f64, i64) {
    if let Some(s) = &t.sub_info {
        return (t.cost + s.cost, 0);
    }
    let mut cost = t.cost;
    let mut work = 0;
    for a in &t.who {
        let r = p.resource(&a.name);
        match r.kind {
            ResKind::Work => {
                let w = dur * a.units as i64 / 100;
                work += w + t.overtime;
                cost += r.rate * w as f64 / 60.0 + r.overtime * t.overtime as f64 / 60.0 + r.per_use;
            }
            ResKind::Material => cost += r.rate * a.amount + r.per_use,
            ResKind::Cost => cost += a.amount,
        }
    }
    (cost, work)
}

// ---------- the passes ----------

struct Run {
    s: Vec<Sched>,
    finish: i64,
}

fn run(p: &Project, cals: &Cals, g: &Graph, eff: &[i64], pins: &HashMap<usize, i64>) -> Run {
    let n = p.tasks.len();
    let cal = &*cals.base;
    let shift = |t: i64, lag: i64, el: bool| if el { t + lag } else if lag >= 0 { cal.add(t, lag) } else { cal.sub(t, -lag) };
    let unshift = |t: i64, lag: i64, el: bool| shift(t, -lag, el);
    let t0 = cal.next_work(p.start * DAY);
    let status_t = p.status.map(|d| (d + 1) * DAY);
    let mut s = vec![Sched::default(); n];

    // Forward pass.
    for &i in &g.order {
        let t = &p.tasks[i];
        if g.summary[i] {
            let ch: Vec<usize> = g.children[i].iter().copied().filter(|&c| !s[c].inactive).collect();
            let ch = if ch.is_empty() { g.children[i].clone() } else { ch };
            s[i].summary = true;
            s[i].inactive = t.inactive || g.children[i].iter().all(|&c| s[c].inactive);
            s[i].es = ch.iter().map(|&c| s[c].es).min().unwrap_or(t0);
            s[i].ef = ch.iter().map(|&c| s[c].ef).max().unwrap_or(t0);
            continue;
        }
        let tc = &*cals.task[i];
        let dur = eff[i];
        let el = t.elapsed;
        let adv = |a: i64, m: i64| if el { a + m } else { tc.add(a, m) };
        let ret = |a: i64, m: i64| if el { a - m } else { tc.sub(a, m) };
        let snap = |a: i64| if el || dur == 0 { a } else { tc.next_work(a) };
        let done = t.actual_finish.is_some() || t.pct >= 100;
        let started = done || t.actual_start.is_some() || t.pct > 0;
        s[i].done = done;
        s[i].started = started;
        s[i].inactive = t.inactive || (0..).scan(g.parents[i], |c, _| { let r = *c; *c = c.and_then(|x| g.parents[x]); r }).take(64).any(|a| p.tasks[a].inactive);
        s[i].dur = dur;

        let mut start_b = t0;
        let mut finish_b = i64::MIN;
        for &k in &g.incoming[i] {
            let l = &g.links[k];
            let q = s[l.from];
            match l.kind {
                Link::FS => start_b = start_b.max(shift(q.ef, l.lag, l.elapsed)),
                Link::SS => start_b = start_b.max(shift(q.es, l.lag, l.elapsed)),
                Link::FF => finish_b = finish_b.max(shift(q.ef, l.lag, l.elapsed)),
                Link::SF => finish_b = finish_b.max(shift(q.es, l.lag, l.elapsed)),
            }
        }
        for x in &t.ext {
            match x.kind {
                Link::FS => start_b = start_b.max(shift(x.ef, x.lag, false)),
                Link::SS => start_b = start_b.max(shift(x.es, x.lag, false)),
                Link::FF => finish_b = finish_b.max(shift(x.ef, x.lag, false)),
                Link::SF => finish_b = finish_b.max(shift(x.es, x.lag, false)),
            }
        }
        let mut chain = Some(i);
        while let Some(c) = chain {
            match p.tasks[c].constraint {
                Constraint::Snet(x) => start_b = start_b.max(x),
                Constraint::Fnet(x) if c == i => finish_b = finish_b.max(end_of_day(x)),
                _ => {}
            }
            chain = g.parents[c];
        }
        // Work that hasn't started can't happen in the past.
        if let (Some(st), false) = (status_t, started) {
            start_b = start_b.max(st);
        }
        if finish_b > i64::MIN {
            let sb = if dur > 0 && !el { tc.sub(tc.prev_work(finish_b), dur) } else { finish_b - if el { dur } else { 0 } };
            start_b = start_b.max(sb);
        }
        let mut es = snap(start_b);
        let mut ef;
        if let Some(m) = t.manual {
            es = m;
            ef = adv(m, dur);
        } else if let Some(af) = t.actual_finish {
            es = t.actual_start.unwrap_or_else(|| ret(af, dur)).min(af);
            ef = af;
        } else {
            if let Some(a) = t.actual_start {
                es = a;
            } else if let Some(&pin) = pins.get(&i) {
                es = pin;
            } else {
                match t.constraint {
                    Constraint::Mso(x) => {
                        let forced = snap(x);
                        s[i].conflict = es > forced;
                        es = forced;
                    }
                    Constraint::Mfo(x) => {
                        let f = if el { end_of_day(x) } else { tc.prev_work(end_of_day(x)) };
                        let forced = ret(f, dur);
                        s[i].conflict = es > forced;
                        es = forced;
                    }
                    _ => {}
                }
                if t.delay > 0 && !started {
                    es = snap(adv(es, t.delay));
                }
            }
            if started && !done {
                // Remaining work resumes after the status date if it's behind.
                let done_part = dur * t.pct as i64 / 100;
                let mut resume = adv(es, done_part);
                if let Some(st) = status_t {
                    if resume < st {
                        resume = snap(st);
                    }
                }
                ef = adv(resume, dur - done_part);
            } else {
                ef = adv(es, dur);
            }
        }
        if dur == 0 && t.actual_finish.is_none() {
            ef = es;
        }
        s[i].es = es;
        s[i].ef = ef;
    }
    let finish = (0..n).filter(|&i| !g.summary[i] && !s[i].inactive).map(|i| s[i].ef).max().unwrap_or(t0);

    // Backward pass. Summaries pass their successors' limits down to their children;
    // start limits only reach the children that start the summary.
    let mut limits: Vec<(i64, i64)> = vec![(i64::MAX, i64::MAX); n];
    for &i in g.order.iter().rev() {
        let t = &p.tasks[i];
        let (mut lf_b, mut ls_b) = match g.parents[i] {
            Some(j) => (limits[j].0, if s[i].es == s[j].es { limits[j].1 } else { i64::MAX }),
            None => (i64::MAX, i64::MAX),
        };
        for &k in &g.outgoing[i] {
            let l = &g.links[k];
            let q = s[l.to];
            match l.kind {
                Link::FS => lf_b = lf_b.min(unshift(q.ls, l.lag, l.elapsed)),
                Link::SS => ls_b = ls_b.min(unshift(q.ls, l.lag, l.elapsed)),
                Link::FF => lf_b = lf_b.min(unshift(q.lf, l.lag, l.elapsed)),
                Link::SF => ls_b = ls_b.min(unshift(q.lf, l.lag, l.elapsed)),
            }
        }
        if let Some(d) = t.deadline {
            lf_b = lf_b.min((d + 1) * DAY);
        }
        match t.constraint {
            Constraint::Snlt(x) => ls_b = ls_b.min(x),
            Constraint::Fnlt(x) => lf_b = lf_b.min(end_of_day(x)),
            _ => {}
        }
        if g.summary[i] {
            limits[i] = (lf_b, ls_b);
            continue;
        }
        let tc = &*cals.task[i];
        let dur = eff[i];
        let el = t.elapsed;
        lf_b = lf_b.min(finish);
        let pinned = s[i].started || t.manual.is_some() || pins.contains_key(&i) || matches!(t.constraint, Constraint::Mso(_) | Constraint::Mfo(_));
        if pinned {
            s[i].ls = s[i].es;
            s[i].lf = s[i].ef;
            if !s[i].done && !pins.contains_key(&i) {
                // Pinned work still shows how much room it has before its limits.
                let lim = if ls_b < i64::MAX { lf_b.min(if el { ls_b + dur } else { tc.add(tc.next_work(ls_b), dur) }) } else { lf_b };
                let room = if el { lim - s[i].ef } else { tc.work_between(s[i].ef, lim) };
                s[i].ls = if el { s[i].es + room } else { tc.add(s[i].es, room.max(0)) };
                s[i].lf = s[i].ef;
                s[i].slack = room;
            }
        } else if dur > 0 {
            let mut lf = if el { lf_b } else { tc.prev_work(lf_b) };
            if ls_b < i64::MAX {
                lf = lf.min(if el { ls_b + dur } else { tc.add(tc.next_work(ls_b), dur) });
            }
            s[i].lf = lf;
            s[i].ls = if el { lf - dur } else { tc.sub(lf, dur) };
        } else {
            let lf = lf_b.min(ls_b);
            s[i].lf = lf;
            s[i].ls = lf;
        }
        if !(pinned && !s[i].done && !pins.contains_key(&i)) {
            s[i].slack = if el { s[i].ls - s[i].es } else { tc.work_between(s[i].es, s[i].ls) };
        }
        if pinned && s[i].done {
            s[i].slack = 0;
        }
        s[i].crit = s[i].slack <= 0 && !s[i].done && !s[i].inactive;
        match t.constraint {
            Constraint::Snlt(x) if s[i].es > x => s[i].conflict = true,
            Constraint::Fnlt(x) if s[i].ef > end_of_day(x) => s[i].conflict = true,
            _ => {}
        }
    }
    Run { s, finish }
}

pub fn schedule(p: &Project) -> Result<Schedule, String> {
    let cals = calendars(p)?;
    schedule_with(p, &cals)
}

pub fn schedule_with(p: &Project, cals: &Cals) -> Result<Schedule, String> {
    let n = p.tasks.len();
    let g = graph(p)?;
    let eff: Vec<i64> = p.tasks.iter().map(|t| effective_duration(p, t)).collect();
    let mut r = run(p, cals, &g, &eff, &HashMap::new());
    // "As late as possible": pin those tasks at their late start and schedule again.
    let alap: HashMap<usize, i64> = (0..n).filter(|&i| !g.summary[i] && p.tasks[i].constraint == Constraint::Alap && !r.s[i].started).map(|i| (i, r.s[i].ls)).collect();
    if !alap.is_empty() {
        let first = r.s.clone();
        r = run(p, cals, &g, &eff, &alap);
        for &i in alap.keys() {
            r.s[i].slack = first[i].slack;
            r.s[i].crit = first[i].crit;
        }
    }
    let cal = &*cals.base;
    let Run { mut s, finish } = r;

    // Free slack, costs and earned value for leaves.
    let status_t = p.status.map(|d| (d + 1) * DAY);
    for i in 0..n {
        if g.summary[i] {
            continue;
        }
        let t = &p.tasks[i];
        let mut free = i64::MAX;
        for &k in &g.outgoing[i] {
            let l = &g.links[k];
            let q = s[l.to];
            let shift = |a: i64| if l.elapsed { a + l.lag } else if l.lag >= 0 { cal.add(a, l.lag) } else { cal.sub(a, -l.lag) };
            let gap = match l.kind {
                Link::FS => cal.work_between(shift(s[i].ef), q.es),
                Link::SS => cal.work_between(shift(s[i].es), q.es),
                Link::FF => cal.work_between(shift(s[i].ef), q.ef),
                Link::SF => cal.work_between(shift(s[i].es), q.ef),
            };
            free = free.min(gap);
        }
        if free == i64::MAX {
            free = cal.work_between(s[i].ef, finish);
        }
        s[i].free_slack = free.clamp(0, s[i].slack.max(0));
        let (cost, work) = task_cost(p, t, eff[i]);
        s[i].span = eff[i];
        s[i].pct = if s[i].done { 100.0 } else { t.sub_info.as_ref().map_or(t.pct as f64, |x| x.pct) };
        if s[i].inactive {
            continue;
        }
        s[i].cost = cost;
        s[i].work = work;
        let base = t.baseline();
        s[i].bac = base.map(|b| b.cost).filter(|c| *c > 0.0).unwrap_or(cost);
        s[i].ev = s[i].bac * s[i].pct / 100.0;
        s[i].ac = t.actual_cost.unwrap_or(cost * s[i].pct / 100.0);
        if let Some(st) = status_t {
            let (bs, bf) = base.map(|b| (b.start, b.finish)).unwrap_or((s[i].es, s[i].ef));
            let frac = if st <= bs {
                0.0
            } else if st >= bf {
                1.0
            } else {
                let total = cal.work_between(bs, bf).max(1) as f64;
                cal.work_between(bs, st) as f64 / total
            };
            s[i].pv = s[i].bac * frac;
        }
    }

    // Roll up summaries (children come first in `order`).
    let mut weight = vec![0f64; n];
    for &i in &g.order {
        if !g.summary[i] {
            weight[i] = if s[i].inactive { 0.0 } else { eff[i].max(1) as f64 };
            continue;
        }
        let ch: Vec<usize> = g.children[i].iter().copied().filter(|&c| !s[c].inactive).collect();
        s[i].ls = ch.iter().map(|&c| s[c].ls).min().unwrap_or(s[i].es);
        s[i].lf = ch.iter().map(|&c| s[c].lf).max().unwrap_or(s[i].ef);
        s[i].slack = ch.iter().map(|&c| s[c].slack).min().unwrap_or(0);
        s[i].free_slack = ch.iter().map(|&c| s[c].free_slack).min().unwrap_or(0);
        s[i].crit = ch.iter().any(|&c| s[c].crit);
        s[i].span = cal.work_between(s[i].es, s[i].ef);
        s[i].dur = s[i].span;
        let sum = |s: &[Sched], f: fn(&Sched) -> f64| ch.iter().map(|&c| f(&s[c])).sum::<f64>();
        let (cost, bac, pv, ev, ac) = (sum(&s, |x| x.cost), sum(&s, |x| x.bac), sum(&s, |x| x.pv), sum(&s, |x| x.ev), sum(&s, |x| x.ac));
        s[i].cost = p.tasks[i].cost + cost;
        s[i].bac = p.tasks[i].cost + bac;
        s[i].pv = pv;
        s[i].ev = ev;
        s[i].ac = p.tasks[i].cost + ac;
        s[i].work = ch.iter().map(|&c| s[c].work).sum();
        weight[i] = ch.iter().map(|&c| weight[c]).sum();
        let done: f64 = ch.iter().map(|&c| weight[c] * s[c].pct).sum();
        s[i].pct = if weight[i] > 0.0 { done / weight[i] } else { 0.0 };
        s[i].done = !ch.is_empty() && ch.iter().all(|&c| s[c].done);
        s[i].started = ch.iter().any(|&c| s[c].started);
    }
    for (i, t) in p.tasks.iter().enumerate() {
        s[i].late = !s[i].inactive && t.deadline.is_some_and(|d| s[i].ef > (d + 1) * DAY);
    }

    let leaves_all: Vec<usize> = (0..n).filter(|&i| !g.summary[i] && !s[i].inactive).collect();
    let total: f64 = leaves_all.iter().map(|&i| weight[i]).sum();
    let progress = if total > 0.0 { leaves_all.iter().map(|&i| weight[i] * s[i].pct).sum::<f64>() / total } else { 0.0 };
    let mut crit: Vec<usize> = leaves_all.iter().copied().filter(|&i| s[i].crit).collect();
    crit.sort_by_key(|&i| (s[i].es, s[i].ef, i));
    let top: Vec<usize> = (0..n).filter(|&i| g.parents[i].is_none() && !s[i].inactive).collect();
    let ev = Ev {
        bac: top.iter().map(|&i| s[i].bac).sum(),
        pv: top.iter().map(|&i| s[i].pv).sum(),
        ev: top.iter().map(|&i| s[i].ev).sum(),
        ac: top.iter().map(|&i| s[i].ac).sum(),
    };
    Ok(Schedule {
        start: cal.next_work(p.start * DAY),
        finish,
        cost: top.iter().map(|&i| s[i].cost).sum(),
        work: top.iter().map(|&i| s[i].work).sum(),
        progress,
        critical: crit.iter().map(|&i| p.tasks[i].id).collect(),
        ev,
        tasks: s,
    })
}

// ---------- resources ----------

#[derive(Clone, Debug, Default)]
pub struct Load {
    pub name: String,
    pub capacity: u32,
    /// Day number → (peak units that day, work minutes that day, task ids).
    pub days: BTreeMap<i64, (u32, i64, Vec<u32>)>,
    pub over_days: usize,
    /// Work in minutes (duration × units).
    pub work: i64,
    pub cost: f64,
    pub tasks: usize,
}

fn task_minutes_on(cal: &Calendar, sc: &Sched, elapsed: bool, day: i64) -> i64 {
    let (a, b) = (sc.es.max(day * DAY), sc.ef.min((day + 1) * DAY));
    if b <= a {
        0
    } else if elapsed {
        b - a
    } else {
        cal.work_between(a, b)
    }
}

/// Per-person workload by day: peak simultaneous units, and minutes of work.
pub fn loads(p: &Project, s: &Schedule) -> Vec<Load> {
    let summary = p.summary_flags();
    let Ok(cals) = calendars(p) else { return vec![] };
    let mut out = vec![];
    for r in p.all_resources() {
        if r.kind != ResKind::Work {
            continue;
        }
        let mut l = Load { name: r.name.clone(), capacity: r.capacity, ..Default::default() };
        let mut per_day: BTreeMap<i64, Vec<(i64, i64, u32, u32, i64)>> = BTreeMap::new();
        for (i, t) in p.tasks.iter().enumerate() {
            let Some(a) = t.who.iter().find(|a| a.name == r.name) else { continue };
            let sc = s.tasks[i];
            if summary[i] || sc.inactive {
                continue;
            }
            l.tasks += 1;
            l.work += sc.dur * a.units as i64 / 100 + t.overtime;
            l.cost += task_cost(&Project { tasks: vec![], ..p.clone_light() }, &Task { who: vec![a.clone()], cost: 0.0, overtime: t.overtime, ..Task::new(0, "", 0) }, sc.dur).0;
            if sc.dur == 0 || sc.ef <= sc.es {
                continue;
            }
            let tc = &*cals.task[i];
            for day in sc.es.div_euclid(DAY)..=(sc.ef - 1).div_euclid(DAY) {
                let mins = task_minutes_on(tc, &sc, t.elapsed, day);
                if mins > 0 {
                    let (a0, b0) = (sc.es.max(day * DAY), sc.ef.min((day + 1) * DAY));
                    per_day.entry(day).or_default().push((a0, b0, a.units, t.id, mins * a.units as i64 / 100));
                }
            }
        }
        for (day, iv) in per_day {
            let mut peak = 0;
            for &(a, _, _, _, _) in &iv {
                let u: u32 = iv.iter().filter(|(x, y, _, _, _)| *x <= a && a < *y).map(|v| v.2).sum();
                peak = peak.max(u);
            }
            if peak > r.capacity {
                l.over_days += 1;
            }
            let mut ids: Vec<u32> = iv.iter().map(|v| v.3).collect();
            ids.dedup();
            l.days.insert(day, (peak, iv.iter().map(|v| v.4).sum(), ids));
        }
        out.push(l);
    }
    out
}

impl Project {
    /// A copy without tasks (cheap) for cost lookups.
    fn clone_light(&self) -> Project {
        Project { tasks: vec![], ..self.clone_no_tasks() }
    }
    fn clone_no_tasks(&self) -> Project {
        Project {
            name: String::new(),
            start: self.start,
            status: None,
            currency: String::new(),
            week: self.week.clone(),
            day_minutes: self.day_minutes,
            exceptions: vec![],
            recurring: vec![],
            calendars: vec![],
            pool: None,
            resources: self.resources.clone(),
            tasks: vec![],
            warnings: vec![],
        }
    }
}

/// Work minutes per day for each task (people's work, or the duration if nobody is assigned).
pub fn task_usage(p: &Project, s: &Schedule) -> Result<Vec<Vec<(i64, i64)>>, String> {
    let cals = calendars(p)?;
    let summary = p.summary_flags();
    Ok(p.tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let sc = s.tasks[i];
            if summary[i] || sc.inactive || sc.ef <= sc.es {
                return vec![];
            }
            let units: i64 = t.who.iter().filter(|a| is_work(p, a)).map(|a| a.units as i64).sum();
            let tc = &*cals.task[i];
            (sc.es.div_euclid(DAY)..=(sc.ef - 1).div_euclid(DAY))
                .filter_map(|d| {
                    let m = task_minutes_on(tc, &sc, t.elapsed, d);
                    (m > 0).then(|| (d, if units > 0 { m * units / 100 } else { m }))
                })
                .collect()
        })
        .collect())
}

/// Delays tasks until nobody is booked beyond their capacity. Lower-priority work
/// moves first; among equals, whatever moves the finish least. Returns (task id, delay).
pub fn level(p: &mut Project) -> Result<Vec<(u32, i64)>, String> {
    for t in &mut p.tasks {
        t.delay = 0;
    }
    let summary = p.summary_flags();
    let people: Vec<Resource> = p.all_resources().into_iter().filter(|r| r.kind == ResKind::Work).collect();
    let mut ignored: HashSet<(i64, String)> = HashSet::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let cals = calendars(p)?;
    for _ in 0..5000 {
        if std::time::Instant::now() > deadline {
            break;
        }
        let s = schedule_with(p, &cals)?;
        let mut found: Option<(i64, String, Vec<(usize, u32)>)> = None;
        for r in &people {
            let mine: Vec<(usize, u32)> = p
                .tasks
                .iter()
                .enumerate()
                .filter(|(i, _)| !summary[*i] && s.tasks[*i].dur > 0 && !s.tasks[*i].inactive && !s.tasks[*i].done)
                .filter_map(|(i, t)| t.who.iter().find(|a| a.name == r.name).map(|a| (i, a.units)))
                .collect();
            let mut times: Vec<i64> = mine.iter().map(|(i, _)| s.tasks[*i].es).collect();
            times.sort();
            times.dedup();
            for at in times {
                if found.as_ref().is_some_and(|f| f.0 <= at) {
                    break;
                }
                if ignored.contains(&(at, r.name.clone())) {
                    continue;
                }
                let active: Vec<(usize, u32)> = mine.iter().copied().filter(|(i, _)| s.tasks[*i].es <= at && at < s.tasks[*i].ef).collect();
                let used: u32 = active.iter().map(|a| a.1).sum();
                if active.len() >= 2 && used > r.capacity {
                    found = Some((at, r.name.clone(), active));
                    break;
                }
            }
        }
        let Some((at, who, mut active)) = found else { break };
        let fixed = |i: usize| {
            let t = &p.tasks[i];
            matches!(t.constraint, Constraint::Mso(_) | Constraint::Mfo(_)) || t.manual.is_some() || s.tasks[i].started
        };
        active.retain(|&(i, _)| !fixed(i));
        let min_priority = active.iter().map(|&(i, _)| p.tasks[i].priority).min();
        active.retain(|&(i, _)| Some(p.tasks[i].priority) == min_priority);
        active.sort_by_key(|&(i, _)| (std::cmp::Reverse(s.tasks[i].slack), std::cmp::Reverse(s.tasks[i].es), std::cmp::Reverse(p.tasks[i].id)));
        let mut best: Option<(i64, usize, i64)> = None;
        let all_now: Vec<usize> = p.tasks.iter().enumerate().filter(|(i, t)| !summary[*i] && t.who.iter().any(|a| a.name == who) && s.tasks[*i].es <= at && at < s.tasks[*i].ef).map(|(i, _)| i).collect();
        for &(v, _) in &active {
            let target = all_now.iter().filter(|&&k| k != v).map(|&k| s.tasks[k].ef).filter(|&e| e > s.tasks[v].es).min();
            let Some(target) = target else { continue };
            let add = cals.task[v].work_between(s.tasks[v].es, target).max(1);
            p.tasks[v].delay += add;
            let finish = schedule_with(p, &cals).map(|x| x.finish).unwrap_or(i64::MAX);
            p.tasks[v].delay -= add;
            if best.is_none_or(|b| finish < b.0) {
                best = Some((finish, v, add));
            }
        }
        match best {
            Some((_, v, add)) => p.tasks[v].delay += add,
            None => {
                ignored.insert((at, who));
            }
        }
    }
    Ok(p.tasks.iter().filter(|t| t.delay > 0).map(|t| (t.id, t.delay)).collect())
}

pub fn clear_leveling(p: &mut Project) -> usize {
    let n = p.tasks.iter().filter(|t| t.delay > 0).count();
    for t in &mut p.tasks {
        t.delay = 0;
    }
    n
}

/// Saves the current dates, costs and work into baseline `n` (0–10).
pub fn set_baseline(p: &mut Project, n: usize) -> Result<(), String> {
    let s = schedule(p)?;
    for (t, sc) in p.tasks.iter_mut().zip(&s.tasks) {
        t.baselines[n.min(BASELINES - 1)] = Some(Baseline { start: sc.es, finish: sc.ef, cost: sc.cost, work: sc.work });
    }
    Ok(())
}

pub fn clear_baseline(p: &mut Project, n: usize) {
    for t in &mut p.tasks {
        t.baselines[n.min(BASELINES - 1)] = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched(text: &str) -> (Project, Schedule) {
        let p = Project::parse(text).unwrap();
        let s = schedule(&p).unwrap();
        (p, s)
    }
    fn at(p: &Project, s: &Schedule, id: u32) -> Sched {
        s.tasks[p.tasks.iter().position(|t| t.id == id).unwrap()]
    }
    fn dt(t: i64) -> String {
        fmt_datetime(t)
    }
    const D: i64 = 480;

    #[test]
    fn critical_path_with_slack() {
        let (p, s) = sched(crate::DEMO);
        assert_eq!(at(&p, &s, 4).slack, 3 * D);
        assert!(at(&p, &s, 5).crit);
        assert_eq!(s.critical, vec![3, 5, 7, 8, 9], "done tasks (1, 2) aren't critical");
        assert_eq!(dt(at(&p, &s, 9).es), "2026-12-08T17:00");
        assert_eq!(dt(at(&p, &s, 1).es), "2026-11-09T08:00");
        // Content writing can slip 9 days before it delays QA (its only successor).
        assert_eq!(at(&p, &s, 6).free_slack, 9 * D);
        assert_eq!(at(&p, &s, 4).free_slack, 3 * D);
    }

    #[test]
    fn link_types_and_lags() {
        let base = "start 2026-11-09\ntask 1 | A | 2d\n";
        let (p, s) = sched(&format!("{base}task 2 | B | 1d | 1SS+4h"));
        assert_eq!(dt(at(&p, &s, 2).es), "2026-11-09T13:00");
        let (p, s) = sched(&format!("{base}task 2 | B | 1d | 1FF"));
        assert_eq!(dt(at(&p, &s, 2).es), "2026-11-10T08:00");
        let (p, s) = sched(&format!("{base}task 2 | B | 1d | 1-1d"));
        assert_eq!(dt(at(&p, &s, 2).es), "2026-11-10T08:00");
        let (p, s) = sched(&format!("{base}task 2 | B | 4h | 1SF+1d"));
        assert_eq!(dt(at(&p, &s, 2).ef), "2026-11-09T17:00");
        // An elapsed lag runs through the night: A ends Tue 17:00, +16eh → Wed 09:00.
        let (p, s) = sched(&format!("{base}task 2 | B | 1d | 1+16eh"));
        assert_eq!(dt(at(&p, &s, 2).es), "2026-11-11T09:00");
    }

    #[test]
    fn summaries_roll_up_and_pass_links() {
        let (p, s) = sched("start 2026-11-09\ntask 1 | Phase |\n  task 2 | A | 2d | | Ana | 100\n  task 3 | B | 3d | | Bo | 0\ntask 4 | After | 1d | 1");
        let ph = at(&p, &s, 1);
        assert!(ph.summary);
        assert_eq!((dt(ph.es), dt(ph.ef)), ("2026-11-09T08:00".into(), "2026-11-11T17:00".into()));
        assert_eq!(dt(at(&p, &s, 4).es), "2026-11-12T08:00");
        assert!((ph.pct - 40.0).abs() < 0.01);
        let (p, s) = sched("start 2026-11-09\ntask 1 | First | 1d\ntask 2 | Phase | | 1\n  task 3 | A | 1d\n  task 4 | B | 1d");
        assert_eq!(dt(at(&p, &s, 3).es), "2026-11-10T08:00");
        assert_eq!(dt(at(&p, &s, 4).es), "2026-11-10T08:00");
    }

    #[test]
    fn start_links_from_summaries_only_bind_the_first_child() {
        // X must start with the phase (SS). Only A starts the phase, so B keeps its slack.
        let (p, s) = sched("start 2026-11-09\ntask 1 | Phase |\n  task 2 | A | 1d\n  task 3 | B | 1d | 2\ntask 4 | X | 5d | 1SS\ntask 5 | End | 0 | 4,1");
        assert_eq!(at(&p, &s, 3).slack, 3 * D, "B was wrongly pinned by the SS link");
    }

    #[test]
    fn constraints_deadlines_and_errors() {
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 1d | | | | snet=2026-11-11");
        assert_eq!(dt(at(&p, &s, 1).es), "2026-11-11T08:00");
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 1d | | | | fnet=2026-11-12");
        assert_eq!(dt(at(&p, &s, 1).ef), "2026-11-12T17:00");
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 3d\ntask 2 | B | 1d | 1 | | | mso=2026-11-10");
        assert!(at(&p, &s, 2).conflict);
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 2d | | | | mfo=2026-11-13");
        assert_eq!((dt(at(&p, &s, 1).es), dt(at(&p, &s, 1).ef)), ("2026-11-12T08:00".into(), "2026-11-13T17:00".into()));
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 3d\ntask 2 | B | 1d | 1 | | | snlt=2026-11-10");
        assert!(at(&p, &s, 2).conflict && at(&p, &s, 2).slack < 0);
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 3d | | | | fnlt=2026-11-10");
        assert!(at(&p, &s, 1).conflict);
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 5d | | | | deadline=2026-11-11");
        assert!(at(&p, &s, 1).late);
        assert_eq!(at(&p, &s, 1).slack, -2 * D);
        for bad in [
            "start 2026-11-09\ntask 1 | A | 1d | 2\ntask 2 | B | 1d | 1",
            "start 2026-11-09\ntask 1 | A | 1d | 9",
            "start 2026-11-09\ntask 1 | P |\n  task 2 | A | 1d | 1",
            "start 2026-11-09\ntask 1 | A | 1d\ntask 1 | B | 1d",
            "start 2026-11-09\ntask 1 | A | 1d | | | | cal=Nope",
        ] {
            assert!(schedule(&Project::parse(bad).unwrap()).is_err(), "{bad}");
        }
    }

    #[test]
    fn as_late_as_possible() {
        // B has 3 days of slack; ALAP pushes it to the end without moving the finish.
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 5d\ntask 2 | B | 2d | | | | alap\ntask 3 | End | 0 | 1,2");
        assert_eq!(dt(at(&p, &s, 2).es), "2026-11-12T08:00");
        assert_eq!(dt(at(&p, &s, 3).es), "2026-11-13T17:00");
    }

    #[test]
    fn effort_driven_tasks_and_costs() {
        let text = "start 2026-11-09
resource Ana | rate 50 | peruse 10 | overtime 75
resource Bo | rate 40
resource Cement | type material | rate 9.5
resource Travel | type cost
task 1 | Pour | 1d | | Ana, Bo, Cement[20], Travel[$400] | | work=32h
task 2 | Solo | 1d | | Ana[50%] | | work=8h
task 3 | Rush | 1d | | Ana | | ot=2h";
        let (p, s) = sched(text);
        assert_eq!(at(&p, &s, 1).dur, 2 * D, "32h of work with 2 people = 2 days");
        assert_eq!(at(&p, &s, 2).dur, 2 * D, "8h at 50% = 2 days");
        // Pour: Ana 16h×50 + 10 per use, Bo 16h×40, 20 bags×9.5, travel 400
        assert!((at(&p, &s, 1).cost - (800.0 + 10.0 + 640.0 + 190.0 + 400.0)).abs() < 0.01, "{}", at(&p, &s, 1).cost);
        // Rush: 8h×50 + 2h overtime×75 + 10
        assert!((at(&p, &s, 3).cost - (400.0 + 150.0 + 10.0)).abs() < 0.01);
        assert_eq!(at(&p, &s, 3).work, 600);
    }

    #[test]
    fn actual_progress_and_status_date() {
        let text = "start 2026-11-09
status 2026-11-12
task 1 | Done | 2d | | | | as=2026-11-09T08:00; af=2026-11-12T17:00
task 2 | Half | 4d | 1 | | 25 | as=2026-11-10T08:00
task 3 | Later | 1d | 2
task 4 | Free | 1d";
        let (p, s) = sched(text);
        let t1 = at(&p, &s, 1);
        assert_eq!(dt(t1.ef), "2026-11-12T17:00", "actual finish wins");
        assert!(t1.done && !t1.crit);
        // Half: 1 of 4 days done (Tue); the rest resumes after the status date (Fri 13th).
        assert_eq!(dt(at(&p, &s, 2).es), "2026-11-10T08:00");
        assert_eq!(dt(at(&p, &s, 2).ef), "2026-11-17T17:00");
        assert_eq!(dt(at(&p, &s, 3).es), "2026-11-18T08:00");
        assert_eq!(dt(at(&p, &s, 4).es), "2026-11-13T08:00", "unstarted work can't be in the past");
    }

    #[test]
    fn earned_value() {
        let mut p = Project::parse("start 2026-11-09\nresource Ana | rate 100\ntask 1 | A | 4d | | Ana").unwrap();
        set_baseline(&mut p, 0).unwrap();
        p.status = Some(parse_date("2026-11-10").unwrap());
        p.tasks[0].pct = 25;
        p.tasks[0].actual_cost = Some(1000.0);
        let s = schedule(&p).unwrap();
        assert!((s.ev.bac - 3200.0).abs() < 0.01);
        assert!((s.ev.pv - 1600.0).abs() < 0.01, "half the baseline time has passed: {}", s.ev.pv);
        assert!((s.ev.ev - 800.0).abs() < 0.01);
        assert!((s.ev.ac - 1000.0).abs() < 0.01);
    }

    #[test]
    fn elapsed_split_manual_and_inactive() {
        // Concrete cures 3 calendar days, weekend included.
        let (p, s) = sched("start 2026-11-09\ntask 1 | Pour | 4d\ntask 2 | Cure | 3ed | 1\ntask 3 | Build | 1d | 2");
        assert_eq!(dt(at(&p, &s, 2).ef), "2026-11-15T17:00");
        assert_eq!(dt(at(&p, &s, 3).es), "2026-11-16T08:00");
        // A 3-day task paused Tue–Wed.
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 3d | | | | split=2026-11-10..2026-11-11");
        assert_eq!(dt(at(&p, &s, 1).ef), "2026-11-13T17:00");
        // Manual task ignores its predecessor.
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 5d\ntask 2 | B | 1d | 1 | | | manual=2026-11-10T08:00");
        assert_eq!(dt(at(&p, &s, 2).es), "2026-11-10T08:00");
        // Inactive tasks hold nothing up and cost nothing.
        let (p, s) = sched("start 2026-11-09\nresource Ana | rate 10\ntask 1 | A | 5d | | Ana | | inactive\ntask 2 | B | 1d | 1");
        assert_eq!(dt(at(&p, &s, 2).es), "2026-11-09T08:00");
        assert_eq!(s.cost, 0.0);
        assert!(!at(&p, &s, 1).crit);
    }

    #[test]
    fn personal_and_task_calendars() {
        // Ana only works Mon–Wed: 5 days of her time take Mon, Tue, Wed, Mon, Tue.
        let (p, s) = sched("start 2026-11-09\nresource Ana | week Thu-Fri off\ntask 1 | A | 5d | | Ana");
        assert_eq!(dt(at(&p, &s, 1).ef), "2026-11-17T17:00");
        // Two people work only when both are available.
        let (p, s) = sched("start 2026-11-09\nresource Ana | week Mon-Fri 08:00-12:00\nresource Bo | week Mon-Fri 10:00-17:00\ntask 1 | Meet | 4h | | Ana, Bo");
        assert_eq!((dt(at(&p, &s, 1).es), dt(at(&p, &s, 1).ef)), ("2026-11-09T10:00".into(), "2026-11-10T12:00".into()));
        let (p, s) = sched("start 2026-11-09\ncalendar Night | week Mon-Fri 22:00-24:00\ntask 1 | Paint | 4h | | | | cal=Night");
        assert_eq!(dt(at(&p, &s, 1).ef), "2026-11-10T24:00".replace("24:00", "00:00").replace("2026-11-10", "2026-11-11"));
        // Repeating holidays are skipped.
        let (p, s) = sched("start 2026-12-23\nholiday every 12-25 Christmas\ntask 1 | A | 3d");
        assert_eq!(dt(at(&p, &s, 1).ef), "2026-12-28T17:00");
    }

    #[test]
    fn leveling_delays_instead_of_linking() {
        let mut p = Project::parse(crate::DEMO).unwrap();
        let before = schedule(&p).unwrap().finish;
        assert!(loads(&p, &schedule(&p).unwrap()).iter().find(|x| x.name == "Ana").unwrap().over_days > 0);
        let changed = level(&mut p).unwrap();
        assert!(!changed.is_empty());
        let s = schedule(&p).unwrap();
        assert!(loads(&p, &s).iter().all(|x| x.over_days == 0), "still over-allocated");
        let slip = p.calendar().unwrap().work_between(before, s.finish);
        assert!(slip <= D, "leveling pushed the finish {slip} minutes");
        assert_eq!(clear_leveling(&mut p), changed.len());
        let mut q = Project::parse("start 2026-11-09\ntask 1 | A | 1d | | Ana[50%]\ntask 2 | B | 1d | | Ana[50%]").unwrap();
        assert!(level(&mut q).unwrap().is_empty());
        // Priority decides who waits.
        let mut q = Project::parse("start 2026-11-09\ntask 1 | A | 1d | | Ana | | priority=900\ntask 2 | B | 1d | | Ana | | priority=100").unwrap();
        level(&mut q).unwrap();
        assert!(q.tasks[1].delay > 0 && q.tasks[0].delay == 0);
    }

    #[test]
    fn loads_count_minutes() {
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 2d | | Ana[50%]");
        let l = &loads(&p, &s)[0];
        assert_eq!(l.days.values().map(|v| v.1).sum::<i64>(), 480);
        assert_eq!(task_usage(&p, &s).unwrap()[0].len(), 2);
    }

    #[test]
    fn baselines_roundtrip() {
        let mut p = Project::parse(crate::DEMO).unwrap();
        set_baseline(&mut p, 0).unwrap();
        set_baseline(&mut p, 4).unwrap();
        let q = Project::parse(&p.serialize()).unwrap();
        assert_eq!(p.tasks[3].baseline(), q.tasks[3].baseline());
        assert!(q.tasks[3].baselines[4].is_some() && q.tasks[3].baseline().unwrap().cost > 0.0);
        clear_baseline(&mut p, 0);
        assert!(p.tasks.iter().all(|t| t.baseline().is_none()));
    }
}
