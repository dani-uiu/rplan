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
    p.resources.iter().find(|r| r.name == a.name).is_none_or(|r| r.kind == ResKind::Work)
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
        let mut who: Vec<_> = t.who.iter().filter(|a| is_work(p, a)).map(|a| p.resource(&a.name)).filter(|r| r.week.is_some() || !r.off.is_empty()).collect();
        who.sort_by(|a, b| a.name.cmp(&b.name));
        if who.is_empty() && t.splits.is_empty() && t.lsplits.is_empty() && t.cal.is_none() {
            task.push(base.clone());
            continue;
        }
        let key = format!("{}|{}|{:?}|{:?}", t.cal.as_deref().unwrap_or(""), who.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join("\u{1}"), t.splits, t.lsplits);
        let c = cache.entry(key).or_insert_with(|| {
            let mut c = if who.is_empty() {
                (*start).clone()
            } else {
                let each: Vec<Calendar> = who.iter().map(|r| personal(&start, r)).collect();
                let refs: Vec<&Calendar> = each.iter().collect();
                // People who never overlap: fall back to the first person's hours.
                Calendar::intersect(&refs).unwrap_or_else(|| each[0].clone())
            };
            for &(a, b) in t.splits.iter().chain(&t.lsplits) {
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
    /// Lag as a percent of the predecessor's duration.
    pct: Option<i32>,
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
                links.push(LinkE { from: pi, to, kind: d.kind, lag: d.lag, elapsed: d.elapsed, pct: d.pct });
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

/// Effective duration: effort-driven tasks divide their work among the people on them,
/// and a person who starts late or has their own amount of work can make it longer.
pub fn effective_duration(p: &Project, t: &Task) -> i64 {
    let base = base_duration(p, t);
    if t.sub_info.is_some() {
        return base;
    }
    t.who.iter().filter(|a| is_work(p, a) && a.detailed()).map(|a| a.delay + assignment_span(a, base)).fold(base_if_shared(p, t, base), i64::max)
}

/// How long one person's part lasts (not counting their delay).
pub fn assignment_span(a: &Assign, base: i64) -> i64 {
    match a.work {
        Some(w) if a.units > 0 => w * 100 / a.units as i64,
        _ => base,
    }
}

/// The task's own duration still counts unless every person has their own work.
fn base_if_shared(p: &Project, t: &Task, base: i64) -> i64 {
    let people: Vec<&Assign> = t.who.iter().filter(|a| is_work(p, a)).collect();
    if !people.is_empty() && people.iter().all(|a| a.work.is_some()) {
        0
    } else {
        base
    }
}

/// The duration before per-person details: effort-driven tasks divide their work.
pub fn base_duration(p: &Project, t: &Task) -> i64 {
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

/// One assignment's work by day: (day, work minutes, units that day, from, to).
/// The total is the person's work; contours shape it, delays and own work move it.
pub fn assignment_days(tc: &Calendar, elapsed: bool, sc: &Sched, base: i64, a: &Assign) -> Vec<(i64, i64, u32, i64, i64)> {
    if sc.ef <= sc.es {
        return vec![];
    }
    let adv = |x: i64, m: i64| if elapsed { x + m } else { tc.add(x, m) };
    let (w0, w1, total) = if a.detailed() {
        let start = if a.delay > 0 { adv(sc.es, a.delay) } else { sc.es };
        let span = assignment_span(a, base);
        let end = adv(start, span).min(sc.ef);
        (start, end, a.work.unwrap_or(span * a.units as i64 / 100))
    } else {
        (sc.es, sc.ef, sc.dur * a.units as i64 / 100)
    };
    if w1 <= w0 {
        return vec![];
    }
    let mut slices = vec![];
    for day in w0.div_euclid(DAY)..=(w1 - 1).div_euclid(DAY) {
        let (x0, x1) = (w0.max(day * DAY), w1.min((day + 1) * DAY));
        let m = if x1 <= x0 { 0 } else if elapsed { x1 - x0 } else { tc.work_between(x0, x1) };
        if m > 0 {
            slices.push((day, m, x0, x1));
        }
    }
    let all: i64 = slices.iter().map(|s| s.1).sum();
    if all == 0 {
        return vec![];
    }
    let mut done = 0i64;
    let raw: Vec<f64> = slices
        .iter()
        .map(|&(_, m, _, _)| {
            let x = (done as f64 + m as f64 / 2.0) / all as f64;
            done += m;
            m as f64 * a.contour.weight(x)
        })
        .collect();
    let sum: f64 = raw.iter().sum::<f64>().max(1e-9);
    let mut out = Vec::with_capacity(slices.len());
    let mut given = 0i64;
    for (k, &(day, m, x0, x1)) in slices.iter().enumerate() {
        // Round so the days add up exactly to the total.
        let upto = (raw[..=k].iter().sum::<f64>() / sum * total as f64).round() as i64;
        let mins = upto - given;
        given = upto;
        let units = (mins as f64 / m as f64 * 100.0).round().max(0.0) as u32;
        out.push((day, mins, if a.contour == Contour::Flat && a.work.is_none() { a.units } else { units }, x0, x1));
    }
    out
}

/// The cost of one person's work, priced at the rate in force on each day.
fn work_cost(r: &Resource, days: &[(i64, i64, u32, i64, i64)], overtime: i64) -> f64 {
    let regular: f64 = if r.rates.is_empty() { r.rate * days.iter().map(|d| d.1).sum::<i64>() as f64 / 60.0 } else { days.iter().map(|d| r.rate_on(d.0) * d.1 as f64 / 60.0).sum() };
    regular + r.overtime * overtime as f64 / 60.0 + r.per_use
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
    // A percent lag is that share of the predecessor's working time.
    let lag_of = |l: &LinkE, s: &[Sched]| -> (i64, bool) {
        match l.pct {
            Some(pc) => {
                let q = s[l.from];
                let len = if g.summary[l.from] { cal.work_between(q.es, q.ef) } else { eff[l.from] };
                (len * pc as i64 / 100, false)
            }
            None => (l.lag, l.elapsed),
        }
    };

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
            let (lag, lel) = lag_of(l, &s);
            match l.kind {
                Link::FS => start_b = start_b.max(shift(q.ef, lag, lel)),
                Link::SS => start_b = start_b.max(shift(q.es, lag, lel)),
                Link::FF => finish_b = finish_b.max(shift(q.ef, lag, lel)),
                Link::SF => finish_b = finish_b.max(shift(q.es, lag, lel)),
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
            let (lag, lel) = lag_of(l, &s);
            match l.kind {
                Link::FS => lf_b = lf_b.min(unshift(q.ls, lag, lel)),
                Link::SS => ls_b = ls_b.min(unshift(q.ls, lag, lel)),
                Link::FF => lf_b = lf_b.min(unshift(q.lf, lag, lel)),
                Link::SF => ls_b = ls_b.min(unshift(q.lf, lag, lel)),
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

/// The forward and backward passes, run twice when there are "as late as possible" tasks.
fn passes(p: &Project, cals: &Cals, g: &Graph, eff: &[i64]) -> Run {
    let n = p.tasks.len();
    let mut r = run(p, cals, g, eff, &HashMap::new());
    // "As late as possible": pin those tasks at their late start and schedule again.
    let alap: HashMap<usize, i64> = (0..n).filter(|&i| !g.summary[i] && p.tasks[i].constraint == Constraint::Alap && !r.s[i].started).map(|i| (i, r.s[i].ls)).collect();
    if !alap.is_empty() {
        let first = r.s.clone();
        r = run(p, cals, g, eff, &alap);
        for &i in alap.keys() {
            r.s[i].slack = first[i].slack;
            r.s[i].crit = first[i].crit;
        }
    }
    r
}

pub fn schedule_with(p: &Project, cals: &Cals) -> Result<Schedule, String> {
    let n = p.tasks.len();
    let g = graph(p)?;
    let eff: Vec<i64> = p.tasks.iter().map(|t| effective_duration(p, t)).collect();
    let cal = &*cals.base;
    let Run { mut s, finish } = passes(p, cals, &g, &eff);

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
            let (lag, lel) = match l.pct {
                Some(pc) => ((if g.summary[l.from] { cal.work_between(s[l.from].es, s[l.from].ef) } else { eff[l.from] }) * pc as i64 / 100, false),
                None => (l.lag, l.elapsed),
            };
            let shift = |a: i64| if lel { a + lag } else if lag >= 0 { cal.add(a, lag) } else { cal.sub(a, -lag) };
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
        let (cost, work) = if t.sub_info.is_none() && t.who.iter().any(|a| a.detailed() || p.resources.iter().any(|r| r.name == a.name && !(r.rates.is_empty() && r.caps.is_empty()))) {
            spread_cost(p, t, &cals.task[i], &s[i], base_duration(p, t))
        } else {
            task_cost(p, t, eff[i])
        };
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
    /// Days whose capacity differs from `capacity` (it changes by date).
    pub caps: BTreeMap<i64, u32>,
}

impl Load {
    pub fn cap_on(&self, day: i64) -> u32 {
        self.caps.get(&day).copied().unwrap_or(self.capacity)
    }
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
            let tc = &*cals.task[i];
            let days = if sc.dur == 0 { vec![] } else { assignment_days(tc, t.elapsed, &sc, base_duration(p, t), a) };
            if a.detailed() {
                l.work += days.iter().map(|d| d.1).sum::<i64>() + t.overtime;
            } else {
                l.work += sc.dur * a.units as i64 / 100 + t.overtime;
            }
            l.cost += if r.rates.is_empty() && !a.detailed() {
                r.rate * (sc.dur * a.units as i64 / 100) as f64 / 60.0 + r.overtime * t.overtime as f64 / 60.0 + r.per_use
            } else {
                work_cost(&r, &days, t.overtime)
            };
            for (day, mins, units, a0, b0) in days {
                if mins > 0 {
                    per_day.entry(day).or_default().push((a0, b0, units, t.id, mins));
                }
            }
        }
        for (day, iv) in per_day {
            let mut peak = 0;
            for &(a, _, _, _, _) in &iv {
                let u: u32 = iv.iter().filter(|(x, y, _, _, _)| *x <= a && a < *y).map(|v| v.2).sum();
                peak = peak.max(u);
            }
            let cap = r.capacity_on(day);
            if cap != r.capacity {
                l.caps.insert(day, cap);
            }
            if peak > cap {
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
            let tc = &*cals.task[i];
            let people: Vec<&Assign> = t.who.iter().filter(|a| is_work(p, a)).collect();
            if people.is_empty() {
                return (sc.es.div_euclid(DAY)..=(sc.ef - 1).div_euclid(DAY))
                    .filter_map(|d| {
                        let m = task_minutes_on(tc, &sc, t.elapsed, d);
                        (m > 0).then_some((d, m))
                    })
                    .collect();
            }
            let base = base_duration(p, t);
            let mut by_day: BTreeMap<i64, i64> = BTreeMap::new();
            for a in people {
                for (d, m, ..) in assignment_days(tc, t.elapsed, &sc, base, a) {
                    *by_day.entry(d).or_default() += m;
                }
            }
            by_day.into_iter().filter(|x| x.1 > 0).collect()
        })
        .collect())
}

/// Each person's work by day on each task: [task][assignment] → (day, minutes).
pub fn assignment_usage(p: &Project, s: &Schedule) -> Result<Vec<Vec<(String, Vec<(i64, i64)>)>>, String> {
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
            let base = base_duration(p, t);
            t.who
                .iter()
                .filter(|a| is_work(p, a))
                .map(|a| (a.name.clone(), assignment_days(&cals.task[i], t.elapsed, &sc, base, a).into_iter().filter(|d| d.1 > 0).map(|d| (d.0, d.1)).collect()))
                .collect()
        })
        .collect())
}

/// (cost, work) of a task from each person's daily work, at the rate of each day.
fn spread_cost(p: &Project, t: &Task, tc: &Calendar, sc: &Sched, base: i64) -> (f64, i64) {
    let mut cost = t.cost;
    let mut work = 0;
    for a in &t.who {
        let r = p.resource(&a.name);
        match r.kind {
            ResKind::Work => {
                let days = assignment_days(tc, t.elapsed, sc, base, a);
                work += days.iter().map(|d| d.1).sum::<i64>() + t.overtime;
                cost += work_cost(&r, &days, t.overtime);
            }
            ResKind::Material => cost += r.rate_on(sc.es.div_euclid(DAY)) * a.amount + r.per_use,
            ResKind::Cost => cost += a.amount,
        }
    }
    (cost, work)
}

/// When each task's cost is spent, day by day: work as it's done (or all at the start or
/// end, as each person's `accrue` says), per-use costs at the start, fixed costs spread.
pub fn cost_days(p: &Project, s: &Schedule) -> Result<Vec<Vec<(i64, f64)>>, String> {
    let cals = calendars(p)?;
    let summary = p.summary_flags();
    let mut out = vec![];
    for (i, t) in p.tasks.iter().enumerate() {
        let sc = s.tasks[i];
        let mut m: BTreeMap<i64, f64> = BTreeMap::new();
        if summary[i] || sc.inactive {
            if summary[i] && t.cost != 0.0 {
                *m.entry(sc.es.div_euclid(DAY)).or_default() += t.cost;
            }
            out.push(m.into_iter().collect());
            continue;
        }
        let tc = &*cals.task[i];
        let (d0, d1) = (sc.es.div_euclid(DAY), (sc.ef - 1).max(sc.es).div_euclid(DAY));
        let work_days: Vec<i64> = (d0..=d1).filter(|&d| sc.ef <= sc.es || task_minutes_on(tc, &sc, t.elapsed, d) > 0).collect();
        let work_days = if work_days.is_empty() { vec![d0] } else { work_days };
        let spread = |m: &mut BTreeMap<i64, f64>, v: f64| {
            for d in &work_days {
                *m.entry(*d).or_default() += v / work_days.len() as f64;
            }
        };
        spread(&mut m, t.cost);
        if let Some(info) = &t.sub_info {
            spread(&mut m, info.cost);
        }
        let base = base_duration(p, t);
        for a in &t.who {
            let r = p.resource(&a.name);
            let (total, days): (f64, Vec<(i64, f64)>) = match r.kind {
                ResKind::Work => {
                    let days = assignment_days(tc, t.elapsed, &sc, base, a);
                    *m.entry(days.first().map_or(d0, |d| d.0)).or_default() += r.per_use;
                    let ot = r.overtime * t.overtime as f64 / 60.0;
                    let per: Vec<(i64, f64)> = days.iter().map(|d| (d.0, r.rate_on(d.0) * d.1 as f64 / 60.0)).collect();
                    let tot = per.iter().map(|x| x.1).sum::<f64>() + ot;
                    if let Some(last) = per.last().map(|x| x.0) {
                        *m.entry(last).or_default() += ot;
                    }
                    (tot, per)
                }
                ResKind::Material => {
                    *m.entry(d0).or_default() += r.per_use;
                    let v = r.rate_on(d0) * a.amount;
                    (v, work_days.iter().map(|d| (*d, v / work_days.len() as f64)).collect())
                }
                ResKind::Cost => (a.amount, work_days.iter().map(|d| (*d, a.amount / work_days.len() as f64)).collect()),
            };
            match r.accrue {
                Accrue::Prorated => {
                    for (d, v) in days {
                        *m.entry(d).or_default() += v;
                    }
                }
                Accrue::Start => *m.entry(days.first().map_or(d0, |x| x.0)).or_default() += total,
                Accrue::End => *m.entry(days.last().map_or(d1, |x| x.0)).or_default() += total,
            }
        }
        out.push(m.into_iter().filter(|x| x.1.abs() > 1e-9).collect());
    }
    Ok(out)
}

/// Sets progress as if work went to plan up to the status date: tasks that should be
/// finished get their actual start and finish, tasks under way get their actual start
/// and the share of their work that falls before the status date. Returns how many changed.
pub fn update_progress(p: &mut Project) -> Result<usize, String> {
    let st = p.status.ok_or("set a status date first")?;
    let st_t = (st + 1) * DAY;
    let cals = calendars(p)?;
    // The plan as it was meant to go: without pushing unstarted work past the status date.
    let plan = Project { status: None, ..p.clone() };
    let s = schedule_with(&plan, &cals)?;
    let summary = p.summary_flags();
    let mut n = 0;
    for (i, t) in p.tasks.iter_mut().enumerate() {
        let sc = s.tasks[i];
        if summary[i] || sc.inactive || sc.done || t.sub.is_some() || sc.es >= st_t {
            continue;
        }
        let before = (t.actual_start, t.actual_finish, t.pct);
        if sc.ef <= st_t {
            t.actual_start.get_or_insert(sc.es);
            t.actual_finish = Some(sc.ef);
            t.pct = 100;
        } else {
            t.actual_start.get_or_insert(sc.es);
            let tc = &*cals.task[i];
            let (done, all) = if t.elapsed { (st_t - sc.es, sc.ef - sc.es) } else { (tc.work_between(sc.es, st_t), tc.work_between(sc.es, sc.ef)) };
            let pct = (done * 100 / all.max(1)).clamp(1, 99) as u8;
            t.pct = t.pct.max(pct);
        }
        if before != (t.actual_start, t.actual_finish, t.pct) {
            n += 1;
        }
    }
    Ok(n)
}

/// Delays tasks until nobody is booked beyond their capacity. Lower-priority work
/// moves first; among equals, whatever moves the finish least (see `LevelOpts` for the
/// other orders, for staying within slack, and for pausing work that's under way).
/// Returns (task id, delay).
pub fn level(p: &mut Project) -> Result<Vec<(u32, i64)>, String> {
    for t in &mut p.tasks {
        t.delay = 0;
        t.lsplits.clear();
    }
    let opts = p.leveling;
    let summary = p.summary_flags();
    let people: Vec<Resource> = p.all_resources().into_iter().filter(|r| r.kind == ResKind::Work).collect();
    // Each person's tasks as (task, units); who is on what doesn't change while leveling.
    // Unstarted "as late as possible" tasks are left out: they follow the finish, so making
    // room around them only pushes the finish and the clash along with it, for ever.
    let floats = |t: &Task| t.constraint == Constraint::Alap && t.pct == 0 && t.actual_start.is_none() && t.actual_finish.is_none();
    let on: Vec<Vec<(usize, u32)>> = people
        .iter()
        .map(|r| p.tasks.iter().enumerate().filter(|(i, t)| !summary[*i] && !floats(t)).filter_map(|(i, t)| t.who.iter().find(|a| a.name == r.name).map(|a| (i, a.units))).collect())
        .collect();
    let mut ignored: HashSet<(i64, usize)> = HashSet::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut cals = calendars(p)?;
    // Delays and pauses don't change the links or the durations: work them out once.
    let g = graph(p)?;
    let eff: Vec<i64> = p.tasks.iter().map(|t| effective_duration(p, t)).collect();
    let finish0 = passes(p, &cals, &g, &eff).finish;
    // Each step settles one clash; packed plans need about one step per pair of clashing tasks.
    for _ in 0..5000 + p.tasks.len().pow(2) {
        if std::time::Instant::now() > deadline {
            break;
        }
        let Run { s, finish: finish_now } = passes(p, &cals, &g, &eff);
        let mut found: Option<(i64, usize, Vec<(usize, u32)>)> = None;
        for (ri, r) in people.iter().enumerate() {
            let mine: Vec<(usize, u32)> = on[ri].iter().copied().filter(|&(i, _)| s[i].dur > 0 && !s[i].inactive && !s[i].done).collect();
            let mut times: Vec<i64> = mine.iter().map(|(i, _)| s[*i].es).collect();
            times.sort();
            times.dedup();
            for at in times {
                if found.as_ref().is_some_and(|f| f.0 <= at) {
                    break;
                }
                if ignored.contains(&(at, ri)) {
                    continue;
                }
                // Only tasks actually working at that moment (not paused around it).
                let active: Vec<(usize, u32)> = mine.iter().copied().filter(|(i, _)| s[*i].es <= at && at < s[*i].ef && (p.tasks[*i].elapsed || cals.task[*i].next_work(at) == at)).collect();
                let used: u32 = active.iter().map(|a| a.1).sum();
                if active.len() >= 2 && used > r.capacity_on(at.div_euclid(DAY)) {
                    found = Some((at, ri, active));
                    break;
                }
            }
        }
        let Some((at, who, mut active)) = found else { break };
        let fixed = |i: usize| {
            let t = &p.tasks[i];
            matches!(t.constraint, Constraint::Mso(_) | Constraint::Mfo(_)) || t.manual.is_some() || (s[i].started && !opts.split)
        };
        active.retain(|&(i, _)| !fixed(i));
        if opts.order != LevelOrder::Id {
            let min_priority = active.iter().map(|&(i, _)| p.tasks[i].priority).min();
            active.retain(|&(i, _)| Some(p.tasks[i].priority) == min_priority);
        }
        match opts.order {
            LevelOrder::Standard => active.sort_by_key(|&(i, _)| (std::cmp::Reverse(s[i].slack), std::cmp::Reverse(s[i].es), std::cmp::Reverse(p.tasks[i].id))),
            // Priority / id order: the later task (by id) is the one that moves.
            LevelOrder::Priority | LevelOrder::Id => {
                active.sort_by_key(|&(i, _)| std::cmp::Reverse(p.tasks[i].id));
                active.truncate(1);
            }
        }
        // Candidates: delay the task, or (if allowed) pause it while the clash lasts.
        let mut best: Option<(i64, usize, i64, Option<(i64, i64)>)> = None;
        let all_now: Vec<usize> = on[who].iter().map(|&(i, _)| i).filter(|&i| s[i].es <= at && at < s[i].ef).collect();
        for &(v, _) in &active {
            let target = all_now.iter().filter(|&&k| k != v).map(|&k| s[k].ef).filter(|&e| e > s[v].es).min();
            let Some(target) = target else { continue };
            if !s[v].started {
                let add = cals.task[v].work_between(s[v].es, target).max(1);
                // Within its slack the finish can't move, and no later candidate can beat that.
                if s[v].slack >= add {
                    if best.is_none_or(|b| finish_now < b.0) {
                        best = Some((finish_now, v, add, None));
                    }
                    break;
                }
                p.tasks[v].delay += add;
                let finish = passes(p, &cals, &g, &eff).finish;
                p.tasks[v].delay -= add;
                if best.is_none_or(|b| finish < b.0) {
                    best = Some((finish, v, add, None));
                }
            }
            if opts.split && s[v].es < at && at.div_euclid(DAY) > s[v].es.div_euclid(DAY) {
                let pause = (at.div_euclid(DAY), ((target - 1).div_euclid(DAY)).max(at.div_euclid(DAY)));
                if p.tasks[v].lsplits.contains(&pause) {
                    continue;
                }
                p.tasks[v].lsplits.push(pause);
                let finish = calendars(p).map_or(i64::MAX, |c| passes(p, &c, &g, &eff).finish);
                p.tasks[v].lsplits.pop();
                if best.is_none_or(|b| finish < b.0) {
                    best = Some((finish, v, 0, Some(pause)));
                }
            }
        }
        if opts.slack_only && best.is_some_and(|b| b.0 > finish0) {
            best = None;
        }
        match best {
            Some((_, v, _, Some(pause))) => {
                p.tasks[v].lsplits.push(pause);
                cals = calendars(p)?;
            }
            Some((_, v, add, None)) => p.tasks[v].delay += add,
            None => {
                ignored.insert((at, who));
            }
        }
    }
    Ok(p.tasks.iter().filter(|t| t.delay > 0 || !t.lsplits.is_empty()).map(|t| (t.id, t.delay)).collect())
}

pub fn clear_leveling(p: &mut Project) -> usize {
    let n = p.tasks.iter().filter(|t| t.delay > 0 || !t.lsplits.is_empty()).count();
    for t in &mut p.tasks {
        t.delay = 0;
        t.lsplits.clear();
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

    #[test]
    fn percent_lags_and_months() {
        // B starts when half of A is done; C waits half of A's length after it.
        let (p, s) = sched("project L\nstart 2026-11-09\ntask 1 | A | 4d\ntask 2 | B | 1d | 1SS+50%\ntask 3 | C | 1d | 1FS+50%\ntask 4 | D | 1mo\n");
        assert_eq!(dt(at(&p, &s, 2).es), "2026-11-11T08:00");
        assert_eq!(dt(at(&p, &s, 3).es), "2026-11-17T08:00");
        assert_eq!(at(&p, &s, 4).dur, 20 * D);
        assert!(p.serialize().contains("task 2 | B | 1d | 1SS+50%"), "{}", p.serialize());
        assert!(p.serialize().contains("task 3 | C | 1d | 1FS+50%"));
    }

    #[test]
    fn people_with_their_own_delay_work_and_contour() {
        let text = "project A\nstart 2026-11-09\nresource Ana | rate 100\nresource Bo | rate 50\ntask 1 | Build | 4d | | Ana[100% delay=1d], Bo[50% work=8h contour=back] | 0\n";
        let (p, s) = sched(text);
        let t = at(&p, &s, 1);
        // Ana starts a day late and still does 4 days: the task takes 5.
        assert_eq!(t.dur, 5 * D);
        assert_eq!(dt(t.ef), "2026-11-13T17:00");
        let ls = loads(&p, &s);
        let ana = ls.iter().find(|l| l.name == "Ana").unwrap();
        assert!(!ana.days.contains_key(&parse_date("2026-11-09").unwrap()), "Ana doesn't work on day 1");
        assert_eq!(ana.work, 4 * D);
        let bo = ls.iter().find(|l| l.name == "Bo").unwrap();
        assert_eq!(bo.work, 8 * 60, "Bo's own 8h");
        let days: Vec<i64> = bo.days.values().map(|v| v.1).collect();
        assert_eq!(days.iter().sum::<i64>(), 480);
        assert!(days.first() < days.last(), "back-loaded: {days:?}");
        // Cost: Ana 32h × 100 + Bo 8h × 50.
        assert_eq!(t.cost, 3200.0 + 400.0);
        // Written back the same way.
        assert!(p.serialize().contains("Ana[100% delay=8h],Bo[50% work=8h contour=back]"), "{}", p.serialize());
        let again = Project::parse(&p.serialize()).unwrap();
        assert_eq!(again.tasks[0].who, p.tasks[0].who);
    }

    #[test]
    fn rates_and_capacity_change_by_date_and_costs_accrue() {
        let text = "project R\nstart 2026-11-09\nresource Ana | rate 100 | rate 200 from 2026-11-11 | capacity 100 | capacity 50 from 2026-11-12 | accrue end\ntask 1 | A | 4d | | Ana | 0\n";
        let (p, s) = sched(text);
        // Mon–Tue at 100, Wed–Thu at 200.
        assert_eq!(at(&p, &s, 1).cost, 16.0 * 100.0 + 16.0 * 200.0);
        let l = &loads(&p, &s)[0];
        assert_eq!(l.over_days, 1, "full time on Thursday is over the 50% she has from then");
        let c = &cost_days(&p, &s).unwrap()[0];
        assert_eq!(c.len(), 1, "accrued at the end: {c:?}");
        assert_eq!(c[0], (parse_date("2026-11-12").unwrap(), 4800.0));
        let r = Project::parse(&p.serialize()).unwrap().resources[0].clone();
        assert_eq!(r, p.resources[0]);
    }

    #[test]
    fn update_progress_to_the_status_date() {
        let mut p = Project::parse("project U\nstart 2026-11-09\nstatus 2026-11-11\ntask 1 | A | 2d\ntask 2 | B | 4d | 1\ntask 3 | C | 2d | 2\n").unwrap();
        assert_eq!(update_progress(&mut p).unwrap(), 2);
        assert_eq!((p.tasks[0].pct, p.tasks[0].actual_finish.map(fmt_datetime)), (100, Some("2026-11-10T17:00".into())));
        assert_eq!(p.tasks[1].pct, 25, "one of B's four days is before the status date");
        assert_eq!(p.tasks[1].actual_start.map(fmt_datetime), Some("2026-11-11T08:00".into()));
        assert_eq!(p.tasks[2].pct, 0);
        assert_eq!(update_progress(&mut p).unwrap(), 0, "nothing more to do");
    }

    #[test]
    fn leveling_leaves_as_late_as_possible_tasks_alone() {
        // They follow the finish, so making room around them used to run away (delays of years).
        for other in ["| 0 | alap", ""] {
            let mut p = Project::parse(&format!("start 2026-11-09
task 1 | A | 3d | | Ana | 0 | alap
task 2 | B | 3d | | Ana {other}
task 3 | C | 3d
task 4 | D | 2d | | Ana
task 5 | E | 1d | | Ana")).unwrap();
            let changed = level(&mut p).unwrap();
            assert!(changed.iter().all(|&(_, d)| d <= 5 * D), "{changed:?}");
            assert!(p.tasks[0].delay == 0 && !changed.is_empty(), "the other clashes are still settled: {changed:?}");
        }
    }

    #[test]
    fn leveling_options() {
        let text = "project V\nstart 2026-11-09\ntask 1 | A | 2d | | Ana | 0\ntask 2 | B | 2d | | Ana | 0\ntask 3 | Long | 6d\n";
        // By id: the later task (2) moves.
        let mut p = Project::parse(&format!("{text}leveling order=id\n")).unwrap();
        level(&mut p).unwrap();
        assert!(p.tasks[1].delay > 0 && p.tasks[0].delay == 0);
        assert!(p.serialize().contains("leveling order=id"));
        // Within slack only: there's room (Long takes 6 days), so it still levels.
        let mut p = Project::parse(&format!("{text}leveling slack\n")).unwrap();
        let f0 = schedule(&p).unwrap().finish;
        level(&mut p).unwrap();
        assert!(p.tasks.iter().any(|t| t.delay > 0));
        assert_eq!(schedule(&p).unwrap().finish, f0);
        // No room at all: nothing moves.
        let mut p = Project::parse("project V\nstart 2026-11-09\nleveling slack\ntask 1 | A | 2d | | Ana | 0\ntask 2 | B | 2d | | Ana | 0\n").unwrap();
        level(&mut p).unwrap();
        assert!(p.tasks.iter().all(|t| t.delay == 0));
        // Splitting: a long task under way pauses for a short urgent one.
        let mut p = Project::parse("project S\nstart 2026-11-09\nleveling split\ntask 1 | Long | 5d | | Ana | 0\ntask 2 | Urgent | 1d | | Ana | 0 | snet=2026-11-11; priority=900\n").unwrap();
        level(&mut p).unwrap();
        assert_eq!(p.tasks[0].lsplits, vec![(parse_date("2026-11-11").unwrap(), parse_date("2026-11-11").unwrap())], "{:?}", p.tasks[0]);
        assert!(p.serialize().contains("lsplit=2026-11-11"));
        clear_leveling(&mut p);
        assert!(p.tasks[0].lsplits.is_empty());
    }

    #[test]
    fn custom_fields_people_details_and_fixed_units_round_trip() {
        let text = "project F\nstart 2026-11-09\nresource Ana | rate 10 | group Eng/Backend | initials AG | email ana@example.com | note Lead\nfield Region | text\nfield Dept | code | Eng, Eng.Backend, Sales\ntask 1 | A | 2d | | Ana | 0 | work=16h; type=units; x:Region=North; x:Dept=Eng.Backend\n";
        let p = Project::parse(text).unwrap();
        assert_eq!(p.fields.len(), 2);
        assert_eq!(p.fields[1].values, vec!["Eng", "Eng.Backend", "Sales"]);
        assert_eq!(p.tasks[0].field("Dept"), Some("Eng.Backend"));
        assert!(p.tasks[0].fixed_units);
        assert_eq!(p.resources[0].group, "Eng/Backend");
        let again = Project::parse(&p.serialize()).unwrap();
        assert_eq!(again.serialize(), p.serialize());
        assert_eq!(again.tasks[0], p.tasks[0]);
    }
}
