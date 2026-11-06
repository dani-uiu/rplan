//! rplan — a fast, plain-text alternative to ProjectLibre.
//!
//! Modules:
//! - `calendar`: dates, working hours, holidays and working-time math
//! - `model`:    the project model and the `.plan` text format
//! - `schedule`: critical path scheduling, workloads, leveling, baselines
//! - `mspdi`:    Microsoft Project XML import/export (ProjectLibre-compatible)
//! - `report`:   terminal reports and HTML export
//! - `server`:   the local web server behind `rplan ui`

mod calendar;
mod model;
mod mspdi;
mod report;
mod schedule;
mod server;

use calendar::*;
use model::*;
use report::{GRN, RST};
use std::io::IsTerminal;
use std::{env, fs, process};

pub const DEMO: &str = "project Website Launch
start 2026-11-09
week Mon-Fri 08:00-12:00 13:00-17:00
week Sat,Sun off
holiday 2026-11-26 Thanksgiving
resource PM | rate 95
resource Ana | rate 85
resource Bo | rate 70
resource Cy | rate 60 | off 2026-11-30
task 1 | Kickoff | 0 | | PM | 100
task 10 | Planning & design |
  task 2 | Requirements | 3d | 1 | Ana | 100
  task 3 | UX design | 5d | 2 | Bo | 60
task 11 | Build |
  task 4 | Backend API | 8d | 2 | Ana | 20
  task 5 | Frontend | 6d | 3 | Bo | 0
  task 6 | Content writing | 4d | 2 | Ana | 0
  task 7 | Integration | 3d | 4,5 | Ana | 0
task 8 | QA & fixes | 4d | 7,6+1d | Cy | 0 | deadline=2026-12-11
task 9 | Launch | 0 | 8 | PM | 0
";

const HELP: &str = "rplan — plain-text project scheduling (critical path, Gantt, resources)

OPEN THE APP
  rplan app <file>                            desktop window (closes rplan when you close it)
  rplan ui <file> [--port 7878] [--no-open]   same editor in a browser tab

PROJECT
  rplan init <file> \"<name>\" <YYYY-MM-DD>     new empty project
  rplan demo <file>                           sample project
  rplan import <in.xml> <file>                from ProjectLibre / MS Project XML
  rplan export <file> <out.xml>               to ProjectLibre / MS Project XML
  rplan html <file> <out.html>                shareable, printable Gantt

TASKS
  rplan add <file> \"<name>\" <duration> [--after 2,3SS+1d] [--who \"Ana,Bo[50%]\"] [--under <id>]
  rplan set <file> <id> [--name ..] [--dur 3d] [--after ..] [--who ..] [--done 50]
                        [--snet DATE|none] [--deadline DATE|none] [--cost 500] [--note ..]
  rplan rm <file> <id>                        (also removes its subtasks)
  rplan indent|outdent <file> <id>            make / unmake a subtask

CALENDAR
  rplan week <file> <Mon-Fri> <08:00-12:00 13:00-17:00 | off>
  rplan dayhours <file> <hours>               hours in one day of duration
  rplan holiday <file> <DATE[..DATE]> [name]
  rplan workday <file> <DATE[..DATE]> [HH:MM-HH:MM ...] [name]
  rplan resource <file> <name> [--rate 85] [--capacity 50] [--off DATE[..DATE],...]

REPORTS & TOOLS
  rplan show | gantt | resources <file>
  rplan level <file>          delay flexible tasks so nobody is over-booked
  rplan unlevel <file>        remove leveling delays
  rplan baseline <file> [clear]

Durations: 3d, 4h, 30m, 2w, 0 = milestone. Links: 3 (finish-to-start), 3SS, 3FF, 3SF, with lag 3+2d or lead 3-4h.";

#[cfg(windows)]
fn enable_ansi() {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(n: u32) -> *mut c_void;
        fn GetConsoleMode(h: *mut c_void, m: *mut u32) -> i32;
        fn SetConsoleMode(h: *mut c_void, m: u32) -> i32;
    }
    // STD_OUTPUT_HANDLE / STD_ERROR_HANDLE; 4 = ENABLE_VIRTUAL_TERMINAL_PROCESSING
    for n in [-11i32 as u32, -12i32 as u32] {
        unsafe {
            let h = GetStdHandle(n);
            let mut m = 0u32;
            if GetConsoleMode(h, &mut m) != 0 {
                SetConsoleMode(h, m | 4);
            }
        }
    }
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' && it.peek() == Some(&'[') {
            for d in it.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn out(s: &str) {
    if std::io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none() {
        print!("{s}");
    } else {
        print!("{}", strip_ansi(s));
    }
}

fn load(path: &str) -> Result<Project, String> {
    Project::parse(&fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"))?)
}

fn save(path: &str, p: &Project) -> Result<(), String> {
    schedule::schedule(p)?;
    fs::write(path, p.serialize()).map_err(|e| format!("can't write {path}: {e}"))
}

fn flag(a: &[String], name: &str) -> Option<String> {
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1)).cloned()
}

fn index_of(p: &Project, id: &str) -> Result<usize, String> {
    let id: u32 = id.trim_start_matches('#').parse().map_err(|_| format!("'{id}' isn't a task id"))?;
    p.tasks.iter().position(|t| t.id == id).ok_or(format!("there's no task {id}"))
}

/// One past the last task inside `i`'s subtree.
fn subtree_end(p: &Project, i: usize) -> usize {
    let lv = p.tasks[i].level;
    let mut j = i + 1;
    while j < p.tasks.len() && p.tasks[j].level > lv {
        j += 1;
    }
    j
}

/// Today's date (UTC) as a day number.
pub fn today() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64 / 86_400).unwrap_or(0)
}

/// An empty project named after its file (`My Project.plan` → "My Project"), starting today.
pub fn blank_project(path: &str) -> Project {
    let stem = std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or("New project");
    let name = clean(stem);
    Project { name: if name.is_empty() { "New project".into() } else { name }, start: today(), ..Default::default() }
}

fn ok(msg: &str) {
    out(&format!("{GRN}✓ {msg}{RST}\n"));
}

fn run() -> Result<(), String> {
    let a: Vec<String> = env::args().skip(1).collect();
    let need = |n: usize| if a.len() < n { Err(format!("not enough arguments\n\n{HELP}")) } else { Ok(()) };
    let cmd = a.first().map(String::as_str).unwrap_or("");
    match cmd {
        "ui" | "app" => {
            need(2)?;
            if fs::metadata(&a[1]).is_err() {
                save(&a[1], &blank_project(&a[1]))?;
                ok(&format!("Created a new, empty project in {}", a[1]));
            }
            let port = flag(&a, "--port").and_then(|v| v.parse().ok()).unwrap_or(7878);
            let launch = if a.iter().any(|x| x == "--no-open") {
                server::Launch::None
            } else if cmd == "app" {
                server::Launch::Window
            } else {
                server::Launch::Browser
            };
            server::serve(&a[1], port, launch)?;
        }
        "init" => {
            need(4)?;
            let p = Project { name: a[2].clone(), start: parse_date(&a[3])?, ..Default::default() };
            save(&a[1], &p)?;
            ok(&format!("Created {}", a[1]));
        }
        "demo" => {
            need(2)?;
            let p = Project::parse(DEMO)?;
            save(&a[1], &p)?;
            ok(&format!("Wrote the sample project to {}\n", a[1]));
            out(&report::gantt(&p)?);
        }
        "import" => {
            need(3)?;
            let xml = fs::read_to_string(&a[1]).map_err(|e| format!("can't read {}: {e}", a[1]))?;
            let p = mspdi::import(&xml)?;
            save(&a[2], &p)?;
            ok(&format!("Imported {} tasks and {} people into {}", p.tasks.len(), p.resources.len(), a[2]));
        }
        "export" => {
            need(3)?;
            fs::write(&a[2], mspdi::export(&load(&a[1])?)?).map_err(|e| e.to_string())?;
            ok(&format!("Wrote {} (open it in ProjectLibre or MS Project)", a[2]));
        }
        "html" => {
            need(3)?;
            fs::write(&a[2], report::html(&load(&a[1])?)?).map_err(|e| e.to_string())?;
            ok(&format!("Wrote {}", a[2]));
        }
        "show" => {
            need(2)?;
            out(&report::show(&load(&a[1])?)?);
        }
        "gantt" => {
            need(2)?;
            out(&report::gantt(&load(&a[1])?)?);
        }
        "resources" => {
            need(2)?;
            out(&report::resources(&load(&a[1])?)?);
        }
        "level" => {
            need(2)?;
            let mut p = load(&a[1])?;
            let changed = schedule::level(&mut p)?;
            for (id, d) in &changed {
                out(&format!("  task {id} delayed by {}\n", fmt_dur(*d, p.day_minutes)));
            }
            save(&a[1], &p)?;
            ok(&format!("Leveled ({} task(s) delayed, no links added)", changed.len()));
            out(&report::resources(&p)?);
        }
        "unlevel" => {
            need(2)?;
            let mut p = load(&a[1])?;
            let n = schedule::clear_leveling(&mut p);
            save(&a[1], &p)?;
            ok(&format!("Removed leveling delays from {n} task(s)"));
        }
        "baseline" => {
            need(2)?;
            let mut p = load(&a[1])?;
            if a.get(2).map(String::as_str) == Some("clear") {
                schedule::clear_baseline(&mut p);
                save(&a[1], &p)?;
                ok("Baseline cleared");
            } else {
                schedule::set_baseline(&mut p)?;
                save(&a[1], &p)?;
                ok("Baseline saved");
            }
        }
        "add" => {
            need(4)?;
            let mut p = load(&a[1])?;
            let mut t = Task::new(p.next_id(), &clean(&a[2]), parse_dur(&a[3], p.day_minutes)?);
            t.deps = parse_deps(&flag(&a, "--after").unwrap_or_default(), p.day_minutes)?;
            t.who = parse_who(&flag(&a, "--who").unwrap_or_default())?;
            let id = t.id;
            match flag(&a, "--under") {
                Some(u) => {
                    let i = index_of(&p, &u)?;
                    t.level = p.tasks[i].level + 1;
                    let at = subtree_end(&p, i);
                    p.tasks.insert(at, t);
                }
                None => p.tasks.push(t),
            }
            save(&a[1], &p)?;
            ok(&format!("Added task {id}"));
        }
        "set" => {
            need(3)?;
            let mut p = load(&a[1])?;
            let i = index_of(&p, &a[2])?;
            let dm = p.day_minutes;
            let t = &mut p.tasks[i];
            if let Some(v) = flag(&a, "--name") {
                t.name = clean(&v);
            }
            if let Some(v) = flag(&a, "--dur").or(flag(&a, "--days")) {
                t.dur = parse_dur(&v, dm)?.max(0);
            }
            if let Some(v) = flag(&a, "--after") {
                t.deps = parse_deps(&v, dm)?;
            }
            if let Some(v) = flag(&a, "--who") {
                t.who = parse_who(&v)?;
            }
            if let Some(v) = flag(&a, "--done") {
                t.pct = v.trim_end_matches('%').parse::<u8>().map_err(|_| "bad --done (0-100)")?.min(100);
            }
            if let Some(v) = flag(&a, "--snet") {
                t.constraint = if v == "none" { Constraint::Asap } else { Constraint::Snet(parse_datetime(&v)?) };
            }
            if let Some(v) = flag(&a, "--deadline") {
                t.deadline = if v == "none" { None } else { Some(parse_date(&v)?) };
            }
            if let Some(v) = flag(&a, "--cost") {
                t.cost = v.parse().map_err(|_| "bad --cost")?;
            }
            if let Some(v) = flag(&a, "--note") {
                t.note = clean(&v);
            }
            let id = t.id;
            save(&a[1], &p)?;
            ok(&format!("Updated task {id}"));
        }
        "rm" => {
            need(3)?;
            let mut p = load(&a[1])?;
            let i = index_of(&p, &a[2])?;
            let end = subtree_end(&p, i);
            let gone: Vec<u32> = p.tasks.drain(i..end).map(|t| t.id).collect();
            for t in &mut p.tasks {
                t.deps.retain(|d| !gone.contains(&d.id));
            }
            save(&a[1], &p)?;
            ok(&format!("Removed {} task(s) and their links", gone.len()));
        }
        "indent" | "outdent" => {
            need(3)?;
            let mut p = load(&a[1])?;
            let i = index_of(&p, &a[2])?;
            let end = subtree_end(&p, i);
            if cmd == "indent" {
                if i == 0 || p.tasks[i - 1].level < p.tasks[i].level {
                    return Err("can't indent: there's no task above at the same level".into());
                }
                p.tasks[i..end].iter_mut().for_each(|t| t.level += 1);
            } else {
                if p.tasks[i].level == 0 {
                    return Err("already at the top level".into());
                }
                p.tasks[i..end].iter_mut().for_each(|t| t.level -= 1);
            }
            p.normalize();
            save(&a[1], &p)?;
            ok(&format!("Task {} is now at level {}", p.tasks[i].id, p.tasks[i].level + 1));
        }
        "week" => {
            need(4)?;
            let mut p = load(&a[1])?;
            let hours = parse_hours(&a[3..].join(" "))?;
            for d in parse_weekdays(&a[2])? {
                p.week[d] = hours.clone();
            }
            save(&a[1], &p)?;
            ok(&format!("{} now: {}", a[2], fmt_hours(&hours)));
        }
        "dayhours" => {
            need(3)?;
            let mut p = load(&a[1])?;
            let h: f64 = a[2].parse().map_err(|_| "bad hours")?;
            if !(0.25..=24.0).contains(&h) {
                return Err("dayhours must be between 0.25 and 24".into());
            }
            p.day_minutes = (h * 60.0).round() as i64;
            save(&a[1], &p)?;
            ok(&format!("One day of duration = {h} hours"));
        }
        "holiday" | "workday" => {
            need(3)?;
            let mut p = load(&a[1])?;
            let (from, to) = parse_range(&a[2])?;
            let rest = &a[3..];
            let nh = rest.iter().take_while(|t| t.contains(':') && t.contains('-')).count();
            let mut hours = parse_hours(&rest[..nh].join(" "))?;
            if cmd == "holiday" {
                hours.clear();
            } else if hours.is_empty() {
                hours = p.week.iter().find(|h| !h.is_empty()).cloned().unwrap_or_default();
            }
            p.exceptions.push(Exception { from, to, name: clean(&rest[nh..].join(" ")), hours });
            save(&a[1], &p)?;
            ok(&format!("Added {cmd} {}", fmt_range(from, to)));
        }
        "resource" => {
            need(3)?;
            let mut p = load(&a[1])?;
            let mut r = p.resource(&a[2]);
            if let Some(v) = flag(&a, "--rate") {
                r.rate = v.parse().map_err(|_| "bad --rate")?;
            }
            if let Some(v) = flag(&a, "--capacity") {
                r.capacity = v.trim_end_matches('%').parse().map_err(|_| "bad --capacity")?;
            }
            if let Some(v) = flag(&a, "--off") {
                r.off = v.split(',').map(str::trim).filter(|x| !x.is_empty()).map(parse_range).collect::<Result<_, _>>()?;
            }
            let name = r.name.clone();
            p.resources.retain(|x| x.name != name);
            p.resources.push(r);
            save(&a[1], &p)?;
            ok(&format!("Saved resource {name}"));
        }
        _ => println!("{HELP}"),
    }
    Ok(())
}

fn main() {
    #[cfg(windows)]
    enable_ansi();
    if let Err(e) = run() {
        // The console may be gone (desktop window mode), so never panic while reporting.
        let _ = std::io::Write::write_fmt(&mut std::io::stderr(), format_args!("{}error:{} {e}\n", report::RED, RST));
        process::exit(1);
    }
}
