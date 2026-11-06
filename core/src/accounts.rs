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
        out.push(Account {
            name,
            path,
            account_id,
            logged_in,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Display one row per signed-in identity. Preserve every profile on disk.
pub fn list(p: &Paths) -> Result<Vec<Account>> {
    let mut all = profiles(p)?;
    // A named profile with private credentials is preferable to a default alias.
    all.sort_by_key(|a| {
        (
            a.name == "default",
            a.path.join("auth.json").is_symlink(),
            a.name.clone(),
        )
    });
    let mut out: Vec<Account> = Vec::new();
    for a in all {
        let duplicate = a
            .account_id
            .as_ref()
            .is_some_and(|id| out.iter().any(|b| b.account_id.as_ref() == Some(id)));
        if !duplicate {
            out.push(a);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn canonical_name(p: &Paths, name: &str) -> Result<String> {
    let all = profiles(p)?;
    let account = all
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| Error::Msg(format!("unknown account '{name}'")))?;
    if let Some(id) = &account.account_id {
        if let Some(canonical) = list(p)?
            .into_iter()
            .find(|a| a.account_id.as_ref() == Some(id))
        {
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

fn validate_profile_name(name: &str, field: &str) -> Result<()> {
    if !valid_name(name) || !safe_segment(name) {
        return Err(Error::Msg(format!(
            "invalid {field} '{name}': use letters, digits, '-' or '_'"
        )));
    }
    Ok(())
}

/// Whether a profile name can be safely joined onto a directory without
/// escaping it. Created profiles already pass [`valid_name`], but names also
/// arrive from UI input, the CLI and disk listings, so the joins in
/// `launch`, `remove` and `remove_user_data_dir` re-check. Unicode and spaces
/// stay valid — only traversal and Windows-only separators are refused.
pub fn safe_segment(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| matches!(c, '/' | '\\' | ':') || c.is_control())
}

#[derive(Debug, Clone, PartialEq)]
pub struct AccountCreation {
    pub account: Account,
    pub shared_store: bool,
}

fn copy_file_secure(from: &Path, to: &Path, canonical_root: &Path) -> Result<()> {
    if from.is_symlink() {
        let resolved = std::fs::canonicalize(from)?;
        if !resolved.starts_with(canonical_root) {
            return Err(Error::Msg(format!(
                "refusing to clone symlink '{}': target '{}' escapes source profile root",
                from.display(),
                resolved.display()
            )));
        }
    }
    std::fs::copy(from, to).map(|_| ()).map_err(Error::Io)
}

fn copy_dir_secure(from: &Path, to: &Path, canonical_root: &Path) -> Result<()> {
    if from.is_symlink() {
        let resolved = std::fs::canonicalize(from)?;
        if !resolved.starts_with(canonical_root) {
            return Err(Error::Msg(format!(
                "refusing to clone directory symlink '{}': target '{}' escapes source profile root",
                from.display(),
                resolved.display()
            )));
        }
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let entry_path = entry.path();
        let target = to.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            let resolved = match std::fs::canonicalize(&entry_path) {
                Ok(r) => r,
                Err(e) => {
                    return Err(Error::Msg(format!(
                        "cannot copy symlink '{}': failed to resolve target: {e}",
                        entry_path.display()
                    )));
                }
            };
            if !resolved.starts_with(canonical_root) {
                return Err(Error::Msg(format!(
                    "refusing to clone symlink '{}': target '{}' escapes source profile root",
                    entry_path.display(),
                    resolved.display()
                )));
            }
        }
        if file_type.is_dir() {
            copy_dir_secure(&entry_path, &target, canonical_root)?;
        } else {
            std::fs::copy(&entry_path, &target)?;
        }
    }
    Ok(())
}

pub fn create_staged(p: &Paths, name: &str, clone_from: Option<&str>) -> Result<(Account, String)> {
    validate_profile_name(name, "account name")?;
    let final_path = p.profiles.join(name);
    if final_path.exists() || final_path.is_symlink() {
        return Err(Error::Msg(format!("account '{name}' already exists")));
    }

    std::fs::create_dir_all(&p.profiles)?;
    let canonical_profiles =
        std::fs::canonicalize(&p.profiles).unwrap_or_else(|_| p.profiles.clone());

    let mut canonical_src_opt = None;
    if let Some(src) = clone_from {
        validate_profile_name(src, "clone source")?;
        let src_path = p.profiles.join(src);
        if !src_path.is_dir() {
            return Err(Error::Msg(format!("clone source '{src}' does not exist")));
        }
        let canonical_src = std::fs::canonicalize(&src_path)?;
        if !canonical_src.starts_with(&canonical_profiles) || canonical_src == canonical_profiles {
            return Err(Error::Msg(format!(
                "clone source '{src}' escapes profile root"
            )));
        }
        canonical_src_opt = Some(canonical_src);
    }

    // Synchronize simultaneous creation attempts using a persistent profile creation lock:
    let create_lock_path = p.profiles.join(format!(".create-{name}.lock"));
    let _create_guard = crate::platform::fs::LockGuard::try_acquire(
        &create_lock_path,
        &format!("simultaneous account creation attempt for '{name}'"),
    )?;

    // Re-check target existence after acquiring creation lock:
    if final_path.exists() || final_path.is_symlink() {
        return Err(Error::Msg(format!("account '{name}' already exists")));
    }

    // Stage in a temporary private hidden directory:
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let token = format!("{}-{}-{}", std::process::id(), nonce, name);
    let staging_dir = p.profiles.join(format!(".staging-{name}-{nonce}"));
    crate::platform::fs::create_private_dir(&staging_dir)?;

    // Write ownership token inside staging directory:
    let token_path = staging_dir.join(".dock-creation-token");
    if let Err(e) = std::fs::write(&token_path, &token) {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(Error::Io(e));
    }

    if let Some(canonical_src) = canonical_src_opt {
        for item in ["config.toml", "rules", "skills"] {
            let from = canonical_src.join(item);
            if !from.exists() && !from.is_symlink() {
                continue;
            }
            let to = staging_dir.join(item);
            let copy_res = if from.is_dir() {
                copy_dir_secure(&from, &to, &canonical_src)
            } else {
                copy_file_secure(&from, &to, &canonical_src)
            };
            if let Err(e) = copy_res {
                let _ = std::fs::remove_dir_all(&staging_dir);
                return Err(e);
            }
        }
    }

    // Atomic publication from staging_dir to final_path:
    // Platform-safe atomic no-replace publication that cannot overwrite an existing directory
    if let Err(e) = crate::platform::fs::atomic_rename_no_replace(&staging_dir, &final_path) {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(e);
    }

    let (account_id, logged_in) = auth_info(&final_path.join("auth.json"));
    Ok((
        Account {
            name: name.to_string(),
            path: final_path,
            account_id,
            logged_in,
        },
        token,
    ))
}

pub fn create(p: &Paths, name: &str, clone_from: Option<&str>) -> Result<Account> {
    let (account, _) = create_staged(p, name, clone_from)?;
    let _ = std::fs::remove_file(account.path.join(".dock-creation-token"));
    Ok(account)
}

/// Transactional account creation workflow.
///
/// Preflights inputs and platform compatibility before filesystem modification.
/// On platforms with validated shared-store support (Linux), links the profile
/// to the shared store with automatic rollback on linking failure.
/// On unverified platforms (Windows, macOS), uses isolation-only behavior,
/// never invoking unsupported shared-store operations.
pub fn add_account(p: &Paths, name: &str, clone_from: Option<&str>) -> Result<AccountCreation> {
    validate_profile_name(name, "account name")?;
    if let Some(src) = clone_from {
        validate_profile_name(src, "clone source")?;
    }

    let should_link =
        crate::platform::capabilities().shared_store_compatibility == crate::platform::Support::Yes;

    let (account, token) = create_staged(p, name, clone_from)?;

    if should_link {
        if let Err(e) = crate::store::link_profile(p, name) {
            // Ownership-safe rollback:
            // Never delete or overwrite pre-existing profile data during rollback.
            // Do not delete a target directory if ownership cannot be established.
            let token_path = account.path.join(".dock-creation-token");
            let disk_token = std::fs::read_to_string(&token_path).unwrap_or_default();
            if disk_token == token {
                let _ = std::fs::remove_dir_all(&account.path);
                return Err(Error::Msg(format!(
                    "could not link shared store for '{name}': {e}"
                )));
            } else {
                return Err(Error::Msg(format!(
                    "could not link shared store for '{name}': {e}; rollback aborted: ownership of '{}' could not be established",
                    account.path.display()
                )));
            }
        }
    }

    // Clean up ownership token upon completed successful creation:
    let _ = std::fs::remove_file(account.path.join(".dock-creation-token"));

    Ok(AccountCreation {
        account,
        shared_store: should_link,
    })
}

pub fn remove(p: &Paths, name: &str, backup: &Path) -> Result<PathBuf> {
    remove_with_proc_root(
        p,
        name,
        backup,
        crate::platform::process::proc_root().as_deref(),
    )
}

pub fn remove_with_proc_root(
    p: &Paths,
    name: &str,
    backup: &Path,
    proc_root: Option<&Path>,
) -> Result<PathBuf> {
    if !safe_segment(name) || name.starts_with('.') {
        return Err(Error::Msg(format!("invalid account name '{name}'")));
    }
    let from = p.profiles.join(name);
    if !from.exists() {
        return Err(Error::Msg(format!("account '{name}' does not exist")));
    }

    if let (Ok(canon_from), Ok(canon_profiles)) = (
        std::fs::canonicalize(&from),
        std::fs::canonicalize(&p.profiles),
    ) {
        if !canon_from.starts_with(&canon_profiles) || canon_from == canon_profiles {
            return Err(Error::Msg(format!("account '{name}' escapes profile root")));
        }
    }

    // Synchronize with creation: ensure no creation is in progress
    let create_lock_path = p.profiles.join(format!(".create-{name}.lock"));
    let _create_guard = crate::platform::fs::LockGuard::try_acquire(
        &create_lock_path,
        &format!("cannot remove: account creation for '{name}' is currently in progress"),
    )?;

    // Synchronize with launch: ensure no launch is in progress
    let launch_lock_path = from.join(".desktop-launch.lock");
    let launch_guard = crate::platform::fs::LockGuard::try_acquire(
        &launch_lock_path,
        &format!("cannot remove: desktop launch for profile '{name}' is currently in progress"),
    )?;

    // Refuse removal when process inspection is unsupported, incomplete, or unreliable
    let proc_root = match proc_root {
        Some(pr) => pr,
        None => {
            return Err(Error::Msg(format!(
                "cannot remove '{name}': native process discovery is unsupported or unavailable on this platform ({}); active Codex windows cannot be safely excluded",
                std::env::consts::OS
            )));
        }
    };

    if !proc_root.is_dir() {
        return Err(Error::Msg(format!(
            "cannot remove '{name}': process discovery root '{}' is missing or unreadable; active Codex windows cannot be safely excluded",
            proc_root.display()
        )));
    }

    let running = crate::windows::scan_running_for(p, proc_root).map_err(|e| {
        Error::Msg(format!(
            "cannot remove '{name}': process inspection failed: {e}"
        ))
    })?;

    let config_root = crate::platform::fs::config_dir_at(p.home());
    let active = running.into_iter().any(|w| {
        if w.account != name {
            return false;
        }
        if !w.user_data_dir.as_os_str().is_empty() && w.user_data_dir.starts_with(&config_root) {
            return true;
        }
        let env =
            std::fs::read(proc_root.join(w.pid.to_string()).join("environ")).unwrap_or_default();
        env.split(|b| *b == 0)
            .find_map(|v| v.strip_prefix(b"CODEX_HOME="))
            .map(|v| PathBuf::from(String::from_utf8_lossy(v).into_owned()))
            .map(|h| h.starts_with(&p.base) || h.starts_with(&p.profiles))
            .unwrap_or(false)
    });
    if active {
        return Err(Error::Msg(format!(
            "cannot remove '{name}': active Codex window is running for this account; close it first"
        )));
    }

    // Destination collision checks before mutating filesystem
    std::fs::create_dir_all(backup)?;
    let to_profile = backup.join(name);
    if to_profile.exists() || std::fs::symlink_metadata(&to_profile).is_ok() {
        return Err(Error::Msg(format!(
            "cannot remove '{name}': backup destination '{}' already exists",
            to_profile.display()
        )));
    }
    let user_data = p.user_data_dir(name);
    let to_user_data = backup.join(format!("Codex-{name}-userdata"));
    if user_data.exists()
        && (to_user_data.exists() || std::fs::symlink_metadata(&to_user_data).is_ok())
    {
        return Err(Error::Msg(format!(
            "cannot remove '{name}': backup destination '{}' already exists",
            to_user_data.display()
        )));
    }

    // Release launch lock guard before moving the profile directory;
    // on Windows NTFS, a directory containing an open file handle cannot be renamed.
    drop(launch_guard);

    // Atomic move of the profile directory to backup (no-replace)
    crate::platform::fs::atomic_rename_no_replace(&from, &to_profile)?;

    // Atomic move of user data directory (if present) to backup (no-replace)
    if user_data.exists() {
        if let Err(e) = crate::platform::fs::atomic_rename_no_replace(&user_data, &to_user_data) {
            return Err(Error::PartialRemoval {
                profile_backup: to_profile,
                message: format!(
                    "could not move user data '{}' to '{}': {e}",
                    user_data.display(),
                    to_user_data.display()
                ),
            });
        }
    }

    Ok(to_profile)
}

/// Validate an account target for login operations.
///
/// Ensures the account name is well-formed, resolves against the actual managed
/// profile list, rejects path traversal and symlink escapes, and verifies that
/// login can only modify the intended profile directory.
pub fn validate_login_target(p: &Paths, name: &str) -> Result<PathBuf> {
    if !safe_segment(name) || name.starts_with('.') {
        return Err(Error::Msg(format!("invalid account name '{name}'")));
    }

    let managed = profiles(p)?;
    if !managed.iter().any(|a| a.name == name) {
        return Err(Error::Msg(format!(
            "account '{name}' is not a managed profile"
        )));
    }

    let target = p.profiles.join(name);
    if !target.is_dir() {
        return Err(Error::Msg(format!("account '{name}' does not exist")));
    }

    let canonical_profiles = std::fs::canonicalize(&p.profiles).map_err(|e| {
        Error::Msg(format!(
            "failed to canonicalize profiles root '{}': {e}",
            p.profiles.display()
        ))
    })?;
    let canonical_target = std::fs::canonicalize(&target).map_err(|e| {
        Error::Msg(format!(
            "failed to canonicalize account directory '{}': {e}",
            target.display()
        ))
    })?;

    if !canonical_target.starts_with(&canonical_profiles) || canonical_target == canonical_profiles
    {
        return Err(Error::Msg(format!(
            "account '{name}' escapes managed profiles directory"
        )));
    }

    Ok(target)
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
    if !safe_segment(name) || name.starts_with('.') {
        return Err(Error::Msg(format!("invalid account name '{name}'")));
    }
    let dir = p.user_data_dir(name);
    if !dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(backup)?;
    let to = backup.join(format!("Codex-{name}-userdata"));
    if to.exists() || std::fs::symlink_metadata(&to).is_ok() {
        return Err(Error::Msg(format!(
            "refusing to move user data: backup destination '{}' already exists",
            to.display()
        )));
    }
    crate::platform::fs::atomic_rename_no_replace(&dir, &to)
}
