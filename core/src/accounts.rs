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

pub fn profiles(p: &Paths) -> Result<Vec<Account>> {
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

/// Display one row per signed-in identity. Preserve every profile on disk.
pub fn list(p: &Paths) -> Result<Vec<Account>> {
    let mut all = profiles(p)?;
    // A named profile with private credentials is preferable to a default alias.
    all.sort_by_key(|a| (a.name == "default", a.path.join("auth.json").is_symlink(), a.name.clone()));
    let mut out: Vec<Account> = Vec::new();
    for a in all {
        let duplicate = a.account_id.as_ref().is_some_and(|id|
            out.iter().any(|b| b.account_id.as_ref() == Some(id)));
        if !duplicate { out.push(a); }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn canonical_name(p: &Paths, name: &str) -> Result<String> {
    let all = profiles(p)?;
    let account = all.iter().find(|a| a.name == name)
        .ok_or_else(|| Error::Msg(format!("unknown account '{name}'")))?;
    if let Some(id) = &account.account_id {
        if let Some(canonical) = list(p)?.into_iter().find(|a| a.account_id.as_ref() == Some(id)) {
            return Ok(canonical.name);
        }
    }
    Ok(name.to_string())
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

/// Prepare a profile for `codex login`, keeping whatever it was signed in with.
///
/// Two facts about `codex login` drive this, both verified against 0.164:
///
/// 1. It writes *through* a symlink: the link survives and its target is
///    rewritten. The live layout links `~/.codex/auth.json` — and with it the
///    `default` profile — at `work/auth.json`, so signing in to `default` would
///    silently replace the `work` account's credentials.
/// 2. It deletes `auth.json` as it starts and only writes a new one on success,
///    so an abandoned sign-in would leave the account logged out.
///
/// So: a symlink becomes a private copy of its target (nothing shared, nothing
/// lost), and a snapshot goes to the backup either way, for
/// [`restore_auth`] to put back if the sign-in never completes.
///
/// Returns whether a link was materialised.
pub fn materialise_auth(profile: &Path, backup: &Path) -> Result<bool> {
    let auth = profile.join("auth.json");
    let meta = match std::fs::symlink_metadata(&auth) {
        Ok(m) => m,
        // No credentials yet: login will create a fresh file, nothing to keep.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(Error::Io(e)),
    };

    let was_link = meta.file_type().is_symlink();
    if was_link {
        // Resolve *before* moving the link: after the rename the path no longer
        // exists, so a later canonicalize would find nothing.
        let target = std::fs::canonicalize(&auth).ok();
        std::fs::create_dir_all(backup)?;
        std::fs::rename(&auth, backup.join("auth.json.link"))?;
        // Carry the credentials over, so the account starts where it was. A
        // dangling link leaves the profile without credentials, so login
        // creates them instead of facing an empty file.
        if let Some(target) = target {
            if target.is_file() {
                std::fs::copy(&target, &auth)?;
            }
        }
    }

    if auth.is_file() {
        std::fs::create_dir_all(backup)?;
        std::fs::copy(&auth, backup.join(PRE_LOGIN))?;
    }
    Ok(was_link)
}

/// The snapshot [`materialise_auth`] takes before a sign-in.
const PRE_LOGIN: &str = "auth.json.pre-login";

/// Put the pre-sign-in credentials back when a sign-in did not complete.
///
/// `codex login` deletes `auth.json` on start, so a cancelled or failed attempt
/// would otherwise leave the account signed out. Only restores when the profile
/// has no credentials at all: a completed sign-in must never be overwritten.
/// Returns whether anything was restored.
pub fn restore_auth(profile: &Path, backup: &Path) -> Result<bool> {
    let auth = profile.join("auth.json");
    if auth.exists() || auth.is_symlink() {
        return Ok(false);
    }
    let snapshot = backup.join(PRE_LOGIN);
    if !snapshot.is_file() {
        return Ok(false);
    }
    std::fs::copy(&snapshot, &auth)?;
    Ok(true)
}

/// Drop the per-window Electron profile that belongs to a removed account.
///
/// Moved, not deleted, into the same backup so a mis-click is recoverable.
/// A missing directory is not an error: the account may never have been opened.
pub fn remove_user_data_dir(p: &Paths, name: &str, backup: &Path) -> Result<()> {
    let dir = p.user_data_dir(name);
    if !dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(backup)?;
    std::fs::rename(&dir, backup.join(format!("Codex-{name}-userdata")))?;
    Ok(())
}
