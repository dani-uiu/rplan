//! Microsoft Project XML (MSPDI) import and export.
//! ProjectLibre, MS Project, GanttProject and others read and write this format.

use crate::calendar::*;
use crate::model::*;
use crate::schedule::schedule;
use std::collections::HashMap;
use std::fmt::Write;

// ---------- a tiny XML reader (elements + text; attributes ignored) ----------

#[derive(Debug, Default, Clone)]
pub struct El {
    pub name: String,
    pub text: String,
    pub children: Vec<El>,
}

impl El {
    pub fn child(&self, n: &str) -> Option<&El> {
        self.children.iter().find(|c| c.name == n)
    }
    pub fn all<'a>(&'a self, n: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.children.iter().filter(move |c| c.name == n)
    }
    pub fn get(&self, n: &str) -> Option<&str> {
        self.child(n).map(|c| c.text.trim()).filter(|t| !t.is_empty())
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(j) = rest.find(';') else { break };
        let ent = &rest[1..j];
        let c = match ent {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn parse_xml(src: &str) -> Result<El, String> {
    let mut stack = vec![El { name: "#doc".into(), ..Default::default() }];
    let mut i = 0;
    let b = src.as_bytes();
    while i < b.len() {
        if b[i] != b'<' {
            let end = src[i..].find('<').map_or(src.len(), |k| i + k);
            stack.last_mut().unwrap().text.push_str(&unescape(&src[i..end]));
            i = end;
            continue;
        }
        let rest = &src[i..];
        if rest.starts_with("<?") {
            i += rest.find("?>").ok_or("unterminated <?")? + 2;
        } else if rest.starts_with("<!--") {
            i += rest.find("-->").ok_or("unterminated comment")? + 3;
        } else if rest.starts_with("<![CDATA[") {
            let k = rest.find("]]>").ok_or("unterminated CDATA")?;
            stack.last_mut().unwrap().text.push_str(&rest[9..k]);
            i += k + 3;
        } else if rest.starts_with("<!") {
            i += rest.find('>').ok_or("unterminated <!")? + 1;
        } else if rest.starts_with("</") {
            let k = rest.find('>').ok_or("unterminated closing tag")?;
            let name = local(rest[2..k].trim());
            let el = stack.pop().ok_or("unbalanced XML")?;
            if el.name != name || stack.is_empty() {
                return Err(format!("XML: expected </{}> but found </{}>", el.name, name));
            }
            stack.last_mut().unwrap().children.push(el);
            i += k + 1;
        } else {
            let k = find_tag_end(rest).ok_or("unterminated tag")?;
            let inner = &rest[1..k];
            let self_closing = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let name = local(inner.split(|c: char| c.is_whitespace()).next().unwrap_or(""));
            let el = El { name: name.to_string(), ..Default::default() };
            if self_closing {
                stack.last_mut().unwrap().children.push(el);
            } else {
                stack.push(el);
            }
            i += k + 1;
        }
    }
    if stack.len() != 1 {
        return Err(format!("XML ended inside <{}>", stack.last().unwrap().name));
    }
    let doc = stack.pop().unwrap();
    doc.children.into_iter().next().ok_or_else(|| "empty XML document".into())
}

fn find_tag_end(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn local(n: &str) -> &str {
    n.rsplit(':').next().unwrap_or(n)
}

fn xml_esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// ---------- MSPDI values ----------

/// `PT8H30M0S`, `P1DT2H`, `PT1.5H` → minutes.
fn parse_iso_dur(s: &str, day_minutes: i64) -> i64 {
    let mut total = 0f64;
    let mut num = String::new();
    let mut in_time = false;
    for c in s.trim().chars() {
        match c {
            'P' => {}
            'T' => in_time = true,
            '0'..='9' | '.' | '-' => num.push(c),
            u => {
                let n: f64 = num.parse().unwrap_or(0.0);
                num.clear();
                total += n * match (u, in_time) {
                    ('W', false) => day_minutes as f64 * 5.0,
                    ('D', false) => day_minutes as f64,
                    ('M', false) => day_minutes as f64 * 20.0,
                    ('Y', false) => day_minutes as f64 * 260.0,
                    ('H', true) => 60.0,
                    ('M', true) => 1.0,
                    ('S', true) => 1.0 / 60.0,
                    _ => 0.0,
                };
            }
        }
    }
    total.round() as i64
}

fn iso_dur(mins: i64) -> String {
    format!("PT{}H{}M0S", mins / 60, mins % 60)
}

fn iso_dt(t: i64) -> String {
    format!("{}:00", fmt_datetime(t))
}

fn hhmm(m: u16) -> String {
    if m >= 1440 {
        "00:00:00".into()
    } else {
        format!("{}:00", fmt_hm(m))
    }
}

fn working_times(el: &El) -> Hours {
    let mut h: Hours = vec![];
    if let Some(wt) = el.child("WorkingTimes") {
        for w in wt.all("WorkingTime") {
            if let (Some(a), Some(b)) = (w.get("FromTime"), w.get("ToTime")) {
                if let (Ok(a), Ok(b)) = (parse_hm(a), parse_hm(b)) {
                    let b = if b == 0 { 1440 } else { b };
                    if b > a {
                        h.push((a, b));
                    }
                }
            }
        }
    }
    h.sort();
    h
}

fn day_of(s: &str) -> Option<i64> {
    parse_datetime(s).ok().map(|t| t.div_euclid(DAY))
}

/// Exceptions in a calendar, both the 2007+ `<Exceptions>` and the older `DayType 0` form.
fn calendar_exceptions(c: &El) -> Vec<Exception> {
    let mut out = vec![];
    let mut push = |el: &El, tp: Option<&El>| {
        let Some(tp) = tp else { return };
        let (Some(from), Some(to)) = (tp.get("FromDate").and_then(day_of), tp.get("ToDate").and_then(day_of)) else { return };
        let working = el.get("DayWorking") == Some("1");
        let hours = if working { working_times(el) } else { vec![] };
        out.push(Exception { from, to: to.max(from), name: el.get("Name").unwrap_or("").to_string(), hours });
    };
    if let Some(ex) = c.child("Exceptions") {
        for e in ex.all("Exception") {
            push(e, e.child("TimePeriod"));
        }
    }
    if let Some(wd) = c.child("WeekDays") {
        for d in wd.all("WeekDay") {
            if d.get("DayType") == Some("0") {
                push(d, d.child("TimePeriod"));
            }
        }
    }
    out
}

// ---------- import ----------

pub fn import(xml: &str) -> Result<Project, String> {
    let root = parse_xml(xml)?;
    if root.name != "Project" {
        return Err(format!("this isn't a Microsoft Project XML file (root element is <{}>)", root.name));
    }
    let mut p = Project::default();
    p.name = root.get("Title").or(root.get("Name")).unwrap_or("Imported project").trim_end_matches(".xml").to_string();
    if let Some(m) = root.get("MinutesPerDay").and_then(|x| x.parse::<i64>().ok()) {
        if m > 0 {
            p.day_minutes = m;
        }
    }
    if let Some(c) = root.get("CurrencySymbol") {
        p.currency = c.to_string();
    }
    let start = root.get("StartDate").and_then(day_of);

    // Calendars
    let cals: Vec<&El> = root.child("Calendars").map(|c| c.all("Calendar").collect()).unwrap_or_default();
    let cal_uid = root.get("CalendarUID");
    let base = cals.iter().find(|c| c.get("UID") == cal_uid).or(cals.iter().find(|c| c.get("IsBaseCalendar") == Some("1"))).copied();
    if let Some(c) = base {
        if let Some(wd) = c.child("WeekDays") {
            for d in wd.all("WeekDay") {
                let Some(t) = d.get("DayType").and_then(|x| x.parse::<usize>().ok()) else { continue };
                if (1..=7).contains(&t) {
                    p.week[t - 1] = if d.get("DayWorking") == Some("1") { working_times(d) } else { vec![] };
                }
            }
        }
        if p.week.iter().all(|h| h.is_empty()) {
            p.week = default_week();
        }
        p.exceptions = calendar_exceptions(c);
    }
    let cal_by_uid: HashMap<&str, &El> = cals.iter().filter_map(|c| c.get("UID").map(|u| (u, *c))).collect();

    // Resources
    let mut res_name: HashMap<String, String> = HashMap::new();
    if let Some(rs) = root.child("Resources") {
        for r in rs.all("Resource") {
            let (Some(uid), Some(name)) = (r.get("UID"), r.get("Name")) else { continue };
            if r.get("IsNull") == Some("1") {
                continue;
            }
            let mut res = Resource::new(&clean(name).replace([',', '[', ']'], " "));
            res.rate = r.get("StandardRate").and_then(|x| x.parse().ok()).unwrap_or(0.0);
            res.capacity = r.get("MaxUnits").and_then(|x| x.parse::<f64>().ok()).map(|u| (u * 100.0).round() as u32).filter(|&u| u > 0).unwrap_or(100);
            if let Some(c) = r.get("CalendarUID").and_then(|u| cal_by_uid.get(u)) {
                if !base.is_some_and(|b| std::ptr::eq(b, *c)) {
                    res.off = calendar_exceptions(c).iter().filter(|e| e.hours.is_empty()).map(|e| (e.from, e.to)).collect();
                }
            }
            res_name.insert(uid.to_string(), res.name.clone());
            p.resources.push(res);
        }
    }

    // Tasks
    let mut tasks: Vec<(String, Task)> = vec![];
    if let Some(ts) = root.child("Tasks") {
        for t in ts.all("Task") {
            let Some(uid) = t.get("UID") else { continue };
            let level: usize = t.get("OutlineLevel").and_then(|x| x.parse().ok()).unwrap_or(1);
            if level == 0 || t.get("IsNull") == Some("1") {
                continue;
            }
            let id: u32 = uid.parse().map_err(|_| format!("task UID '{uid}' isn't a number"))?;
            let summary = t.get("Summary") == Some("1");
            let mut dur = t.get("Duration").map(|d| parse_iso_dur(d, p.day_minutes)).unwrap_or(p.day_minutes);
            if t.get("Milestone") == Some("1") {
                dur = 0;
            }
            let mut task = Task::new(id, &clean(t.get("Name").unwrap_or("Untitled")), if summary { p.day_minutes } else { dur.max(0) });
            task.level = level - 1;
            task.pct = t.get("PercentComplete").and_then(|x| x.parse::<f64>().ok()).unwrap_or(0.0).clamp(0.0, 100.0) as u8;
            task.cost = t.get("FixedCost").and_then(|x| x.parse().ok()).unwrap_or(0.0);
            task.note = clean(t.get("Notes").unwrap_or(""));
            let cdate = t.get("ConstraintDate").and_then(|x| parse_datetime(x).ok());
            task.constraint = match (t.get("ConstraintType"), cdate) {
                (Some("2"), Some(d)) => Constraint::Mso(d),
                (Some("4"), Some(d)) => Constraint::Snet(d),
                (Some("6"), Some(d)) => Constraint::Fnet(d),
                _ => Constraint::Asap,
            };
            task.deadline = t.get("Deadline").and_then(day_of);
            // LevelingDelay is stored in tenths of minutes.
            task.delay = t.get("LevelingDelay").and_then(|x| x.parse::<f64>().ok()).map(|x| (x / 10.0).round() as i64).unwrap_or(0).max(0);
            for l in t.all("PredecessorLink") {
                let Some(pid) = l.get("PredecessorUID").and_then(|x| x.parse().ok()) else { continue };
                let kind = match l.get("Type") {
                    Some("0") => Link::FF,
                    Some("2") => Link::SF,
                    Some("3") => Link::SS,
                    _ => Link::FS,
                };
                let lag = l.get("LinkLag").and_then(|x| x.parse::<f64>().ok()).map(|x| (x / 10.0).round() as i64).unwrap_or(0);
                task.deps.push(Dep { id: pid, kind, lag });
            }
            for b in t.all("Baseline") {
                if b.get("Number").unwrap_or("0") == "0" {
                    if let (Some(s), Some(f)) = (b.get("Start").and_then(|x| parse_datetime(x).ok()), b.get("Finish").and_then(|x| parse_datetime(x).ok())) {
                        task.baseline = Some((s, f));
                    }
                }
            }
            tasks.push((uid.to_string(), task));
        }
    }
    // Assignments
    if let Some(asg) = root.child("Assignments") {
        for a in asg.all("Assignment") {
            let (Some(tu), Some(ru)) = (a.get("TaskUID"), a.get("ResourceUID")) else { continue };
            let Some(name) = res_name.get(ru) else { continue };
            let units = a.get("Units").and_then(|x| x.parse::<f64>().ok()).map(|u| (u * 100.0).round() as u32).filter(|&u| u > 0).unwrap_or(100);
            if let Some((_, t)) = tasks.iter_mut().find(|(u, _)| u == tu) {
                if !t.who.iter().any(|w| &w.name == name) {
                    t.who.push(Assign { name: name.clone(), units });
                }
            }
        }
    }
    let ids: std::collections::HashSet<u32> = tasks.iter().map(|(_, t)| t.id).collect();
    p.tasks = tasks.into_iter().map(|(_, t)| t).collect();
    for t in &mut p.tasks {
        t.deps.retain(|d| ids.contains(&d.id) && d.id != t.id);
    }
    p.start = start.unwrap_or_else(|| p.tasks.iter().filter_map(|t| t.baseline.map(|b| b.0.div_euclid(DAY))).min().unwrap_or(0));
    p.normalize();
    p.calendar()?;
    Ok(p)
}

// ---------- export ----------

pub fn export(p: &Project) -> Result<String, String> {
    let s = schedule(p)?;
    let summary = p.summary_flags();
    let wbs = p.wbs();
    let dm = p.day_minutes;
    let mut x = String::new();
    let tag = |x: &mut String, n: &str, v: &str| {
        let _ = write!(x, "<{n}>{}</{n}>", xml_esc(v));
    };
    x.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Project xmlns=\"http://schemas.microsoft.com/project\">");
    tag(&mut x, "SaveVersion", "14");
    tag(&mut x, "Name", &format!("{}.xml", p.name));
    tag(&mut x, "Title", &p.name);
    tag(&mut x, "ScheduleFromStart", "1");
    tag(&mut x, "StartDate", &iso_dt(s.start));
    tag(&mut x, "FinishDate", &iso_dt(s.finish));
    tag(&mut x, "CurrencySymbol", &p.currency);
    tag(&mut x, "CalendarUID", "1");
    let first = p.week.iter().find(|h| !h.is_empty()).cloned().unwrap_or_default();
    tag(&mut x, "DefaultStartTime", &hhmm(first.first().map_or(480, |h| h.0)));
    tag(&mut x, "DefaultFinishTime", &hhmm(first.last().map_or(1020, |h| h.1)));
    tag(&mut x, "MinutesPerDay", &dm.to_string());
    tag(&mut x, "MinutesPerWeek", &(dm * 5).to_string());
    tag(&mut x, "DaysPerMonth", "20");

    let times = |x: &mut String, h: &Hours| {
        if h.is_empty() {
            return;
        }
        x.push_str("<WorkingTimes>");
        for (a, b) in h {
            let _ = write!(x, "<WorkingTime><FromTime>{}</FromTime><ToTime>{}</ToTime></WorkingTime>", hhmm(*a), hhmm(*b));
        }
        x.push_str("</WorkingTimes>");
    };
    let exception = |x: &mut String, e: &Exception| {
        let _ = write!(
            x,
            "<Exception><EnteredByOccurrences>0</EnteredByOccurrences><TimePeriod><FromDate>{}T00:00:00</FromDate><ToDate>{}T23:59:00</ToDate></TimePeriod><Occurrences>1</Occurrences><Name>{}</Name><Type>1</Type><DayWorking>{}</DayWorking>",
            fmt_date(e.from),
            fmt_date(e.to),
            xml_esc(&e.name),
            if e.hours.is_empty() { 0 } else { 1 }
        );
        times(x, &e.hours);
        x.push_str("</Exception>");
    };
    x.push_str("<Calendars><Calendar><UID>1</UID><Name>Standard</Name><IsBaseCalendar>1</IsBaseCalendar><BaseCalendarUID>-1</BaseCalendarUID><WeekDays>");
    for (i, h) in p.week.iter().enumerate() {
        let _ = write!(x, "<WeekDay><DayType>{}</DayType><DayWorking>{}</DayWorking>", i + 1, if h.is_empty() { 0 } else { 1 });
        times(&mut x, h);
        x.push_str("</WeekDay>");
    }
    x.push_str("</WeekDays>");
    if !p.exceptions.is_empty() {
        x.push_str("<Exceptions>");
        for e in &p.exceptions {
            exception(&mut x, e);
        }
        x.push_str("</Exceptions>");
    }
    x.push_str("</Calendar>");
    let people = p.all_resources();
    for (k, r) in people.iter().enumerate() {
        let _ = write!(x, "<Calendar><UID>{}</UID><Name>{}</Name><IsBaseCalendar>0</IsBaseCalendar><BaseCalendarUID>1</BaseCalendarUID>", k + 2, xml_esc(&r.name));
        if !r.off.is_empty() {
            x.push_str("<Exceptions>");
            for &(a, b) in &r.off {
                exception(&mut x, &Exception { from: a, to: b, name: "Day off".into(), hours: vec![] });
            }
            x.push_str("</Exceptions>");
        }
        x.push_str("</Calendar>");
    }
    x.push_str("</Calendars><Tasks>");
    let _ = write!(
        x,
        "<Task><UID>0</UID><ID>0</ID><Name>{}</Name><Type>1</Type><IsNull>0</IsNull><OutlineNumber>0</OutlineNumber><OutlineLevel>0</OutlineLevel><Start>{}</Start><Finish>{}</Finish><Summary>1</Summary></Task>",
        xml_esc(&p.name),
        iso_dt(s.start),
        iso_dt(s.finish)
    );
    for (i, t) in p.tasks.iter().enumerate() {
        let sc = s.tasks[i];
        let dur = if summary[i] { sc.span } else { t.dur };
        x.push_str("<Task>");
        tag(&mut x, "UID", &t.id.to_string());
        tag(&mut x, "ID", &(i + 1).to_string());
        tag(&mut x, "Name", &t.name);
        tag(&mut x, "Type", "1");
        tag(&mut x, "IsNull", "0");
        tag(&mut x, "WBS", &wbs[i]);
        tag(&mut x, "OutlineNumber", &wbs[i]);
        tag(&mut x, "OutlineLevel", &(t.level + 1).to_string());
        tag(&mut x, "Priority", "500");
        tag(&mut x, "Start", &iso_dt(sc.es));
        tag(&mut x, "Finish", &iso_dt(sc.ef));
        tag(&mut x, "Duration", &iso_dur(dur));
        tag(&mut x, "DurationFormat", "7");
        tag(&mut x, "Milestone", if !summary[i] && t.dur == 0 { "1" } else { "0" });
        tag(&mut x, "Summary", if summary[i] { "1" } else { "0" });
        tag(&mut x, "Critical", if sc.crit { "1" } else { "0" });
        tag(&mut x, "EarlyStart", &iso_dt(sc.es));
        tag(&mut x, "EarlyFinish", &iso_dt(sc.ef));
        tag(&mut x, "LateStart", &iso_dt(sc.ls));
        tag(&mut x, "LateFinish", &iso_dt(sc.lf));
        tag(&mut x, "TotalSlack", &(sc.slack * 10).to_string());
        tag(&mut x, "FixedCost", &t.cost.to_string());
        tag(&mut x, "PercentComplete", &(sc.pct.round() as i64).to_string());
        tag(&mut x, "Cost", &format!("{:.2}", sc.cost));
        let (ct, cd) = match t.constraint {
            Constraint::Asap => ("0", None),
            Constraint::Mso(d) => ("2", Some(d)),
            Constraint::Snet(d) => ("4", Some(d)),
            Constraint::Fnet(d) => ("6", Some(d)),
        };
        tag(&mut x, "ConstraintType", ct);
        if let Some(d) = cd {
            tag(&mut x, "ConstraintDate", &iso_dt(d));
        }
        if let Some(d) = t.deadline {
            tag(&mut x, "Deadline", &format!("{}T17:00:00", fmt_date(d)));
        }
        if t.delay > 0 {
            tag(&mut x, "LevelingDelay", &(t.delay * 10).to_string());
            tag(&mut x, "LevelingDelayFormat", "7");
        }
        if !t.note.is_empty() {
            tag(&mut x, "Notes", &t.note);
        }
        for d in &t.deps {
            let ty = match d.kind {
                Link::FF => 0,
                Link::FS => 1,
                Link::SF => 2,
                Link::SS => 3,
            };
            let _ = write!(
                x,
                "<PredecessorLink><PredecessorUID>{}</PredecessorUID><Type>{ty}</Type><CrossProject>0</CrossProject><LinkLag>{}</LinkLag><LagFormat>7</LagFormat></PredecessorLink>",
                d.id,
                d.lag * 10
            );
        }
        if let Some((a, b)) = t.baseline {
            let _ = write!(x, "<Baseline><Number>0</Number><Start>{}</Start><Finish>{}</Finish><Duration>{}</Duration></Baseline>", iso_dt(a), iso_dt(b), iso_dur(dur));
        }
        x.push_str("</Task>");
    }
    x.push_str("</Tasks><Resources>");
    for (k, r) in people.iter().enumerate() {
        let _ = write!(
            x,
            "<Resource><UID>{0}</UID><ID>{0}</ID><Name>{1}</Name><Type>1</Type><IsNull>0</IsNull><MaxUnits>{2:.2}</MaxUnits><StandardRate>{3}</StandardRate><StandardRateFormat>2</StandardRateFormat><CalendarUID>{4}</CalendarUID></Resource>",
            k + 1,
            xml_esc(&r.name),
            r.capacity as f64 / 100.0,
            r.rate,
            k + 2
        );
    }
    x.push_str("</Resources><Assignments>");
    let mut uid = 1;
    for (i, t) in p.tasks.iter().enumerate() {
        if summary[i] {
            continue;
        }
        for a in &t.who {
            let Some(k) = people.iter().position(|r| r.name == a.name) else { continue };
            let _ = write!(
                x,
                "<Assignment><UID>{uid}</UID><TaskUID>{}</TaskUID><ResourceUID>{}</ResourceUID><Finish>{}</Finish><Start>{}</Start><Units>{:.2}</Units><Work>{}</Work></Assignment>",
                t.id,
                k + 1,
                iso_dt(s.tasks[i].ef),
                iso_dt(s.tasks[i].es),
                a.units as f64 / 100.0,
                iso_dur(t.dur * a.units as i64 / 100)
            );
            uid += 1;
        }
    }
    x.push_str("</Assignments></Project>\n");
    Ok(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_reader_handles_the_basics() {
        let e = parse_xml("<?xml version=\"1.0\"?><!-- hi --><a x=\"1>2\"><b>1 &amp; 2</b><c/><ns:d><![CDATA[<raw>]]></ns:d></a>").unwrap();
        assert_eq!(e.name, "a");
        assert_eq!(e.get("b"), Some("1 & 2"));
        assert!(e.child("c").is_some());
        assert_eq!(e.get("d"), Some("<raw>"));
        assert!(parse_xml("<a><b></a>").is_err());
        assert_eq!(parse_iso_dur("PT8H30M0S", 480), 510);
        assert_eq!(parse_iso_dur("P1DT2H", 480), 600);
    }

    #[test]
    fn export_then_import_keeps_the_plan() {
        let src = "project Round & Trip
start 2026-11-09
week Mon-Thu 08:00-12:00 13:00-17:00
week Fri 08:00-12:00
week Sat,Sun off
holiday 2026-11-26 Thanksgiving
workday 2026-12-05 09:00-13:00 Release
resource Ana | rate 85 | capacity 50 | off 2026-11-20
task 1 | Phase |
  task 2 | A | 3d | | Ana[50%], Bo | 40 | snet=2026-11-10; deadline=2026-11-20; cost=100; note=hi
  task 3 | B | 4h | 2SS+1d | Bo | 0 | delay=2h
task 4 | Done | 0 | 3FF-30m
";
        let p = Project::parse(src).unwrap();
        let xml = export(&p).unwrap();
        let q = import(&xml).unwrap();
        assert_eq!(q.name, "Round & Trip");
        assert_eq!(q.start, p.start);
        assert_eq!(q.week, p.week);
        assert_eq!(q.exceptions, p.exceptions);
        assert_eq!(q.tasks.len(), 4);
        let summary = p.summary_flags();
        for (i, (a, b)) in p.tasks.iter().zip(&q.tasks).enumerate() {
            assert_eq!((a.id, &a.name, a.level, &a.deps, &a.who, a.constraint, a.deadline, &a.note, a.delay), (b.id, &b.name, b.level, &b.deps, &b.who, b.constraint, b.deadline, &b.note, b.delay));
            if !summary[i] {
                assert_eq!((a.dur, a.pct), (b.dur, b.pct));
            }
        }
        let ana = q.resources.iter().find(|r| r.name == "Ana").unwrap();
        assert_eq!((ana.rate, ana.capacity, ana.off.clone()), (85.0, 50, vec![(p.resources[0].off[0])]));
        assert_eq!(schedule(&p).unwrap().finish, schedule(&q).unwrap().finish);
    }

    #[test]
    fn imports_a_projectlibre_style_file() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Project xmlns="http://schemas.microsoft.com/project"><Name>libre.xml</Name><StartDate>2026-11-09T08:00:00</StartDate><MinutesPerDay>480</MinutesPerDay>
<Tasks><Task><UID>0</UID><OutlineLevel>0</OutlineLevel><Name>root</Name></Task>
<Task><UID>5</UID><Name>Design</Name><OutlineLevel>1</OutlineLevel><Duration>PT16H0M0S</Duration><Summary>0</Summary></Task>
<Task><UID>7</UID><Name>Build</Name><OutlineLevel>1</OutlineLevel><Duration>PT24H0M0S</Duration>
<PredecessorLink><PredecessorUID>5</PredecessorUID><Type>1</Type><LinkLag>4800</LinkLag></PredecessorLink></Task></Tasks>
<Resources><Resource><UID>1</UID><Name>Dev</Name><MaxUnits>1.00</MaxUnits></Resource></Resources>
<Assignments><Assignment><TaskUID>7</TaskUID><ResourceUID>1</ResourceUID><Units>1</Units></Assignment>
<Assignment><TaskUID>5</TaskUID><ResourceUID>-65535</ResourceUID></Assignment></Assignments></Project>"#;
        let p = import(xml).unwrap();
        assert_eq!(p.name, "libre");
        assert_eq!(p.tasks.len(), 2);
        assert_eq!(p.tasks[1].deps, vec![Dep { id: 5, kind: Link::FS, lag: 480 }]);
        assert_eq!(p.tasks[1].who[0].name, "Dev");
        assert!(p.tasks[0].who.is_empty());
        assert!(import("<html></html>").is_err());
    }
}
