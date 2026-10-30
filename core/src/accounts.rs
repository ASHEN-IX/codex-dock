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

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub fn create(p: &Paths, name: &str, clone_from: Option<&str>) -> Result<Account> {
    if !valid_name(name) {
        return Err(Error::Msg(format!(
            "invalid account name '{name}': use letters, digits, '-' or '_'"
        )));
    }
    let path = p.profiles.join(name);
    if path.exists() {
        return Err(Error::Msg(format!("account '{name}' already exists")));
    }
    std::fs::create_dir_all(&path)?;

    if let Some(src) = clone_from {
        for item in ["config.toml", "rules", "skills"] {
            let from = p.profiles.join(src).join(item);
            if !from.exists() {
                continue;
            }
            let to = path.join(item);
            if from.is_dir() {
                copy_dir(&from, &to)?;
            } else {
                std::fs::copy(&from, &to)?;
            }
        }
    }

    let (account_id, logged_in) = auth_info(&path.join("auth.json"));
    Ok(Account { name: name.to_string(), path, account_id, logged_in })
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

pub fn remove(p: &Paths, name: &str, backup: &Path) -> Result<PathBuf> {
    let from = p.profiles.join(name);
    if !from.exists() {
        return Err(Error::Msg(format!("account '{name}' does not exist")));
    }
    std::fs::create_dir_all(backup)?;
    let to = backup.join(name);
    std::fs::rename(&from, &to)?;
    Ok(to)
}
