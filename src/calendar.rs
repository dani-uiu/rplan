//! Dates, times and working calendars.
//!
//! Time is an `i64` count of minutes since 1970-01-01 00:00 (local, no time zone).
//! A calendar says which minutes are working time: a weekly pattern of hour
//! ranges plus dated exceptions (holidays, or special working days).

use std::collections::BTreeMap;

pub const DAY: i64 = 1440;
pub const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// Upper bound on how far calendar walks may search (≈ 30 years).
const MAX_DAYS: i64 = 11_000;

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { yoe + era * 400 + 1 } else { yoe + era * 400 }, m, d)
}

/// 0 = Sunday … 6 = Saturday.
pub fn weekday(day: i64) -> usize {
    (day + 4).rem_euclid(7) as usize
}

/// Parses `YYYY-MM-DD` into a day number.
pub fn parse_date(s: &str) -> Result<i64, String> {
    let s = s.trim();
    let p: Vec<&str> = s.split('-').collect();
    let bad = || format!("bad date '{s}' (expected YYYY-MM-DD)");
    if p.len() != 3 {
        return Err(bad());
    }
    let n = |x: &str| x.parse::<i64>().map_err(|_| bad());
    let (y, m, d) = (n(p[0])?, n(p[1])?, n(p[2])?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(bad());
    }
    let z = days_from_civil(y, m, d);
    if civil_from_days(z) != (y, m, d) {
        return Err(bad());
    }
    Ok(z)
}

/// Parses `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM[:SS]` into minutes.
pub fn parse_datetime(s: &str) -> Result<i64, String> {
    let s = s.trim();
    match s.split_once(['T', ' ']) {
        Some((d, t)) => Ok(parse_date(d)? * DAY + parse_hm(t)? as i64),
        None => Ok(parse_date(s)? * DAY),
    }
}

/// Parses `HH:MM` (seconds ignored) into minutes after midnight; allows 24:00.
pub fn parse_hm(s: &str) -> Result<u16, String> {
    let p: Vec<&str> = s.trim().split(':').collect();
    let bad = || format!("bad time '{s}' (expected HH:MM)");
    if p.len() < 2 {
        return Err(bad());
    }
    let h: u16 = p[0].parse().map_err(|_| bad())?;
    let m: u16 = p[1].parse().map_err(|_| bad())?;
    if h > 24 || m > 59 || (h == 24 && m > 0) {
        return Err(bad());
    }
    Ok(h * 60 + m)
}

pub fn fmt_date(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn fmt_hm(m: u16) -> String {
    format!("{:02}:{:02}", m / 60, m % 60)
}

pub fn fmt_datetime(t: i64) -> String {
    format!("{}T{}", fmt_date(t.div_euclid(DAY)), fmt_hm(t.rem_euclid(DAY) as u16))
}

pub type Hours = Vec<(u16, u16)>;

/// Parses `08:00-12:00 13:00-17:00` (space or comma separated). `off` = no hours.
pub fn parse_hours(s: &str) -> Result<Hours, String> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("off") {
        return Ok(vec![]);
    }
    let mut v: Hours = vec![];
    for part in s.split([' ', ',']).filter(|x| !x.is_empty()) {
        let (a, b) = part.split_once('-').ok_or(format!("bad hours '{part}' (expected 08:00-12:00)"))?;
        let (a, b) = (parse_hm(a)?, parse_hm(b)?);
        if b <= a {
            return Err(format!("hours '{part}' end before they start"));
        }
        v.push((a, b));
    }
    v.sort();
    for w in v.windows(2) {
        if w[1].0 < w[0].1 {
            return Err(format!("hours {}-{} and {}-{} overlap", fmt_hm(w[0].0), fmt_hm(w[0].1), fmt_hm(w[1].0), fmt_hm(w[1].1)));
        }
    }
    Ok(v)
}

pub fn fmt_hours(h: &Hours) -> String {
    if h.is_empty() {
        return "off".into();
    }
    h.iter().map(|(a, b)| format!("{}-{}", fmt_hm(*a), fmt_hm(*b))).collect::<Vec<_>>().join(" ")
}

/// A dated exception: a range of days that are either off (`hours` empty)
/// or worked with special hours.
#[derive(Clone, Debug, PartialEq)]
pub struct Exception {
    pub from: i64,
    pub to: i64,
    pub name: String,
    pub hours: Hours,
}

#[derive(Clone, Debug)]
pub struct Calendar {
    pub week: [Hours; 7],
    exceptions: BTreeMap<i64, Hours>,
}

impl Calendar {
    pub fn new(week: [Hours; 7], exceptions: &[Exception]) -> Result<Calendar, String> {
        if week.iter().all(|h| h.is_empty()) {
            return Err("the calendar has no working hours on any weekday".into());
        }
        let mut c = Calendar { week, exceptions: BTreeMap::new() };
        for e in exceptions {
            c.add_exception(e);
        }
        Ok(c)
    }

    pub fn add_exception(&mut self, e: &Exception) {
        for d in e.from..=e.to.min(e.from + MAX_DAYS) {
            self.exceptions.insert(d, e.hours.clone());
        }
    }

    pub fn hours(&self, day: i64) -> &Hours {
        self.exceptions.get(&day).unwrap_or(&self.week[weekday(day)])
    }

    pub fn is_working_day(&self, day: i64) -> bool {
        !self.hours(day).is_empty()
    }

    /// Earliest working instant at or after `t`.
    pub fn next_work(&self, t: i64) -> i64 {
        let d0 = t.div_euclid(DAY);
        for day in d0..d0 + MAX_DAYS {
            for &(a, b) in self.hours(day) {
                let (s, e) = (day * DAY + a as i64, day * DAY + b as i64);
                if e > t {
                    return s.max(t);
                }
            }
        }
        t
    }

    /// Latest working instant at or before `t` (an end-of-work boundary).
    pub fn prev_work(&self, t: i64) -> i64 {
        let d0 = (t - 1).div_euclid(DAY);
        for day in (d0 - MAX_DAYS..=d0).rev() {
            for &(a, b) in self.hours(day).iter().rev() {
                let (s, e) = (day * DAY + a as i64, day * DAY + b as i64);
                if s < t {
                    return e.min(t);
                }
            }
        }
        t
    }

    /// Moves forward from `t` by `mins` working minutes.
    pub fn add(&self, t: i64, mins: i64) -> i64 {
        if mins < 0 {
            return self.sub(t, -mins);
        }
        if mins == 0 {
            return t;
        }
        let mut left = mins;
        let d0 = t.div_euclid(DAY);
        for day in d0..d0 + MAX_DAYS {
            for &(a, b) in self.hours(day) {
                let (s, e) = (day * DAY + a as i64, day * DAY + b as i64);
                if e <= t {
                    continue;
                }
                let st = s.max(t);
                if e - st >= left {
                    return st + left;
                }
                left -= e - st;
            }
        }
        t + mins
    }

    /// Moves backward from `t` by `mins` working minutes.
    pub fn sub(&self, t: i64, mins: i64) -> i64 {
        if mins < 0 {
            return self.add(t, -mins);
        }
        if mins == 0 {
            return t;
        }
        let mut left = mins;
        let d0 = (t - 1).div_euclid(DAY);
        for day in (d0 - MAX_DAYS..=d0).rev() {
            for &(a, b) in self.hours(day).iter().rev() {
                let (s, e) = (day * DAY + a as i64, day * DAY + b as i64);
                if s >= t {
                    continue;
                }
                let en = e.min(t);
                if en - s >= left {
                    return en - left;
                }
                left -= en - s;
            }
        }
        t - mins
    }

    /// Working minutes between `a` and `b` (negative if `b < a`).
    pub fn work_between(&self, a: i64, b: i64) -> i64 {
        if b < a {
            return -self.work_between(b, a);
        }
        let mut n = 0;
        for day in a.div_euclid(DAY)..=(b - 1).div_euclid(DAY).max(a.div_euclid(DAY)) {
            for &(x, y) in self.hours(day) {
                let (s, e) = (day * DAY + x as i64, day * DAY + y as i64);
                n += (e.min(b) - s.max(a)).max(0);
            }
        }
        n
    }
}

/// Default week: Mon–Fri 08:00-12:00 13:00-17:00 (8 h/day).
pub fn default_week() -> [Hours; 7] {
    let std = vec![(480, 720), (780, 1020)];
    [vec![], std.clone(), std.clone(), std.clone(), std.clone(), std, vec![]]
}

/// Parses weekday specs like `Mon-Fri`, `Sat`, `Mon,Wed,Fri`.
pub fn parse_weekdays(s: &str) -> Result<Vec<usize>, String> {
    let find = |n: &str| {
        WEEKDAYS
            .iter()
            .position(|w| w.eq_ignore_ascii_case(n.trim().get(..3).unwrap_or("")))
            .ok_or(format!("unknown weekday '{n}' (use Mon, Tue, …)"))
    };
    let mut out = vec![];
    for part in s.split(',').filter(|x| !x.trim().is_empty()) {
        match part.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (find(a)?, find(b)?);
                let mut i = a;
                loop {
                    out.push(i);
                    if i == b {
                        break;
                    }
                    i = (i + 1) % 7;
                }
            }
            None => out.push(find(part)?),
        }
    }
    Ok(out)
}

/// Formats a weekday set compactly (`Mon-Fri`, `Sat,Sun`).
pub fn fmt_weekdays(days: &[usize]) -> String {
    // Order Mon..Sun so Mon-Fri reads naturally.
    let order = [1, 2, 3, 4, 5, 6, 0];
    let mut parts: Vec<String> = vec![];
    let mut run: Vec<usize> = vec![];
    for &d in &order {
        if days.contains(&d) {
            run.push(d);
        } else if !run.is_empty() {
            parts.push(run_str(&run));
            run.clear();
        }
    }
    if !run.is_empty() {
        parts.push(run_str(&run));
    }
    parts.join(",")
}

fn run_str(run: &[usize]) -> String {
    if run.len() >= 3 {
        format!("{}-{}", WEEKDAYS[run[0]], WEEKDAYS[*run.last().unwrap()])
    } else {
        run.iter().map(|d| WEEKDAYS[*d]).collect::<Vec<_>>().join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal() -> Calendar {
        Calendar::new(default_week(), &[]).unwrap()
    }
    fn t(s: &str) -> i64 {
        parse_datetime(s).unwrap()
    }

    #[test]
    fn dates_roundtrip_and_validate() {
        for z in [-1000, 0, 19000, 20766] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
        assert_eq!(weekday(parse_date("2026-11-09").unwrap()), 1);
        assert!(parse_date("2026-02-30").is_err());
        assert_eq!(fmt_datetime(t("2026-11-09T13:30")), "2026-11-09T13:30");
    }

    #[test]
    fn working_time_math() {
        let c = cal();
        // Friday 16:00 + 2h -> Monday 09:00
        assert_eq!(fmt_datetime(c.add(t("2026-11-06T16:00"), 120)), "2026-11-09T09:00");
        // a full day from Monday 08:00 ends Monday 17:00
        assert_eq!(fmt_datetime(c.add(t("2026-11-09T08:00"), 480)), "2026-11-09T17:00");
        assert_eq!(fmt_datetime(c.next_work(t("2026-11-07T10:00"))), "2026-11-09T08:00");
        assert_eq!(fmt_datetime(c.next_work(t("2026-11-09T12:30"))), "2026-11-09T13:00");
        assert_eq!(fmt_datetime(c.prev_work(t("2026-11-09T08:00"))), "2026-11-06T17:00");
        assert_eq!(fmt_datetime(c.sub(t("2026-11-09T17:00"), 480)), "2026-11-09T08:00");
        assert_eq!(c.work_between(t("2026-11-06T08:00"), t("2026-11-09T17:00")), 960);
        assert_eq!(c.work_between(t("2026-11-09T17:00"), t("2026-11-06T08:00")), -960);
    }

    #[test]
    fn exceptions_and_weekdays() {
        let off = Exception { from: parse_date("2026-11-10").unwrap(), to: parse_date("2026-11-10").unwrap(), name: "x".into(), hours: vec![] };
        let sat = Exception { from: parse_date("2026-11-07").unwrap(), to: parse_date("2026-11-07").unwrap(), name: "".into(), hours: parse_hours("09:00-13:00").unwrap() };
        let c = Calendar::new(default_week(), &[off, sat]).unwrap();
        assert_eq!(fmt_datetime(c.add(t("2026-11-06T17:00"), 240)), "2026-11-07T13:00");
        assert_eq!(fmt_datetime(c.add(t("2026-11-09T17:00"), 60)), "2026-11-11T09:00");
        assert_eq!(parse_weekdays("Mon-Fri").unwrap(), vec![1, 2, 3, 4, 5]);
        assert_eq!(parse_weekdays("Sat-Mon").unwrap(), vec![6, 0, 1]);
        assert_eq!(fmt_weekdays(&[1, 2, 3, 4, 5]), "Mon-Fri");
        assert_eq!(fmt_weekdays(&[6, 0]), "Sat,Sun");
        assert!(parse_hours("09:00-08:00").is_err());
        assert!(parse_hours("08:00-12:00 11:00-13:00").is_err());
        assert!(Calendar::new(Default::default(), &[]).is_err());
    }
}
