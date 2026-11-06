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

impl Calendar {
    /// Days that have explicit exceptions (holidays, special working days).
    pub fn exception_days(&self) -> impl Iterator<Item = &i64> {
        self.exceptions.keys()
    }

    /// Working time shared by every calendar (people who must work together).
    /// Returns `None` if they never overlap.
    pub fn intersect(cals: &[&Calendar]) -> Option<Calendar> {
        let first = cals.first()?;
        if cals.len() == 1 {
            return Some((*first).clone());
        }
        let mut week: [Hours; 7] = first.week.clone();
        for c in &cals[1..] {
            for (d, h) in week.iter_mut().enumerate() {
                *h = intersect_hours(h, &c.week[d]);
            }
        }
        if week.iter().all(|h| h.is_empty()) {
            return None;
        }
        let mut days: Vec<i64> = cals.iter().flat_map(|c| c.exception_days().copied()).collect();
        days.sort();
        days.dedup();
        let mut out = Calendar { week, exceptions: BTreeMap::new() };
        for d in days {
            let mut h = first.hours(d).clone();
            for c in &cals[1..] {
                h = intersect_hours(&h, c.hours(d));
            }
            out.exceptions.insert(d, h);
        }
        Some(out)
    }

    /// Marks a range of days as not working.
    pub fn block_days(&mut self, from: i64, to: i64) {
        for d in from..=to.min(from + MAX_DAYS) {
            self.exceptions.insert(d, vec![]);
        }
    }

    /// Replaces the weekly pattern (keeps dated exceptions).
    pub fn with_week(&self, week: &[Hours; 7]) -> Calendar {
        Calendar { week: week.clone(), exceptions: self.exceptions.clone() }
    }
}

pub fn intersect_hours(a: &Hours, b: &Hours) -> Hours {
    let mut out = vec![];
    for &(a0, a1) in a {
        for &(b0, b1) in b {
            let (s, e) = (a0.max(b0), a1.min(b1));
            if s < e {
                out.push((s, e));
            }
        }
    }
    out.sort();
    out
}

// ---------- recurring holidays ----------

pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    /// Same month and day every year.
    Fixed { month: u8, day: u8 },
    /// e.g. 3rd Monday of January; `n = -1` means the last one.
    Nth { n: i8, weekday: u8, month: u8 },
    /// Days relative to (Western) Easter Sunday.
    Easter { offset: i32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shift {
    None,
    /// Saturday → Friday, Sunday → Monday (US style).
    Observed,
    /// Moved to the following Monday unless it is one (Colombia's "Ley Emiliani").
    Monday,
    /// Weekend → next weekday that isn't already a holiday (UK style).
    Substitute,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recurring {
    pub rule: Rule,
    pub shift: Shift,
    pub name: String,
}

/// Easter Sunday (Gregorian, anonymous algorithm) as a day number.
pub fn easter(year: i64) -> i64 {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    days_from_civil(year, month, day)
}

fn month_index(s: &str) -> Option<u8> {
    let s = s.trim().to_ascii_lowercase();
    MONTHS.iter().position(|m| s.starts_with(&m.to_ascii_lowercase())).map(|i| i as u8 + 1)
}

/// Parses the part after `every`: `12-25 [shift] Name`, `3rd Mon Jan [shift] Name`, `easter-2 [shift] Name`.
pub fn parse_recurring(s: &str) -> Result<Recurring, String> {
    let toks: Vec<&str> = s.split_whitespace().collect();
    let bad = || format!("bad repeating date '{s}' — try 12-25, 3rd Mon Jan, last Mon May or easter-2");
    let first = *toks.first().ok_or_else(bad)?;
    let (rule, used) = if let Some(off) = first.strip_prefix("easter") {
        let offset = if off.is_empty() { 0 } else { off.parse::<i32>().map_err(|_| bad())? };
        (Rule::Easter { offset }, 1)
    } else if let Some((m, d)) = first.split_once('-').filter(|(m, _)| m.len() <= 2 && m.chars().all(|c| c.is_ascii_digit())) {
        let (month, day): (u8, u8) = (m.parse().map_err(|_| bad())?, d.parse().map_err(|_| bad())?);
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return Err(bad());
        }
        (Rule::Fixed { month, day }, 1)
    } else {
        let n = match first.to_ascii_lowercase().as_str() {
            "1st" | "first" => 1,
            "2nd" | "second" => 2,
            "3rd" | "third" => 3,
            "4th" | "fourth" => 4,
            "5th" | "fifth" => 5,
            "last" => -1,
            _ => return Err(bad()),
        };
        let wd = toks.get(1).and_then(|w| WEEKDAYS.iter().position(|x| x.eq_ignore_ascii_case(w.get(..3).unwrap_or("")))).ok_or_else(bad)?;
        let month = toks.get(2).and_then(|m| month_index(m)).ok_or_else(bad)?;
        (Rule::Nth { n, weekday: wd as u8, month }, 3)
    };
    let mut rest = &toks[used..];
    let shift = match rest.first().map(|t| t.to_ascii_lowercase()) {
        Some(t) if t == "observed" => Shift::Observed,
        Some(t) if t == "monday" => Shift::Monday,
        Some(t) if t == "substitute" => Shift::Substitute,
        _ => Shift::None,
    };
    if shift != Shift::None {
        rest = &rest[1..];
    }
    Ok(Recurring { rule, shift, name: rest.join(" ") })
}

pub fn fmt_recurring(r: &Recurring) -> String {
    let rule = match &r.rule {
        Rule::Fixed { month, day } => format!("{month:02}-{day:02}"),
        Rule::Nth { n, weekday, month } => {
            let nth = match n {
                1 => "1st",
                2 => "2nd",
                3 => "3rd",
                4 => "4th",
                5 => "5th",
                _ => "last",
            };
            format!("{nth} {} {}", WEEKDAYS[*weekday as usize], MONTHS[*month as usize - 1])
        }
        Rule::Easter { offset: 0 } => "easter".into(),
        Rule::Easter { offset } => format!("easter{offset:+}"),
    };
    let shift = match r.shift {
        Shift::None => "",
        Shift::Observed => " observed",
        Shift::Monday => " monday",
        Shift::Substitute => " substitute",
    };
    format!("{rule}{shift}{}", if r.name.is_empty() { String::new() } else { format!(" {}", r.name) })
}

fn days_in_month(y: i64, m: i64) -> i64 {
    let next = if m == 12 { days_from_civil(y + 1, 1, 1) } else { days_from_civil(y, m + 1, 1) };
    next - days_from_civil(y, m, 1)
}

/// The rule's date in a given year, before any weekend shifting.
pub fn rule_date(rule: &Rule, year: i64) -> Option<i64> {
    match *rule {
        Rule::Fixed { month, day } => {
            let (m, d) = (month as i64, day as i64);
            (d <= days_in_month(year, m)).then(|| days_from_civil(year, m, d))
        }
        Rule::Nth { n, weekday: wd, month } => {
            let m = month as i64;
            if n > 0 {
                let first = days_from_civil(year, m, 1);
                let delta = (wd as i64 - weekday(first) as i64).rem_euclid(7);
                let d = first + delta + 7 * (n as i64 - 1);
                (d < first + days_in_month(year, m)).then_some(d)
            } else {
                let last = days_from_civil(year, m, days_in_month(year, m));
                Some(last - (weekday(last) as i64 - wd as i64).rem_euclid(7))
            }
        }
        Rule::Easter { offset } => Some(easter(year) + offset as i64),
    }
}

/// Concrete holiday dates for the given years (in rule order, so substitutes don't collide).
pub fn expand_recurring(rules: &[Recurring], years: std::ops::RangeInclusive<i64>) -> Vec<Exception> {
    let mut out = vec![];
    for y in years {
        let mut taken: Vec<i64> = vec![];
        for r in rules {
            let Some(mut d) = rule_date(&r.rule, y) else { continue };
            let wd = weekday(d);
            match r.shift {
                Shift::None => {}
                Shift::Observed => {
                    if wd == 6 {
                        d -= 1;
                    } else if wd == 0 {
                        d += 1;
                    }
                }
                Shift::Monday => d += (8 - wd as i64) % 7,
                Shift::Substitute => {
                    if wd == 0 || wd == 6 {
                        d += 1;
                        while weekday(d) == 0 || weekday(d) == 6 || taken.contains(&d) {
                            d += 1;
                        }
                    }
                }
            }
            taken.push(d);
            out.push(Exception { from: d, to: d, name: r.name.clone(), hours: vec![] });
        }
    }
    out
}

/// Public holidays for a few countries, as `.plan` repeating-holiday rules.
/// National days only — regional and one-off holidays must be added by hand.
pub fn country_holidays(code: &str) -> Option<&'static [&'static str]> {
    Some(match code.to_ascii_uppercase().as_str() {
        "US" => &[
            "01-01 observed New Year's Day",
            "3rd Mon Jan Martin Luther King Jr. Day",
            "3rd Mon Feb Washington's Birthday",
            "last Mon May Memorial Day",
            "06-19 observed Juneteenth",
            "07-04 observed Independence Day",
            "1st Mon Sep Labor Day",
            "2nd Mon Oct Columbus Day",
            "11-11 observed Veterans Day",
            "4th Thu Nov Thanksgiving",
            "12-25 observed Christmas Day",
        ],
        "UK" | "GB" => &[
            "01-01 substitute New Year's Day",
            "easter-2 Good Friday",
            "easter+1 Easter Monday",
            "1st Mon May Early May bank holiday",
            "last Mon May Spring bank holiday",
            "last Mon Aug Summer bank holiday",
            "12-25 substitute Christmas Day",
            "12-26 substitute Boxing Day",
        ],
        "ES" => &[
            "01-01 Año Nuevo",
            "01-06 Epifanía del Señor",
            "easter-2 Viernes Santo",
            "05-01 Fiesta del Trabajo",
            "08-15 Asunción de la Virgen",
            "10-12 Fiesta Nacional de España",
            "11-01 Todos los Santos",
            "12-06 Día de la Constitución",
            "12-08 Inmaculada Concepción",
            "12-25 Navidad",
        ],
        "MX" => &[
            "01-01 Año Nuevo",
            "1st Mon Feb Día de la Constitución",
            "3rd Mon Mar Natalicio de Benito Juárez",
            "05-01 Día del Trabajo",
            "09-16 Día de la Independencia",
            "3rd Mon Nov Día de la Revolución",
            "12-25 Navidad",
        ],
        "CO" => &[
            "01-01 Año Nuevo",
            "01-06 monday Día de los Reyes Magos",
            "03-19 monday Día de San José",
            "easter-3 Jueves Santo",
            "easter-2 Viernes Santo",
            "05-01 Día del Trabajo",
            "easter+43 Ascensión del Señor",
            "easter+64 Corpus Christi",
            "easter+71 Sagrado Corazón",
            "06-29 monday San Pedro y San Pablo",
            "07-20 Día de la Independencia",
            "08-07 Batalla de Boyacá",
            "08-15 monday Asunción de la Virgen",
            "10-12 monday Día de la Raza",
            "11-01 monday Todos los Santos",
            "11-11 monday Independencia de Cartagena",
            "12-08 Inmaculada Concepción",
            "12-25 Navidad",
        ],
        _ => return None,
    })
}

pub const COUNTRIES: [(&str, &str); 5] = [("CO", "Colombia"), ("ES", "España"), ("MX", "México"), ("UK", "United Kingdom"), ("US", "United States")];

/// Today's local date as a day number (falls back to UTC).
pub fn local_today() -> i64 {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTime {
            year: u16,
            month: u16,
            dow: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            ms: u16,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetLocalTime(t: *mut SystemTime);
        }
        let mut t = SystemTime { year: 0, month: 0, dow: 0, day: 0, hour: 0, minute: 0, second: 0, ms: 0 };
        unsafe { GetLocalTime(&mut t) };
        if t.year > 0 {
            return days_from_civil(t.year as i64, t.month as i64, t.day as i64);
        }
    }
    #[cfg(unix)]
    {
        #[repr(C)]
        struct Tm {
            sec: i32,
            min: i32,
            hour: i32,
            mday: i32,
            mon: i32,
            year: i32,
            wday: i32,
            yday: i32,
            isdst: i32,
            gmtoff: std::ffi::c_long,
            zone: *const std::ffi::c_char,
        }
        extern "C" {
            fn time(t: *mut i64) -> i64;
            fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
        }
        let mut tm = Tm { sec: 0, min: 0, hour: 0, mday: 0, mon: 0, year: 0, wday: 0, yday: 0, isdst: 0, gmtoff: 0, zone: std::ptr::null() };
        let now = unsafe { time(std::ptr::null_mut()) };
        if !unsafe { localtime_r(&now, &mut tm) }.is_null() {
            return days_from_civil(tm.year as i64 + 1900, tm.mon as i64 + 1, tm.mday as i64);
        }
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64 / 86_400).unwrap_or(0)
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

    fn on(rule: &str, year: i64) -> String {
        let r = parse_recurring(rule).unwrap();
        fmt_date(expand_recurring(&[r], year..=year)[0].from)
    }

    #[test]
    fn repeating_holidays() {
        assert_eq!(fmt_date(easter(2026)), "2026-04-05");
        assert_eq!(fmt_date(easter(2027)), "2027-03-28");
        assert_eq!(fmt_date(easter(2024)), "2024-03-31");
        assert_eq!(on("12-25 Christmas", 2026), "2026-12-25");
        assert_eq!(on("4th Thu Nov Thanksgiving", 2026), "2026-11-26");
        assert_eq!(on("last Mon May Memorial Day", 2026), "2026-05-25");
        assert_eq!(on("3rd Mon Jan", 2027), "2027-01-18");
        assert_eq!(on("easter-2 Good Friday", 2026), "2026-04-03");
        // US observed: July 4 2026 is a Saturday → Friday July 3
        assert_eq!(on("07-04 observed", 2026), "2026-07-03");
        // Colombia: Jan 6 2027 (Wed) moves to Monday Jan 11
        assert_eq!(on("01-06 monday Reyes", 2027), "2027-01-11");
        assert_eq!(on("01-06 monday Reyes", 2025), "2025-01-06", "already a Monday");
        // UK 2027: Christmas Sat → Mon 27, Boxing Sun → Tue 28
        let uk: Vec<Recurring> = country_holidays("UK").unwrap().iter().map(|l| parse_recurring(l).unwrap()).collect();
        let d: Vec<String> = expand_recurring(&uk, 2027..=2027).iter().map(|e| fmt_date(e.from)).collect();
        assert!(d.contains(&"2027-12-27".into()) && d.contains(&"2027-12-28".into()), "{d:?}");
        // every preset parses and round-trips
        for (code, _) in COUNTRIES {
            for line in country_holidays(code).unwrap() {
                let r = parse_recurring(line).unwrap();
                assert_eq!(parse_recurring(&fmt_recurring(&r)).unwrap(), r, "{line}");
            }
        }
        assert!(parse_recurring("13-01").is_err());
        assert!(parse_recurring("6th Mon Jan").is_err());
        assert!(local_today() > 20_000);
    }

    #[test]
    fn intersecting_calendars() {
        let a = Calendar::new(default_week(), &[]).unwrap();
        let mut wk = default_week();
        wk[1] = vec![(600, 900)];
        wk[2] = vec![];
        let b = Calendar::new(wk, &[]).unwrap();
        let c = Calendar::intersect(&[&a, &b]).unwrap();
        assert_eq!(c.week[1], vec![(600, 720), (780, 900)]);
        assert!(c.week[2].is_empty());
        let only_sat = { let mut w: [Hours; 7] = Default::default(); w[6] = vec![(480, 600)]; Calendar::new(w, &[]).unwrap() };
        assert!(Calendar::intersect(&[&a, &only_sat]).is_none());
    }
}
