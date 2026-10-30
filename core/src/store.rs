use crate::db;
use crate::paths::Paths;

pub const SHARED: [&str; 5] = [
    "state_5.sqlite",
    "sessions",
    "archived_sessions",
    "memories_1.sqlite",
    "history.jsonl",
];

#[derive(Debug, Clone, PartialEq)]
pub enum StoreState {
    Shared,
    NotConfigured,
    Split { threads_in_profile: usize },
    AuthCycle,
    Nested { profile: String },
}

#[derive(Debug, Clone)]
pub struct StoreReport {
    pub state: StoreState,
    pub threads: usize,
    pub projects: Vec<String>,
    pub inodes: Vec<(String, u64)>,
}

pub fn inspect(p: &Paths) -> StoreReport {
    let base_db = p.base.join("state_5.sqlite");

    let mut accounts = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&p.profiles) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with('.') && e.path().is_dir() {
                accounts.push(name);
            }
        }
    }
    accounts.sort();

    // Nested: the profiles dir lives inside the base store.
    if let (Ok(a), Ok(b)) = (std::fs::canonicalize(&p.profiles), std::fs::canonicalize(&p.base)) {
        if a.starts_with(&b) {
            return report(StoreState::Nested { profile: first(&accounts) }, &base_db);
        }
    }

    // Auth cycle: base auth.json resolves into a profile that resolves back to base.
    let base_auth = p.base.join("auth.json");
    if base_auth.is_symlink() {
        if let Ok(target) = std::fs::canonicalize(&base_auth) {
            for name in &accounts {
                if let Ok(prof) = std::fs::canonicalize(p.profiles.join(name)) {
                    if target.starts_with(&prof) {
                        return report(StoreState::AuthCycle, &base_db);
                    }
                }
            }
        }
    }

    if !base_db.exists() {
        return report(StoreState::NotConfigured, &base_db);
    }

    // Split: a profile has its own real DB holding threads.
    for name in &accounts {
        let prof_db = p.profiles.join(name).join("state_5.sqlite");
        if prof_db.exists() && !prof_db.is_symlink() {
            if let Ok(n) = db::thread_count(&prof_db) {
                if n > 0 {
                    return report(StoreState::Split { threads_in_profile: n }, &base_db);
                }
            }
        }
    }

    report(StoreState::Shared, &base_db)
}

fn first(v: &[String]) -> String {
    v.first().cloned().unwrap_or_default()
}

fn report(state: StoreState, base_db: &std::path::Path) -> StoreReport {
    let threads = db::thread_count(base_db).unwrap_or(0);
    let projects = db::project_names(base_db).unwrap_or_default();
    StoreReport { state, threads, projects, inodes: Vec::new() }
}
