use crate::error::{Error, Result};
use crate::paths::Paths;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Launcher {
    pub bin: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindowProc {
    pub pid: u32,
    pub account: String,
    pub user_data_dir: PathBuf,
}

const FLAG: &str = "--user-data-dir=";

pub fn command_for(p: &Paths, name: &str) -> Launcher {
    Launcher {
        bin: "/usr/bin/chatgpt".to_string(),
        args: vec![format!("{FLAG}{}", user_data_dir(p, name).display())],
        env: vec![("CODEX_HOME".to_string(), p.profiles.join(name).display().to_string())],
    }
}

pub fn user_data_dir(p: &Paths, name: &str) -> PathBuf {
    p.home().join(".config").join(format!("Codex-{name}"))
}

pub fn launch(p: &Paths, name: &str) -> Result<WindowProc> {
    let l = command_for(p, name);
    let mut cmd = std::process::Command::new(&l.bin);
    cmd.args(&l.args);
    for (k, v) in &l.env {
        cmd.env(k, v);
    }
    let child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::Msg(format!("Codex Desktop not found at {}", l.bin))
        } else {
            Error::Io(e)
        }
    })?;
    Ok(WindowProc {
        pid: child.id(),
        account: name.to_string(),
        user_data_dir: user_data_dir(p, name),
    })
}

pub fn running(proc_root: &Path) -> Vec<WindowProc> {
    let mut out = Vec::new();
    let rd = match std::fs::read_dir(proc_root) {
        Ok(r) => r,
        Err(_) => return out,
    };
    for entry in rd.flatten() {
        let pid: u32 = match entry.file_name().to_string_lossy().parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let raw = match std::fs::read(entry.path().join("cmdline")) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let joined = String::from_utf8_lossy(&raw).replace('\0', " ");
        let Some(pos) = joined.find(FLAG) else { continue };
        let rest = &joined[pos + FLAG.len()..];
        let dir = rest.split_whitespace().next().unwrap_or("");
        if dir.is_empty() {
            continue;
        }
        let user_data_dir = PathBuf::from(dir);
        let account = user_data_dir
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("Codex-"))
            .unwrap_or("")
            .to_string();
        if account.is_empty() {
            continue;
        }
        out.push(WindowProc { pid, account, user_data_dir });
    }
    out.sort_by(|a, b| a.pid.cmp(&b.pid));
    out
}
