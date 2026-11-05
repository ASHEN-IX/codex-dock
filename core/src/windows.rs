use crate::accounts;
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
        bin: crate::tool::desktop().to_string_lossy().into_owned(),
        args: vec![format!("{FLAG}{}", user_data_dir(p, name).display())],
        env: vec![(
            "CODEX_HOME".to_string(),
            p.profiles.join(name).display().to_string(),
        )],
    }
}

pub fn user_data_dir(p: &Paths, name: &str) -> PathBuf {
    p.user_data_dir(name)
}

pub fn launch(p: &Paths, name: &str) -> Result<WindowProc> {
    let canonical = accounts::canonical_name(p, name)?;
    if !accounts::safe_segment(&canonical) {
        return Err(Error::Msg(format!("invalid account name '{canonical}'")));
    }
    let name = canonical.as_str();
    // Safe desktop launching and native process discovery are both required.
    // Without a process table we can neither find an existing window nor wait for it.
    // Without safe desktop launching we cannot guarantee clean execution.
    crate::platform::require(
        crate::platform::capabilities().safe_desktop_launch,
        "desktop launch",
    )?;
    crate::platform::require(
        crate::platform::capabilities().native_process_discovery,
        "desktop launch",
    )?;

    // 1. Check if a Codex Dock repair is currently active; refuse launch to prevent conflicting writes
    let repair_lock_path = p.base.join(".dock-repair.lock");
    if repair_lock_path.is_file() {
        if let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&repair_lock_path)
        {
            if file.try_lock().is_err() {
                return Err(Error::Msg(
                    "cannot launch desktop: Codex Dock repair is currently in progress".to_string(),
                ));
            }
            let _ = file.unlock();
        }
    }

    let proc_root = crate::platform::process::proc_root().expect("probe capability just checked");
    let _lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(p.profiles.join(name).join(".desktop-launch.lock"))?;
    _lock.try_lock().map_err(|_| {
        Error::Msg(format!(
            "cannot launch desktop: profile '{name}' is currently locked by a repair or launch operation"
        ))
    })?;

    if let Some(existing) = scan_running_for(p, &proc_root)?
        .into_iter()
        .find(|w| w.account == name)
    {
        focus_pid(existing.pid);
        return Ok(existing);
    }
    crate::store::sync_projects(p, name)?;
    let l = command_for(p, name);
    let mut cmd = std::process::Command::new(&l.bin);
    cmd.args(&l.args);
    for (k, v) in &l.env {
        cmd.env(k, v);
    }
    let child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::Msg(format!("Codex Desktop not found at {}. Put it on PATH or set CODEX_DESKTOP_BIN to its executable path.", l.bin))
        } else {
            Error::Io(e)
        }
    })?;
    // Keep the launch lock until the shell wrapper has exec'd Electron, so a
    // concurrent click can discover the process before deciding to launch.
    for _ in 0..50 {
        if scan_running_for(p, &proc_root)
            .unwrap_or_default()
            .iter()
            .any(|w| w.account == name)
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Ok(WindowProc {
        pid: child.id(),
        account: name.to_string(),
        user_data_dir: user_data_dir(p, name),
    })
}

/// Fail-closed native process discovery scanning the given `proc_root`.
/// Returns an error if the process table cannot be read or verified.
pub fn scan_running(proc_root: &Path) -> Result<Vec<WindowProc>> {
    if !crate::platform::process::supported() {
        return Err(Error::Msg(
            "native process discovery is not supported on this platform".to_string(),
        ));
    }
    if !proc_root.exists() {
        return Err(Error::Msg(format!(
            "process discovery root '{}' does not exist",
            proc_root.display()
        )));
    }
    if !proc_root.is_dir() {
        return Err(Error::Msg(format!(
            "process discovery root '{}' is not a directory",
            proc_root.display()
        )));
    }
    let rd = std::fs::read_dir(proc_root).map_err(|e| {
        Error::Msg(format!(
            "failed to read process discovery root '{}': {e}",
            proc_root.display()
        ))
    })?;

    let mut out = Vec::new();
    for entry in rd {
        let entry = entry.map_err(|e| {
            Error::Msg(format!(
                "failed to enumerate process directory entry in '{}': {e}",
                proc_root.display()
            ))
        })?;
        let pid: u32 = match entry.file_name().to_string_lossy().parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let cmdline_path = entry.path().join("cmdline");
        let raw = match std::fs::read(&cmdline_path) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Transient process exited between read_dir and read(cmdline).
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                // Check if this unreadable process could be a Codex process candidate.
                let comm_path = entry.path().join("comm");
                let comm = std::fs::read_to_string(&comm_path).unwrap_or_default();
                let comm_trimmed = comm.trim();
                if matches!(comm_trimmed, "chatgpt" | "ChatGPT" | "codex-desktop") {
                    return Err(Error::Msg(format!(
                        "permission denied inspecting candidate Codex process pid {pid}: {e}"
                    )));
                }
                continue;
            }
            Err(e) => {
                return Err(Error::Msg(format!(
                    "failed to read command line for pid {pid} at '{}': {e}",
                    cmdline_path.display()
                )));
            }
        };
        let mut args: Vec<_> = raw
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect();
        // Electron can rewrite /proc/cmdline into one process-title string.
        // Split flag boundaries, not whitespace: profile paths may contain spaces.
        if args.len() == 1 {
            if let Some((exe, flags)) = args[0].split_once(" --") {
                let mut expanded = vec![exe.to_owned()];
                expanded.extend(flags.split(" --").map(|flag| format!("--{flag}")));
                args = expanded;
            }
        }
        let Some(exe) = args.first() else { continue };
        let bin = Path::new(exe)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let desktop = crate::tool::desktop();
        let configured = desktop.file_name().and_then(|s| s.to_str());
        if !(matches!(bin, "chatgpt" | "ChatGPT" | "codex-desktop") || configured == Some(bin))
            || args.iter().any(|s| s.starts_with("--type="))
        {
            continue;
        }
        let dir = args
            .iter()
            .find_map(|s| s.strip_prefix(FLAG).map(str::to_owned))
            .or_else(|| {
                args.windows(2)
                    .find(|a| a[0] == "--user-data-dir")
                    .map(|a| a[1].clone())
            });
        let user_data_dir = dir.map(PathBuf::from).unwrap_or_default();
        let account = user_data_dir
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_prefix("Codex-"))
            .unwrap_or("default")
            .to_string();
        out.push(WindowProc {
            pid,
            account,
            user_data_dir,
        });
    }
    out.sort_by_key(|a| a.pid);
    Ok(out)
}

pub fn running(proc_root: &Path) -> Vec<WindowProc> {
    scan_running(proc_root).unwrap_or_default()
}

pub fn scan_running_for(p: &Paths, proc_root: &Path) -> Result<Vec<WindowProc>> {
    let visible = accounts::list(p).unwrap_or_default();
    let procs = scan_running(proc_root)?;
    Ok(procs
        .into_iter()
        .map(|mut w| {
            let env = std::fs::read(proc_root.join(w.pid.to_string()).join("environ"))
                .unwrap_or_default();
            let home = env
                .split(|b| *b == 0)
                .find_map(|v| v.strip_prefix(b"CODEX_HOME="))
                .map(|v| PathBuf::from(String::from_utf8_lossy(v).into_owned()));
            let profile = home.unwrap_or_else(|| {
                if w.account == "default" {
                    p.base.clone()
                } else {
                    p.profiles.join(&w.account)
                }
            });
            let (id, _) = accounts::auth_info(&profile.join("auth.json"));
            if let Some(id) = id {
                if let Some(a) = visible.iter().find(|a| a.account_id.as_ref() == Some(&id)) {
                    w.account = a.name.clone();
                }
            }
            if w.user_data_dir.as_os_str().is_empty() {
                w.user_data_dir = crate::platform::fs::config_dir_at(p.home()).join("Codex");
            }
            w
        })
        .collect())
}

pub fn running_for(p: &Paths, proc_root: &Path) -> Vec<WindowProc> {
    scan_running_for(p, proc_root).unwrap_or_default()
}

pub fn focus(account: &str) -> bool {
    if !crate::platform::process::supported() {
        return false;
    }
    let p = Paths::real();
    let name = accounts::canonical_name(&p, account).unwrap_or_else(|_| account.to_string());
    let Some(proc_root) = crate::platform::process::proc_root() else {
        return false;
    };
    let Some(proc) = running_for(&p, &proc_root)
        .into_iter()
        .find(|w| w.account == name)
    else {
        return false;
    };
    focus_pid(proc.pid)
}

fn focus_pid(pid: u32) -> bool {
    // KWin scripting: one-shot script that activates the window for this pid.
    let script = format!(
        "for (const c of workspace.windowList()) {{ \
           if (c.pid === {pid} && c.normalWindow) {{ c.minimized = false; workspace.activeWindow = c; break; }} }}"
    );
    if let Ok(path) = write_temp_script(&script) {
        let path = path.to_string_lossy().to_string();
        let loaded = std::process::Command::new("qdbus6")
            .args([
                "org.kde.KWin",
                "/Scripting",
                "org.kde.kwin.Scripting.loadScript",
                &path,
            ])
            .output();
        let id = loaded
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<i32>()
                    .ok()
            })
            .filter(|id| *id >= 0);
        if let Some(id) = id {
            let ran = std::process::Command::new("qdbus6")
                .args([
                    "org.kde.KWin",
                    &format!("/Scripting/Script{id}"),
                    "org.kde.kwin.Script.run",
                ])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            let _ = std::process::Command::new("qdbus6")
                .args([
                    "org.kde.KWin",
                    "/Scripting",
                    "org.kde.kwin.Scripting.unloadScript",
                    &path,
                ])
                .output();
            let _ = std::fs::remove_file(&path);
            if ran {
                return true;
            }
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }

    // wmctrl -lp supplies the X11 window ID and owning process separately.
    if let Ok(output) = std::process::Command::new("wmctrl").arg("-lp").output() {
        for row in String::from_utf8_lossy(&output.stdout).lines() {
            let fields: Vec<_> = row.split_whitespace().collect();
            if fields.len() >= 3 && fields[2].parse::<u32>().ok() == Some(pid) {
                return std::process::Command::new("wmctrl")
                    .args(["-i", "-a", fields[0]])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);
            }
        }
    }
    false
}

fn write_temp_script(script: &str) -> Result<std::path::PathBuf> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "codex-dock-focus-{}-{nonce}.js",
        std::process::id()
    ));
    std::fs::write(&path, script)?;
    Ok(path)
}
