//! Built-in HTTP server for the visual editor (`rplan ui`).
//!
//! Security model: the server only listens on 127.0.0.1, and every request must
//! (1) carry a Host header naming this server (blocks DNS rebinding), (2) carry no
//! foreign Origin, and (3) for API calls, present the per-session secret token that
//! is embedded in the page. Other websites can't read the page, so they can't
//! learn the token, and a custom header can't be sent cross-site without a CORS
//! preflight, which this server never approves.

use crate::calendar::*;
use crate::model::*;
use crate::report::{BLD, DIM, GRN, RST};
use crate::schedule::*;
use crate::{history, mspdi, report};
use std::collections::hash_map::RandomState;
use std::fmt::Write as _;
use std::hash::{BuildHasher, Hasher};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, thread};

const UI: &str = include_str!("ui/index.html");
/// The interface's scripts and styles, built into the program.
const ASSETS: [(&str, &str, &str); 9] = [
    ("/ui/style.css", "text/css; charset=utf-8", include_str!("ui/style.css")),
    ("/ui/i18n.js", "text/javascript; charset=utf-8", include_str!("ui/i18n.js")),
    ("/ui/core.js", "text/javascript; charset=utf-8", include_str!("ui/core.js")),
    ("/ui/gantt.js", "text/javascript; charset=utf-8", include_str!("ui/gantt.js")),
    ("/ui/views.js", "text/javascript; charset=utf-8", include_str!("ui/views.js")),
    ("/ui/panels.js", "text/javascript; charset=utf-8", include_str!("ui/panels.js")),
    ("/ui/reports.js", "text/javascript; charset=utf-8", include_str!("ui/reports.js")),
    ("/ui/print.js", "text/javascript; charset=utf-8", include_str!("ui/print.js")),
    ("/ui/events.js", "text/javascript; charset=utf-8", include_str!("ui/events.js")),
];

/// Decodes `%XX` and `+` in a URL query value.
pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()).ok_or(()) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub const ICON_ICO: &[u8] = include_bytes!("../assets/rplan.ico");
pub const ICON_SVG: &[u8] = include_bytes!("../assets/rplan.svg");
pub const ICON_PNG: &[u8] = include_bytes!("../assets/rplan.png");

fn icon(ctype: &'static str, bytes: &[u8]) -> Resp {
    let mut r = Resp::new(200, ctype, bytes.to_vec());
    r.extra.push(("Cache-Control".into(), "public, max-age=86400".into()));
    r
}
const MAX_BODY: usize = 16 << 20;
/// The request line and headers together.
const MAX_HEAD: u64 = 64 << 10;

pub struct Ctx {
    /// The project file being edited (can be switched from the app).
    pub path: Mutex<String>,
    pub port: u16,
    pub token: String,
    pub lock: Mutex<()>,
    /// Seconds since the epoch of the last request (used to exit after the window closes).
    pub last_seen: AtomicU64,
}

impl Ctx {
    pub fn new(path: &str, port: u16, token: String) -> Ctx {
        Ctx { path: Mutex::new(path.to_string()), port, token, lock: Mutex::new(()), last_seen: AtomicU64::new(now_secs()) }
    }
    pub fn path(&self) -> String {
        self.path.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// How `serve` shows the editor.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    /// Just print the address.
    None,
    /// Open a tab in the default browser.
    Browser,
    /// Open a standalone app window; quit when it closes.
    Window,
}

// ---------- project files next to the current one ----------

fn dir_of(path: &str) -> PathBuf {
    Path::new(path).parent().filter(|p| !p.as_os_str().is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

/// Resolves a bare `.plan` file name inside the current project's folder.
fn sibling(path: &str, name: &str) -> Result<String, String> {
    let bad = name.is_empty() || name.contains(['/', '\\', ':', '\0']) || name.starts_with('.') || !name.to_ascii_lowercase().ends_with(".plan");
    if bad {
        return Err(format!("“{name}” isn't a project file in this folder"));
    }
    let p = dir_of(path).join(name);
    if !p.is_file() {
        return Err(format!("“{name}” doesn't exist any more"));
    }
    Ok(p.to_string_lossy().to_string())
}

/// Turns a project name into a safe Windows/macOS/Linux file name.
pub fn safe_file_name(name: &str) -> String {
    let mut s: String = name.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) || c.is_control() { '-' } else { c }).collect();
    s = s.trim().trim_end_matches(['.', ' ']).to_string();
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str()) || ((stem.starts_with("COM") || stem.starts_with("LPT")) && stem.len() == 4);
    if s.is_empty() || reserved {
        s = format!("{s} project").trim().to_string();
    }
    s.chars().take(120).collect()
}

fn files_json(path: &str) -> String {
    let dir = dir_of(path);
    let current = Path::new(path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let mut files: Vec<(u128, String, String)> = fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("plan")) && e.path().is_file())
                .take(500)
                .map(|e| {
                    let file = e.file_name().to_string_lossy().to_string();
                    let modified = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis());
                    // The name is on one of the first lines: don't read whole files just to list them.
                    let head = fs::File::open(e.path()).ok().map(|f| BufReader::new(f).lines().take(40).map_while(Result::ok));
                    let name = head.and_then(|mut l| l.find_map(|l| l.trim().strip_prefix("project ").map(|n| n.trim().to_string()))).unwrap_or_default();
                    (modified, file, name)
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    let list: Vec<String> = files.iter().map(|(m, f, n)| format!(r#"{{"file":{},"name":{},"modified":{m}}}"#, js(f), js(n))).collect();
    format!(r#"{{"ok":true,"dir":{},"current":{},"files":[{}]}}"#, js(&dir.to_string_lossy()), js(&current), list.join(","))
}

pub struct Req {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Req {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    fn param(&self, name: &str) -> Option<&str> {
        self.query.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v)
    }
}

pub struct Resp {
    pub code: u16,
    pub ctype: &'static str,
    pub extra: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    fn new(code: u16, ctype: &'static str, body: impl Into<Vec<u8>>) -> Resp {
        Resp { code, ctype, extra: vec![], body: body.into() }
    }
    fn json(code: u16, body: String) -> Resp {
        Resp::new(code, "application/json; charset=utf-8", body)
    }
    fn err(code: u16, msg: &str) -> Resp {
        Resp::json(code, format!(r#"{{"ok":false,"error":{}}}"#, js(msg)))
    }
    fn download(ctype: &'static str, name: &str, body: String) -> Resp {
        let mut r = Resp::new(200, ctype, body);
        r.extra.push(("Content-Disposition".into(), format!("attachment; filename=\"{}\"", name.replace(['"', '\\', '/', '\r', '\n'], "_"))));
        r
    }
}

pub fn new_token() -> String {
    let mut out = String::new();
    for k in 0..2u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(k);
        h.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        h.write_u32(std::process::id());
        let _ = write!(out, "{:016x}", h.finish());
    }
    out
}

pub fn js(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '<' => o.push_str("\\u003c"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn rev(path: &str) -> String {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| t.duration_since(UNIX_EPOCH).map(|d| d.as_micros()).unwrap_or(0))
        .unwrap_or(0)
        .to_string()
}

fn num(x: f64) -> String {
    if x.is_finite() {
        format!("{:.2}", x).trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        "0".into()
    }
}

/// Project, computed schedule and workloads as JSON for the UI.
/// Times are minutes since midnight of the project start day; days are offsets from it.
pub fn project_json(p: &Project, path: &str) -> Result<String, String> {
    let s = schedule(p)?;
    let t0 = p.start * DAY;
    let cal = p.calendar()?;
    let wbs = p.wbs();
    let rel = |t: i64| t - t0;
    let dayoff = |d: i64| d - p.start;
    let hours = |h: &Hours| format!("[{}]", h.iter().map(|(a, b)| format!("[{a},{b}]")).collect::<Vec<_>>().join(","));
    let week = |w: &[Hours; 7]| format!("[{}]", w.iter().map(hours).collect::<Vec<_>>().join(","));
    let ranges = |r: &[(i64, i64)]| format!("[{}]", r.iter().map(|(a, b)| format!("[{},{}]", dayoff(*a), dayoff(*b))).collect::<Vec<_>>().join(","));
    let opt = |v: Option<i64>| v.map_or("null".to_string(), |x| x.to_string());
    let mut o = String::new();
    let _ = write!(
        o,
        r#"{{"ok":true,"path":{},"rev":"{}","name":{},"start":"{}","status":{},"currency":{},"dayMinutes":{},"pool":{},"week":{},"exceptions":[{}],"recurring":[{}],"calendars":[{}],"warnings":[{}],"#,
        js(path),
        rev(path),
        js(&p.name),
        fmt_date(p.start),
        opt(p.status.map(dayoff)),
        js(&p.currency),
        p.day_minutes,
        p.pool.as_deref().map_or("null".into(), js),
        week(&p.week),
        p.exceptions
            .iter()
            .map(|e| format!(r#"{{"from":{},"to":{},"name":{},"hours":{}}}"#, dayoff(e.from), dayoff(e.to), js(&e.name), hours(&e.hours)))
            .collect::<Vec<_>>()
            .join(","),
        p.recurring.iter().map(|r| js(&fmt_recurring(r))).collect::<Vec<_>>().join(","),
        p.calendars.iter().map(|c| format!(r#"{{"name":{},"week":{}}}"#, js(&c.name), week(&c.week))).collect::<Vec<_>>().join(","),
        p.warnings.iter().map(|w| js(w)).collect::<Vec<_>>().join(",")
    );
    // Concrete holiday dates (dated + repeating) within the project's years, for drawing.
    let (y0, y1) = (civil_from_days(p.start).0 - 1, civil_from_days(s.finish.div_euclid(DAY)).0 + 2);
    let holidays: Vec<String> = expand_recurring(&p.recurring, y0..=y1).iter().map(|e| format!("[{},{}]", dayoff(e.from), js(&e.name))).collect();
    let _ = write!(o, r#""holidays":[{}],"#, holidays.join(","));
    let fields: Vec<String> = p.fields.iter().map(|f| format!(r#"{{"name":{},"kind":"{}","values":[{}]}}"#, js(&f.name), f.kind, f.values.iter().map(|v| js(v)).collect::<Vec<_>>().join(","))).collect();
    let order = match p.leveling.order {
        LevelOrder::Standard => "standard",
        LevelOrder::Priority => "priority",
        LevelOrder::Id => "id",
    };
    let _ = write!(o, r#""fields":[{}],"leveling":{{"slack":{},"split":{},"order":"{order}"}},"#, fields.join(","), p.leveling.slack_only, p.leveling.split);
    let declared: Vec<&str> = p.resources.iter().filter(|r| !r.pooled).map(|r| r.name.as_str()).collect();
    let ress: Vec<String> = p
        .all_resources()
        .iter()
        .map(|r| {
            format!(
                r#"{{"name":{},"kind":"{}","rate":{},"capacity":{},"declared":{},"pooled":{},"perUse":{},"overtime":{},"unit":{},"week":{},"off":{},"rates":[{}],"caps":[{}],"accrue":"{}","group":{},"initials":{},"email":{},"note":{}}}"#,
                js(&r.name),
                match r.kind {
                    ResKind::Work => "work",
                    ResKind::Material => "material",
                    ResKind::Cost => "cost",
                },
                num(r.rate),
                r.capacity,
                declared.contains(&r.name.as_str()),
                r.pooled,
                num(r.per_use),
                num(r.overtime),
                js(&r.unit),
                r.week.as_ref().map_or("null".into(), week),
                ranges(&r.off),
                r.rates.iter().map(|(d, x)| format!("[{},{}]", dayoff(*d), num(*x))).collect::<Vec<_>>().join(","),
                r.caps.iter().map(|(d, x)| format!("[{},{x}]", dayoff(*d))).collect::<Vec<_>>().join(","),
                r.accrue.code(),
                js(&r.group),
                js(&r.initials),
                js(&r.email),
                js(&r.note)
            )
        })
        .collect();
    let lds: Vec<String> = loads(p, &s)
        .iter()
        .map(|l| {
            let days: Vec<String> = l
                .days
                .iter()
                .map(|(d, (u, m, ids))| format!("[{},{u},{m},[{}]]", dayoff(*d), ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")))
                .collect();
            let caps: Vec<String> = l.caps.iter().map(|(d, c)| format!("[{},{c}]", dayoff(*d))).collect();
            format!(r#"{{"name":{},"capacity":{},"work":{},"cost":{},"tasks":{},"over":{},"days":[{}],"caps":[{}]}}"#, js(&l.name), l.capacity, l.work, num(l.cost), l.tasks, l.over_days, days.join(","), caps.join(","))
        })
        .collect();
    let others: Vec<String> = crate::external::other_loads(p, path)
        .iter()
        .map(|(name, days)| {
            let mut d: Vec<(&i64, &(u32, i64, Vec<String>))> = days.iter().collect();
            d.sort_by_key(|x| *x.0);
            let d: Vec<String> = d.iter().map(|(day, (u, m, projects))| format!("[{},{u},{m},[{}]]", dayoff(**day), projects.iter().map(|x| js(x)).collect::<Vec<_>>().join(","))).collect();
            format!(r#"{{"name":{},"days":[{}]}}"#, js(name), d.join(","))
        })
        .collect();
    let _ = write!(
        o,
        r#""resources":[{}],"loads":[{}],"otherLoads":[{}],"startT":{},"finish":{},"finishDate":"{}","span":{},"progress":{},"cost":{},"work":{},"ev":{{"bac":{},"pv":{},"ev":{},"ac":{}}},"critical":[{}],"text":{},"tasks":["#,
        ress.join(","),
        lds.join(","),
        others.join(","),
        rel(s.start),
        rel(s.finish),
        fmt_date((s.finish - 1).div_euclid(DAY).max(p.start)),
        cal.work_between(s.start, s.finish),
        num(s.progress),
        num(s.cost),
        s.work,
        num(s.ev.bac),
        num(s.ev.pv),
        num(s.ev.ev),
        num(s.ev.ac),
        s.critical.iter().map(u32::to_string).collect::<Vec<_>>().join(","),
        js(&p.serialize())
    );
    for (i, t) in p.tasks.iter().enumerate() {
        let sc = s.tasks[i];
        let deps: Vec<String> = t.deps.iter().map(|d| format!(r#"{{"id":{},"kind":"{}","lag":{},"elapsed":{},"pct":{}}}"#, d.id, d.kind.as_str(), d.lag, d.elapsed, d.pct.map_or("null".into(), |x| x.to_string()))).collect();
        let who: Vec<String> = t
            .who
            .iter()
            .map(|a| {
                let form = match a.form {
                    Form::Pct => "pct",
                    Form::Qty => "qty",
                    Form::Money => "money",
                };
                format!(r#"{{"name":{},"units":{},"amount":{},"form":"{form}","delay":{},"work":{},"contour":"{}"}}"#, js(&a.name), a.units, num(a.amount), a.delay, opt(a.work), a.contour.code())
            })
            .collect();
        let cons = match t.constraint {
            Constraint::Asap => "null".to_string(),
            c => format!(r#"{{"type":"{}","at":{}}}"#, c.code(), opt(c.date().map(rel))),
        };
        let baselines: Vec<String> = t
            .baselines
            .iter()
            .enumerate()
            .filter_map(|(n, b)| b.map(|b| format!(r#"{{"n":{n},"s":{},"f":{},"cost":{},"work":{}}}"#, rel(b.start), rel(b.finish), num(b.cost), b.work)))
            .collect();
        let sub_info = t.sub_info.as_ref().map_or("null".into(), |x| format!(r#"{{"name":{},"span":{},"cost":{},"pct":{},"tasks":{}}}"#, js(&x.name), x.span, num(x.cost), num(x.pct), x.tasks));
        let xdeps: Vec<String> = t.xdeps.iter().map(|x| format!(r#"{{"file":{},"id":{},"kind":"{}","lag":{}}}"#, js(&x.file), x.id, x.kind.as_str(), x.lag)).collect();
        let ext: Vec<String> = t.ext.iter().map(|x| format!(r#"{{"label":{},"es":{},"ef":{}}}"#, js(&x.label), rel(x.es), rel(x.ef))).collect();
        let xf: Vec<String> = t.fields.iter().map(|(k, v)| format!("{}:{}", js(k), js(v))).collect();
        let _ = write!(
            o,
            r#"{}{{"id":{},"name":{},"level":{},"wbs":"{}","summary":{},"dur":{},"edur":{},"elapsed":{},"work":{},"workT":{},"deps":[{}],"who":[{}],"pct":{},"pctR":{},"constraint":{cons},"deadline":{},"fixed":{},"cost":{},"delay":{},"base":{},"baselines":[{}],"note":{},"es":{},"ef":{},"ls":{},"lf":{},"slack":{},"free":{},"crit":{},"late":{},"conflict":{},"span":{},"slip":{},"as":{},"af":{},"ac":{},"ot":{},"splits":{},"manual":{},"inactive":{},"priority":{},"cal":{},"sub":{},"subInfo":{sub_info},"xdeps":[{}],"ext":[{}],"done":{},"started":{},"bac":{},"pv":{},"ev":{},"acv":{},"fixedUnits":{},"lsplits":{},"x":{{{}}}}}"#,
            if i > 0 { "," } else { "" },
            t.id,
            js(&t.name),
            t.level,
            wbs[i],
            sc.summary,
            t.dur,
            sc.dur,
            t.elapsed,
            opt(t.work),
            sc.work,
            deps.join(","),
            who.join(","),
            t.pct,
            num(sc.pct),
            t.deadline.map_or("null".into(), |d| dayoff(d).to_string()),
            num(t.cost),
            num(sc.cost),
            t.delay,
            t.baseline().map_or("null".into(), |b| format!("[{},{}]", rel(b.start), rel(b.finish))),
            baselines.join(","),
            js(&t.note),
            rel(sc.es),
            rel(sc.ef),
            rel(sc.ls),
            rel(sc.lf),
            sc.slack,
            sc.free_slack,
            sc.crit,
            sc.late,
            sc.conflict,
            sc.span,
            t.baseline().map_or("null".into(), |b| cal.work_between(b.finish, sc.ef).to_string()),
            opt(t.actual_start.map(rel)),
            opt(t.actual_finish.map(rel)),
            t.actual_cost.map_or("null".into(), num),
            t.overtime,
            ranges(&t.splits),
            opt(t.manual.map(rel)),
            sc.inactive,
            t.priority,
            t.cal.as_deref().map_or("null".into(), js),
            t.sub.as_deref().map_or("null".into(), js),
            xdeps.join(","),
            ext.join(","),
            sc.done,
            sc.started,
            num(sc.bac),
            num(sc.pv),
            num(sc.ev),
            num(sc.ac),
            t.fixed_units,
            ranges(&t.lsplits),
            xf.join(",")
        );
    }
    o.push_str("]}");
    Ok(o)
}

/// Work minutes per day, by task and by person, for the usage views.
pub fn usage_json(p: &Project) -> Result<String, String> {
    let s = schedule(p)?;
    let u = task_usage(p, &s)?;
    let days = |v: &[(i64, i64)], f: f64| v.iter().map(|(d, m)| format!("[{},{}]", d - p.start, (*m as f64 * f).round() as i64)).collect::<Vec<_>>().join(",");
    let tasks: Vec<String> = p.tasks.iter().zip(&u).filter(|(_, v)| !v.is_empty()).map(|(t, v)| format!("[{},[{}]]", t.id, days(v, 1.0))).collect();
    let by_person = assignment_usage(p, &s)?;
    let mut people: Vec<String> = vec![];
    for r in p.all_resources().iter().filter(|r| r.kind == ResKind::Work) {
        let mut rows = vec![];
        for (t, parts) in p.tasks.iter().zip(&by_person) {
            if let Some((_, v)) = parts.iter().find(|(n, v)| *n == r.name && !v.is_empty()) {
                rows.push(format!("[{},[{}]]", t.id, days(v, 1.0)));
            }
        }
        people.push(format!(r#"{{"name":{},"tasks":[{}]}}"#, js(&r.name), rows.join(",")));
    }
    let costs: Vec<String> = p
        .tasks
        .iter()
        .zip(cost_days(p, &s)?)
        .filter(|(_, v)| !v.is_empty())
        .map(|(t, v)| format!("[{},[{}]]", t.id, v.iter().map(|(d, c)| format!("[{},{}]", d - p.start, num(*c))).collect::<Vec<_>>().join(",")))
        .collect();
    Ok(format!(r#"{{"ok":true,"tasks":[{}],"people":[{}],"costs":[{}]}}"#, tasks.join(","), people.join(","), costs.join(",")))
}

fn load(path: &str) -> Result<Project, String> {
    let mut p = Project::parse(&fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"))?)?;
    p.warnings = crate::external::resolve(&mut p, path);
    Ok(p)
}

fn save(path: &str, p: &Project) -> Result<(), String> {
    save_noted(path, p, "Edit", false)
}

/// Saves atomically, first remembering the old version in the history.
fn save_noted(path: &str, p: &Project, note: &str, force: bool) -> Result<(), String> {
    let text = crate::plan_text(p)?; // never write a plan that can't be scheduled
    if let Ok(old) = fs::read_to_string(path) {
        history::record(path, &old, note, force);
    }
    crate::write_atomic(path, &text)
}

fn allowed_hosts(port: u16) -> [String; 3] {
    [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")]
}

/// Security checks shared by all routes. `None` = allowed.
pub fn check(req: &Req, ctx: &Ctx) -> Option<Resp> {
    let hosts = allowed_hosts(ctx.port);
    match req.header("Host") {
        Some(h) if hosts.iter().any(|a| a.eq_ignore_ascii_case(h.trim())) => {}
        _ => return Some(Resp::err(403, "forbidden: unexpected Host header")),
    }
    if let Some(o) = req.header("Origin") {
        if !hosts.iter().any(|a| o.trim().eq_ignore_ascii_case(&format!("http://{a}"))) {
            return Some(Resp::err(403, "forbidden: cross-site request"));
        }
    }
    if req.path.starts_with("/api/") {
        let tok = req.header("X-Rplan-Token").or_else(|| req.param("token"));
        if tok != Some(ctx.token.as_str()) {
            return Some(Resp::err(403, "forbidden: missing or wrong session token — reload the page"));
        }
    }
    None
}

pub fn route(req: &Req, ctx: &Ctx) -> Resp {
    if let Some(r) = check(req, ctx) {
        return r;
    }
    ctx.last_seen.store(now_secs(), Ordering::Relaxed);
    // The page, its scripts and the keep-alive don't touch the project: answer them
    // without waiting for a long change (like leveling) to finish.
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => {
            let mut r = Resp::new(200, "text/html; charset=utf-8", UI.replace("__RPLAN_TOKEN__", &ctx.token));
            r.extra.push((
                "Content-Security-Policy".into(),
                "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; form-action 'none'; frame-ancestors 'none'; base-uri 'none'".into(),
            ));
            return r;
        }
        ("GET", a) if a.starts_with("/ui/") => {
            return match ASSETS.iter().find(|(p, _, _)| *p == a) {
                Some((_, ctype, body)) => Resp::new(200, ctype, *body),
                None => Resp::err(404, "not found"),
            }
        }
        ("GET", "/favicon.ico") => return icon("image/x-icon", ICON_ICO),
        ("GET", "/favicon.svg") => return icon("image/svg+xml", ICON_SVG),
        ("GET", "/apple-touch-icon.png" | "/icon.png") => return icon("image/png", ICON_PNG),
        ("GET", "/api/ping") => return Resp::json(200, r#"{"ok":true}"#.into()),
        _ => {}
    }
    let _guard = ctx.lock.lock().unwrap_or_else(|e| e.into_inner());
    let current = ctx.path();
    let path = current.as_str();
    let switch_to = |np: String, p: &Project| -> Resp {
        match project_json(p, &np) {
            Ok(j) => {
                remember_last(&np);
                *ctx.path.lock().unwrap_or_else(|e| e.into_inner()) = np;
                Resp::json(200, j)
            }
            Err(e) => Resp::err(400, &e),
        }
    };
    let reply = |r: Result<String, String>| match r {
        Ok(j) => Resp::json(200, j),
        Err(e) => Resp::err(400, &e),
    };
    // Writes must be based on the version the browser last saw.
    let stale = || match req.header("X-Rplan-Rev") {
        Some(r) if r != rev(path) => Some(Resp::err(409, "the file changed on disk since you loaded it — reloaded the latest version")),
        _ => None,
    };
    let mutate = |f: &dyn Fn(&mut Project) -> Result<String, String>| -> Resp {
        if let Some(r) = stale() {
            return r;
        }
        reply(load(path).and_then(|mut p| {
            let note = f(&mut p)?;
            // Big one-click changes always get their own history entry.
            save_noted(path, &p, &note, true)?;
            let j = project_json(&p, path)?;
            Ok(format!(r#"{},"note":{}}}"#, &j[..j.len() - 1], js(&note)))
        }))
    };
    let param = |name: &str| req.param(name).and_then(|v| v.parse::<usize>().ok());
    let parse_resolved = |text: &str| -> Result<Project, String> {
        let mut p = Project::parse(text)?;
        p.warnings = crate::external::resolve(&mut p, path);
        Ok(p)
    };
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/project") => reply(load(path).and_then(|p| project_json(&p, path))),
        ("GET", "/api/files") => Resp::json(200, files_json(path)),
        ("POST", "/api/open") => match sibling(path, req.body.trim()).and_then(|np| load(&np).map(|p| (np, p))) {
            Ok((np, p)) => switch_to(np, &p),
            Err(e) => Resp::err(400, &e),
        },
        ("POST", "/api/create") => {
            let name = clean(req.body.trim());
            if name.is_empty() {
                return Resp::err(400, "give the new project a name");
            }
            let np = dir_of(path).join(format!("{}.plan", safe_file_name(&name)));
            if np.exists() {
                return Resp::err(400, &format!("{} already exists — open it instead", np.file_name().unwrap_or_default().to_string_lossy()));
            }
            let np = np.to_string_lossy().to_string();
            let mut b = crate::blank_project(&np);
            b.name = name;
            if let Ok(cur) = load(path) {
                // Same working week as the project you came from.
                b.week = cur.week;
                b.day_minutes = cur.day_minutes;
                b.currency = cur.currency;
            }
            match save(&np, &b) {
                Ok(()) => switch_to(np, &b),
                Err(e) => Resp::err(400, &e),
            }
        }
        ("GET", "/api/rev") => Resp::json(200, format!(r#"{{"rev":"{}"}}"#, rev(path))),
        ("POST", "/api/save") => {
            if let Some(r) = stale() {
                return r;
            }
            let note = req.param("note").map(url_decode).unwrap_or_else(|| "Edit".into());
            reply(parse_resolved(&req.body).and_then(|p| {
                save_noted(path, &p, &note, false)?;
                project_json(&p, path)
            }))
        }
        ("POST", "/api/import") => {
            if let Some(r) = stale() {
                return r;
            }
            reply(mspdi::import(&req.body).and_then(|p| {
                let p = parse_resolved(&p.serialize())?;
                save_noted(path, &p, "Import", true)?;
                project_json(&p, path)
            }))
        }
        ("GET", "/api/usage") => reply(load(path).and_then(|p| usage_json(&p))),
        ("GET", "/api/holidays") => {
            let list: Vec<String> = COUNTRIES
                .iter()
                .map(|(code, name)| format!(r#"{{"code":"{code}","name":{},"rules":[{}]}}"#, js(name), country_holidays(code).unwrap_or(&[]).iter().map(|r| js(r)).collect::<Vec<_>>().join(",")))
                .collect();
            Resp::json(200, format!(r#"{{"ok":true,"countries":[{}]}}"#, list.join(",")))
        }
        ("GET", "/api/history") => {
            let list: Vec<String> = history::entries(path)
                .iter()
                .enumerate()
                .map(|(n, e)| {
                    let name = e.text.lines().find_map(|l| l.strip_prefix("project ")).unwrap_or("").trim();
                    let tasks = e.text.lines().filter(|l| l.trim_start().starts_with("task ")).count();
                    format!(r#"{{"n":{n},"ms":{},"note":{},"name":{},"tasks":{tasks},"size":{}}}"#, e.ms, js(&e.note), js(name), e.text.len())
                })
                .collect();
            Resp::json(200, format!(r#"{{"ok":true,"entries":[{}]}}"#, list.join(",")))
        }
        ("GET", "/api/history/diff") => {
            let entries = history::entries(path);
            let Some(e) = param("n").and_then(|n| entries.get(n)) else { return Resp::err(400, "no such version") };
            let current = fs::read_to_string(path).unwrap_or_default();
            reply(history::diff(&e.text, &current).map(|d| {
                let list = |v: &[String]| v.iter().map(|x| js(x)).collect::<Vec<_>>().join(",");
                let changed: Vec<String> = d.changed.iter().map(|(t, f)| format!(r#"{{"task":{},"fields":[{}]}}"#, js(t), list(f))).collect();
                // "added" = in the current file but not in that version, i.e. restoring removes them.
                format!(r#"{{"ok":true,"added":[{}],"removed":[{}],"changed":[{}],"settings":{},"text":{}}}"#, list(&d.added), list(&d.removed), changed.join(","), d.settings, js(&e.text))
            }))
        }
        ("POST", "/api/history/restore") => {
            if let Some(r) = stale() {
                return r;
            }
            let entries = history::entries(path);
            let Some(e) = param("n").and_then(|n| entries.get(n)) else { return Resp::err(400, "no such version") };
            reply(parse_resolved(&e.text).and_then(|p| {
                save_noted(path, &p, "Restore an earlier version", true)?;
                project_json(&p, path)
            }))
        }
        ("POST", "/api/level") => mutate(&|p| {
            let n = level(p)?.len();
            Ok(if n == 0 { "Nothing to level — nobody is over-allocated ✓".into() } else { format!("Leveled: delayed {n} task(s) so nobody is double-booked") })
        }),
        ("POST", "/api/unlevel") => mutate(&|p| Ok(format!("Removed leveling delays from {} task(s)", clear_leveling(p)))),
        ("POST", "/api/progress") => mutate(&|p| {
            let n = update_progress(p)?;
            Ok(if n == 0 { "Progress is already up to the status date ✓".into() } else { format!("Updated progress on {n} task(s) to the status date") })
        }),
        ("POST", "/api/baseline") => {
            let n = param("n").unwrap_or(0).min(BASELINES - 1);
            mutate(&|p| set_baseline(p, n).map(|_| if n == 0 { "Baseline saved — later changes show as slips".into() } else { format!("Saved baseline {n}") }))
        }
        ("POST", "/api/baseline/clear") => {
            let n = param("n").unwrap_or(0).min(BASELINES - 1);
            mutate(&|p| {
                clear_baseline(p, n);
                Ok(if n == 0 { "Baseline cleared".into() } else { format!("Baseline {n} cleared") })
            })
        }
        ("POST", "/api/new") => mutate(&|p| {
            // Start fresh but keep the working week, hours per day and currency.
            let mut b = crate::blank_project(path);
            b.week = p.week.clone();
            b.day_minutes = p.day_minutes;
            b.currency = p.currency.clone();
            *p = b;
            Ok("Started a new, empty project — Ctrl+Z brings the old one back".into())
        }),
        ("POST", "/api/sample") => mutate(&|p| {
            *p = Project::parse(crate::DEMO)?;
            Ok("Loaded the sample project — Ctrl+Z brings yours back".into())
        }),
        ("GET", "/api/export.html") => match load(path).and_then(|p| report::html(&p).map(|h| (p, h))) {
            Ok((p, h)) => Resp::download("text/html; charset=utf-8", &format!("{}.html", p.name), h),
            Err(e) => Resp::err(400, &e),
        },
        ("GET", "/api/export.xml") => match load(path).and_then(|p| mspdi::export(&p).map(|x| (p, x))) {
            Ok((p, x)) => Resp::download("application/xml; charset=utf-8", &format!("{}.xml", p.name), x),
            Err(e) => Resp::err(400, &e),
        },
        ("GET", "/api/export.plan") => match fs::read_to_string(path) {
            Ok(t) => Resp::download("text/plain; charset=utf-8", path.rsplit(['/', '\\']).next().unwrap_or("project.plan"), t),
            Err(e) => Resp::err(400, &e.to_string()),
        },
        _ => Resp::err(404, "not found"),
    }
}

fn read_req(st: &TcpStream) -> Result<Req, Resp> {
    let mut rd = BufReader::new(st).take(MAX_HEAD);
    let mut line = String::new();
    rd.read_line(&mut line).map_err(|_| Resp::err(400, "bad request"))?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let (path, query) = target.split_once('?').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((target.clone(), String::new()));
    let mut headers = vec![];
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if rd.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" || h == "\n" {
            break;
        }
        if headers.len() > 100 {
            return Err(Resp::err(431, "too many headers"));
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().map_err(|_| Resp::err(400, "bad content-length"))?;
                if len > MAX_BODY {
                    return Err(Resp::err(413, "request too large"));
                }
            }
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    if rd.limit() == 0 {
        return Err(Resp::err(431, "request headers too large"));
    }
    rd.set_limit(len as u64);
    let mut body = vec![0u8; len];
    rd.read_exact(&mut body).map_err(|_| Resp::err(400, "incomplete body"))?;
    let body = String::from_utf8(body).map_err(|_| Resp::err(400, "body must be UTF-8"))?;
    Ok(Req { method, path, query, headers, body })
}

fn write_resp(mut st: &TcpStream, r: Resp) {
    let reason = match r.code {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    };
    let mut head = format!(
        "HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\n{}X-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n",
        r.code,
        r.ctype,
        r.body.len(),
        if r.extra.iter().any(|(k, _)| k.eq_ignore_ascii_case("Cache-Control")) { "" } else { "Cache-Control: no-store\r\n" }
    );
    for (k, v) in &r.extra {
        let _ = write!(head, "{k}: {v}\r\n");
    }
    head.push_str("\r\n");
    let _ = st.write_all(head.as_bytes());
    let _ = st.write_all(&r.body);
    let _ = st.flush();
}

pub fn open_browser(url: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

/// Chromium-based browsers that support `--app` windows, most preferred first.
pub fn find_app_browser() -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = vec![];
    if cfg!(windows) {
        for base in [std::env::var("ProgramFiles(x86)").ok(), std::env::var("ProgramFiles").ok(), std::env::var("LOCALAPPDATA").ok()].into_iter().flatten() {
            c.push(Path::new(&base).join(r"Microsoft\Edge\Application\msedge.exe"));
            c.push(Path::new(&base).join(r"Google\Chrome\Application\chrome.exe"));
        }
    } else if cfg!(target_os = "macos") {
        c.push("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into());
        c.push("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into());
        c.push("/Applications/Chromium.app/Contents/MacOS/Chromium".into());
    } else {
        for n in ["google-chrome", "chromium", "chromium-browser", "microsoft-edge"] {
            c.push(Path::new("/usr/bin").join(n));
        }
    }
    c.into_iter().find(|p| p.is_file())
}

/// Where the Mac app keeps the name of the project it had open last.
#[cfg(target_os = "macos")]
fn last_file() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| Path::new(&h).join("Library/Application Support/rplan/last-project"))
}

/// Remembers `path` as the project to reopen when the Mac app is started from the Finder.
pub fn remember_last(path: &str) {
    #[cfg(target_os = "macos")]
    if let (Some(f), Ok(abs)) = (last_file(), std::fs::canonicalize(path)) {
        let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new(".")));
        let _ = std::fs::write(f, abs.to_string_lossy().as_bytes());
    }
    #[cfg(not(target_os = "macos"))]
    let _ = path;
}

/// The project the Mac app had open last, if it still exists.
#[cfg(target_os = "macos")]
pub fn last_project() -> Option<String> {
    let p = std::fs::read_to_string(last_file()?).ok()?;
    let p = p.trim();
    Path::new(p).is_file().then(|| p.to_string())
}

fn app_profile_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").or_else(|_| std::env::var("HOME").map(|h| format!("{h}/.local/share"))).unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().to_string());
    Path::new(&base).join("rplan").join("window")
}

/// Opens the editor in its own window. Returns true if we can tell when it closes.
fn open_window(url: &str, ctx: Arc<Ctx>) -> bool {
    let Some(browser) = find_app_browser() else {
        open_browser(url);
        return false;
    };
    let child = std::process::Command::new(browser)
        .arg(format!("--app={url}"))
        .arg(format!("--user-data-dir={}", app_profile_dir().display()))
        .args(["--no-first-run", "--no-default-browser-check", "--window-size=1440,900"])
        .spawn();
    let Ok(mut child) = child else {
        open_browser(url);
        return false;
    };
    thread::spawn(move || {
        let t0 = std::time::Instant::now();
        let _ = child.wait();
        if t0.elapsed() > Duration::from_secs(5) {
            std::process::exit(0); // the window was closed
        }
        // The browser handed the window to an already-running instance, so we
        // can't wait on it: quit once the page stops checking in.
        loop {
            thread::sleep(Duration::from_secs(10));
            if now_secs().saturating_sub(ctx.last_seen.load(Ordering::Relaxed)) > 150 {
                std::process::exit(0);
            }
        }
    });
    true
}

/// When started from a shortcut or a double-click, Windows gives us a console
/// window of our own; hide it once the app window is up. Leaves terminals alone.
#[cfg(windows)]
fn hide_own_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        fn FreeConsole() -> i32;
    }
    let mut ids = [0u32; 4];
    unsafe {
        if GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) == 1 {
            FreeConsole();
        }
    }
}

pub fn serve(path: &str, port: u16, launch: Launch) -> Result<(), String> {
    load(path)?;
    let listener = (port..port.saturating_add(20)).find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok()).ok_or("could not open a local port")?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let ctx = Arc::new(Ctx::new(path, port, new_token()));
    let url = format!("http://127.0.0.1:{port}/");
    let _ = writeln!(std::io::stdout(), "{GRN}✓ rplan is running at {BLD}{url}{RST}  {DIM}(editing {path} — press Ctrl+C to stop){RST}");
    match launch {
        Launch::None => {}
        Launch::Browser => open_browser(&url),
        Launch::Window => {
            #[cfg(any(windows, target_os = "macos"))]
            {
                let accept = Arc::clone(&ctx);
                thread::spawn(move || accept_loop(listener, accept));
                let title = format!("{} — rplan", Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
                #[cfg(windows)]
                hide_own_console();
                remember_last(path);
                let opened = Arc::clone(&ctx);
                let on_open = move |f: String| {
                    if load(&f).is_err() {
                        return false;
                    }
                    remember_last(&f);
                    *opened.path.lock().unwrap_or_else(|e| e.into_inner()) = f;
                    true
                };
                let err = crate::window::run(&url, &title, || std::process::exit(0), on_open).err().unwrap_or_default();
                // No usable web view (e.g. WebView2 missing): use a browser window instead.
                let _ = writeln!(std::io::stderr(), "rplan: the app window could not start ({err}); using a browser window");
                open_window(&url, Arc::clone(&ctx));
                loop {
                    thread::park();
                }
            }
            #[cfg(not(any(windows, target_os = "macos")))]
            open_window(&url, Arc::clone(&ctx));
        }
    }
    accept_loop(listener, ctx);
    Ok(())
}

fn accept_loop(listener: TcpListener, ctx: Arc<Ctx>) {
    for st in listener.incoming().flatten() {
        let ctx = Arc::clone(&ctx);
        thread::spawn(move || {
            let _ = st.set_read_timeout(Some(Duration::from_secs(15)));
            let _ = st.set_write_timeout(Some(Duration::from_secs(15)));
            let resp = match read_req(&st) {
                Ok(req) => route(&req, &ctx),
                Err(r) => r,
            };
            write_resp(&st, resp);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        let dir = std::env::temp_dir().join(format!("rplan-test-{}", new_token()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.plan").to_string_lossy().to_string();
        fs::write(&path, crate::DEMO).unwrap();
        Ctx::new(&path, 7999, "secret".into())
    }
    fn req(method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Req {
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        Req {
            method: method.into(),
            path: path.into(),
            query: query.into(),
            headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            body: body.into(),
        }
    }
    const OK: (&str, &str) = ("Host", "127.0.0.1:7999");
    const TOK: (&str, &str) = ("X-Rplan-Token", "secret");

    #[test]
    fn blocks_cross_site_and_rebinding() {
        let c = ctx();
        assert_eq!(route(&req("POST", "/api/save", &[OK], "x"), &c).code, 403, "no token");
        assert_eq!(route(&req("POST", "/api/save", &[OK, ("X-Rplan-Token", "guess")], "x"), &c).code, 403, "wrong token");
        assert_eq!(route(&req("GET", "/api/project", &[("Host", "evil.com:7999"), TOK], ""), &c).code, 403, "DNS rebinding");
        assert_eq!(route(&req("GET", "/", &[("Host", "evil.com")], ""), &c).code, 403, "page via foreign host");
        assert_eq!(route(&req("POST", "/api/level", &[OK, TOK, ("Origin", "https://evil.com")], ""), &c).code, 403, "foreign origin");
        assert_eq!(route(&req("GET", "/api/project", &[OK], ""), &c).code, 403, "reading needs the token too");
        // The file was never touched.
        assert_eq!(fs::read_to_string(c.path()).unwrap(), crate::DEMO);
    }

    #[test]
    fn serves_page_and_api_with_token() {
        let c = ctx();
        let page = route(&req("GET", "/", &[OK], ""), &c);
        assert_eq!(page.code, 200);
        assert!(String::from_utf8(page.body).unwrap().contains("\"secret\""));
        let r = route(&req("GET", "/api/project", &[OK, TOK, ("Origin", "http://127.0.0.1:7999")], ""), &c);
        assert_eq!(r.code, 200);
        let body = String::from_utf8(r.body).unwrap();
        assert!(body.starts_with(r#"{"ok":true"#) && body.contains("Backend API"));
        assert_eq!(route(&req("GET", "/api/export.xml?token=secret", &[OK], ""), &c).code, 200);
        assert_eq!(route(&req("POST", "/api/level", &[OK, TOK], ""), &c).code, 200);
        assert!(fs::read_to_string(c.path()).unwrap().contains("delay="));
    }

    #[test]
    fn open_and_create_project_files_in_the_same_folder() {
        let c = ctx();
        let dir = dir_of(&c.path());
        let body = |r: Resp| String::from_utf8(r.body).unwrap();
        // create a new project next to the current one; it becomes the current file
        let r = route(&req("POST", "/api/create", &[OK, TOK], "Office: Move?"), &c);
        let code = r.code;
        assert_eq!(code, 200, "{}", body(r));
        assert!(c.path().ends_with("Office- Move-.plan"), "{}", c.path());
        assert!(fs::read_to_string(c.path()).unwrap().starts_with("project Office: Move?\n"));
        assert_eq!(route(&req("POST", "/api/create", &[OK, TOK], "Office: Move?"), &c).code, 400, "no overwrite");
        // list and switch back
        let files = body(route(&req("GET", "/api/files", &[OK, TOK], ""), &c));
        assert!(files.contains(r#""file":"t.plan""#) && files.contains(r#""name":"Website Launch""#), "{files}");
        assert_eq!(route(&req("POST", "/api/open", &[OK, TOK], "t.plan"), &c).code, 200);
        assert!(c.path().ends_with("t.plan"));
        // nothing outside the folder, nothing that isn't a .plan
        fs::write(dir.join("notes.txt"), "x").unwrap();
        for bad in ["../t.plan", "..\\t.plan", "C:\\Windows\\win.ini", "/etc/passwd", "notes.txt", "missing.plan", ".plan", ""] {
            assert_eq!(route(&req("POST", "/api/open", &[OK, TOK], bad), &c).code, 400, "{bad}");
        }
        assert!(c.path().ends_with("t.plan"));
        assert_eq!(route(&req("POST", "/api/open", &[OK], "t.plan"), &c).code, 403, "needs the token");
        assert_eq!(safe_file_name("CON"), "CON project");
        assert_eq!(safe_file_name("  a/b\\c.  "), "a-b-c");
    }

    #[test]
    fn serves_ui_scripts_under_a_strict_policy() {
        let c = ctx();
        let page = route(&req("GET", "/", &[OK], ""), &c);
        let csp = page.extra.iter().find(|(k, _)| k == "Content-Security-Policy").map(|(_, v)| v.clone()).unwrap();
        assert!(csp.contains("script-src 'self'") && !csp.contains("script-src 'unsafe-inline'"), "{csp}");
        for (path, _, _) in ASSETS {
            let r = route(&req("GET", path, &[OK], ""), &c);
            assert_eq!(r.code, 200, "{path}");
            assert!(!r.body.is_empty());
        }
        assert_eq!(route(&req("GET", "/ui/nope.js", &[OK], ""), &c).code, 404);
        assert_eq!(url_decode("Le%C3%B3n+%E2%80%94+ok%2"), "León — ok%2");
    }

    #[test]
    fn serves_the_favicon() {
        let c = ctx();
        let r = route(&req("GET", "/favicon.ico", &[OK], ""), &c);
        assert_eq!((r.code, r.ctype), (200, "image/x-icon"));
        assert_eq!(&r.body[..4], &[0, 0, 1, 0], "ICO header");
        assert_eq!(route(&req("GET", "/favicon.svg", &[OK], ""), &c).ctype, "image/svg+xml");
        assert_eq!(route(&req("GET", "/favicon.ico", &[("Host", "evil.com")], ""), &c).code, 403);
        let page = String::from_utf8(route(&req("GET", "/", &[OK], ""), &c).body).unwrap();
        assert!(page.contains(r#"href="/favicon.ico""#));
        assert!(crate::report::html(&Project::parse(crate::DEMO).unwrap()).unwrap().contains(r#"<link rel="icon" href="data:image/svg+xml,"#));
    }

    #[test]
    fn new_blank_project_and_sample() {
        let c = ctx();
        fs::write(c.path(), "project X\nstart 2026-11-09\nweek Sat 09:00-13:00\ndayhours 7\ntask 1 | A | 1d").unwrap();
        assert_eq!(route(&req("POST", "/api/new", &[OK, TOK], ""), &c).code, 200);
        let p = Project::parse(&fs::read_to_string(c.path()).unwrap()).unwrap();
        assert!(p.tasks.is_empty());
        assert_eq!(p.name, "t", "named after the file");
        assert_eq!(p.start, crate::today());
        assert_eq!((p.week[6].clone(), p.day_minutes), (vec![(540, 780)], 420), "keeps the calendar");
        assert_eq!(route(&req("POST", "/api/sample", &[OK, TOK], ""), &c).code, 200);
        assert!(fs::read_to_string(c.path()).unwrap().contains("Website Launch"));
        assert_eq!(crate::blank_project("C:/x/My Project.plan").name, "My Project");
    }

    #[test]
    fn never_writes_a_plan_that_would_not_read_back() {
        let c = ctx();
        let mut p = Project::parse(crate::DEMO).unwrap();
        p.resources[0].capacity = 0;
        assert!(save(&c.path(), &p).unwrap_err().contains("capacity"));
        assert_eq!(fs::read_to_string(c.path()).unwrap(), crate::DEMO, "the file is untouched");
        assert!(!Path::new(&format!("{}.tmp", c.path())).exists());
    }

    #[test]
    fn rejects_bad_plans_and_stale_writes() {
        let c = ctx();
        let r = route(&req("POST", "/api/save", &[OK, TOK], "start 2026-11-09\ntask 1 | A | 1d | 2\ntask 2 | B | 1d | 1"), &c);
        assert_eq!(r.code, 400);
        assert!(String::from_utf8(r.body).unwrap().contains("loop"));
        assert_eq!(route(&req("POST", "/api/save", &[OK, TOK, ("X-Rplan-Rev", "1")], crate::DEMO), &c).code, 409);
        let cur = rev(&c.path());
        assert_eq!(route(&req("POST", "/api/save", &[OK, TOK, ("X-Rplan-Rev", &cur)], crate::DEMO), &c).code, 200);
        assert_eq!(fs::read_to_string(c.path()).unwrap(), Project::parse(crate::DEMO).unwrap().serialize());
    }
}
