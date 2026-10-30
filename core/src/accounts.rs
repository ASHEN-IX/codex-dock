use crate::error::{Error, Result};
use crate::paths::Paths;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub name: String,
    pub path: PathBuf,
    pub account_id: Option<String>,
    pub logged_in: bool,
}

pub fn auth_info(path: &Path) -> (Option<String>, bool) {
    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(_) => return (None, false),
    };
    let v: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return (None, false),
    };
    let id = v
        .get("tokens")
        .and_then(|t| t.get("account_id"))
        .and_then(|a| a.as_str())
        .map(|s| s.to_string());
    let logged_in = id.is_some() || v.get("OPENAI_API_KEY").and_then(|k| k.as_str()).is_some();
    (id, logged_in)
}

pub fn list(p: &Paths) -> Result<Vec<Account>> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(&p.profiles) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(Error::Io(e)),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let path = std::fs::canonicalize(entry.path()).unwrap_or_else(|_| entry.path());
        if !path.is_dir() {
            continue;
        }
        let (account_id, logged_in) = auth_info(&entry.path().join("auth.json"));
        out.push(Account { name, path, account_id, logged_in });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}
