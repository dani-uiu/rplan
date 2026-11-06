//! Things that live in other files: the shared team (`pool team.plan`),
//! sub-projects (`sub=Other.plan`) and links to tasks in other projects
//! (`ext=Other.plan#5`). Paths are relative to the project's folder.

use crate::model::*;
use crate::schedule::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

fn dir_of(path: &str) -> PathBuf {
    Path::new(path).parent().filter(|p| !p.as_os_str().is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn key(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

type Loaded = Result<(Project, Schedule), String>;

struct Resolver {
    cache: HashMap<PathBuf, Loaded>,
    stack: HashSet<PathBuf>,
    warnings: Vec<String>,
}

impl Resolver {
    fn get(&mut self, file: &Path, depth: usize) -> Loaded {
        let k = key(file);
        if let Some(r) = self.cache.get(&k) {
            return r.clone();
        }
        if self.stack.contains(&k) {
            return Err("these projects include each other in a loop".into());
        }
        if depth > 8 {
            return Err("projects are nested too deeply".into());
        }
        self.stack.insert(k.clone());
        let r = (|| {
            let text = fs::read_to_string(file).map_err(|e| format!("can't read it ({e})"))?;
            let mut p = Project::parse(&text)?;
            self.resolve(&mut p, &file.to_string_lossy(), depth + 1);
            let s = schedule(&p)?;
            Ok((p, s))
        })();
        self.stack.remove(&k);
        self.cache.insert(k, r.clone());
        r
    }

    fn resolve(&mut self, p: &mut Project, path: &str, depth: usize) {
        let dir = dir_of(path);
        if let Some(pool) = p.pool.clone() {
            match fs::read_to_string(dir.join(&pool)).map_err(|e| e.to_string()).and_then(|t| Project::parse(&t)) {
                Ok(team) => {
                    for mut r in team.resources {
                        if !p.resources.iter().any(|x| x.name == r.name) {
                            r.pooled = true;
                            p.resources.push(r);
                        }
                    }
                }
                Err(e) => self.warnings.push(format!("Team file “{pool}”: {e}")),
            }
        }
        for i in 0..p.tasks.len() {
            if let Some(sub) = p.tasks[i].sub.clone() {
                match self.get(&dir.join(&sub), depth) {
                    Ok((sp, ss)) => {
                        let cal = sp.calendar().ok();
                        let span = cal.map_or(0, |c| c.work_between(ss.start, ss.finish));
                        p.tasks[i].sub_info = Some(SubInfo { name: sp.name.clone(), span, cost: ss.cost, pct: ss.progress, tasks: sp.tasks.len() });
                    }
                    Err(e) => self.warnings.push(format!("Task {} (sub-project “{sub}”): {e}", p.tasks[i].id)),
                }
            }
            for x in p.tasks[i].xdeps.clone() {
                match self.get(&dir.join(&x.file), depth) {
                    Ok((op, os)) => match op.tasks.iter().position(|t| t.id == x.id) {
                        Some(j) => p.tasks[i].ext.push(ExtLink {
                            kind: x.kind,
                            lag: x.lag,
                            es: os.tasks[j].es,
                            ef: os.tasks[j].ef,
                            label: format!("{} #{} {}", op.name, x.id, op.tasks[j].name),
                        }),
                        None => self.warnings.push(format!("Task {}: “{}” has no task {}", p.tasks[i].id, x.file, x.id)),
                    },
                    Err(e) => self.warnings.push(format!("Task {} (link to “{}”): {e}", p.tasks[i].id, x.file)),
                }
            }
        }
    }
}

/// Fills in pooled resources, sub-project summaries and external links.
/// Returns warnings for anything that couldn't be found.
pub fn resolve(p: &mut Project, path: &str) -> Vec<String> {
    let mut r = Resolver { cache: HashMap::new(), stack: HashSet::from([key(Path::new(path))]), warnings: vec![] };
    r.resolve(p, path, 0);
    r.warnings
}

/// Workload from the other projects in the same folder that share this team file:
/// person → day → (peak units, work minutes, project names).
pub fn other_loads(p: &Project, path: &str) -> HashMap<String, HashMap<i64, (u32, i64, Vec<String>)>> {
    let mut out: HashMap<String, HashMap<i64, (u32, i64, Vec<String>)>> = HashMap::new();
    let Some(pool) = &p.pool else { return out };
    let me = key(Path::new(path));
    let Ok(rd) = fs::read_dir(dir_of(path)) else { return out };
    for e in rd.flatten().take(60) {
        let f = e.path();
        if !f.extension().is_some_and(|x| x.eq_ignore_ascii_case("plan")) || key(&f) == me {
            continue;
        }
        let Ok(text) = fs::read_to_string(&f) else { continue };
        if !text.lines().any(|l| l.trim() == format!("pool {pool}")) {
            continue;
        }
        let Ok(mut op) = Project::parse(&text) else { continue };
        resolve(&mut op, &f.to_string_lossy());
        let Ok(os) = schedule(&op) else { continue };
        for l in loads(&op, &os) {
            let m = out.entry(l.name.clone()).or_default();
            for (d, (u, mins, _)) in l.days {
                let v = m.entry(d).or_insert((0, 0, vec![]));
                v.0 += u;
                v.1 += mins;
                if !v.2.contains(&op.name) {
                    v.2.push(op.name.clone());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_subprojects_and_external_links() {
        let dir = std::env::temp_dir().join(format!("rplan-ext-{}", crate::server::new_token()));
        fs::create_dir_all(&dir).unwrap();
        let w = |n: &str, t: &str| fs::write(dir.join(n), t).unwrap();
        w("team.plan", "project Team\nresource Ana | rate 100 | week Mon-Wed 08:00-12:00 13:00-17:00 | week Thu-Sun off\n");
        w("sub.plan", "project Sub\nstart 2026-11-09\ntask 1 | S1 | 2d\ntask 2 | S2 | 3d | 1 | | 40\n");
        w("other.plan", "project Other\nstart 2026-11-09\npool team.plan\ntask 7 | Supplier | 4d | | Ana\n");
        w("main.plan", "project Main\nstart 2026-11-09\npool team.plan\ntask 1 | Subproject | 1d | | | | sub=sub.plan\ntask 2 | After supplier | 1d | | | | ext=other.plan#7\ntask 3 | Work | 1d | | Ana\ntask 4 | Broken | 1d | | | | ext=missing.plan#1\n");
        let path = dir.join("main.plan").to_string_lossy().to_string();
        let mut p = Project::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        let warn = resolve(&mut p, &path);
        assert_eq!(warn.len(), 1, "{warn:?}");
        assert!(p.resources.iter().any(|r| r.name == "Ana" && r.pooled && r.rate == 100.0));
        let si = p.tasks[0].sub_info.as_ref().unwrap();
        assert_eq!((si.span, si.tasks), (5 * 480, 2));
        let s = schedule(&p).unwrap();
        assert_eq!(s.tasks[0].dur, 5 * 480, "sub-project duration");
        // Ana works Mon–Wed in "other", so the supplier task ends Monday the 16th.
        assert_eq!(crate::calendar::fmt_datetime(s.tasks[1].es), "2026-11-17T08:00");
        assert!(!p.serialize().contains("resource Ana"), "pooled people stay in the team file");
        let other = other_loads(&p, &path);
        assert!(other.get("Ana").is_some_and(|d| !d.is_empty()));
        // loops between projects are reported, not followed forever
        w("a.plan", "project A\nstart 2026-11-09\ntask 1 | X | 1d | | | | sub=b.plan\n");
        w("b.plan", "project B\nstart 2026-11-09\ntask 1 | Y | 1d | | | | sub=a.plan\n");
        let pa = dir.join("a.plan").to_string_lossy().to_string();
        let mut a = Project::parse(&fs::read_to_string(&pa).unwrap()).unwrap();
        assert!(resolve(&mut a, &pa).iter().any(|w| w.contains("loop")));
    }
}
