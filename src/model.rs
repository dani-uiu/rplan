//! Project model and the plain-text `.plan` file format.
//!
//! ```text
//! project Website Launch
//! start 2026-11-09
//! currency $
//! week Mon-Fri 08:00-12:00 13:00-17:00
//! week Sat,Sun off
//! dayhours 8
//! holiday 2026-11-26 Thanksgiving
//! holiday 2026-12-24..2026-12-31 Winter break
//! workday 2026-12-05 09:00-13:00 Release Saturday
//! resource Ana | rate 85 | capacity 100 | off 2026-11-20..2026-11-21
//! task 10 | Design phase |
//!   task 2 | Requirements | 3d | 1 | Ana | 100
//!   task 3 | UX design | 5d | 2FS+1d, 1SS | Bo[50%] | 60 | snet=2026-11-12; deadline=2026-11-20; cost=500
//! ```
//! Indentation (2 spaces per level) makes summary tasks. Task columns:
//! id | name | duration | predecessors | resources | % done | attributes

use crate::calendar::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    FS,
    SS,
    FF,
    SF,
}

impl Link {
    pub fn as_str(self) -> &'static str {
        match self {
            Link::FS => "FS",
            Link::SS => "SS",
            Link::FF => "FF",
            Link::SF => "SF",
        }
    }
    pub fn parse(s: &str) -> Option<Link> {
        Some(match s.to_ascii_uppercase().as_str() {
            "" | "FS" => Link::FS,
            "SS" => Link::SS,
            "FF" => Link::FF,
            "SF" => Link::SF,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Dep {
    pub id: u32,
    pub kind: Link,
    /// Working minutes; may be negative (lead).
    pub lag: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assign {
    pub name: String,
    /// Percent of the person's time (100 = full time).
    pub units: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Constraint {
    Asap,
    /// Start no earlier than (minutes).
    Snet(i64),
    /// Finish no earlier than (minutes).
    Fnet(i64),
    /// Must start on (minutes).
    Mso(i64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: u32,
    pub name: String,
    pub level: usize,
    /// Working minutes (0 = milestone). Ignored for summary tasks.
    pub dur: i64,
    pub deps: Vec<Dep>,
    pub who: Vec<Assign>,
    pub pct: u8,
    pub constraint: Constraint,
    /// Day number; the task should finish by the end of this day.
    pub deadline: Option<i64>,
    pub cost: f64,
    /// Leveling delay in working minutes (set by `level`).
    pub delay: i64,
    pub baseline: Option<(i64, i64)>,
    pub note: String,
}

impl Task {
    pub fn new(id: u32, name: &str, dur: i64) -> Task {
        Task {
            id,
            name: name.into(),
            level: 0,
            dur,
            deps: vec![],
            who: vec![],
            pct: 0,
            constraint: Constraint::Asap,
            deadline: None,
            cost: 0.0,
            delay: 0,
            baseline: None,
            note: String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub name: String,
    /// Cost per working hour at 100% units.
    pub rate: f64,
    /// Max units in percent (50 = half time).
    pub capacity: u32,
    /// Personal days off: inclusive day ranges.
    pub off: Vec<(i64, i64)>,
}

impl Resource {
    pub fn new(name: &str) -> Resource {
        Resource { name: name.into(), rate: 0.0, capacity: 100, off: vec![] }
    }
}

#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    /// Day number of the project start.
    pub start: i64,
    pub currency: String,
    pub week: [Hours; 7],
    /// Minutes in one "day" of duration.
    pub day_minutes: i64,
    pub exceptions: Vec<Exception>,
    pub resources: Vec<Resource>,
    pub tasks: Vec<Task>,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            name: "Untitled".into(),
            start: 0,
            currency: "$".into(),
            week: default_week(),
            day_minutes: 480,
            exceptions: vec![],
            resources: vec![],
            tasks: vec![],
        }
    }
}

// ---------- small parsers / formatters ----------

/// Durations: `3` or `3d` (days), `1.5d`, `4h`, `30m`, `2w` (5-day weeks).
pub fn parse_dur(s: &str, day_minutes: i64) -> Result<i64, String> {
    let s = s.trim();
    let bad = || format!("bad duration '{s}' (try 3d, 4h, 30m or 0 for a milestone)");
    if s.is_empty() {
        return Err(bad());
    }
    let (num, unit) = match s.char_indices().find(|(_, c)| c.is_ascii_alphabetic()) {
        Some((i, _)) => (&s[..i], s[i..].trim().to_ascii_lowercase()),
        None => (s, "d".to_string()),
    };
    let n: f64 = num.trim().parse().map_err(|_| bad())?;
    let per = match unit.as_str() {
        "d" | "day" | "days" | "ed" => day_minutes as f64,
        "h" | "hr" | "hrs" | "hour" | "hours" => 60.0,
        "m" | "min" | "mins" => 1.0,
        "w" | "wk" | "week" | "weeks" => day_minutes as f64 * 5.0,
        _ => return Err(bad()),
    };
    let v = (n * per).round() as i64;
    if !n.is_finite() || v.abs() > day_minutes * 365 * 50 {
        return Err(bad());
    }
    Ok(v)
}

pub fn fmt_dur(mins: i64, day_minutes: i64) -> String {
    if mins == 0 {
        return "0".into();
    }
    if day_minutes > 0 && mins % day_minutes == 0 {
        format!("{}d", mins / day_minutes)
    } else if mins % 60 == 0 {
        format!("{}h", mins / 60)
    } else {
        format!("{mins}m")
    }
}

/// Predecessors: `3`, `3SS`, `3FF+2d`, `3-4h`, `3+2` (days). Comma-separated.
pub fn parse_deps(s: &str, day_minutes: i64) -> Result<Vec<Dep>, String> {
    let mut out = vec![];
    for part in s.split(',').map(str::trim).filter(|x| !x.is_empty()) {
        let bad = || format!("bad predecessor '{part}' (examples: 3, 3SS, 3FS+2d, 3FF-4h)");
        let p = part.trim_start_matches('#');
        let digits = p.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            return Err(bad());
        }
        let id: u32 = p[..digits].parse().map_err(|_| bad())?;
        let rest = p[digits..].trim();
        let letters = rest.chars().take_while(char::is_ascii_alphabetic).count();
        let kind = Link::parse(&rest[..letters]).ok_or_else(bad)?;
        let lag_s = rest[letters..].trim();
        let lag = if lag_s.is_empty() {
            0
        } else {
            let (sign, num) = match lag_s.as_bytes()[0] {
                b'+' => (1, &lag_s[1..]),
                b'-' => (-1, &lag_s[1..]),
                _ => return Err(bad()),
            };
            sign * parse_dur(num, day_minutes).map_err(|_| bad())?
        };
        out.push(Dep { id, kind, lag });
    }
    Ok(out)
}

pub fn fmt_deps(deps: &[Dep], day_minutes: i64) -> String {
    deps.iter()
        .map(|d| {
            let k = if d.kind == Link::FS { "" } else { d.kind.as_str() };
            let lag = match d.lag {
                0 => String::new(),
                l if l > 0 => format!("+{}", fmt_dur(l, day_minutes)),
                l => format!("-{}", fmt_dur(-l, day_minutes)),
            };
            format!("{}{k}{lag}", d.id)
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Resources: `Ana, Bo[50%]`.
pub fn parse_who(s: &str) -> Result<Vec<Assign>, String> {
    let mut out: Vec<Assign> = vec![];
    for part in s.split(',').map(str::trim).filter(|x| !x.is_empty()) {
        let (name, units) = match part.split_once('[') {
            Some((n, u)) => {
                let u = u.trim_end_matches(']').trim().trim_end_matches('%');
                (n.trim(), u.parse::<u32>().map_err(|_| format!("bad units in '{part}' (example: Bo[50%])"))?)
            }
            None => (part, 100),
        };
        if name.is_empty() {
            return Err(format!("missing name in '{part}'"));
        }
        if units == 0 || units > 1000 {
            return Err(format!("units in '{part}' must be 1–1000%"));
        }
        if !out.iter().any(|a| a.name == name) {
            out.push(Assign { name: name.to_string(), units });
        }
    }
    Ok(out)
}

pub fn fmt_who(w: &[Assign]) -> String {
    w.iter()
        .map(|a| if a.units == 100 { a.name.clone() } else { format!("{}[{}%]", a.name, a.units) })
        .collect::<Vec<_>>()
        .join(",")
}

/// `2026-11-20` or `2026-11-20..2026-11-22`.
pub fn parse_range(s: &str) -> Result<(i64, i64), String> {
    match s.split_once("..") {
        Some((a, b)) => {
            let (a, b) = (parse_date(a)?, parse_date(b)?);
            if b < a {
                return Err(format!("date range '{s}' ends before it starts"));
            }
            Ok((a, b))
        }
        None => {
            let d = parse_date(s)?;
            Ok((d, d))
        }
    }
}

pub fn fmt_range(a: i64, b: i64) -> String {
    if a == b {
        fmt_date(a)
    } else {
        format!("{}..{}", fmt_date(a), fmt_date(b))
    }
}

fn fmt_when(t: i64) -> String {
    if t.rem_euclid(DAY) == 0 {
        fmt_date(t.div_euclid(DAY))
    } else {
        fmt_datetime(t)
    }
}

fn fmt_num(x: f64) -> String {
    if x.fract() == 0.0 {
        format!("{}", x as i64)
    } else {
        format!("{x:.2}").trim_end_matches('0').to_string()
    }
}

/// Removes characters that would break the line-based format.
pub fn clean(s: &str) -> String {
    s.replace(['|', '\r', '\n', ';'], " ").trim().to_string()
}

fn is_hours_token(s: &str) -> bool {
    s.eq_ignore_ascii_case("off") || (s.contains('-') && s.contains(':') && s.chars().next().is_some_and(|c| c.is_ascii_digit()))
}

// ---------- parse ----------

impl Project {
    pub fn parse(text: &str) -> Result<Project, String> {
        let mut p = Project::default();
        let mut have_start = false;
        for (ln, raw) in text.lines().enumerate() {
            let err = |m: String| format!("line {}: {m}", ln + 1);
            let indent: usize = raw.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 2 } else { 1 }).sum();
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (kw, rest) = line.split_once([' ', '\t']).unwrap_or((line, ""));
            let rest = rest.trim();
            match kw {
                "project" => p.name = rest.to_string(),
                "start" => {
                    p.start = parse_date(rest).map_err(err)?;
                    have_start = true;
                }
                "currency" => p.currency = rest.to_string(),
                "dayhours" => {
                    let h: f64 = rest.parse().map_err(|_| err(format!("bad dayhours '{rest}'")))?;
                    if !(0.25..=24.0).contains(&h) {
                        return Err(err("dayhours must be between 0.25 and 24".into()));
                    }
                    p.day_minutes = (h * 60.0).round() as i64;
                }
                "week" => {
                    let (days, hours) = rest.split_once([' ', '\t']).unwrap_or((rest, "off"));
                    let hours = parse_hours(hours).map_err(err)?;
                    for d in parse_weekdays(days).map_err(err)? {
                        p.week[d] = hours.clone();
                    }
                }
                "holiday" | "workday" => {
                    let mut toks = rest.split_whitespace();
                    let range = toks.next().ok_or_else(|| err("missing date".into()))?;
                    let (from, to) = parse_range(range).map_err(err)?;
                    let toks: Vec<&str> = toks.collect();
                    let nh = toks.iter().take_while(|t| is_hours_token(t)).count();
                    let mut hours = parse_hours(&toks[..nh].join(" ")).map_err(err)?;
                    if kw == "holiday" {
                        hours.clear();
                    } else if hours.is_empty() {
                        hours = p.week.iter().find(|h| !h.is_empty()).cloned().unwrap_or_default();
                    }
                    p.exceptions.push(Exception { from, to, name: toks[nh..].join(" "), hours });
                }
                "resource" => {
                    let f: Vec<&str> = rest.split('|').map(str::trim).collect();
                    let mut r = Resource::new(f[0]);
                    if r.name.is_empty() {
                        return Err(err("resource needs a name".into()));
                    }
                    for kv in &f[1..] {
                        let (k, v) = kv.split_once([' ', '=']).unwrap_or((kv, ""));
                        let v = v.trim();
                        match k {
                            "rate" => r.rate = v.parse().map_err(|_| err(format!("bad rate '{v}'")))?,
                            "capacity" => {
                                r.capacity = v.trim_end_matches('%').parse().map_err(|_| err(format!("bad capacity '{v}'")))?;
                                if r.capacity == 0 {
                                    return Err(err("capacity must be above 0%".into()));
                                }
                            }
                            "off" => {
                                for part in v.split(',').map(str::trim).filter(|x| !x.is_empty()) {
                                    r.off.push(parse_range(part).map_err(err)?);
                                }
                            }
                            "" => {}
                            _ => return Err(err(format!("unknown resource field '{k}'"))),
                        }
                    }
                    p.resources.retain(|x| x.name != r.name);
                    p.resources.push(r);
                }
                "task" => {
                    let f: Vec<&str> = rest.split('|').map(str::trim).collect();
                    if f.len() < 2 {
                        return Err(err("task needs at least: id | name".into()));
                    }
                    let id = f[0].parse().map_err(|_| err(format!("bad task id '{}'", f[0])))?;
                    let get = |i: usize| f.get(i).copied().unwrap_or("");
                    let dur = match get(2) {
                        "" => p.day_minutes,
                        s => parse_dur(s, p.day_minutes).map_err(err)?,
                    };
                    if dur < 0 {
                        return Err(err("duration can't be negative".into()));
                    }
                    let mut t = Task::new(id, get(1), dur);
                    t.level = indent / 2;
                    t.deps = parse_deps(get(3), p.day_minutes).map_err(err)?;
                    t.who = parse_who(get(4)).map_err(err)?;
                    t.pct = match get(5) {
                        "" => 0,
                        s => s.trim_end_matches('%').trim().parse::<u8>().map_err(|_| err(format!("bad % done '{s}'")))?.min(100),
                    };
                    for kv in get(6).split(';').map(str::trim).filter(|x| !x.is_empty()) {
                        let (k, v) = kv.split_once('=').ok_or_else(|| err(format!("bad attribute '{kv}' (expected key=value)")))?;
                        let v = v.trim();
                        match k.trim() {
                            "snet" => t.constraint = Constraint::Snet(parse_datetime(v).map_err(err)?),
                            "fnet" => t.constraint = Constraint::Fnet(parse_datetime(v).map_err(err)?),
                            "mso" => t.constraint = Constraint::Mso(parse_datetime(v).map_err(err)?),
                            "deadline" => t.deadline = Some(parse_date(v).map_err(err)?),
                            "cost" => t.cost = v.parse().map_err(|_| err(format!("bad cost '{v}'")))?,
                            "delay" => t.delay = parse_dur(v, p.day_minutes).map_err(err)?.max(0),
                            "base" => {
                                let (a, b) = v.split_once('/').ok_or_else(|| err("baseline must be START/FINISH".into()))?;
                                t.baseline = Some((parse_datetime(a).map_err(err)?, parse_datetime(b).map_err(err)?));
                            }
                            "note" => t.note = v.to_string(),
                            other => return Err(err(format!("unknown task attribute '{other}'"))),
                        }
                    }
                    p.tasks.push(t);
                }
                _ => return Err(err(format!("unknown keyword '{kw}'"))),
            }
        }
        if !have_start {
            return Err("missing 'start YYYY-MM-DD' line".into());
        }
        p.normalize();
        p.calendar()?;
        Ok(p)
    }

    /// Fixes outline levels so each task is at most one level deeper than the previous one.
    pub fn normalize(&mut self) {
        let mut prev: Option<usize> = None;
        for t in &mut self.tasks {
            t.level = match prev {
                None => 0,
                Some(p) => t.level.min(p + 1),
            };
            prev = Some(t.level);
        }
    }

    pub fn serialize(&self) -> String {
        use std::fmt::Write;
        let dm = self.day_minutes;
        let mut s = String::new();
        let _ = writeln!(s, "project {}", clean(&self.name));
        let _ = writeln!(s, "start {}", fmt_date(self.start));
        if self.currency != "$" {
            let _ = writeln!(s, "currency {}", clean(&self.currency));
        }
        // Group weekdays that share the same hours.
        let mut groups: Vec<(Hours, Vec<usize>)> = vec![];
        for d in [1, 2, 3, 4, 5, 6, 0] {
            match groups.iter_mut().find(|(h, _)| *h == self.week[d]) {
                Some(g) => g.1.push(d),
                None => groups.push((self.week[d].clone(), vec![d])),
            }
        }
        for (h, days) in groups {
            let _ = writeln!(s, "week {} {}", fmt_weekdays(&days), fmt_hours(&h));
        }
        if dm != 480 {
            let _ = writeln!(s, "dayhours {}", fmt_num(dm as f64 / 60.0));
        }
        for e in &self.exceptions {
            let name = if e.name.is_empty() { String::new() } else { format!(" {}", clean(&e.name)) };
            if e.hours.is_empty() {
                let _ = writeln!(s, "holiday {}{name}", fmt_range(e.from, e.to));
            } else {
                let _ = writeln!(s, "workday {} {}{name}", fmt_range(e.from, e.to), fmt_hours(&e.hours));
            }
        }
        for r in &self.resources {
            let mut line = format!("resource {}", clean(&r.name));
            if r.rate != 0.0 {
                let _ = write!(line, " | rate {}", fmt_num(r.rate));
            }
            if r.capacity != 100 {
                let _ = write!(line, " | capacity {}", r.capacity);
            }
            if !r.off.is_empty() {
                let _ = write!(line, " | off {}", r.off.iter().map(|(a, b)| fmt_range(*a, *b)).collect::<Vec<_>>().join(", "));
            }
            let _ = writeln!(s, "{line}");
        }
        let _ = writeln!(s, "# id | name | duration | predecessors | resources | % done | attributes");
        let summary = self.summary_flags();
        for (i, t) in self.tasks.iter().enumerate() {
            let mut attrs: Vec<String> = vec![];
            match t.constraint {
                Constraint::Asap => {}
                Constraint::Snet(x) => attrs.push(format!("snet={}", fmt_when(x))),
                Constraint::Fnet(x) => attrs.push(format!("fnet={}", fmt_when(x))),
                Constraint::Mso(x) => attrs.push(format!("mso={}", fmt_when(x))),
            }
            if let Some(d) = t.deadline {
                attrs.push(format!("deadline={}", fmt_date(d)));
            }
            if t.cost != 0.0 {
                attrs.push(format!("cost={}", fmt_num(t.cost)));
            }
            if t.delay > 0 {
                attrs.push(format!("delay={}", fmt_dur(t.delay, dm)));
            }
            if let Some((a, b)) = t.baseline {
                attrs.push(format!("base={}/{}", fmt_datetime(a), fmt_datetime(b)));
            }
            if !t.note.is_empty() {
                attrs.push(format!("note={}", clean(&t.note)));
            }
            let dur = if summary[i] { String::new() } else { fmt_dur(t.dur, dm) };
            let mut line = format!(
                "{}task {} | {} | {} | {} | {} | {}",
                "  ".repeat(t.level),
                t.id,
                clean(&t.name),
                dur,
                fmt_deps(&t.deps, dm),
                clean(&fmt_who(&t.who)),
                t.pct
            );
            if !attrs.is_empty() {
                let _ = write!(line, " | {}", attrs.join("; "));
            }
            let _ = writeln!(s, "{}", line.trim_end());
        }
        s
    }

    /// `true` for tasks that have children.
    pub fn summary_flags(&self) -> Vec<bool> {
        (0..self.tasks.len()).map(|i| self.tasks.get(i + 1).is_some_and(|n| n.level > self.tasks[i].level)).collect()
    }

    /// Index of each task's parent summary.
    pub fn parents(&self) -> Vec<Option<usize>> {
        let mut stack: Vec<usize> = vec![];
        let mut out = vec![None; self.tasks.len()];
        for (i, t) in self.tasks.iter().enumerate() {
            while stack.last().is_some_and(|&j| self.tasks[j].level >= t.level) {
                stack.pop();
            }
            out[i] = stack.last().copied();
            stack.push(i);
        }
        out
    }

    /// Outline numbers like `1`, `1.2`, `1.2.1`.
    pub fn wbs(&self) -> Vec<String> {
        let mut counters: Vec<usize> = vec![];
        self.tasks
            .iter()
            .map(|t| {
                counters.truncate(t.level + 1);
                while counters.len() < t.level + 1 {
                    counters.push(0);
                }
                counters[t.level] += 1;
                counters.iter().map(usize::to_string).collect::<Vec<_>>().join(".")
            })
            .collect()
    }

    pub fn calendar(&self) -> Result<Calendar, String> {
        Calendar::new(self.week.clone(), &self.exceptions)
    }

    pub fn resource(&self, name: &str) -> Resource {
        self.resources.iter().find(|r| r.name == name).cloned().unwrap_or_else(|| Resource::new(name))
    }

    /// Everyone named on a task or declared as a resource, declared ones first.
    pub fn all_resources(&self) -> Vec<Resource> {
        let mut out = self.resources.clone();
        for t in &self.tasks {
            for a in &t.who {
                if !out.iter().any(|r| r.name == a.name) {
                    out.push(Resource::new(&a.name));
                }
            }
        }
        out
    }

    pub fn next_id(&self) -> u32 {
        self.tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "project Demo
start 2026-11-09
currency €
week Mon-Thu 08:00-12:00 13:00-17:00
week Fri 08:00-12:00
dayhours 7.5
holiday 2026-11-26 Thanksgiving
holiday 2026-12-24..2026-12-31 Winter break
workday 2026-12-05 09:00-13:00 Release Saturday
resource Ana | rate 85.5 | capacity 50 | off 2026-11-20..2026-11-21, 2026-12-01
task 1 | Phase |
  task 2 | A | 3d | | Ana[50%], Bo | 10 | snet=2026-11-10; deadline=2026-11-20; cost=100; note=hi there
  task 3 | B | 4h | 2SS+1d,1 | | | mso=2026-11-12T13:00; base=2026-11-09T08:00/2026-11-10T12:00; delay=2h
task 4 | Done | 0 | 3FF-30m
";

    #[test]
    fn parses_everything() {
        let p = Project::parse(SAMPLE).unwrap();
        assert_eq!(p.currency, "€");
        assert_eq!(p.day_minutes, 450);
        assert_eq!(p.week[5], vec![(480, 720)]);
        assert!(p.week[6].is_empty());
        assert_eq!(p.exceptions.len(), 3);
        assert_eq!(p.exceptions[2].hours, vec![(540, 780)]);
        assert_eq!(p.exceptions[2].name, "Release Saturday");
        assert_eq!(p.resources[0].capacity, 50);
        assert_eq!(p.resources[0].off.len(), 2);
        assert_eq!(p.tasks[1].level, 1);
        assert_eq!(p.tasks[1].who[0], Assign { name: "Ana".into(), units: 50 });
        assert_eq!(p.tasks[2].deps[0], Dep { id: 2, kind: Link::SS, lag: 450 });
        assert_eq!(p.tasks[3].deps[0], Dep { id: 3, kind: Link::FF, lag: -30 });
        assert_eq!(p.tasks[2].dur, 240);
        assert_eq!(p.tasks[2].delay, 120);
        assert_eq!(p.summary_flags(), vec![true, false, false, false]);
        assert_eq!(p.parents(), vec![None, Some(0), Some(0), None]);
        assert_eq!(p.wbs(), vec!["1", "1.1", "1.2", "2"]);
    }

    #[test]
    fn serialize_is_stable() {
        let p = Project::parse(SAMPLE).unwrap();
        let once = p.serialize();
        let twice = Project::parse(&once).unwrap().serialize();
        assert_eq!(once, twice);
        assert!(once.contains("  task 3 | B | 4h | 2SS+1d,1 |  | 0 | mso=2026-11-12T13:00;"));
    }

    #[test]
    fn old_format_still_works() {
        let p = Project::parse("project X\nstart 2026-11-09\nholiday 2026-11-26\ntask 1 | A | 3 | | Ana | 100\ntask 2 | B | 2 | 1+1 | | ").unwrap();
        assert_eq!(p.tasks[0].dur, 480 * 3);
        assert_eq!(p.tasks[1].deps[0].lag, 480);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Project::parse("task 1 | A | 1").is_err());
        assert!(Project::parse("start 2026-11-09\ntask 1 | A | -1").is_err());
        assert!(Project::parse("start 2026-11-09\ntask 1 | A | 1 | 2XY").is_err());
        assert!(Project::parse("start 2026-11-09\nweek Mon-Sun off").is_err());
        assert!(parse_who("Ana[0%]").is_err());
        assert_eq!(parse_dur("1.5d", 480).unwrap(), 720);
        assert_eq!(parse_dur("2w", 480).unwrap(), 4800);
        assert_eq!(fmt_dur(720, 480), "12h");
    }

    #[test]
    fn levels_are_normalized() {
        let p = Project::parse("start 2026-11-09\n    task 1 | A | 1\n      task 2 | B | 1").unwrap();
        assert_eq!(p.tasks[0].level, 0);
        assert_eq!(p.tasks[1].level, 1);
    }
}
