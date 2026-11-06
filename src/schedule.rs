//! Critical Path Method over working time, with summary tasks, FS/SS/FF/SF
//! links, lags, constraints, deadlines, per-person calendars, costs and
//! resource leveling.

use crate::calendar::*;
use crate::model::*;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Copy, Debug, Default)]
pub struct Sched {
    pub es: i64,
    pub ef: i64,
    pub ls: i64,
    pub lf: i64,
    /// Total slack in working minutes (negative = behind a deadline/constraint).
    pub slack: i64,
    pub crit: bool,
    pub summary: bool,
    /// Finishes after its deadline.
    pub late: bool,
    /// A "must start on" date conflicts with its predecessors.
    pub conflict: bool,
    pub cost: f64,
    /// Working minutes between start and finish (summaries) or duration.
    pub span: i64,
    /// Percent complete (summaries: weighted by duration).
    pub pct: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Schedule {
    pub tasks: Vec<Sched>,
    pub start: i64,
    pub finish: i64,
    pub cost: f64,
    pub progress: f64,
    /// Critical leaf tasks in start order.
    pub critical: Vec<u32>,
}

struct LinkE {
    from: usize,
    to: usize,
    kind: Link,
    lag: i64,
}

fn end_of_day(t: i64) -> i64 {
    if t.rem_euclid(DAY) == 0 {
        t + DAY
    } else {
        t
    }
}

/// Calendar for a task: project calendar minus its people's days off.
pub fn task_calendars(p: &Project, base: &Calendar) -> Vec<Calendar> {
    let mut cache: HashMap<String, Calendar> = HashMap::new();
    p.tasks
        .iter()
        .map(|t| {
            let res: Vec<Resource> = t.who.iter().map(|a| p.resource(&a.name)).filter(|r| !r.off.is_empty()).collect();
            if res.is_empty() {
                return base.clone();
            }
            let mut key: Vec<&str> = res.iter().map(|r| r.name.as_str()).collect();
            key.sort();
            let key = key.join("\u{1}");
            cache
                .entry(key)
                .or_insert_with(|| {
                    let mut c = base.clone();
                    for r in &res {
                        for &(a, b) in &r.off {
                            c.add_exception(&Exception { from: a, to: b, name: String::new(), hours: vec![] });
                        }
                    }
                    c
                })
                .clone()
        })
        .collect()
}

pub fn schedule(p: &Project) -> Result<Schedule, String> {
    let n = p.tasks.len();
    let cal = p.calendar()?;
    let cals = task_calendars(p, &cal);
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

    // Effective links: a link to a summary applies to every task inside it.
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
            let mut targets = vec![];
            leaves(ti, &children, &mut targets);
            for to in targets {
                links.push(LinkE { from: pi, to, kind: d.kind, lag: d.lag });
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

    let shift = |t: i64, lag: i64| if lag >= 0 { cal.add(t, lag) } else { cal.sub(t, -lag) };
    let unshift = |t: i64, lag: i64| if lag >= 0 { cal.sub(t, lag) } else { cal.add(t, -lag) };
    let t0 = cal.next_work(p.start * DAY);
    let mut s = vec![Sched::default(); n];

    // Forward pass.
    for &i in &order {
        let t = &p.tasks[i];
        if summary[i] {
            s[i].summary = true;
            s[i].es = children[i].iter().map(|&c| s[c].es).min().unwrap_or(t0);
            s[i].ef = children[i].iter().map(|&c| s[c].ef).max().unwrap_or(t0);
            continue;
        }
        let tc = &cals[i];
        let mut start_b = t0;
        let mut finish_b = i64::MIN;
        for &k in &incoming[i] {
            let l = &links[k];
            let q = s[l.from];
            match l.kind {
                Link::FS => start_b = start_b.max(shift(q.ef, l.lag)),
                Link::SS => start_b = start_b.max(shift(q.es, l.lag)),
                Link::FF => finish_b = finish_b.max(shift(q.ef, l.lag)),
                Link::SF => finish_b = finish_b.max(shift(q.es, l.lag)),
            }
        }
        let mut chain = Some(i);
        while let Some(c) = chain {
            match p.tasks[c].constraint {
                Constraint::Snet(x) => start_b = start_b.max(x),
                Constraint::Fnet(x) if c == i => finish_b = finish_b.max(end_of_day(x)),
                _ => {}
            }
            chain = parents[c];
        }
        if finish_b > i64::MIN {
            let sb = if t.dur > 0 { tc.sub(tc.prev_work(finish_b), t.dur) } else { finish_b };
            start_b = start_b.max(sb);
        }
        let mut es = if t.dur > 0 { tc.next_work(start_b) } else { start_b };
        if let Constraint::Mso(x) = t.constraint {
            let forced = if t.dur > 0 { tc.next_work(x) } else { x };
            s[i].conflict = es > forced;
            es = forced;
        }
        if t.delay > 0 {
            es = tc.next_work(tc.add(es, t.delay));
        }
        s[i].es = es;
        s[i].ef = if t.dur > 0 { tc.add(es, t.dur) } else { es };
    }
    let finish = (0..n).filter(|&i| !summary[i]).map(|i| s[i].ef).max().unwrap_or(t0);

    // Backward pass. Summaries pass their successors' limits down to their children.
    let mut limits: Vec<(i64, i64)> = vec![(i64::MAX, i64::MAX); n];
    for &i in order.iter().rev() {
        let t = &p.tasks[i];
        let (mut lf_b, mut ls_b) = parents[i].map(|j| limits[j]).unwrap_or((i64::MAX, i64::MAX));
        for &k in &outgoing[i] {
            let l = &links[k];
            let q = s[l.to];
            match l.kind {
                Link::FS => lf_b = lf_b.min(unshift(q.ls, l.lag)),
                Link::SS => ls_b = ls_b.min(unshift(q.ls, l.lag)),
                Link::FF => lf_b = lf_b.min(unshift(q.lf, l.lag)),
                Link::SF => ls_b = ls_b.min(unshift(q.lf, l.lag)),
            }
        }
        if let Some(d) = t.deadline {
            lf_b = lf_b.min((d + 1) * DAY);
        }
        if summary[i] {
            limits[i] = (lf_b, ls_b);
            continue;
        }
        let tc = &cals[i];
        lf_b = lf_b.min(finish);
        if let Constraint::Mso(_) = t.constraint {
            s[i].ls = s[i].es;
            s[i].lf = s[i].ef;
        } else if t.dur > 0 {
            let mut lf = tc.prev_work(lf_b);
            if ls_b < i64::MAX {
                lf = lf.min(tc.add(tc.next_work(ls_b), t.dur));
            }
            s[i].lf = lf;
            s[i].ls = tc.sub(lf, t.dur);
        } else {
            let lf = lf_b.min(ls_b);
            s[i].lf = lf;
            s[i].ls = lf;
        }
        s[i].slack = tc.work_between(s[i].es, s[i].ls);
        s[i].crit = s[i].slack <= 0;
        s[i].span = t.dur;
        s[i].pct = t.pct as f64;
        let res_cost: f64 = t.who.iter().map(|a| p.resource(&a.name).rate * a.units as f64 / 100.0 * t.dur as f64 / 60.0).sum();
        s[i].cost = t.cost + res_cost;
    }

    // Roll up summaries (children come first in `order`).
    let mut weight = vec![0f64; n];
    for &i in &order {
        if !summary[i] {
            weight[i] = p.tasks[i].dur.max(1) as f64;
            continue;
        }
        let ch = &children[i];
        s[i].ls = ch.iter().map(|&c| s[c].ls).min().unwrap_or(s[i].es);
        s[i].lf = ch.iter().map(|&c| s[c].lf).max().unwrap_or(s[i].ef);
        s[i].slack = ch.iter().map(|&c| s[c].slack).min().unwrap_or(0);
        s[i].crit = s[i].slack <= 0;
        s[i].span = cal.work_between(s[i].es, s[i].ef);
        s[i].cost = p.tasks[i].cost + ch.iter().map(|&c| s[c].cost).sum::<f64>();
        weight[i] = ch.iter().map(|&c| weight[c]).sum();
        let done: f64 = ch.iter().map(|&c| weight[c] * s[c].pct).sum();
        s[i].pct = if weight[i] > 0.0 { done / weight[i] } else { 0.0 };
    }
    for (i, t) in p.tasks.iter().enumerate() {
        s[i].late = t.deadline.is_some_and(|d| s[i].ef > (d + 1) * DAY);
    }

    let leaves_all: Vec<usize> = (0..n).filter(|&i| !summary[i]).collect();
    let total: f64 = leaves_all.iter().map(|&i| weight[i]).sum();
    let progress = if total > 0.0 { leaves_all.iter().map(|&i| weight[i] * s[i].pct).sum::<f64>() / total } else { 0.0 };
    let mut crit: Vec<usize> = leaves_all.iter().copied().filter(|&i| s[i].crit).collect();
    crit.sort_by_key(|&i| (s[i].es, s[i].ef, i));
    Ok(Schedule {
        start: t0,
        finish,
        cost: (0..n).filter(|&i| parents[i].is_none()).map(|i| s[i].cost).sum(),
        progress,
        critical: crit.iter().map(|&i| p.tasks[i].id).collect(),
        tasks: s,
    })
}

// ---------- resources ----------

#[derive(Clone, Debug, Default)]
pub struct Load {
    pub name: String,
    pub capacity: u32,
    /// Day number → (peak units that day, task ids).
    pub days: BTreeMap<i64, (u32, Vec<u32>)>,
    pub over_days: usize,
    /// Work in minutes (duration × units).
    pub work: i64,
    pub cost: f64,
    pub tasks: usize,
}

/// Per-person workload by day, using peak simultaneous units each day.
pub fn loads(p: &Project, s: &Schedule) -> Vec<Load> {
    let summary = p.summary_flags();
    let cal = p.calendar().ok();
    let cals = cal.as_ref().map(|c| task_calendars(p, c));
    let mut out = vec![];
    for r in p.all_resources() {
        let mut l = Load { name: r.name.clone(), capacity: r.capacity, ..Default::default() };
        let mut per_day: BTreeMap<i64, Vec<(i64, i64, u32, u32)>> = BTreeMap::new();
        for (i, t) in p.tasks.iter().enumerate() {
            let Some(a) = t.who.iter().find(|a| a.name == r.name) else { continue };
            if summary[i] {
                continue;
            }
            l.tasks += 1;
            l.work += t.dur * a.units as i64 / 100;
            if t.dur == 0 {
                continue;
            }
            let sc = s.tasks[i];
            for day in sc.es.div_euclid(DAY)..=(sc.ef - 1).div_euclid(DAY) {
                let (a0, b0) = (sc.es.max(day * DAY), sc.ef.min((day + 1) * DAY));
                let works = cals.as_ref().map_or(true, |c| c[i].work_between(a0, b0) > 0);
                if works {
                    per_day.entry(day).or_default().push((a0, b0, a.units, t.id));
                }
            }
        }
        l.cost = r.rate * l.work as f64 / 60.0;
        for (day, iv) in per_day {
            let mut peak = 0;
            for &(a, _, _, _) in &iv {
                let u: u32 = iv.iter().filter(|(x, y, _, _)| *x <= a && a < *y).map(|v| v.2).sum();
                peak = peak.max(u);
            }
            if peak > r.capacity {
                l.over_days += 1;
            }
            let mut ids: Vec<u32> = iv.iter().map(|v| v.3).collect();
            ids.dedup();
            l.days.insert(day, (peak, ids));
        }
        out.push(l);
    }
    out
}

/// Delays tasks until nobody is booked beyond their capacity.
/// Earlier, more critical work keeps its place. Returns (task id, delay) for delayed tasks.
pub fn level(p: &mut Project) -> Result<Vec<(u32, i64)>, String> {
    for t in &mut p.tasks {
        t.delay = 0;
    }
    let summary = p.summary_flags();
    let people = p.all_resources();
    let mut ignored: HashSet<(i64, String)> = HashSet::new();
    for _ in 0..5000 {
        let s = schedule(p)?;
        let mut found: Option<(i64, String, u32, Vec<(usize, u32)>)> = None;
        for r in &people {
            let mine: Vec<(usize, u32)> = p
                .tasks
                .iter()
                .enumerate()
                .filter(|(i, t)| !summary[*i] && t.dur > 0)
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
                    found = Some((at, r.name.clone(), r.capacity, active));
                    break;
                }
            }
        }
        let Some((at, who, _cap, mut active)) = found else { break };
        let pinned_flags: Vec<bool> = p.tasks.iter().map(|t| matches!(t.constraint, Constraint::Mso(_))).collect();
        let pinned = |i: usize| pinned_flags[i];
        // Most flexible first: on ties, delay the task with the most slack.
        active.sort_by_key(|&(i, _)| (std::cmp::Reverse(s.tasks[i].slack), std::cmp::Reverse(s.tasks[i].es), std::cmp::Reverse(p.tasks[i].id)));
        // Try delaying each clashing task until the others free up; keep the
        // choice that moves the project finish the least.
        let cals = task_calendars(p, &p.calendar()?);
        let mut best: Option<(i64, usize, i64)> = None;
        for &(v, _) in &active {
            if pinned(v) {
                continue;
            }
            let target = active.iter().filter(|(k, _)| *k != v).map(|(k, _)| s.tasks[*k].ef).filter(|&e| e > s.tasks[v].es).min();
            let Some(target) = target else { continue };
            let add = cals[v].work_between(s.tasks[v].es, target).max(1);
            p.tasks[v].delay += add;
            let finish = schedule(p).map(|x| x.finish).unwrap_or(i64::MAX);
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

pub fn set_baseline(p: &mut Project) -> Result<(), String> {
    let s = schedule(p)?;
    for (t, sc) in p.tasks.iter_mut().zip(&s.tasks) {
        t.baseline = Some((sc.es, sc.ef));
    }
    Ok(())
}

pub fn clear_baseline(p: &mut Project) {
    for t in &mut p.tasks {
        t.baseline = None;
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

    #[test]
    fn critical_path_with_slack() {
        let (p, s) = sched(crate::DEMO);
        assert_eq!(at(&p, &s, 4).slack, 3 * 480);
        assert!(at(&p, &s, 5).crit);
        assert_eq!(s.critical, vec![1, 2, 3, 5, 7, 8, 9]);
        // Thanksgiving (Nov 26) is skipped; launch milestone sits at QA's finish.
        assert_eq!(dt(at(&p, &s, 9).es), "2026-12-08T17:00");
        assert_eq!(dt(at(&p, &s, 1).es), "2026-11-09T08:00");
    }

    #[test]
    fn link_types_and_lags() {
        let base = "start 2026-11-09\ntask 1 | A | 2d\n";
        let (p, s) = sched(&format!("{base}task 2 | B | 1d | 1SS+4h"));
        assert_eq!(dt(at(&p, &s, 2).es), "2026-11-09T13:00");
        let (p, s) = sched(&format!("{base}task 2 | B | 1d | 1FF"));
        assert_eq!(dt(at(&p, &s, 2).ef), "2026-11-10T17:00");
        assert_eq!(dt(at(&p, &s, 2).es), "2026-11-10T08:00");
        let (p, s) = sched(&format!("{base}task 2 | B | 1d | 1-1d"));
        assert_eq!(dt(at(&p, &s, 2).es), "2026-11-10T08:00");
        let (p, s) = sched(&format!("{base}task 2 | B | 4h | 1SF+1d"));
        assert_eq!(dt(at(&p, &s, 2).ef), "2026-11-09T17:00");
    }

    #[test]
    fn summaries_roll_up_and_pass_links() {
        let (p, s) = sched(
            "start 2026-11-09
task 1 | Phase |
  task 2 | A | 2d | | Ana | 100
  task 3 | B | 3d | | Bo | 0
task 4 | After | 1d | 1",
        );
        let ph = at(&p, &s, 1);
        assert!(ph.summary);
        assert_eq!(dt(ph.es), "2026-11-09T08:00");
        assert_eq!(dt(ph.ef), "2026-11-11T17:00");
        assert_eq!(dt(at(&p, &s, 4).es), "2026-11-12T08:00");
        assert!((ph.pct - 40.0).abs() < 0.01);
        assert_eq!(at(&p, &s, 2).slack, 480);
        // a link *to* a summary applies to its children
        let (p, s) = sched("start 2026-11-09\ntask 1 | First | 1d\ntask 2 | Phase | | 1\n  task 3 | A | 1d\n  task 4 | B | 1d");
        assert_eq!(dt(at(&p, &s, 3).es), "2026-11-10T08:00");
        assert_eq!(dt(at(&p, &s, 4).es), "2026-11-10T08:00");
    }

    #[test]
    fn constraints_deadlines_and_errors() {
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 1d | | | | snet=2026-11-11");
        assert_eq!(dt(at(&p, &s, 1).es), "2026-11-11T08:00");
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 1d | | | | fnet=2026-11-12");
        assert_eq!(dt(at(&p, &s, 1).ef), "2026-11-12T17:00");
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 3d\ntask 2 | B | 1d | 1 | | | mso=2026-11-10");
        assert!(at(&p, &s, 2).conflict);
        let (p, s) = sched("start 2026-11-09\ntask 1 | A | 5d | | | | deadline=2026-11-11");
        assert!(at(&p, &s, 1).late);
        assert_eq!(at(&p, &s, 1).slack, -2 * 480);
        for bad in [
            "start 2026-11-09\ntask 1 | A | 1d | 2\ntask 2 | B | 1d | 1",
            "start 2026-11-09\ntask 1 | A | 1d | 9",
            "start 2026-11-09\ntask 1 | P |\n  task 2 | A | 1d | 1",
            "start 2026-11-09\ntask 1 | A | 1d\ntask 1 | B | 1d",
        ] {
            assert!(schedule(&Project::parse(bad).unwrap()).is_err(), "{bad}");
        }
    }

    #[test]
    fn resource_calendars_costs_and_hours() {
        let (p, s) = sched(
            "start 2026-11-09
week Mon-Fri 09:00-13:00
dayhours 4
resource Ana | rate 50 | off 2026-11-10
task 1 | A | 2d | | Ana
task 2 | B | 2d | | Bo",
        );
        assert_eq!(dt(at(&p, &s, 1).ef), "2026-11-11T13:00");
        assert_eq!(dt(at(&p, &s, 2).ef), "2026-11-10T13:00");
        assert!((at(&p, &s, 1).cost - 400.0).abs() < 0.01);
        assert!((s.cost - 400.0).abs() < 0.01);
    }

    #[test]
    fn leveling_delays_instead_of_linking() {
        let mut p = Project::parse(crate::DEMO).unwrap();
        let before = schedule(&p).unwrap().finish;
        let l = loads(&p, &schedule(&p).unwrap());
        assert!(l.iter().find(|x| x.name == "Ana").unwrap().over_days > 0);
        let changed = level(&mut p).unwrap();
        assert!(!changed.is_empty());
        let s = schedule(&p).unwrap();
        assert!(loads(&p, &s).iter().all(|x| x.over_days == 0), "still over-allocated");
        assert!(p.tasks.iter().all(|t| t.deps.len() <= 2), "leveling must not add links");
        // Ana has 18 days of work; the best plan costs one extra day.
        let slip = p.calendar().unwrap().work_between(before, s.finish);
        assert!(slip <= 480, "leveling pushed the finish {slip} minutes");
        assert_eq!(clear_leveling(&mut p), changed.len());
        // Half-time people can do two half-time tasks at once.
        let mut q = Project::parse("start 2026-11-09\ntask 1 | A | 1d | | Ana[50%]\ntask 2 | B | 1d | | Ana[50%]").unwrap();
        assert!(level(&mut q).unwrap().is_empty());
    }

    #[test]
    fn baseline_roundtrip() {
        let mut p = Project::parse(crate::DEMO).unwrap();
        set_baseline(&mut p).unwrap();
        let q = Project::parse(&p.serialize()).unwrap();
        assert_eq!(p.tasks[3].baseline, q.tasks[3].baseline);
        clear_baseline(&mut p);
        assert!(p.tasks.iter().all(|t| t.baseline.is_none()));
    }
}
