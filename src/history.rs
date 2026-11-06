//! Version history: earlier versions of a project are appended to
//! `<folder>/.rplan-history/<file>.history`, so you can see what changed and go back.
//! The file is append-only (it's only rewritten to trim it when it gets very large).

use crate::model::Project;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MARK: &str = "=== ";
/// Quiet edits within this many seconds share one history entry.
const COALESCE_SECS: u64 = 90;
const MAX_BYTES: u64 = 30 << 20;
const KEEP_WHEN_TRIMMING: usize = 300;

static LAST: Mutex<Option<HashMap<PathBuf, u64>>> = Mutex::new(None);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn history_file(path: &str) -> PathBuf {
    let p = Path::new(path);
    let dir = p.parent().filter(|d| !d.as_os_str().is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    dir.join(".rplan-history").join(format!("{}.history", p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| "project.plan".into())))
}

pub struct Entry {
    pub ms: u64,
    pub note: String,
    pub text: String,
}

pub fn entries(path: &str) -> Vec<Entry> {
    let Ok(all) = fs::read_to_string(history_file(path)) else { return vec![] };
    let mut out: Vec<Entry> = vec![];
    for line in all.split_inclusive('\n') {
        if let Some(h) = line.strip_prefix(MARK) {
            let h = h.trim_end();
            let (ms, note) = h.split_once(" | ").unwrap_or((h, ""));
            out.push(Entry { ms: ms.trim().parse().unwrap_or(0), note: note.to_string(), text: String::new() });
        } else if let Some(e) = out.last_mut() {
            e.text.push_str(line);
        }
    }
    out
}

/// Remembers the file's current content (the version about to be replaced).
/// Skipped if it's already the latest entry, or if the last entry is recent and `force` is off.
pub fn record(path: &str, current: &str, note: &str, force: bool) {
    if current.trim().is_empty() {
        return;
    }
    let file = history_file(path);
    let mut guard = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let last = guard.get_or_insert_with(HashMap::new);
    let now = now_ms();
    if !force && last.get(&file).is_some_and(|t| now.saturating_sub(*t) < COALESCE_SECS * 1000) {
        return;
    }
    if entries(path).last().is_some_and(|e| e.text == current) {
        return;
    }
    if fs::create_dir_all(file.parent().unwrap()).is_err() {
        return;
    }
    if fs::metadata(&file).is_ok_and(|m| m.len() > MAX_BYTES) {
        let mut keep = entries(path);
        let drop = keep.len().saturating_sub(KEEP_WHEN_TRIMMING);
        keep.drain(..drop);
        let body: String = keep.iter().map(|e| format!("{MARK}{} | {}\n{}", e.ms, e.note, e.text)).collect();
        let _ = fs::write(&file, body);
    }
    let note = note.replace(['\n', '\r'], " ");
    let mut body = format!("{MARK}{now} | {note}\n{current}");
    if !body.ends_with('\n') {
        body.push('\n');
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&file) {
        let _ = f.write_all(body.as_bytes());
        last.insert(file, now);
    }
}

/// A human summary of what differs between an old version and the current one.
pub struct Diff {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<(String, Vec<String>)>,
    pub settings: bool,
}

const COLUMNS: [&str; 7] = ["id", "name", "duration", "predecessors", "people", "% done", "details"];

pub fn diff(old: &str, new: &str) -> Result<Diff, String> {
    let (a, b) = (Project::parse(old)?, Project::parse(new)?);
    let lines = |p: &Project| -> HashMap<u32, (String, String)> {
        let text = p.serialize();
        let by_id: HashMap<u32, String> = text
            .lines()
            .filter_map(|l| {
                let t = l.trim_start().strip_prefix("task ")?;
                let id: u32 = t.split('|').next()?.trim().parse().ok()?;
                Some((id, l.trim().to_string()))
            })
            .collect();
        p.tasks.iter().map(|t| (t.id, (t.name.clone(), by_id.get(&t.id).cloned().unwrap_or_default()))).collect()
    };
    let (la, lb) = (lines(&a), lines(&b));
    let mut d = Diff { added: vec![], removed: vec![], changed: vec![], settings: false };
    for t in &b.tasks {
        match la.get(&t.id) {
            None => d.added.push(format!("#{} {}", t.id, t.name)),
            Some((_, old_line)) => {
                let new_line = &lb[&t.id].1;
                if old_line != new_line {
                    let (oc, nc): (Vec<&str>, Vec<&str>) = (old_line.split('|').map(str::trim).collect(), new_line.split('|').map(str::trim).collect());
                    let fields: Vec<String> = (1..COLUMNS.len()).filter(|&i| oc.get(i) != nc.get(i)).map(|i| COLUMNS[i].to_string()).collect();
                    d.changed.push((format!("#{} {}", t.id, t.name), fields));
                }
            }
        }
    }
    for t in &a.tasks {
        if !lb.contains_key(&t.id) {
            d.removed.push(format!("#{} {}", t.id, t.name));
        }
    }
    let head = |p: &Project| p.serialize().lines().filter(|l| !l.trim_start().starts_with("task ")).collect::<Vec<_>>().join("\n");
    d.settings = head(&a) != head(&b);
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_diffs_versions() {
        let dir = std::env::temp_dir().join(format!("rplan-hist-{}", crate::server::new_token()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.plan").to_string_lossy().to_string();
        let v1 = "project P\nstart 2026-11-09\ntask 1 | A | 1d\ntask 2 | B | 2d\n";
        let v2 = "project P\nstart 2026-11-09\ntask 1 | A | 3d\ntask 3 | C | 1d\n";
        record(&path, v1, "first", false);
        record(&path, v1, "same again", true);
        record(&path, v2, "too soon", false);
        record(&path, v2, "forced", true);
        let e = entries(&path);
        assert_eq!(e.len(), 2, "duplicates and quick edits are coalesced");
        assert_eq!((e[0].note.as_str(), e[0].text.as_str()), ("first", v1));
        let d = diff(v1, v2).unwrap();
        assert_eq!(d.added, vec!["#3 C"]);
        assert_eq!(d.removed, vec!["#2 B"]);
        assert_eq!(d.changed, vec![("#1 A".to_string(), vec!["duration".to_string()])]);
        assert!(!d.settings);
    }
}
