//! Linux procfs native process discovery implementation.

use crate::accounts;
use crate::error::{Error, Result};
use crate::paths::Paths;
use std::path::{Path, PathBuf};

use super::{
    attribute_process, is_candidate_codex_name, is_electron_helper, DiscoveredProcess,
    ProcessDiscoveryReport,
};

const ESRCH: i32 = 3;

pub fn process_exited(dir: &Path, err: &std::io::Error) -> bool {
    if err.kind() == std::io::ErrorKind::NotFound || err.raw_os_error() == Some(ESRCH) {
        return true;
    }
    matches!(
        std::fs::symlink_metadata(dir),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound
    )
}

pub fn scan_processes(p: &Paths) -> Result<ProcessDiscoveryReport> {
    scan_processes_at(p, Path::new("/proc"))
}

pub fn scan_processes_at(p: &Paths, proc_root: &Path) -> Result<ProcessDiscoveryReport> {
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

    let managed = accounts::list(p).unwrap_or_default();
    let desktop = crate::tool::desktop();
    let configured_desktop = desktop.file_name().and_then(|s| s.to_str());

    let mut processes = Vec::new();
    let warnings = Vec::new();

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
            Err(e) if process_exited(&entry.path(), &e) => continue,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                let comm_path = entry.path().join("comm");
                let comm = std::fs::read_to_string(&comm_path).unwrap_or_default();
                let comm_trimmed = comm.trim();
                if is_candidate_codex_name(comm_trimmed) || configured_desktop == Some(comm_trimmed)
                {
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

        let mut args: Vec<String> = raw
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .collect();

        // Electron can rewrite /proc/cmdline into a single process-title string:
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

        let is_candidate = is_candidate_codex_name(bin) || configured_desktop == Some(bin);
        if !is_candidate || is_electron_helper(bin, &args) {
            continue;
        }

        let user_data_dir = super::extract_user_data_dir(&args);

        // Inspect environment for CODEX_HOME
        let env_raw = std::fs::read(entry.path().join("environ")).unwrap_or_default();
        let codex_home = env_raw
            .split(|b| *b == 0)
            .find_map(|v| v.strip_prefix(b"CODEX_HOME="))
            .map(|v| PathBuf::from(String::from_utf8_lossy(v).into_owned()));

        let attribution =
            attribute_process(user_data_dir.as_deref(), codex_home.as_deref(), &managed);

        processes.push(DiscoveredProcess {
            pid,
            exe_path: Some(PathBuf::from(exe)),
            attribution,
            user_data_dir,
        });
    }

    processes.sort_by_key(|p| p.pid);
    Ok(ProcessDiscoveryReport {
        processes,
        warnings,
    })
}
