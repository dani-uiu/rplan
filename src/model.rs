//! Project model and the plain-text `.plan` file format.
//!
//! ```text
//! project Website Launch
//! start 2026-11-09
//! status 2026-11-20
//! currency $
//! pool team.plan
//! week Mon-Fri 08:00-12:00 13:00-17:00
//! week Sat,Sun off
//! dayhours 8
//! holiday 2026-11-26 Thanksgiving
//! holiday every 12-25 observed Christmas
//! workday 2026-12-05 09:00-13:00 Release Saturday
//! calendar Night shift | week Mon-Fri 22:00-24:00 | week Sat,Sun off
//! resource Ana | rate 85 | capacity 100 | overtime 120 | peruse 50 | week Mon-Wed 08:00-17:00 | off 2026-11-20
//! resource Cement | type material | unit bags | rate 9.5
//! resource Travel | type cost
//! task 10 | Design phase |
//!   task 2 | Requirements | 3d | | Ana | 100 | as=2026-11-09T08:00; af=2026-11-11T17:00
//!   task 3 | UX design | 5d | 2FS+1d | Bo[50%], Cement[20], Travel[$400] | 60 | work=40h; snet=2026-11-12; deadline=2026-11-20
//! ```
//! Indentation (2 spaces per level) makes summary tasks. Task columns:
//! id | name | duration | predecessors | resources | % done | attributes

use crate::calendar::*;
use std::borrow::Cow;

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
    /// Minutes; may be negative (lead). Working time unless `elapsed`.
    pub lag: i64,
    pub elapsed: bool,
    /// Lag as a percent of the predecessor's duration (`3FS+50%`); replaces `lag`.
    pub pct: Option<i32>,
}

/// How an assignment's number was written: `Bo[50%]`, `Cement[20]`, `Travel[$400]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Form {
    Pct,
    Qty,
    Money,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Assign {
    pub name: String,
    /// Percent of a person's time (100 = full time).
    pub units: u32,
    /// Quantity (materials) or money (cost resources).
    pub amount: f64,
    pub form: Form,
    /// Working minutes after the task starts before this person starts on it.
    pub delay: i64,
    /// This person's own work (minutes); their part lasts work ÷ units.
    pub work: Option<i64>,
    /// How their work is spread over their part of the task.
    pub contour: Contour,
}

/// Work contours: how an assignment's hours are spread over its time.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Contour {
    #[default]
    Flat,
    Front,
    Back,
    Bell,
    Early,
    Late,
    Turtle,
}

impl Contour {
    pub const ALL: [Contour; 7] = [Contour::Flat, Contour::Front, Contour::Back, Contour::Bell, Contour::Early, Contour::Late, Contour::Turtle];
    pub fn code(self) -> &'static str {
        match self {
            Contour::Flat => "flat",
            Contour::Front => "front",
            Contour::Back => "back",
            Contour::Bell => "bell",
            Contour::Early => "early",
            Contour::Late => "late",
            Contour::Turtle => "turtle",
        }
    }
    pub fn parse(s: &str) -> Option<Contour> {
        Contour::ALL.into_iter().find(|c| c.code() == s.to_ascii_lowercase().trim_end_matches("-loaded").trim_end_matches("-peak"))
    }
    /// Relative intensity at position x (0 = start, 1 = end of the assignment).
    pub fn weight(self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        let pi = std::f64::consts::PI;
        match self {
            Contour::Flat => 1.0,
            Contour::Front => 1.0 - 0.8 * x,
            Contour::Back => 0.2 + 0.8 * x,
            Contour::Bell => 0.15 + 0.85 * (pi * x).sin(),
            Contour::Early => if x < 0.25 { 0.3 + 0.7 * x / 0.25 } else { 1.0 - 0.8 * (x - 0.25) / 0.75 },
            Contour::Late => Contour::Early.weight(1.0 - x),
            Contour::Turtle => 0.5 + 0.5 * (pi * x).sin(),
        }
    }
}

impl Assign {
    pub fn person(name: &str, units: u32) -> Assign {
        Assign { name: name.into(), units, amount: units as f64, form: Form::Pct, delay: 0, work: None, contour: Contour::Flat }
    }
    /// True if this assignment changes when or how its person works (not just units).
    pub fn detailed(&self) -> bool {
        self.delay > 0 || self.work.is_some() || self.contour != Contour::Flat
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Constraint {
    Asap,
    /// As late as possible (without delaying the project).
    Alap,
    /// Start no earlier than (minutes).
    Snet(i64),
    /// Start no later than.
    Snlt(i64),
    /// Finish no earlier than.
    Fnet(i64),
    /// Finish no later than.
    Fnlt(i64),
    /// Must start on.
    Mso(i64),
    /// Must finish on.
    Mfo(i64),
}

impl Constraint {
    pub fn code(&self) -> &'static str {
        match self {
            Constraint::Asap => "asap",
            Constraint::Alap => "alap",
            Constraint::Snet(_) => "snet",
            Constraint::Snlt(_) => "snlt",
            Constraint::Fnet(_) => "fnet",
            Constraint::Fnlt(_) => "fnlt",
            Constraint::Mso(_) => "mso",
            Constraint::Mfo(_) => "mfo",
        }
    }
    pub fn date(&self) -> Option<i64> {
        match *self {
            Constraint::Snet(x) | Constraint::Snlt(x) | Constraint::Fnet(x) | Constraint::Fnlt(x) | Constraint::Mso(x) | Constraint::Mfo(x) => Some(x),
            _ => None,
        }
    }
    pub fn from_code(code: &str, at: Option<i64>) -> Option<Constraint> {
        Some(match (code, at) {
            ("asap", _) => Constraint::Asap,
            ("alap", _) => Constraint::Alap,
            ("snet", Some(x)) => Constraint::Snet(x),
            ("snlt", Some(x)) => Constraint::Snlt(x),
            ("fnet", Some(x)) => Constraint::Fnet(x),
            ("fnlt", Some(x)) => Constraint::Fnlt(x),
            ("mso", Some(x)) => Constraint::Mso(x),
            ("mfo", Some(x)) => Constraint::Mfo(x),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Baseline {
    pub start: i64,
    pub finish: i64,
    pub cost: f64,
    /// Work minutes (duration × units, summed over people).
    pub work: i64,
}

pub const BASELINES: usize = 11;

/// A predecessor in another project file: `ext=Other.plan#5FS+1d`.
#[derive(Clone, Debug, PartialEq)]
pub struct XDep {
    pub file: String,
    pub id: u32,
    pub kind: Link,
    pub lag: i64,
}

/// An external predecessor resolved at load time (not saved).
#[derive(Clone, Debug, PartialEq)]
pub struct ExtLink {
    pub kind: Link,
    pub lag: i64,
    pub es: i64,
    pub ef: i64,
    pub label: String,
}

/// A sub-project summary resolved at load time (not saved).
#[derive(Clone, Debug, PartialEq)]
pub struct SubInfo {
    pub name: String,
    pub span: i64,
    pub cost: f64,
    pub pct: f64,
    pub tasks: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: u32,
    pub name: String,
    pub level: usize,
    /// Minutes (0 = milestone). Working time unless `elapsed`. Ignored for summaries
    /// and recomputed for effort-driven tasks (`work`).
    pub dur: i64,
    pub elapsed: bool,
    pub deps: Vec<Dep>,
    pub who: Vec<Assign>,
    pub pct: u8,
    pub constraint: Constraint,
    /// Day number; the task should finish by the end of this day.
    pub deadline: Option<i64>,
    pub cost: f64,
    /// Leveling delay in working minutes (set by `level`).
    pub delay: i64,
    /// Baseline 0 is "the" baseline; 1–10 are extra snapshots.
    pub baselines: Vec<Option<Baseline>>,
    pub note: String,
    /// Effort-driven: total work in minutes; duration = work ÷ assigned units.
    pub work: Option<i64>,
    pub actual_start: Option<i64>,
    pub actual_finish: Option<i64>,
    pub actual_cost: Option<f64>,
    /// Overtime minutes per assigned person.
    pub overtime: i64,
    /// Paused day ranges (the task is split around them).
    pub splits: Vec<(i64, i64)>,
    /// Manually scheduled: fixed start time, ignores links.
    pub manual: Option<i64>,
    pub inactive: bool,
    /// Leveling priority, 0–1000 (higher = keep its dates).
    pub priority: u32,
    /// Named calendar (instead of the project calendar).
    pub cal: Option<String>,
    pub xdeps: Vec<XDep>,
    /// Another .plan file this task stands for.
    pub sub: Option<String>,
    /// Fixed-units task (with `work`): changing the work changes the duration, the
    /// people's units stay as they are.
    pub fixed_units: bool,
    /// Custom field values (`x:Region=North`), by field name.
    pub fields: Vec<(String, String)>,
    /// Pauses added by resource leveling (cleared with the leveling).
    pub lsplits: Vec<(i64, i64)>,
    // --- resolved at load time, never saved ---
    pub ext: Vec<ExtLink>,
    pub sub_info: Option<SubInfo>,
}

impl Task {
    pub fn new(id: u32, name: &str, dur: i64) -> Task {
        Task {
            id,
            name: name.into(),
            level: 0,
            dur,
            elapsed: false,
            deps: vec![],
            who: vec![],
            pct: 0,
            constraint: Constraint::Asap,
            deadline: None,
            cost: 0.0,
            delay: 0,
            baselines: vec![None; BASELINES],
            note: String::new(),
            work: None,
            actual_start: None,
            actual_finish: None,
            actual_cost: None,
            overtime: 0,
            splits: vec![],
            manual: None,
            inactive: false,
            priority: 500,
            cal: None,
            xdeps: vec![],
            sub: None,
            fixed_units: false,
            fields: vec![],
            lsplits: vec![],
            ext: vec![],
            sub_info: None,
        }
    }
    pub fn baseline(&self) -> Option<Baseline> {
        self.baselines.first().copied().flatten()
    }
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResKind {
    Work,
    Material,
    Cost,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Resource {
    pub name: String,
    pub kind: ResKind,
    /// Work: cost per hour at 100%. Material: cost per unit. Cost: unused.
    pub rate: f64,
    /// Max units in percent (50 = half time).
    pub capacity: u32,
    /// Personal days off: inclusive day ranges.
    pub off: Vec<(i64, i64)>,
    pub per_use: f64,
    /// Cost per overtime hour.
    pub overtime: f64,
    /// Material label, e.g. "bags".
    pub unit: String,
    /// Personal working week (otherwise the project's).
    pub week: Option<[Hours; 7]>,
    /// Comes from the shared team file (`pool`), not this project.
    pub pooled: bool,
    /// Later rates: from this day on, this rate (sorted by day).
    pub rates: Vec<(i64, f64)>,
    /// Later capacities: from this day on, this many percent (sorted by day).
    pub caps: Vec<(i64, u32)>,
    /// When the cost counts: at the start, at the end, or as the work is done.
    pub accrue: Accrue,
    pub group: String,
    pub initials: String,
    pub email: String,
    pub note: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Accrue {
    Start,
    #[default]
    Prorated,
    End,
}

impl Accrue {
    pub fn code(self) -> &'static str {
        match self {
            Accrue::Start => "start",
            Accrue::Prorated => "prorated",
            Accrue::End => "end",
        }
    }
}

impl Resource {
    pub fn new(name: &str) -> Resource {
        Resource {
            name: name.into(),
            kind: ResKind::Work,
            rate: 0.0,
            capacity: 100,
            off: vec![],
            per_use: 0.0,
            overtime: 0.0,
            unit: String::new(),
            week: None,
            pooled: false,
            rates: vec![],
            caps: vec![],
            accrue: Accrue::Prorated,
            group: String::new(),
            initials: String::new(),
            email: String::new(),
            note: String::new(),
        }
    }
    /// Cost per hour (or per unit) on a given day.
    pub fn rate_on(&self, day: i64) -> f64 {
        self.rates.iter().rev().find(|(d, _)| *d <= day).map_or(self.rate, |x| x.1)
    }
    /// Capacity in percent on a given day.
    pub fn capacity_on(&self, day: i64) -> u32 {
        self.caps.iter().rev().find(|(d, _)| *d <= day).map_or(self.capacity, |x| x.1)
    }
}

/// A custom field: `field Region | text`, `field Dept | code | Eng, Eng.Backend, Sales`.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldDef {
    pub name: String,
    /// text, number, date, flag or code (an outline code: values like `Eng.Backend`).
    pub kind: String,
    /// Allowed values (outline codes and pick lists).
    pub values: Vec<String>,
}

pub const FIELD_KINDS: [&str; 5] = ["text", "number", "date", "flag", "code"];

/// How resource leveling works: `leveling slack split order=id`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LevelOpts {
    /// Only use slack: never push the project's finish date.
    pub slack_only: bool,
    /// Allow pausing a task that's already under way instead of delaying all of it.
    pub split: bool,
    pub order: LevelOrder,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum LevelOrder {
    /// Priority, then the most slack moves first (whatever delays the finish least).
    #[default]
    Standard,
    /// Priority, then the later task (by id) moves first.
    Priority,
    /// Only the task id: the later task moves first.
    Id,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamedCal {
    pub name: String,
    pub week: [Hours; 7],
}

#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    /// Day number of the project start.
    pub start: i64,
    /// Progress is measured as of the end of this day.
    pub status: Option<i64>,
    pub currency: String,
    pub week: [Hours; 7],
    /// Minutes in one "day" of duration.
    pub day_minutes: i64,
    pub exceptions: Vec<Exception>,
    pub recurring: Vec<Recurring>,
    pub calendars: Vec<NamedCal>,
    pub pool: Option<String>,
    pub resources: Vec<Resource>,
    pub tasks: Vec<Task>,
    pub fields: Vec<FieldDef>,
    pub leveling: LevelOpts,
    /// Problems found while loading linked files (not saved).
    pub warnings: Vec<String>,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            name: "Untitled".into(),
            start: 0,
            status: None,
            currency: "$".into(),
            week: default_week(),
            day_minutes: 480,
            exceptions: vec![],
            recurring: vec![],
            calendars: vec![],
            pool: None,
            resources: vec![],
            tasks: vec![],
            fields: vec![],
            leveling: LevelOpts::default(),
            warnings: vec![],
        }
    }
}

// ---------- small parsers / formatters ----------

/// Durations: `3`/`3d` (days), `1.5d`, `4h`, `30m`, `2w`; elapsed: `3ed`, `12eh`, `1ew`.
pub fn parse_dur_e(s: &str, day_minutes: i64) -> Result<(i64, bool), String> {
    let s = s.trim().trim_end_matches('?');
    let bad = || format!("bad duration '{s}' (try 3d, 4h, 30m, 2w, 3ed or 0 for a milestone)");
    if s.is_empty() {
        return Err(bad());
    }
    let (num, unit) = match s.char_indices().find(|(_, c)| c.is_ascii_alphabetic()) {
        Some((i, _)) => (&s[..i], s[i..].trim().to_ascii_lowercase()),
        None => (s, "d".to_string()),
    };
    let n: f64 = num.trim().parse().map_err(|_| bad())?;
    let (per, elapsed) = match unit.as_str() {
        "d" | "day" | "days" => (day_minutes as f64, false),
        "h" | "hr" | "hrs" | "hour" | "hours" => (60.0, false),
        "m" | "min" | "mins" => (1.0, false),
        "w" | "wk" | "week" | "weeks" => (day_minutes as f64 * 5.0, false),
        // A month of duration is 20 working days; an elapsed month is 30 days.
        "mo" | "mon" | "month" | "months" => (day_minutes as f64 * 20.0, false),
        "emo" | "emonth" | "emonths" => (43200.0, true),
        "ed" | "edays" => (1440.0, true),
        "eh" | "ehr" | "ehours" => (60.0, true),
        "em" | "emin" => (1.0, true),
        "ew" | "eweeks" => (10080.0, true),
        _ => return Err(bad()),
    };
    let v = (n * per).round() as i64;
    if !n.is_finite() || v.abs() > 1440 * 365 * 50 {
        return Err(bad());
    }
    Ok((v, elapsed))
}

pub fn parse_dur(s: &str, day_minutes: i64) -> Result<i64, String> {
    parse_dur_e(s, day_minutes).map(|(v, _)| v)
}

pub fn fmt_dur(mins: i64, day_minutes: i64) -> String {
    fmt_dur_e(mins, false, day_minutes)
}

/// Effort is always written in hours (`40h`) or minutes, independent of the day length.
pub fn fmt_effort(mins: i64) -> String {
    if mins % 60 == 0 {
        format!("{}h", mins / 60)
    } else {
        format!("{mins}m")
    }
}

pub fn fmt_dur_e(mins: i64, elapsed: bool, day_minutes: i64) -> String {
    if mins == 0 {
        return "0".into();
    }
    if elapsed {
        return if mins % 1440 == 0 { format!("{}ed", mins / 1440) } else if mins % 60 == 0 { format!("{}eh", mins / 60) } else { format!("{mins}em") };
    }
    if day_minutes > 0 && mins % day_minutes == 0 {
        format!("{}d", mins / day_minutes)
    } else if mins % 60 == 0 {
        format!("{}h", mins / 60)
    } else {
        format!("{mins}m")
    }
}

/// Predecessors: `3`, `3SS`, `3FF+2d`, `3-4h`, `3+2` (days), `3+1ed`. Comma-separated.
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
        let (kind, lag, elapsed, pct) = parse_link_tail_pct(&p[digits..], day_minutes).ok_or_else(bad)?;
        out.push(Dep { id, kind, lag, elapsed, pct });
    }
    Ok(out)
}

/// `SS+2d` → (kind, lag, elapsed)
fn parse_link_tail(rest: &str, day_minutes: i64) -> Option<(Link, i64, bool)> {
    match parse_link_tail_pct(rest, day_minutes)? {
        (_, _, _, Some(_)) => None,
        (k, l, e, None) => Some((k, l, e)),
    }
}

/// `SS+2d` or `FS+50%` → (kind, lag, elapsed, percent)
fn parse_link_tail_pct(rest: &str, day_minutes: i64) -> Option<(Link, i64, bool, Option<i32>)> {
    let rest = rest.trim();
    if let Some(body) = rest.strip_suffix('%') {
        let letters = body.chars().take_while(char::is_ascii_alphabetic).count();
        let kind = Link::parse(&body[..letters])?;
        let n: i32 = body[letters..].trim().trim_start_matches('+').trim().parse().ok()?;
        if !(-10000..=10000).contains(&n) {
            return None;
        }
        return Some((kind, 0, false, Some(n)));
    }
    let letters = rest.chars().take_while(char::is_ascii_alphabetic).count();
    let kind = Link::parse(&rest[..letters])?;
    let lag_s = rest[letters..].trim();
    if lag_s.is_empty() {
        return Some((kind, 0, false, None));
    }
    let (sign, num) = match lag_s.as_bytes()[0] {
        b'+' => (1, &lag_s[1..]),
        b'-' => (-1, &lag_s[1..]),
        _ => return None,
    };
    let (v, e) = parse_dur_e(num, day_minutes).ok()?;
    Some((kind, sign * v, e, None))
}

fn fmt_lag(lag: i64, elapsed: bool, dm: i64) -> String {
    match lag {
        0 => String::new(),
        l if l > 0 => format!("+{}", fmt_dur_e(l, elapsed, dm)),
        l => format!("-{}", fmt_dur_e(-l, elapsed, dm)),
    }
}

pub fn fmt_deps(deps: &[Dep], day_minutes: i64) -> String {
    deps.iter()
        .map(|d| {
            let lag = match d.pct {
                Some(0) => String::new(),
                Some(n) => format!("{}{n}%", if n > 0 { "+" } else { "" }),
                None => fmt_lag(d.lag, d.elapsed, day_minutes),
            };
            // Percent lags always name the link type, so "3+50%" isn't misread.
            let kind = if d.kind == Link::FS && d.pct.is_none() { "" } else { d.kind.as_str() };
            format!("{}{}{}", d.id, kind, lag)
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Resources: `Ana, Bo[50%], Cement[20], Travel[$400]`.
pub fn parse_who(s: &str) -> Result<Vec<Assign>, String> {
    parse_who_dm(s, 480)
}

/// Resources, with per-person details: `Ana[50% delay=1d work=16h contour=back]`.
pub fn parse_who_dm(s: &str, day_minutes: i64) -> Result<Vec<Assign>, String> {
    let mut out: Vec<Assign> = vec![];
    for part in s.split(',').map(str::trim).filter(|x| !x.is_empty()) {
        let a = match part.split_once('[') {
            Some((n, u)) => {
                let inner = u.trim_end_matches(']').trim();
                let mut amount_tok = "";
                let (mut delay, mut work, mut contour) = (0, None, Contour::Flat);
                for tok in inner.split_whitespace() {
                    match tok.split_once('=') {
                        Some(("delay", v)) => delay = parse_dur(v, day_minutes).map_err(|e| format!("{part}: {e}"))?.max(0),
                        Some(("work", v)) => work = Some(parse_dur(v, day_minutes).map_err(|e| format!("{part}: {e}"))?.max(0)),
                        Some(("contour", v)) => contour = Contour::parse(v).ok_or(format!("{part}: contour must be flat, front, back, bell, early, late or turtle"))?,
                        Some((k, _)) => return Err(format!("{part}: unknown assignment detail '{k}' (use delay=, work= or contour=)")),
                        None if amount_tok.is_empty() => amount_tok = tok,
                        None => return Err(format!("bad amount in '{part}'")),
                    }
                }
                let raw = if amount_tok.is_empty() { "100%" } else { amount_tok };
                let bad = || format!("bad amount in '{part}' (examples: Bo[50%], Cement[20], Travel[$400])");
                let (form, num) = if let Some(x) = raw.strip_suffix('%') {
                    (Form::Pct, x.trim())
                } else if let Some(x) = raw.strip_prefix('$') {
                    (Form::Money, x.trim())
                } else {
                    (Form::Qty, raw)
                };
                let amount: f64 = num.replace(',', ".").parse().map_err(|_| bad())?;
                if !amount.is_finite() || amount < 0.0 {
                    return Err(bad());
                }
                let units = if form == Form::Money { 100 } else { amount.round() as u32 };
                if form == Form::Pct && (units == 0 || units > 1000) {
                    return Err(format!("units in '{part}' must be 1–1000%"));
                }
                Assign { name: n.trim().to_string(), units: units.max(1), amount, form, delay, work, contour }
            }
            None => Assign::person(part, 100),
        };
        if a.name.is_empty() {
            return Err(format!("missing name in '{part}'"));
        }
        if !out.iter().any(|x| x.name == a.name) {
            out.push(a);
        }
    }
    Ok(out)
}

pub fn fmt_who(w: &[Assign]) -> String {
    w.iter()
        .map(|a| {
            let mut more = String::new();
            if a.delay > 0 {
                more.push_str(&format!(" delay={}", fmt_effort(a.delay)));
            }
            if let Some(x) = a.work {
                more.push_str(&format!(" work={}", fmt_effort(x)));
            }
            if a.contour != Contour::Flat {
                more.push_str(&format!(" contour={}", a.contour.code()));
            }
            match a.form {
                Form::Pct if a.units == 100 && more.is_empty() => a.name.clone(),
                Form::Pct => format!("{}[{}%{more}]", a.name, a.units),
                Form::Qty => format!("{}[{}]", a.name, fmt_num(a.amount)),
                Form::Money => format!("{}[${}]", a.name, fmt_num(a.amount)),
            }
        })
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

fn parse_ranges(s: &str) -> Result<Vec<(i64, i64)>, String> {
    s.split(',').map(str::trim).filter(|x| !x.is_empty()).map(parse_range).collect()
}

fn fmt_ranges(r: &[(i64, i64)]) -> String {
    r.iter().map(|(a, b)| fmt_range(*a, *b)).collect::<Vec<_>>().join(",")
}

fn fmt_when(t: i64) -> String {
    if t.rem_euclid(DAY) == 0 {
        fmt_date(t.div_euclid(DAY))
    } else {
        fmt_datetime(t)
    }
}

pub fn fmt_num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x:.4}").trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// A plain number: rejects `nan` and `inf`, which Rust's own parser accepts.
pub fn parse_num(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|x| x.is_finite())
}

/// Removes characters that would break the line-based format.
pub fn clean(s: &str) -> String {
    s.replace(['|', '\r', '\n', ';'], " ").trim().to_string()
}

/// A custom field name: no characters that separate attributes or values.
pub fn clean_field(s: &str) -> String {
    s.replace(['|', '\r', '\n', ';', '=', ','], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_hours_token(s: &str) -> bool {
    s.eq_ignore_ascii_case("off") || (s.contains('-') && s.contains(':') && s.chars().next().is_some_and(|c| c.is_ascii_digit()))
}

/// `week Mon-Fri 08:00-12:00 13:00-17:00` (the part after `week`) applied onto `week`.
fn apply_week(week: &mut [Hours; 7], spec: &str) -> Result<(), String> {
    let (days, hours) = spec.trim().split_once([' ', '\t']).unwrap_or((spec.trim(), "off"));
    let hours = parse_hours(hours)?;
    for d in parse_weekdays(days)? {
        week[d] = hours.clone();
    }
    Ok(())
}

/// Weekday groups that share hours, Mon first: `["Mon-Fri 08:00-12:00 13:00-17:00", "Sat,Sun off"]`.
pub fn fmt_week(week: &[Hours; 7]) -> Vec<String> {
    let mut groups: Vec<(Hours, Vec<usize>)> = vec![];
    for d in [1, 2, 3, 4, 5, 6, 0] {
        match groups.iter_mut().find(|(h, _)| *h == week[d]) {
            Some(g) => g.1.push(d),
            None => groups.push((week[d].clone(), vec![d])),
        }
    }
    groups.iter().map(|(h, days)| format!("{} {}", fmt_weekdays(days), fmt_hours(h))).collect()
}

fn parse_baseline(v: &str) -> Result<Baseline, String> {
    let p: Vec<&str> = v.split('/').collect();
    if p.len() < 2 {
        return Err("baseline must be START/FINISH[/COST[/WORK]]".into());
    }
    Ok(Baseline {
        start: parse_datetime(p[0])?,
        finish: parse_datetime(p[1])?,
        cost: p.get(2).map(|c| parse_num(c).ok_or(format!("bad baseline cost '{c}'"))).transpose()?.unwrap_or(0.0),
        work: p.get(3).map(|w| w.parse().map_err(|_| format!("bad baseline work '{w}'"))).transpose()?.unwrap_or(0),
    })
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
                "status" => p.status = if rest.is_empty() { None } else { Some(parse_date(rest).map_err(err)?) },
                "currency" => p.currency = rest.to_string(),
                "pool" => p.pool = (!rest.is_empty()).then(|| rest.to_string()),
                "field" => {
                    let f: Vec<&str> = rest.split('|').map(str::trim).collect();
                    let name = clean_field(f[0]);
                    if name.is_empty() {
                        return Err(err("field needs a name".into()));
                    }
                    let kind = f.get(1).copied().unwrap_or("text").to_ascii_lowercase();
                    if !FIELD_KINDS.contains(&kind.as_str()) {
                        return Err(err(format!("field type must be text, number, date, flag or code (not '{kind}')")));
                    }
                    let values = f.get(2).map(|v| v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
                    p.fields.retain(|x| x.name != name);
                    p.fields.push(FieldDef { name, kind, values });
                }
                "leveling" => {
                    for tok in rest.split_whitespace() {
                        match tok {
                            "slack" => p.leveling.slack_only = true,
                            "split" => p.leveling.split = true,
                            "order=standard" => p.leveling.order = LevelOrder::Standard,
                            "order=priority" => p.leveling.order = LevelOrder::Priority,
                            "order=id" => p.leveling.order = LevelOrder::Id,
                            _ => return Err(err(format!("unknown leveling option '{tok}' (slack, split, order=standard|priority|id)"))),
                        }
                    }
                }
                "dayhours" => {
                    let h: f64 = rest.parse().map_err(|_| err(format!("bad dayhours '{rest}'")))?;
                    if !(0.25..=24.0).contains(&h) {
                        return Err(err("dayhours must be between 0.25 and 24".into()));
                    }
                    p.day_minutes = (h * 60.0).round() as i64;
                }
                "week" => apply_week(&mut p.week, rest).map_err(err)?,
                "calendar" => {
                    let f: Vec<&str> = rest.split('|').map(str::trim).collect();
                    let name = f[0].to_string();
                    if name.is_empty() {
                        return Err(err("calendar needs a name".into()));
                    }
                    let mut week = p.week.clone();
                    for spec in &f[1..] {
                        let spec = spec.strip_prefix("week").ok_or_else(|| err(format!("calendar field '{spec}' should start with 'week'")))?;
                        apply_week(&mut week, spec).map_err(err)?;
                    }
                    p.calendars.retain(|c| c.name != name);
                    p.calendars.push(NamedCal { name, week });
                }
                "holiday" | "workday" => {
                    if let Some(rule) = rest.strip_prefix("every ") {
                        if kw == "workday" {
                            return Err(err("repeating dates are for holidays only".into()));
                        }
                        p.recurring.push(parse_recurring(rule).map_err(err)?);
                        continue;
                    }
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
                    let mut week: Option<[Hours; 7]> = None;
                    for kv in &f[1..] {
                        let (k, v) = kv.split_once([' ', '=']).unwrap_or((kv, ""));
                        let v = v.trim();
                        let num = |v: &str| parse_num(v).ok_or_else(|| err(format!("bad {k} '{v}'")));
                        match k {
                            "rate" => match v.split_once(" from ") {
                                Some((x, d)) => r.rates.push((parse_date(d.trim()).map_err(err)?, num(x.trim())?)),
                                None => r.rate = num(v)?,
                            },
                            "accrue" => {
                                r.accrue = match v {
                                    "start" => Accrue::Start,
                                    "end" => Accrue::End,
                                    "prorated" => Accrue::Prorated,
                                    _ => return Err(err(format!("accrue must be start, end or prorated (not '{v}')"))),
                                }
                            }
                            "group" => r.group = v.to_string(),
                            "initials" => r.initials = v.to_string(),
                            "email" => r.email = v.to_string(),
                            "note" => r.note = v.to_string(),
                            "peruse" => r.per_use = num(v)?,
                            "overtime" => r.overtime = num(v)?,
                            "unit" => r.unit = v.to_string(),
                            "type" => {
                                r.kind = match v {
                                    "work" => ResKind::Work,
                                    "material" => ResKind::Material,
                                    "cost" => ResKind::Cost,
                                    _ => return Err(err(format!("resource type must be work, material or cost (not '{v}')"))),
                                }
                            }
                            "capacity" => {
                                let (x, from) = match v.split_once(" from ") {
                                    Some((x, d)) => (x.trim(), Some(parse_date(d.trim()).map_err(err)?)),
                                    None => (v, None),
                                };
                                let c: u32 = x.trim_end_matches('%').parse().map_err(|_| err(format!("bad capacity '{v}'")))?;
                                if c == 0 {
                                    return Err(err("capacity must be above 0%".into()));
                                }
                                match from {
                                    Some(d) => r.caps.push((d, c)),
                                    None => r.capacity = c,
                                }
                            }
                            "off" => r.off.extend(parse_ranges(v).map_err(err)?),
                            "week" => apply_week(week.get_or_insert_with(|| p.week.clone()), v).map_err(err)?,
                            "" => {}
                            _ => return Err(err(format!("unknown resource field '{k}'"))),
                        }
                    }
                    if let Some(w) = &week {
                        if w.iter().all(|h| h.is_empty()) {
                            return Err(err(format!("{} has no working hours on any day", r.name)));
                        }
                    }
                    r.week = week;
                    r.rates.sort_by_key(|x| x.0);
                    r.caps.sort_by_key(|x| x.0);
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
                    let (dur, elapsed) = match get(2) {
                        "" => (p.day_minutes, false),
                        s => parse_dur_e(s, p.day_minutes).map_err(err)?,
                    };
                    if dur < 0 {
                        return Err(err("duration can't be negative".into()));
                    }
                    let mut t = Task::new(id, get(1), dur);
                    t.elapsed = elapsed;
                    t.level = indent / 2;
                    t.deps = parse_deps(get(3), p.day_minutes).map_err(err)?;
                    t.who = parse_who_dm(get(4), p.day_minutes).map_err(err)?;
                    t.pct = match get(5) {
                        "" => 0,
                        s => s.trim_end_matches('%').trim().parse::<u8>().map_err(|_| err(format!("bad % done '{s}'")))?.min(100),
                    };
                    for kv in get(6).split(';').map(str::trim).filter(|x| !x.is_empty()) {
                        let (k, v) = kv.split_once('=').map(|(k, v)| (k.trim(), v.trim())).unwrap_or((kv, ""));
                        let dt = |v: &str| parse_datetime(v).map_err(err);
                        match k {
                            "snet" | "snlt" | "fnet" | "fnlt" | "mso" | "mfo" => t.constraint = Constraint::from_code(k, Some(dt(v)?)).unwrap(),
                            "alap" => t.constraint = Constraint::Alap,
                            "deadline" => t.deadline = Some(parse_date(v).map_err(err)?),
                            "cost" => t.cost = parse_num(v).ok_or_else(|| err(format!("bad cost '{v}'")))?,
                            "delay" => t.delay = parse_dur(v, p.day_minutes).map_err(err)?.max(0),
                            "note" => t.note = v.to_string(),
                            "work" => t.work = Some(parse_dur(v, p.day_minutes).map_err(err)?.max(0)),
                            "as" => t.actual_start = Some(dt(v)?),
                            "af" => t.actual_finish = Some(dt(v)?),
                            "ac" => t.actual_cost = Some(parse_num(v).ok_or_else(|| err(format!("bad actual cost '{v}'")))?),
                            "ot" => t.overtime = parse_dur(v, p.day_minutes).map_err(err)?.max(0),
                            "split" => t.splits = parse_ranges(v).map_err(err)?,
                            "manual" => t.manual = Some(dt(v)?),
                            "inactive" => t.inactive = true,
                            "priority" => t.priority = v.parse::<u32>().map_err(|_| err(format!("bad priority '{v}'")))?.min(1000),
                            "cal" => t.cal = Some(v.to_string()),
                            "sub" => t.sub = Some(v.to_string()),
                            "type" => match v {
                                "units" => t.fixed_units = true,
                                "work" | "duration" => t.fixed_units = false,
                                _ => return Err(err(format!("task type must be units, work or duration (not '{v}')"))),
                            },
                            "lsplit" => t.lsplits = parse_ranges(v).map_err(err)?,
                            x if x.starts_with("x:") => {
                                let name = clean_field(&x[2..]);
                                if !name.is_empty() && !v.is_empty() {
                                    t.fields.retain(|(k, _)| *k != name);
                                    t.fields.push((name, v.to_string()));
                                }
                            }
                            "ext" => {
                                for part in v.split(',').map(str::trim).filter(|x| !x.is_empty()) {
                                    let bad = || err(format!("bad external link '{part}' (example: Other.plan#5FS+1d)"));
                                    let (file, tail) = part.rsplit_once('#').ok_or_else(bad)?;
                                    let digits = tail.chars().take_while(char::is_ascii_digit).count();
                                    let id: u32 = tail[..digits].parse().map_err(|_| bad())?;
                                    let (kind, lag, _) = parse_link_tail(&tail[digits..], p.day_minutes).ok_or_else(bad)?;
                                    t.xdeps.push(XDep { file: file.trim().to_string(), id, kind, lag });
                                }
                            }
                            b if b.starts_with("base") => {
                                let n: usize = if b == "base" { 0 } else { b[4..].parse().map_err(|_| err(format!("unknown task attribute '{b}'")))? };
                                if n >= BASELINES {
                                    return Err(err(format!("baselines go from base to base{}", BASELINES - 1)));
                                }
                                t.baselines[n] = Some(parse_baseline(v).map_err(err)?);
                            }
                            other => return Err(err(format!("unknown task attribute '{other}'"))),
                        }
                    }
                    p.tasks.push(t);
                }
                _ => return Err(err(format!("unknown keyword '{kw}'"))),
            }
        }
        if !have_start {
            if !p.tasks.is_empty() {
                return Err("missing 'start YYYY-MM-DD' line".into());
            }
            p.start = local_today();
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
        if let Some(st) = self.status {
            let _ = writeln!(s, "status {}", fmt_date(st));
        }
        if self.currency != "$" {
            let _ = writeln!(s, "currency {}", clean(&self.currency));
        }
        if let Some(pool) = &self.pool {
            let _ = writeln!(s, "pool {}", clean(pool));
        }
        for g in fmt_week(&self.week) {
            let _ = writeln!(s, "week {g}");
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
        for r in &self.recurring {
            let _ = writeln!(s, "holiday every {}", clean(&fmt_recurring(r)));
        }
        for c in &self.calendars {
            let weeks: Vec<String> = fmt_week(&c.week).iter().map(|g| format!(" | week {g}")).collect();
            let _ = writeln!(s, "calendar {}{}", clean(&c.name), weeks.concat());
        }
        for r in self.resources.iter().filter(|r| !r.pooled) {
            let _ = writeln!(s, "{}", resource_line(r));
        }
        for f in &self.fields {
            let values = if f.values.is_empty() { String::new() } else { format!(" | {}", f.values.iter().map(|v| clean(v).replace(',', " ")).collect::<Vec<_>>().join(", ")) };
            let _ = writeln!(s, "field {} | {}{values}", clean_field(&f.name), f.kind);
        }
        if self.leveling != LevelOpts::default() {
            let l = self.leveling;
            let mut opts = vec![];
            if l.slack_only {
                opts.push("slack");
            }
            if l.split {
                opts.push("split");
            }
            match l.order {
                LevelOrder::Standard => {}
                LevelOrder::Priority => opts.push("order=priority"),
                LevelOrder::Id => opts.push("order=id"),
            }
            let _ = writeln!(s, "leveling {}", opts.join(" "));
        }
        let _ = writeln!(s, "# id | name | duration | predecessors | resources | % done | attributes");
        let summary = self.summary_flags();
        for (i, t) in self.tasks.iter().enumerate() {
            let mut attrs: Vec<String> = vec![];
            match t.constraint {
                Constraint::Asap => {}
                Constraint::Alap => attrs.push("alap".into()),
                c => attrs.push(format!("{}={}", c.code(), fmt_when(c.date().unwrap()))),
            }
            if let Some(d) = t.deadline {
                attrs.push(format!("deadline={}", fmt_date(d)));
            }
            if let Some(w) = t.work {
                attrs.push(format!("work={}", fmt_effort(w)));
            }
            if t.cost != 0.0 {
                attrs.push(format!("cost={}", fmt_num(t.cost)));
            }
            if t.delay > 0 {
                attrs.push(format!("delay={}", fmt_dur(t.delay, dm)));
            }
            if let Some(x) = t.actual_start {
                attrs.push(format!("as={}", fmt_datetime(x)));
            }
            if let Some(x) = t.actual_finish {
                attrs.push(format!("af={}", fmt_datetime(x)));
            }
            if let Some(x) = t.actual_cost {
                attrs.push(format!("ac={}", fmt_num(x)));
            }
            if t.overtime > 0 {
                attrs.push(format!("ot={}", fmt_effort(t.overtime)));
            }
            if !t.splits.is_empty() {
                attrs.push(format!("split={}", fmt_ranges(&t.splits)));
            }
            if let Some(x) = t.manual {
                attrs.push(format!("manual={}", fmt_datetime(x)));
            }
            if t.inactive {
                attrs.push("inactive".into());
            }
            if t.priority != 500 {
                attrs.push(format!("priority={}", t.priority));
            }
            if let Some(c) = &t.cal {
                attrs.push(format!("cal={}", clean(c)));
            }
            if let Some(f) = &t.sub {
                attrs.push(format!("sub={}", clean(f)));
            }
            if t.fixed_units {
                attrs.push("type=units".into());
            }
            if !t.lsplits.is_empty() {
                attrs.push(format!("lsplit={}", fmt_ranges(&t.lsplits)));
            }
            for (k, v) in &t.fields {
                attrs.push(format!("x:{}={}", clean_field(k), clean(v)));
            }
            if !t.xdeps.is_empty() {
                let x: Vec<String> = t.xdeps.iter().map(|x| format!("{}#{}{}{}", clean(&x.file), x.id, if x.kind == Link::FS { "" } else { x.kind.as_str() }, fmt_lag(x.lag, false, dm))).collect();
                attrs.push(format!("ext={}", x.join(",")));
            }
            for (n, b) in t.baselines.iter().enumerate() {
                if let Some(b) = b {
                    let key = if n == 0 { "base".to_string() } else { format!("base{n}") };
                    attrs.push(format!("{key}={}/{}/{}/{}", fmt_datetime(b.start), fmt_datetime(b.finish), fmt_num((b.cost * 100.0).round() / 100.0), b.work));
                }
            }
            if !t.note.is_empty() {
                attrs.push(format!("note={}", clean(&t.note)));
            }
            let dur = if summary[i] { String::new() } else { fmt_dur_e(t.dur, t.elapsed, dm) };
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

    /// Dated exceptions plus repeating holidays expanded over the years the project can span.
    pub fn all_exceptions(&self) -> Vec<Exception> {
        let y = civil_from_days(self.start).0;
        let mut ex = expand_recurring(&self.recurring, y - 1..=y + 25);
        ex.extend(self.exceptions.iter().cloned());
        ex
    }

    pub fn calendar(&self) -> Result<Calendar, String> {
        let cal = Calendar::new(self.week.clone(), &self.all_exceptions())?;
        for c in &self.calendars {
            if c.week.iter().all(|h| h.is_empty()) {
                return Err(format!("calendar '{}' has no working hours on any day", c.name));
            }
        }
        Ok(cal)
    }

    /// The project calendar with a named calendar's working week.
    pub fn named_calendar(&self, base: &Calendar, name: &str) -> Result<Calendar, String> {
        let c = self.calendars.iter().find(|c| c.name == name).ok_or(format!("there's no calendar called '{name}'"))?;
        Ok(base.with_week(&c.week))
    }

    /// The declared resource, or a plain full-time person for a name that's only used on tasks.
    pub fn resource(&self, name: &str) -> Cow<'_, Resource> {
        self.resources.iter().find(|r| r.name == name).map_or_else(|| Cow::Owned(Resource::new(name)), Cow::Borrowed)
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

pub fn resource_line(r: &Resource) -> String {
    use std::fmt::Write;
    let mut line = format!("resource {}", clean(&r.name));
    match r.kind {
        ResKind::Work => {}
        ResKind::Material => line.push_str(" | type material"),
        ResKind::Cost => line.push_str(" | type cost"),
    }
    if !r.unit.is_empty() {
        let _ = write!(line, " | unit {}", clean(&r.unit));
    }
    if r.rate != 0.0 {
        let _ = write!(line, " | rate {}", fmt_num(r.rate));
    }
    if r.capacity != 100 {
        let _ = write!(line, " | capacity {}", r.capacity);
    }
    if r.per_use != 0.0 {
        let _ = write!(line, " | peruse {}", fmt_num(r.per_use));
    }
    if r.overtime != 0.0 {
        let _ = write!(line, " | overtime {}", fmt_num(r.overtime));
    }
    for (d, x) in &r.rates {
        let _ = write!(line, " | rate {} from {}", fmt_num(*x), fmt_date(*d));
    }
    for (d, x) in &r.caps {
        let _ = write!(line, " | capacity {x} from {}", fmt_date(*d));
    }
    if r.accrue != Accrue::Prorated {
        let _ = write!(line, " | accrue {}", r.accrue.code());
    }
    for (k, v) in [("group", &r.group), ("initials", &r.initials), ("email", &r.email), ("note", &r.note)] {
        if !v.is_empty() {
            let _ = write!(line, " | {k} {}", clean(v));
        }
    }
    if let Some(w) = &r.week {
        for g in fmt_week(w) {
            let _ = write!(line, " | week {g}");
        }
    }
    if !r.off.is_empty() {
        let _ = write!(line, " | off {}", r.off.iter().map(|(a, b)| fmt_range(*a, *b)).collect::<Vec<_>>().join(", "));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "project Demo
start 2026-11-09
status 2026-11-12
currency €
pool team.plan
week Mon-Thu 08:00-12:00 13:00-17:00
week Fri 08:00-12:00
dayhours 7.5
holiday 2026-11-26 Thanksgiving
holiday 2026-12-24..2026-12-31 Winter break
holiday every 12-25 observed Christmas
holiday every 3rd Mon Jan MLK Day
workday 2026-12-05 09:00-13:00 Release Saturday
calendar Night | week Mon-Fri 22:00-24:00
resource Ana | rate 85.5 | capacity 50 | peruse 20 | overtime 120 | week Mon-Wed 08:00-17:00 | week Thu-Sun off | off 2026-11-20..2026-11-21, 2026-12-01
resource Cement | type material | unit bags | rate 9.5
resource Travel | type cost
task 1 | Phase |
  task 2 | A | 3d | | Ana[50%], Bo, Cement[20], Travel[$400] | 10 | snet=2026-11-10; deadline=2026-11-20; cost=100; note=hi there; work=40h; as=2026-11-10T08:00; ac=1200; ot=4h
  task 3 | B | 4h | 2SS+1d,1 | | | mso=2026-11-12T13:00; base=2026-11-09T08:00/2026-11-10T12:00/350/240; base3=2026-11-09T08:00/2026-11-09T12:00; delay=2h; priority=800
task 4 | Done | 0 | 3FF-30m | | | alap; inactive; cal=Night; split=2026-11-12..2026-11-13; manual=2026-11-10T08:00
task 5 | Cure | 3ed | 4+12eh | | | ext=Other.plan#7SS+1d,More.plan#2; sub=Sub.plan; af=2026-11-14T08:00; fnlt=2026-11-30
";

    #[test]
    fn parses_everything() {
        let p = Project::parse(SAMPLE).unwrap();
        assert_eq!(p.currency, "€");
        assert_eq!(p.day_minutes, 450);
        assert_eq!(p.status, Some(parse_date("2026-11-12").unwrap()));
        assert_eq!(p.pool.as_deref(), Some("team.plan"));
        assert_eq!(p.week[5], vec![(480, 720)]);
        assert!(p.week[6].is_empty());
        assert_eq!(p.exceptions.len(), 3);
        assert_eq!(p.recurring.len(), 2);
        assert_eq!(p.exceptions[2].hours, vec![(540, 780)]);
        assert_eq!(p.calendars[0].week[1], vec![(1320, 1440)]);
        let ana = &p.resources[0];
        assert_eq!((ana.capacity, ana.per_use, ana.overtime, ana.off.len()), (50, 20.0, 120.0, 2));
        assert_eq!(ana.week.as_ref().unwrap()[1], vec![(480, 1020)]);
        assert!(ana.week.as_ref().unwrap()[4].is_empty());
        assert_eq!(p.resources[1].kind, ResKind::Material);
        assert_eq!(p.resources[2].kind, ResKind::Cost);
        let t2 = &p.tasks[1];
        assert_eq!(t2.who[0], Assign::person("Ana", 50));
        assert_eq!((t2.who[2].amount, t2.who[2].form), (20.0, Form::Qty));
        assert_eq!((t2.who[3].amount, t2.who[3].form), (400.0, Form::Money));
        assert_eq!((t2.work, t2.overtime, t2.actual_cost), (Some(2400), 240, Some(1200.0)));
        assert!(t2.actual_start.is_some());
        let t3 = &p.tasks[2];
        assert_eq!(t3.deps[0], Dep { id: 2, kind: Link::SS, lag: 450, elapsed: false, pct: None });
        assert_eq!(t3.baseline().unwrap().cost, 350.0);
        assert!(t3.baselines[3].is_some());
        assert_eq!(t3.priority, 800);
        let t4 = &p.tasks[3];
        assert_eq!(t4.deps[0], Dep { id: 3, kind: Link::FF, lag: -30, elapsed: false, pct: None });
        assert!(t4.inactive && t4.constraint == Constraint::Alap && t4.manual.is_some());
        assert_eq!((t4.cal.as_deref(), t4.splits.len()), (Some("Night"), 1));
        let t5 = &p.tasks[4];
        assert!(t5.elapsed && t5.dur == 3 * 1440);
        assert_eq!(t5.deps[0], Dep { id: 4, kind: Link::FS, lag: 720, elapsed: true, pct: None });
        assert_eq!(t5.xdeps[0], XDep { file: "Other.plan".into(), id: 7, kind: Link::SS, lag: 450 });
        assert_eq!(t5.sub.as_deref(), Some("Sub.plan"));
        assert!(matches!(t5.constraint, Constraint::Fnlt(_)));
        assert_eq!(p.summary_flags(), vec![true, false, false, false, false]);
        assert_eq!(p.wbs(), vec!["1", "1.1", "1.2", "2", "3"]);
    }

    #[test]
    fn serialize_is_stable() {
        let p = Project::parse(SAMPLE).unwrap();
        let once = p.serialize();
        let twice = Project::parse(&once).unwrap().serialize();
        assert_eq!(once, twice);
        assert!(once.contains("holiday every 12-25 observed Christmas"), "{once}");
        assert!(once.contains("resource Ana | rate 85.5 | capacity 50 | peruse 20 | overtime 120 | week Mon-Wed 08:00-17:00 | week Thu-Sun off"), "{once}");
        assert!(once.contains("Cement[20],Travel[$400]"));
        assert!(once.contains("task 5 | Cure | 3ed | 4+12eh |"), "{once}");
    }

    #[test]
    fn pooled_resources_are_not_saved() {
        let mut p = Project::parse("start 2026-11-09").unwrap();
        let mut r = Resource::new("Zed");
        r.pooled = true;
        p.resources.push(r);
        assert!(!p.serialize().contains("Zed"));
    }

    #[test]
    fn old_format_still_works() {
        let p = Project::parse("project X\nstart 2026-11-09\nholiday 2026-11-26\ntask 1 | A | 3 | | Ana | 100\ntask 2 | B | 2 | 1+1 | Bo[50] | | base=2026-11-09T08:00/2026-11-10T17:00").unwrap();
        assert_eq!(p.tasks[0].dur, 480 * 3);
        assert_eq!(p.tasks[1].deps[0].lag, 480);
        assert_eq!(p.tasks[1].who[0].units, 50);
        assert_eq!(p.tasks[1].baseline().unwrap().cost, 0.0);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Project::parse("task 1 | A | 1").is_err());
        assert!(Project::parse("start 2026-11-09\ntask 1 | A | -1").is_err());
        assert!(Project::parse("start 2026-11-09\ntask 1 | A | 1 | 2XY").is_err());
        assert!(Project::parse("start 2026-11-09\nweek Mon-Sun off").is_err());
        assert!(Project::parse("start 2026-11-09\nresource X | type robot").is_err());
        assert!(Project::parse("start 2026-11-09\ntask 1 | A | 1 | | | | base11=2026-11-09/2026-11-10").is_err());
        assert!(Project::parse("start 2026-11-09\nworkday every 12-25").is_err());
        assert!(parse_who("Ana[0%]").is_err());
        assert_eq!(parse_dur("1.5d", 480).unwrap(), 720);
        assert_eq!(parse_dur("2w", 480).unwrap(), 4800);
        assert_eq!(parse_dur("3 days?", 480).unwrap(), 1440);
        assert_eq!(parse_dur_e("2ed", 480).unwrap(), (2880, true));
        assert_eq!(fmt_dur(720, 480), "12h");
        assert!(Project::parse("project Empty").is_ok(), "no tasks, no start → starts today");
    }

    #[test]
    fn levels_are_normalized() {
        let p = Project::parse("start 2026-11-09\n    task 1 | A | 1\n      task 2 | B | 1").unwrap();
        assert_eq!(p.tasks[0].level, 0);
        assert_eq!(p.tasks[1].level, 1);
    }
}
