//! Explicit, one-shot usage-window pings. No schedules or automatic retries.
use crate::{accounts, error::{Error, Result}, Paths};
use std::{path::Path, process::{Command, Stdio}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

const PROMPT: &str = "Reply with exactly pong. Do not use tools.";
const COOLDOWN: u64 = 60;

fn command(binary: &Path, profile: &Path, workspace: &Path) -> Command {
    let mut cmd = Command::new(binary);
    cmd.args(["exec", "--ephemeral", "--ignore-user-config", "--ignore-rules",
        "--skip-git-repo-check", "--sandbox", "read-only", "--color", "never",
        "--disable", "shell_tool", "--disable", "hooks", "--disable", "plugins",
        "--disable", "multi_agent", "-c", "web_search=\"disabled\"",
        "-c", "approval_policy=\"never\"", "-c", "model_reasoning_effort=\"low\"",
        "-c", "project_doc_max_bytes=0", PROMPT])
        .env("CODEX_HOME", profile)
        .current_dir(workspace)
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for key in ["OPENAI_API_KEY", "CODEX_API_KEY", "OPENAI_BASE_URL", "OPENAI_API_BASE", "CODEX_API_BASE_URL"] {
        cmd.env_remove(key);
    }
    cmd
}

// Explicitly unlock before closing: another thread may fork while a ping holds
// the lock, briefly keeping the same open file description alive in its child.
struct PingLock(std::fs::File);
impl Drop for PingLock {
    fn drop(&mut self) { let _ = self.0.unlock(); }
}

struct Workspace(std::path::PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

/// Runs only after a user clicks Ping. Uses the account's credentials directly
/// so token refreshes are not lost in a throwaway credential copy.
pub fn send(p: &Paths, name: &str) -> Result<String> {
    send_with(p, name, &crate::tool::codex(), Duration::from_secs(90))
}

fn send_with(p: &Paths, name: &str, binary: &Path, timeout: Duration) -> Result<String> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    // Resolve from the actual account list, never from an unchecked path input.
    let account = accounts::list(p)?.into_iter().find(|a| a.name == name && a.logged_in && a.account_id.is_some())
        .ok_or_else(|| Error::Msg("Ping requires a signed-in ChatGPT account, not an API-key profile.".into()))?;
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).mode(0o600)
        .open(account.path.join(".dock-ping.lock"))?;
    lock.try_lock().map_err(|_| Error::Msg("A ping is already running for this account.".into()))?;
    let _lock = PingLock(lock);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let stamp = account.path.join(".dock-last-ping");
    if let Some(last) = std::fs::read_to_string(&stamp).ok().and_then(|s| s.parse::<u64>().ok()) {
        if now.as_secs().saturating_sub(last) < COOLDOWN {
            return Err(Error::Msg("Wait a minute before sending another ping to this account.".into()));
        }
    }
    let workspace = Workspace(std::env::temp_dir().join(format!("codex-dock-ping-{}-{}", std::process::id(), now.as_nanos())));
    std::fs::DirBuilder::new().mode(0o700).create(&workspace.0)?;
    // Keep failed attempts in the cooldown too: errors must not cause repeat spending.
    std::fs::write(&stamp, now.as_secs().to_string())?;
    let output = workspace.0.join("reply.txt");
    let mut child = command(binary, &account.path, &workspace.0)
        .args(["--output-last-message"]).arg(&output).spawn()
        .map_err(|e| Error::Msg(format!("Could not start Codex for the ping: {e}")))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return if status.success() {
                let reply = std::fs::read_to_string(&output).unwrap_or_default();
                if reply.trim().is_empty() {
                    Err(Error::Msg("Codex exited without a reply. Ping delivery is unconfirmed; refresh usage before retrying.".into()))
                } else {
                    Ok(reply.trim().chars().take(200).collect())
                }
            } else {
                Err(Error::Msg("Ping failed. Check account access, remaining usage, network, and that Codex CLI supports exec --ignore-user-config. No automatic retry was sent.".into()))
            },
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(100)),
            result => {
                let _ = child.kill(); let _ = child.wait();
                return Err(Error::Msg(match result {
                    Err(e) => format!("Could not monitor ping: {e}. The request may have used allowance."),
                    _ => "Ping timed out. It may have used allowance; refresh usage before trying again.".into(),
                }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn ping_uses_isolated_workspace_and_preserves_auth_and_chats() {
        let root = std::env::temp_dir().join(format!("dock-ping-test-{}", std::process::id()));
        let p = Paths::new(root.join("base"), root.join("profiles"));
        std::fs::create_dir_all(&root).unwrap();
        accounts::create(&p, "one", None).unwrap();
        let auth = p.profiles.join("one/auth.json");
        let credentials = r#"{"tokens":{"account_id":"fake-account"}}"#;
        std::fs::write(&auth, credentials).unwrap();
        let binary = root.join("fake-codex");
        std::fs::write(&binary, "#!/bin/sh\n[ \"$CODEX_HOME\" != \"$PWD\" ] || exit 2\n[ ! -f ./auth.json ] || exit 3\nwhile [ $# -gt 0 ]; do\n if [ \"$1\" = --output-last-message ]; then shift; printf pong > \"$1\"; fi\n shift\ndone\nexit 0\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(send_with(&p, "../one", &binary, Duration::from_secs(1)).is_err());
        assert_eq!(send_with(&p, "one", &binary, Duration::from_secs(1)).unwrap(), "pong");
        assert_eq!(std::fs::read_to_string(auth).unwrap(), credentials);
        assert!(!p.profiles.join("one/sessions").exists());
        assert!(send_with(&p, "one", &binary, Duration::from_secs(1)).unwrap_err().to_string().contains("Wait a minute"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn command_is_ephemeral_read_only_and_ignores_user_automation() {
        let cmd = command(Path::new("codex"), Path::new("/profiles/one"), Path::new("/tmp/ping"));
        let args: Vec<_> = cmd.get_args().map(|s| s.to_str().unwrap()).collect();
        for flag in ["--ephemeral", "--ignore-user-config", "--ignore-rules", "read-only", "shell_tool", "hooks", "plugins", "multi_agent"] {
            assert!(args.contains(&flag));
        }
        assert_eq!(args.last(), Some(&PROMPT));
        assert_eq!(cmd.get_current_dir(), Some(Path::new("/tmp/ping")));
        assert!(cmd.get_envs().any(|(k,v)| k == "OPENAI_API_KEY" && v.is_none()));
    }

    #[test]
    fn failed_and_timed_out_requests_are_not_reported_as_success() {
        let root = std::env::temp_dir().join(format!("dock-ping-failure-{}", std::process::id()));
        let p = Paths::new(root.join("base"), root.join("profiles"));
        std::fs::create_dir_all(&root).unwrap();
        let binary = root.join("fake-codex");
        for (name, script, expected) in [
            ("failure", "#!/bin/sh\nexit 1\n", "Ping failed"),
            ("timeout", "#!/bin/sh\nexec sleep 5\n", "timed out"),
            ("empty", "#!/bin/sh\nexit 0\n", "without a reply"),
        ] {
            accounts::create(&p, name, None).unwrap();
            std::fs::write(p.profiles.join(name).join("auth.json"), format!(r#"{{"tokens":{{"account_id":"{name}"}}}}"#)).unwrap();
            std::fs::write(&binary, script).unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
            let error = send_with(&p, name, &binary, Duration::from_millis(150)).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
            assert!(p.profiles.join(name).join(".dock-last-ping").exists());
        }
        accounts::create(&p, "signedout", None).unwrap();
        assert!(send_with(&p, "signedout", &binary, Duration::from_secs(1)).is_err());
        accounts::create(&p, "apikey", None).unwrap();
        std::fs::write(p.profiles.join("apikey/auth.json"), r#"{"OPENAI_API_KEY":"fake-key"}"#).unwrap();
        assert!(send_with(&p, "apikey", &binary, Duration::from_secs(1)).unwrap_err().to_string().contains("not an API-key"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
