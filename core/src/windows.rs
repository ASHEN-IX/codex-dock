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

/// `ESRCH` — "No such process" — is 3 on Linux, macOS and the BSDs. `std` maps
/// it to `ErrorKind::Uncategorized`, so it cannot be matched by kind.
const ESRCH: i32 = 3;

/// Whether a failed read of `proc/<pid>/…` means the process exited.
///
/// The kernel may reap a pid at any point after `read_dir` listed it, so this
/// is routine churn, not a broken scan. Matching only `ErrorKind::NotFound`
/// missed the common case: reading the vanished entry's `cmdline` fails with
/// `ESRCH`, which `std` reports as `Uncategorized`. A pid directory that is
/// itself gone is the same fact, confirmed without trusting an errno value.
///
/// Anything else — an I/O error with the process still present — stays a real
/// failure, so an unreadable process table is never mistaken for "no windows".
pub(crate) fn process_exited(dir: &Path, err: &std::io::Error) -> bool {
    if err.kind() == std::io::ErrorKind::NotFound || err.raw_os_error() == Some(ESRCH) {
        return true;
    }
    matches!(
        std::fs::symlink_metadata(dir),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound
    )
}

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
        if let Ok(file) = crate::platform::fs::open_private(&repair_lock_path) {
            if file.try_lock().is_err() {
                return Err(Error::Msg(
                    "cannot launch desktop: Codex Dock repair is currently in progress".to_string(),
                ));
            }
            let _ = file.unlock();
        }
    }

    let proc_root = crate::platform::process::proc_root().expect("probe capability just checked");
    let launch_lock_path = p.profiles.join(name).join(".desktop-launch.lock");
    let _launch_guard = crate::platform::fs::LockGuard::try_acquire(
        &launch_lock_path,
        &format!("profile '{name}' launch"),
    )?;

    if let Some(existing) = scan_running_for(p, &proc_root)?
        .into_iter()
        .find(|w| w.account == name)
    {
        if focus_pid(existing.pid) {
            return Ok(existing);
        }
        return Err(Error::Msg(format!(
            "Codex for '{name}' is running (pid {}) but its window could not be restored. Quit that Codex process, then open the account again.",
            existing.pid
        )));
    }
    crate::store::sync_projects(p, name)?;
    let l = command_for(p, name);
    let mut cmd = std::process::Command::new(&l.bin);
    cmd.args(&l.args);
    for (k, v) in &l.env {
        cmd.env(k, v);
    }
    cmd.stdin(std::process::Stdio::null());
    // The account desktop must outlive a CLI launcher and must not be a
    // background job of that launcher's terminal.
    detach_from_controlling_terminal(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::Msg(format!("Codex Desktop not found at {}. Put it on PATH or set CODEX_DESKTOP_BIN to its executable path.", l.bin))
        } else {
            Error::Io(e)
        }
    })?;
    // A successful spawn is not proof that a window opened. Keep the launch
    // lock through startup and verify a real window, including forwarded
    // launches where the short-lived launcher exits successfully.
    let result = (|| {
        for _ in 0..100 {
            if let Some(window) = scan_running_for(p, &proc_root)?
                .into_iter()
                .find(|w| w.account == name)
            {
                if focus_pid(window.pid) {
                    return Ok(window);
                }
            }
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    return Err(Error::Msg(format!(
                        "Codex for '{name}' exited before opening a window ({status})"
                    )));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Err(Error::Msg(format!(
        "Codex started for '{name}', but no window could be shown. Check whether Codex Desktop is running in the background."
    )))
    })();
    // Reap the desktop when it eventually exits without tying its lifetime
    // to the short-lived CLI launch request.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    result
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
            // Transient: the process exited between read_dir and read(cmdline).
            Err(e) if process_exited(&entry.path(), &e) => continue,
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
            // Explicit named windows belong to their profile, even when two
            // profiles currently have the same signed-in identity.
            if w.account == "default" {
                let resolved = std::fs::canonicalize(&profile).unwrap_or(profile.clone());
                if let Some(a) = visible.iter().find(|a| a.path == resolved) {
                    w.account = a.name.clone();
                } else {
                    let (id, _) = accounts::auth_info(&profile.join("auth.json"));
                    if let Some(id) = id {
                        if let Some(a) = visible.iter().find(|a| a.account_id.as_ref() == Some(&id))
                        {
                            w.account = a.name.clone();
                        }
                    }
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

/// Leave the caller's terminal session before exec.
///
/// `process_group(0)` is only `setpgid(0, 0)`. On a terminal that puts the
/// desktop in a background process group of the same session, and the kernel
/// stops it with SIGTTIN before the GPU process can start. A new session has
/// no controlling terminal, so the window is neither stopped nor killed when
/// the launcher exits.
fn detach_from_controlling_terminal(cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: `setsid` is async-signal-safe. `pre_exec` runs in the forked
        // child before it execs, and that child is not a process-group leader,
        // so the call creates a new session instead of failing.
        unsafe {
            cmd.pre_exec(|| {
                extern "C" {
                    fn setsid() -> i32;
                }
                if setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    {
        let _ = cmd;
    }
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

fn kwin_window_match_ids(raw: &str) -> Vec<&str> {
    raw.split('"')
        .filter(|id| {
            let Some(uuid) = id.strip_prefix("0_{").and_then(|id| id.strip_suffix('}')) else {
                return false;
            };
            uuid.len() == 36
                && uuid.bytes().enumerate().all(|(i, b)| {
                    if matches!(i, 8 | 13 | 18 | 23) {
                        b == b'-'
                    } else {
                        b.is_ascii_hexdigit()
                    }
                })
        })
        .collect()
}

fn focus_kwin_with(pid: u32, mut query: impl FnMut(&[&str]) -> Option<String>) -> bool {
    let Some(matches) = query(&[
        "--literal",
        "org.kde.KWin",
        "/WindowsRunner",
        "org.kde.krunner1.Match",
        "windows",
    ]) else {
        return false;
    };
    for id in kwin_window_match_ids(&matches) {
        let Some(info) = query(&[
            "org.kde.KWin",
            "/KWin",
            "org.kde.KWin.getWindowInfo",
            &id[2..],
        ]) else {
            continue;
        };
        let owner = info.lines().find_map(|line| {
            line.strip_prefix("pid: ")
                .and_then(|value| value.trim().parse::<u32>().ok())
        });
        if owner == Some(pid)
            && query(&[
                "org.kde.KWin",
                "/WindowsRunner",
                "org.kde.krunner1.Run",
                id,
                "",
            ])
            .is_some()
        {
            return true;
        }
    }
    false
}

/// Window ids owned by `pid`, as reported by `xdotool search --pid`.
fn x11_window_ids(pid: u32, run: &mut impl FnMut(&[&str]) -> Option<String>) -> Vec<String> {
    run(&["search", "--pid", &pid.to_string()])
        .map(|out| {
            out.lines()
                .map(str::trim)
                .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Area of one X11 window, or `None` if the geometry cannot be read.
fn x11_window_area(id: &str, run: &mut impl FnMut(&[&str]) -> Option<String>) -> Option<u64> {
    let out = run(&["getwindowgeometry", "--shell", id])?;
    let field = |name: &str| -> u64 {
        out.lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    };
    Some(field("WIDTH=").saturating_mul(field("HEIGHT=")))
}

/// A window this small is an Electron helper (tray, offscreen, clipboard), not
/// the account's real window. The real one is a full page.
const MIN_ACCOUNT_WINDOW_AREA: u64 = 100_000;

/// Restore and activate a profile window that the compositor never adopted.
///
/// Codex Desktop's Electron window can exist in X11 while staying **unmapped**:
/// `WM_STATE` is absent, so KWin never manages it, it never enters
/// `_NET_CLIENT_LIST`, and no KWin window-runner entry exists to activate. The
/// process is healthy and `xdotool search --pid` still finds the window, so
/// mapping it is what makes the compositor adopt and raise it — verified live
/// against Electron 156 / Codex Desktop 26.1023.
///
/// Mapping is not destructive: an already-mapped window is unaffected, and the
/// process, its credentials and its chats are untouched.
fn focus_x11_with(pid: u32, mut run: impl FnMut(&[&str]) -> Option<String>) -> bool {
    let mut windows: Vec<(String, u64)> = x11_window_ids(pid, &mut run)
        .into_iter()
        .filter_map(|id| x11_window_area(&id, &mut run).map(|area| (id, area)))
        .filter(|(_, area)| *area >= MIN_ACCOUNT_WINDOW_AREA)
        .collect();
    // Raise the largest first: the account's page, not a helper surface.
    windows.sort_by_key(|(_, area)| std::cmp::Reverse(*area));

    let mut restored = false;
    for (id, _) in &windows {
        if run(&["windowmap", id]).is_some() {
            restored = true;
        }
    }
    if !restored {
        return false;
    }
    // Activate after mapping: KWin only manages a window once it is mapped.
    for (id, _) in &windows {
        if run(&["windowactivate", id]).is_some() {
            return true;
        }
    }
    false
}

fn focus_x11_pid(pid: u32) -> bool {
    focus_x11_with(pid, |args| {
        std::process::Command::new("xdotool")
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
    })
}

fn focus_pid(pid: u32) -> bool {
    // KWin's native window runner restores minimized windows and switches to
    // their virtual desktop. Only report success for a verified window PID.
    if focus_kwin_with(pid, |args| {
        std::process::Command::new("qdbus6")
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
    }) {
        return true;
    }

    // A Codex window the compositor never adopted is invisible to every
    // window-manager path above; mapping it through X11 is the only way back.
    if focus_x11_pid(pid) {
        return true;
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

#[cfg(test)]
mod focus_tests {
    use super::*;

    const MATCH: &str = "0_{12345678-1234-1234-1234-123456789abc}";

    #[test]
    fn native_activation_targets_the_window_on_its_desktop() {
        let mut activated = false;
        let focused = focus_kwin_with(42, |args| {
            if args.contains(&"org.kde.krunner1.Match") {
                Some(format!("[Argument: \"{MATCH}\"]"))
            } else if args.contains(&"org.kde.KWin.getWindowInfo") {
                Some("caption: a private window title\npid: 42\n".into())
            } else {
                assert_eq!(
                    args,
                    [
                        "org.kde.KWin",
                        "/WindowsRunner",
                        "org.kde.krunner1.Run",
                        MATCH,
                        ""
                    ]
                );
                activated = true;
                Some(String::new())
            }
        });
        assert!(focused);
        assert!(activated);
    }

    #[test]
    fn a_running_process_without_a_matching_window_is_not_success() {
        let focused = focus_kwin_with(42, |args| {
            if args.contains(&"org.kde.krunner1.Match") {
                Some(format!("\"{MATCH}\""))
            } else if args.contains(&"org.kde.KWin.getWindowInfo") {
                Some("pid: 142\n".into())
            } else {
                panic!("must not activate a different process's window");
            }
        });
        assert!(!focused);
    }

    #[test]
    fn failed_activation_is_not_reported_as_success() {
        assert!(!focus_kwin_with(42, |args| {
            if args.contains(&"org.kde.krunner1.Match") {
                Some(format!("\"{MATCH}\""))
            } else if args.contains(&"org.kde.KWin.getWindowInfo") {
                Some("pid: 42\n".into())
            } else {
                None
            }
        }));
    }

    /// A Codex window can exist in X11 while never being mapped, in which case
    /// no window-manager path can find it. These cover the mapping fallback.
    #[test]
    fn an_unmapped_window_is_mapped_and_activated() {
        let mut mapped: Vec<String> = Vec::new();
        let mut activated: Vec<String> = Vec::new();
        let mut stub = |args: &[&str]| match args {
            ["search", "--pid", _] => Some("29360132".into()),
            ["getwindowgeometry", "--shell", _] => {
                Some("WINDOW=1\nX=0\nY=0\nWIDTH=1280\nHEIGHT=820\n".into())
            }
            ["windowmap", id] => {
                mapped.push((*id).to_string());
                Some(String::new())
            }
            ["windowactivate", id] => {
                activated.push((*id).to_string());
                Some(String::new())
            }
            _ => None,
        };
        assert!(focus_x11_with(42, &mut stub));
        assert_eq!(mapped, ["29360132"], "the real window must be mapped");
        assert_eq!(activated, ["29360132"], "and then activated");
    }

    #[test]
    fn electron_helper_surfaces_are_not_mistaken_for_the_account_window() {
        // The live layout also exposes 10x10 and 200x200 helper windows; mapping
        // one of those would restore nothing a user can see.
        let mut acted: Vec<String> = Vec::new();
        let mut stub = |args: &[&str]| match args {
            ["search", "--pid", _] => Some("100\n200".into()),
            ["getwindowgeometry", "--shell", "100"] => Some("WIDTH=10\nHEIGHT=10\n".into()),
            ["getwindowgeometry", "--shell", "200"] => Some("WIDTH=200\nHEIGHT=200\n".into()),
            _ => {
                acted.push(args.join(" "));
                Some(String::new())
            }
        };
        assert!(!focus_x11_with(42, &mut stub));
        assert!(acted.is_empty(), "nothing may be mapped: {acted:?}");
    }

    #[test]
    fn the_largest_window_is_restored_first() {
        let mut order: Vec<String> = Vec::new();
        let mut stub = |args: &[&str]| match args {
            ["search", "--pid", _] => Some("1\n2".into()),
            ["getwindowgeometry", "--shell", "1"] => Some("WIDTH=1280\nHEIGHT=820\n".into()),
            ["getwindowgeometry", "--shell", "2"] => Some("WIDTH=760\nHEIGHT=640\n".into()),
            ["windowmap", id] => {
                order.push((*id).to_string());
                Some(String::new())
            }
            ["windowactivate", id] => {
                order.push(format!("activate {id}"));
                Some(String::new())
            }
            _ => None,
        };
        assert!(focus_x11_with(42, &mut stub));
        assert_eq!(order.first().map(String::as_str), Some("1"));
    }

    /// `/proc/<pid>/stat` session field. `comm` is wrapped in parentheses and
    /// may contain spaces, so the numeric fields start after the last `)`.
    #[cfg(target_os = "linux")]
    fn session_of(pid: u32) -> u32 {
        let raw = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("stat");
        let rest = raw.rsplit_once(')').expect("comm").1;
        rest.split_whitespace().nth(3).unwrap().parse().unwrap()
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_launched_desktop_is_its_own_session_not_a_background_job() {
        let mut cmd = std::process::Command::new("/bin/sleep");
        cmd.arg("60");
        cmd.stdin(std::process::Stdio::null());
        detach_from_controlling_terminal(&mut cmd);
        let mut child = cmd.spawn().expect("spawn sleep");
        let sid = session_of(child.id());
        let mine = session_of(std::process::id());
        let _ = child.kill();
        let _ = child.wait();
        assert_ne!(
            sid, mine,
            "the desktop must leave the launcher's session; setpgid keeps it a background tty job"
        );
        assert_eq!(sid, child.id(), "setsid makes the child the session leader");
    }

    #[test]
    fn a_process_with_no_windows_is_not_reported_as_restored() {
        let mut stub = |_args: &[&str]| Some(String::new());
        assert!(!focus_x11_with(42, &mut stub));
    }
}
