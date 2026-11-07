//! Terminal reports and the standalone HTML export.

use crate::calendar::*;
use crate::model::*;
use crate::schedule::*;
use std::fmt::Write as _;

pub const RED: &str = "\x1b[31m";
pub const GRN: &str = "\x1b[32m";
pub const YEL: &str = "\x1b[33m";
pub const DIM: &str = "\x1b[2m";
pub const BLD: &str = "\x1b[1m";
pub const RST: &str = "\x1b[0m";

pub fn finish_day(sc: &Sched) -> i64 {
    if sc.ef > sc.es {
        (sc.ef - 1).div_euclid(DAY)
    } else {
        sc.es.div_euclid(DAY)
    }
}

pub fn days_str(mins: i64, dm: i64) -> String {
    let d = mins as f64 / dm as f64;
    if (d - d.round()).abs() < 0.01 {
        format!("{}d", d.round() as i64)
    } else {
        format!("{d:.1}d")
    }
}

pub fn money(x: f64, cur: &str) -> String {
    let whole = x.round() as i64;
    let s = whole.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{}{cur}{out}", if whole < 0 { "-" } else { "" })
}

fn pad(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n >= w {
        s.chars().take(w).collect()
    } else {
        format!("{s}{}", " ".repeat(w - n))
    }
}

pub fn summary_line(p: &Project, s: &Schedule) -> String {
    let cal = p.calendar().unwrap();
    let crit: Vec<String> = s.critical.iter().map(u32::to_string).collect();
    format!(
        "{BLD}Finish:{RST} {}   {BLD}Duration:{RST} {}   {BLD}Progress:{RST} {:.0}%   {BLD}Cost:{RST} {}   {BLD}Critical path:{RST} {RED}{}{RST}",
        fmt_date((s.finish - 1).div_euclid(DAY).max(p.start)),
        days_str(cal.work_between(s.start, s.finish), p.day_minutes),
        s.progress,
        money(s.cost, &p.currency),
        crit.join(" → ")
    )
}

pub fn show(p: &Project) -> Result<String, String> {
    let s = schedule(p)?;
    let dm = p.day_minutes;
    let wbs = p.wbs();
    let w = p.tasks.iter().map(|t| t.name.chars().count() + t.level * 2).max().unwrap_or(4).clamp(4, 40);
    let mut o = String::new();
    let _ = writeln!(o, "{BLD}{}{RST}", p.name);
    let _ = writeln!(
        o,
        "{BLD}{:<6} {:>4}  {}  {:>6}  {:<16}  {:<16}  {:>6}  {:<12} {:>4}  {}{RST}",
        "WBS", "ID", pad("Task", w), "Dur", "Start", "Finish", "Slack", "Who", "Done", "After"
    );
    for (i, t) in p.tasks.iter().enumerate() {
        let sc = s.tasks[i];
        let col = if sc.summary { BLD } else if sc.crit { RED } else { "" };
        let name = format!("{}{}", "  ".repeat(t.level), t.name);
        let dur = if sc.summary { days_str(sc.span, dm) } else if t.dur == 0 { "◆".into() } else { fmt_dur(t.dur, dm) };
        let mut flags = String::new();
        if sc.late {
            flags.push_str(&format!(" {YEL}⚠ late{RST}"));
        }
        if sc.conflict {
            flags.push_str(&format!(" {YEL}⚠ constraint{RST}"));
        }
        if t.delay > 0 {
            flags.push_str(&format!(" {DIM}(leveled +{}){RST}", fmt_dur(t.delay, dm)));
        }
        let _ = writeln!(
            o,
            "{col}{:<6} {:>4}  {}  {:>6}  {:<16}  {:<16}  {:>6}  {} {:>3.0}%  {}{RST}{flags}",
            wbs[i],
            t.id,
            pad(&name, w),
            dur,
            fmt_datetime(sc.es).replace('T', " "),
            fmt_datetime(sc.ef).replace('T', " "),
            days_str(sc.slack, dm),
            pad(&fmt_who(&t.who), 12),
            sc.pct,
            fmt_deps(&t.deps, dm)
        );
    }
    let _ = writeln!(o, "\n{}", summary_line(p, &s));
    Ok(o)
}

pub fn gantt(p: &Project) -> Result<String, String> {
    let s = schedule(p)?;
    let cal = p.calendar()?;
    let first = p.start;
    let last = (s.finish - 1).div_euclid(DAY).max(first);
    let days = (last - first + 1).min(400) as usize;
    let w = p.tasks.iter().map(|t| t.name.chars().count() + t.level * 2).max().unwrap_or(4).clamp(4, 28);
    let mut o = String::new();
    let mut hdr = String::new();
    let mut d = 0usize;
    while d < days {
        let day = first + d as i64;
        let (_, m, dd) = civil_from_days(day);
        if weekday(day) == 1 || d == 0 {
            let lab = format!("|{m:02}/{dd:02}");
            hdr.push_str(&lab);
            d += lab.len();
        } else {
            hdr.push(' ');
            d += 1;
        }
    }
    hdr.truncate(days);
    let _ = writeln!(o, "{BLD}{:>4} {} {DIM}{hdr}{RST}", "", pad(&p.name, w));
    for (i, t) in p.tasks.iter().enumerate() {
        let sc = s.tasks[i];
        let (a, b) = (sc.es.div_euclid(DAY) - first, finish_day(&sc) - first);
        let col = if sc.summary { BLD } else if sc.crit { RED } else { GRN };
        let mut bar = String::new();
        let task_days = (a..=b).filter(|k| cal.is_working_day(first + k)).count() as f64;
        let done_days = (task_days * sc.pct / 100.0).round() as usize;
        let mut wd_idx = 0usize;
        for k in 0..days as i64 {
            let day = first + k;
            let c = if k < a || k > b {
                if !cal.is_working_day(day) {
                    format!("{DIM}·{RST}")
                } else {
                    " ".into()
                }
            } else if t.dur == 0 && !sc.summary {
                "◆".into()
            } else if sc.summary {
                "▀".into()
            } else if !cal.is_working_day(day) {
                format!("{DIM}░{RST}{col}")
            } else {
                wd_idx += 1;
                if wd_idx <= done_days { "█".into() } else { "▒".into() }
            };
            bar.push_str(&c);
        }
        let _ = writeln!(o, "{:>4} {} {col}{}{RST}", t.id, pad(&format!("{}{}", "  ".repeat(t.level), t.name), w), bar.trim_end());
    }
    let _ = writeln!(o, "\n{RED}█ critical{RST}  {GRN}█ has slack{RST}  ▒ remaining  ▀ summary  ◆ milestone  {DIM}· day off{RST}");
    let _ = writeln!(o, "{}", summary_line(p, &s));
    Ok(o)
}

pub fn resources(p: &Project) -> Result<String, String> {
    let s = schedule(p)?;
    let mut o = String::new();
    let ls = loads(p, &s);
    if ls.is_empty() {
        return Ok("No resources assigned yet.\n".into());
    }
    let mut over = 0;
    for l in &ls {
        let _ = writeln!(
            o,
            "{BLD}{}{RST}  {} of work · {} task(s) · capacity {}% · {}",
            l.name,
            days_str(l.work, p.day_minutes),
            l.tasks,
            l.capacity,
            money(l.cost, &p.currency)
        );
        let bad: Vec<(&i64, &(u32, i64, Vec<u32>))> = l.days.iter().filter(|(d, v)| v.0 > l.cap_on(**d)).collect();
        if let (Some(f), Some(e)) = (bad.first(), bad.last()) {
            over += 1;
            let mut ids: Vec<u32> = bad.iter().flat_map(|(_, v)| v.2.iter().copied()).collect();
            ids.sort();
            ids.dedup();
            let _ = writeln!(o, "  {RED}⚠ over-allocated on {} day(s), {} → {}, tasks {ids:?}{RST}", bad.len(), fmt_date(*f.0), fmt_date(*e.0));
        }
    }
    if over == 0 {
        let _ = writeln!(o, "{GRN}✓ Nobody is over-allocated.{RST}");
    } else {
        let _ = writeln!(o, "\nTip: `rplan level <file>` delays flexible tasks to fix this.");
    }
    Ok(o)
}

/// The app icon as an inline `data:` URI, so exported pages work offline.
fn favicon_data_uri() -> String {
    let svg = String::from_utf8_lossy(crate::server::ICON_SVG);
    let enc = svg.trim().replace('"', "'").replace('%', "%25").replace('#', "%23").replace('<', "%3C").replace('>', "%3E");
    format!("data:image/svg+xml,{enc}")
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Self-contained, print-friendly HTML/SVG Gantt (light/dark aware).
pub fn html(p: &Project) -> Result<String, String> {
    let s = schedule(p)?;
    let cal = p.calendar()?;
    let first = p.start;
    let last = (s.finish - 1).div_euclid(DAY).max(first) + 2;
    let ndays = (last - first + 1).min(800);
    let (dw, rh, lw) = (20f64, 26i64, 300f64);
    let x = |t: i64| lw + (t - first * DAY) as f64 / DAY as f64 * dw;
    let (wpx, hpx) = (lw + ndays as f64 * dw + 40.0, 44 + p.tasks.len() as i64 * rh + 10);
    let mut g = String::new();
    for d in 0..ndays {
        let day = first + d;
        let px = lw + d as f64 * dw;
        if !cal.is_working_day(day) {
            let _ = write!(g, r#"<rect x="{px}" y="30" width="{dw}" height="{}" class="off"/>"#, hpx - 30);
        }
        if weekday(day) == 1 || d == 0 {
            let (_, m, dd) = civil_from_days(day);
            let _ = write!(g, r#"<line x1="{px}" y1="26" x2="{px}" y2="{hpx}" class="grid"/><text x="{}" y="20" class="lab">{m:02}/{dd:02}</text>"#, px + 3.0);
        }
    }
    let idx: std::collections::HashMap<u32, usize> = p.tasks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
    for (i, t) in p.tasks.iter().enumerate() {
        let sc = s.tasks[i];
        let y = 40 + i as i64 * rh;
        let cls = if sc.summary { "sum" } else if sc.crit { "crit" } else { "ok" };
        let tip = format!(
            "{} — {} → {} · slack {} · {:.0}% · {}",
            t.name,
            fmt_datetime(sc.es).replace('T', " "),
            fmt_datetime(sc.ef).replace('T', " "),
            days_str(sc.slack, p.day_minutes),
            sc.pct,
            fmt_who(&t.who)
        );
        let weight = if sc.summary { "600" } else { "400" };
        let _ = write!(g, r#"<text x="{}" y="{}" class="name" font-weight="{weight}">{}</text>"#, 8 + t.level * 14, y + 16, esc(&t.name));
        let (x1, x2) = (x(sc.es), x(sc.ef).max(x(sc.es) + 2.0));
        if !sc.summary && t.dur == 0 {
            let _ = write!(g, r#"<g><title>{}</title><rect x="{}" y="{}" width="11" height="11" transform="rotate(45 {x1} {})" class="ms"/></g>"#, esc(&tip), x1 - 5.5, y + 6, y + 11);
        } else if sc.summary {
            let _ = write!(g, r#"<g><title>{}</title><path d="M{x1},{} H{x2} V{} L{},{} H{} L{x1},{} Z" class="sum"/></g>"#, esc(&tip), y + 6, y + 16, x2 - 5.0, y + 11, x1 + 5.0, y + 16);
        } else {
            let done = (x2 - x1) * sc.pct / 100.0;
            let _ = write!(
                g,
                r#"<g><title>{}</title><rect x="{x1}" y="{}" width="{}" height="14" rx="3" class="{cls}"/><rect x="{x1}" y="{}" width="{done}" height="14" rx="3" class="done"/></g>"#,
                esc(&tip),
                y + 4,
                x2 - x1,
                y + 4
            );
        }
        if let Some(Baseline { start: a, finish: b, .. }) = t.baseline() {
            let _ = write!(g, r#"<rect x="{}" y="{}" width="{}" height="3" class="base"/>"#, x(a), y + 20, (x(b) - x(a)).max(2.0));
        }
        for d in &t.deps {
            if let Some(&j) = idx.get(&d.id) {
                let q = s.tasks[j];
                let from = if matches!(d.kind, Link::SS | Link::SF) { x(q.es) } else { x(q.ef) };
                let to = if matches!(d.kind, Link::FF | Link::SF) { x2 } else { x1 };
                let (y1, y2) = (40 + j as i64 * rh + 11, y + 11);
                let _ = write!(g, r#"<path d="M{from},{y1} h5 V{y2} H{}" class="arrow" marker-end="url(#a)"/>"#, to - 1.0);
            }
        }
    }
    Ok(format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>{name}</title><link rel="icon" href="{fav}"><style>
:root{{--bg:#fff;--fg:#1d1d1f;--mut:#86868b;--grid:#e5e5ea;--off:#f4f4f7;--ok:#3b7cff;--crit:#e5484d;--sum:#30343f;--done:rgba(0,0,0,.25);--ms:#f5a524}}
@media (prefers-color-scheme:dark){{:root{{--bg:#111214;--fg:#f2f2f7;--mut:#8e8e93;--grid:#2c2c2e;--off:#18191c;--ok:#4f8cff;--crit:#ff5d6c;--sum:#d6d9e0;--done:rgba(0,0,0,.35)}}}}
@media print{{:root{{--bg:#fff;--fg:#000;--off:#f2f2f2}}}}
body{{background:var(--bg);color:var(--fg);font:14px system-ui,sans-serif;margin:0;padding:16px}}h1{{font-size:20px;margin:0 0 4px}}p{{color:var(--mut);margin:0 0 16px}}
.wrap{{overflow-x:auto}}.grid{{stroke:var(--grid)}}.off{{fill:var(--off)}}.lab{{fill:var(--mut);font-size:11px}}.name{{fill:var(--fg);font-size:12.5px}}
.ok{{fill:var(--ok)}}.crit{{fill:var(--crit)}}.sum{{fill:var(--sum)}}.done{{fill:var(--done)}}.ms{{fill:var(--ms)}}.base{{fill:var(--mut);opacity:.6}}.arrow{{fill:none;stroke:var(--mut);stroke-width:1.1}}
</style></head><body><h1>{name}</h1><p>{summary}</p><div class="wrap">
<svg width="{wpx}" height="{hpx}" xmlns="http://www.w3.org/2000/svg"><defs><marker id="a" viewBox="0 0 6 6" refX="5" refY="3" markerWidth="6" markerHeight="6" orient="auto"><path d="M0,0 L6,3 L0,6z" style="fill:var(--mut)"/></marker></defs>{g}</svg></div></body></html>"#,
        name = esc(&p.name),
        fav = favicon_data_uri(),
        summary = esc(&format!(
            "Finishes {} · {} · {:.0}% done · {} · red = critical path",
            fmt_date((s.finish - 1).div_euclid(DAY).max(p.start)),
            days_str(cal.work_between(s.start, s.finish), p.day_minutes),
            s.progress,
            money(s.cost, &p.currency)
        ))
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_render() {
        let p = Project::parse(crate::DEMO).unwrap();
        let s = show(&p).unwrap();
        assert!(s.contains("Backend API") && s.contains("Critical path"));
        assert!(gantt(&p).unwrap().contains("◆"));
        assert!(resources(&p).unwrap().contains("over-allocated"));
        let h = html(&p).unwrap();
        assert!(h.starts_with("<!doctype html>") && h.contains("Website Launch"));
        assert_eq!(money(1234567.4, "$"), "$1,234,567");
        assert_eq!(money(-50.0, "€"), "-€50");
    }
}
