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
    let name = canonical.as_str();
    let _lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(p.profiles.join(name).join(".desktop-launch.lock"))?;
    _lock.lock()?;
    if let Some(existing) = running_for(p, Path::new("/proc"))
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
        if running_for(p, Path::new("/proc"))
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
    out
}

/// Bring an existing Codex window for `account` to the front instead of
/// spawning a duplicate. KDE Plasma on Wayland exposes window control through
/// KWin's scripting D-Bus interface; on other desktops `wmctrl` is the
/// fallback. Both are best-effort: failing to raise a window must not stop the
/// caller. Launching an already-running account still reuses its process.
/// Resolve legacy/default processes to their current account identity.
pub fn running_for(p: &Paths, proc_root: &Path) -> Vec<WindowProc> {
    let visible = accounts::list(p).unwrap_or_default();
    running(proc_root)
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
                w.user_data_dir = p.home().join(".config/Codex");
            }
            w
        })
        .collect()
}

pub fn focus(account: &str) -> bool {
    let p = Paths::real();
    let name = accounts::canonical_name(&p, account).unwrap_or_else(|_| account.to_string());
    let Some(proc) = running_for(&p, Path::new("/proc"))
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
