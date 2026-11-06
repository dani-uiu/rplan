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
use crate::{mspdi, report};
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

const UI: &str = include_str!("ui.html");
pub const ICON_ICO: &[u8] = include_bytes!("../assets/rplan.ico");
pub const ICON_SVG: &[u8] = include_bytes!("../assets/rplan.svg");
pub const ICON_PNG: &[u8] = include_bytes!("../assets/rplan.png");

fn icon(ctype: &'static str, bytes: &[u8]) -> Resp {
    let mut r = Resp::new(200, ctype, bytes.to_vec());
    r.extra.push(("Cache-Control".into(), "public, max-age=86400".into()));
    r
}
const MAX_BODY: usize = 16 << 20;

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
                    let name = fs::read_to_string(e.path()).ok().and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("project ").map(|n| n.trim().to_string()))).unwrap_or_default();
                    (modified, file, name)
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by(|a, b| b.0.cmp(&a.0));
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
/// Times are minutes since midnight of the project start day.
pub fn project_json(p: &Project, path: &str) -> Result<String, String> {
    let s = schedule(p)?;
    let t0 = p.start * DAY;
    let cal = p.calendar()?;
    let wbs = p.wbs();
    let rel = |t: i64| t - t0;
    let dayoff = |d: i64| d - p.start;
    let hours = |h: &Hours| format!("[{}]", h.iter().map(|(a, b)| format!("[{a},{b}]")).collect::<Vec<_>>().join(","));
    let mut o = String::new();
    let _ = write!(
        o,
        r#"{{"ok":true,"path":{},"rev":"{}","name":{},"start":"{}","currency":{},"dayMinutes":{},"week":[{}],"exceptions":[{}],"#,
        js(path),
        rev(path),
        js(&p.name),
        fmt_date(p.start),
        js(&p.currency),
        p.day_minutes,
        p.week.iter().map(hours).collect::<Vec<_>>().join(","),
        p.exceptions
            .iter()
            .map(|e| format!(r#"{{"from":{},"to":{},"name":{},"hours":{}}}"#, dayoff(e.from), dayoff(e.to), js(&e.name), hours(&e.hours)))
            .collect::<Vec<_>>()
            .join(",")
    );
    let declared: Vec<&str> = p.resources.iter().map(|r| r.name.as_str()).collect();
    let ress: Vec<String> = p
        .all_resources()
        .iter()
        .map(|r| {
            format!(
                r#"{{"name":{},"rate":{},"capacity":{},"declared":{},"off":[{}]}}"#,
                js(&r.name),
                num(r.rate),
                r.capacity,
                declared.contains(&r.name.as_str()),
                r.off.iter().map(|(a, b)| format!("[{},{}]", dayoff(*a), dayoff(*b))).collect::<Vec<_>>().join(",")
            )
        })
        .collect();
    let lds: Vec<String> = loads(p, &s)
        .iter()
        .map(|l| {
            let days: Vec<String> = l
                .days
                .iter()
                .map(|(d, (u, ids))| format!("[{},{u},[{}]]", dayoff(*d), ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")))
                .collect();
            format!(
                r#"{{"name":{},"capacity":{},"work":{},"cost":{},"tasks":{},"over":{},"days":[{}]}}"#,
                js(&l.name),
                l.capacity,
                l.work,
                num(l.cost),
                l.tasks,
                l.over_days,
                days.join(",")
            )
        })
        .collect();
    let _ = write!(
        o,
        r#""resources":[{}],"loads":[{}],"startT":{},"finish":{},"finishDate":"{}","span":{},"progress":{},"cost":{},"critical":[{}],"text":{},"tasks":["#,
        ress.join(","),
        lds.join(","),
        rel(s.start),
        rel(s.finish),
        fmt_date((s.finish - 1).div_euclid(DAY).max(p.start)),
        cal.work_between(s.start, s.finish),
        num(s.progress),
        num(s.cost),
        s.critical.iter().map(u32::to_string).collect::<Vec<_>>().join(","),
        js(&p.serialize())
    );
    for (i, t) in p.tasks.iter().enumerate() {
        let sc = s.tasks[i];
        let deps: Vec<String> = t.deps.iter().map(|d| format!(r#"{{"id":{},"kind":"{}","lag":{}}}"#, d.id, d.kind.as_str(), d.lag)).collect();
        let who: Vec<String> = t.who.iter().map(|a| format!(r#"{{"name":{},"units":{}}}"#, js(&a.name), a.units)).collect();
        let cons = match t.constraint {
            Constraint::Asap => "null".to_string(),
            Constraint::Snet(x) => format!(r#"{{"type":"snet","at":{}}}"#, rel(x)),
            Constraint::Fnet(x) => format!(r#"{{"type":"fnet","at":{}}}"#, rel(x)),
            Constraint::Mso(x) => format!(r#"{{"type":"mso","at":{}}}"#, rel(x)),
        };
        let _ = write!(
            o,
            r#"{}{{"id":{},"name":{},"level":{},"wbs":"{}","summary":{},"dur":{},"deps":[{}],"who":[{}],"pct":{},"pctR":{},"constraint":{cons},"deadline":{},"fixed":{},"cost":{},"delay":{},"base":{},"note":{},"es":{},"ef":{},"ls":{},"lf":{},"slack":{},"crit":{},"late":{},"conflict":{},"span":{},"slip":{}}}"#,
            if i > 0 { "," } else { "" },
            t.id,
            js(&t.name),
            t.level,
            wbs[i],
            sc.summary,
            t.dur,
            deps.join(","),
            who.join(","),
            t.pct,
            num(sc.pct),
            t.deadline.map_or("null".into(), |d| dayoff(d).to_string()),
            num(t.cost),
            num(sc.cost),
            t.delay,
            t.baseline.map_or("null".into(), |(a, b)| format!("[{},{}]", rel(a), rel(b))),
            js(&t.note),
            rel(sc.es),
            rel(sc.ef),
            rel(sc.ls),
            rel(sc.lf),
            sc.slack,
            sc.crit,
            sc.late,
            sc.conflict,
            sc.span,
            t.baseline.map_or("null".into(), |(_, bf)| cal.work_between(bf, sc.ef).to_string())
        );
    }
    o.push_str("]}");
    Ok(o)
}

fn load(path: &str) -> Result<Project, String> {
    Project::parse(&fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"))?)
}

fn save(path: &str, p: &Project) -> Result<(), String> {
    schedule(p)?; // never write a plan that can't be scheduled
    let tmp = format!("{path}.tmp");
    fs::write(&tmp, p.serialize()).map_err(|e| format!("can't write {tmp}: {e}"))?;
    fs::rename(&tmp, path).map_err(|e| format!("can't replace {path}: {e}"))
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
    let _guard = ctx.lock.lock().unwrap_or_else(|e| e.into_inner());
    let current = ctx.path();
    let path = current.as_str();
    let switch_to = |np: String, p: &Project| -> Resp {
        match project_json(p, &np) {
            Ok(j) => {
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
            save(path, &p)?;
            let j = project_json(&p, path)?;
            Ok(format!(r#"{},"note":{}}}"#, &j[..j.len() - 1], js(&note)))
        }))
    };
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => {
            let mut r = Resp::new(200, "text/html; charset=utf-8", UI.replace("__RPLAN_TOKEN__", &ctx.token));
            r.extra.push((
                "Content-Security-Policy".into(),
                "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; form-action 'none'; frame-ancestors 'none'; base-uri 'none'".into(),
            ));
            r
        }
        ("GET", "/favicon.ico") => icon("image/x-icon", ICON_ICO),
        ("GET", "/favicon.svg") => icon("image/svg+xml", ICON_SVG),
        ("GET", "/apple-touch-icon.png" | "/icon.png") => icon("image/png", ICON_PNG),
        ("GET", "/api/project") => reply(load(path).and_then(|p| project_json(&p, path))),
        ("GET", "/api/ping") => Resp::json(200, r#"{"ok":true}"#.into()),
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
            reply(Project::parse(&req.body).and_then(|p| {
                save(path, &p)?;
                project_json(&p, path)
            }))
        }
        ("POST", "/api/import") => {
            if let Some(r) = stale() {
                return r;
            }
            reply(mspdi::import(&req.body).and_then(|p| {
                save(path, &p)?;
                project_json(&p, path)
            }))
        }
        ("POST", "/api/level") => mutate(&|p| {
            let n = level(p)?.len();
            Ok(if n == 0 { "Nothing to level — nobody is over-allocated ✓".into() } else { format!("Leveled: delayed {n} task(s) so nobody is double-booked") })
        }),
        ("POST", "/api/unlevel") => mutate(&|p| Ok(format!("Removed leveling delays from {} task(s)", clear_leveling(p)))),
        ("POST", "/api/baseline") => mutate(&|p| set_baseline(p).map(|_| "Baseline saved — later changes show as slips".into())),
        ("POST", "/api/baseline/clear") => mutate(&|p| {
            clear_baseline(p);
            Ok("Baseline cleared".into())
        }),
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
    let mut rd = BufReader::new(st);
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

fn open_browser(url: &str) {
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
            if open_window(&url, Arc::clone(&ctx)) {
                #[cfg(windows)]
                hide_own_console();
            }
        }
    }
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
    Ok(())
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
        assert_eq!(fs::read_to_string(&c.path()).unwrap(), crate::DEMO);
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
        assert!(fs::read_to_string(&c.path()).unwrap().contains("delay="));
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
        let p = Project::parse(&fs::read_to_string(&c.path()).unwrap()).unwrap();
        assert!(p.tasks.is_empty());
        assert_eq!(p.name, "t", "named after the file");
        assert_eq!(p.start, crate::today());
        assert_eq!((p.week[6].clone(), p.day_minutes), (vec![(540, 780)], 420), "keeps the calendar");
        assert_eq!(route(&req("POST", "/api/sample", &[OK, TOK], ""), &c).code, 200);
        assert!(fs::read_to_string(&c.path()).unwrap().contains("Website Launch"));
        assert_eq!(crate::blank_project("C:/x/My Project.plan").name, "My Project");
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
        assert_eq!(fs::read_to_string(&c.path()).unwrap(), Project::parse(crate::DEMO).unwrap().serialize());
    }
}
