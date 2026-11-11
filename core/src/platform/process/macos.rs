//! macOS native process discovery using libproc and sysctl(KERN_PROCARGS2).

use crate::accounts;
use crate::error::{Error, Result};
use crate::paths::Paths;
use std::path::{Path, PathBuf};

use super::{
    attribute_process, classify_process, is_candidate_codex_name, DiscoveredProcess,
    ProcessDiscoveryReport, ProcessIdentity, ProfileAttribution,
};

const PROC_ALL_PIDS: u32 = 1;
const PROC_PIDPATHINFO_MAXSIZE: usize = 4096;

const CTL_KERN: i32 = 1;
const KERN_PROCARGS2: i32 = 49;

const ESRCH: i32 = 3;
const EPERM: i32 = 1;
const EACCES: i32 = 13;

extern "C" {
    fn proc_listpids(
        proc_type: u32,
        type_info: u32,
        buffer: *mut std::ffi::c_void,
        buffersize: i32,
    ) -> i32;

    fn proc_pidpath(pid: i32, buffer: *mut std::ffi::c_void, buffersize: u32) -> i32;

    fn proc_name(pid: i32, buffer: *mut std::ffi::c_void, buffersize: u32) -> i32;

    fn sysctl(
        name: *const i32,
        namelen: u32,
        oldp: *mut std::ffi::c_void,
        oldlenp: *mut usize,
        newp: *const std::ffi::c_void,
        newlen: usize,
    ) -> i32;
}

/// Retrieve all active PIDs on macOS via `proc_listpids`.
fn list_all_pids() -> Result<Vec<i32>> {
    let bytes_needed = unsafe { proc_listpids(PROC_ALL_PIDS, 0, std::ptr::null_mut(), 0) };
    if bytes_needed <= 0 {
        return Err(Error::Msg(format!(
            "failed to query process table size via proc_listpids: {}",
            std::io::Error::last_os_error()
        )));
    }

    let count = (bytes_needed as usize) / std::mem::size_of::<i32>();
    let mut pids: Vec<i32> = vec![0; count + 64];
    let buffer_size = (pids.len() * std::mem::size_of::<i32>()) as i32;

    let actual_bytes = unsafe {
        proc_listpids(
            PROC_ALL_PIDS,
            0,
            pids.as_mut_ptr() as *mut std::ffi::c_void,
            buffer_size,
        )
    };
    if actual_bytes <= 0 {
        return Err(Error::Msg(format!(
            "failed to enumerate process table via proc_listpids: {}",
            std::io::Error::last_os_error()
        )));
    }

    let actual_count = (actual_bytes as usize) / std::mem::size_of::<i32>();
    pids.truncate(actual_count);
    pids.retain(|&pid| pid > 0);
    Ok(pids)
}

/// Query short process name via `proc_name`.
fn query_proc_name(pid: i32) -> Option<String> {
    let mut buf = [0u8; 1024];
    let ret = unsafe {
        proc_name(
            pid,
            buf.as_mut_ptr() as *mut std::ffi::c_void,
            buf.len() as u32,
        )
    };
    if ret <= 0 {
        return None;
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(ret as usize);
    Some(String::from_utf8_lossy(&buf[..len]).into_owned())
}

/// Query full executable path for a PID via `proc_pidpath`.
fn query_pid_path(pid: i32) -> std::result::Result<Option<PathBuf>, std::io::Error> {
    let mut buf = [0u8; PROC_PIDPATHINFO_MAXSIZE];
    let ret = unsafe {
        proc_pidpath(
            pid,
            buf.as_mut_ptr() as *mut std::ffi::c_void,
            PROC_PIDPATHINFO_MAXSIZE as u32,
        )
    };

    if ret <= 0 {
        let err = std::io::Error::last_os_error();
        let raw = err.raw_os_error();
        if raw == Some(ESRCH) {
            // Process exited mid-scan: routine churn
            return Ok(None);
        }
        return Err(err);
    }

    let len = buf.iter().position(|&b| b == 0).unwrap_or(ret as usize);
    let s = String::from_utf8_lossy(&buf[..len]).into_owned();
    Ok(Some(PathBuf::from(s)))
}

/// Query command line arguments and environment variables via `sysctl(KERN_PROCARGS2)`.
fn query_proc_args(pid: i32) -> std::result::Result<(Vec<String>, Vec<String>), std::io::Error> {
    let mib = [CTL_KERN, KERN_PROCARGS2, pid];
    let mut size: usize = 0;

    let ret = unsafe {
        sysctl(
            mib.as_ptr(),
            3,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null(),
            0,
        )
    };

    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if size <= 4 {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut data = vec![0u8; size];
    let ret = unsafe {
        sysctl(
            mib.as_ptr(),
            3,
            data.as_mut_ptr() as *mut std::ffi::c_void,
            &mut size,
            std::ptr::null(),
            0,
        )
    };

    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }

    let argc = i32::from_ne_bytes(data[0..4].try_into().unwrap_or([0, 0, 0, 0]));
    if argc <= 0 {
        return Ok((Vec::new(), Vec::new()));
    }

    // Skip executable path string (null-terminated):
    let Some(exe_null_offset) = data[4..].iter().position(|&b| b == 0) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut cursor = 4 + exe_null_offset + 1;

    // Skip null padding between exe path and argv[0]:
    while cursor < data.len() && data[cursor] == 0 {
        cursor += 1;
    }

    // Read argc null-terminated argv strings:
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        if cursor >= data.len() {
            break;
        }
        let len = data[cursor..]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(data.len() - cursor);
        let arg = String::from_utf8_lossy(&data[cursor..cursor + len]).into_owned();
        args.push(arg);
        cursor += len + 1;
    }

    // Read remaining null-terminated environment strings:
    let mut envs = Vec::new();
    while cursor < data.len() {
        let len = data[cursor..]
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(data.len() - cursor);
        if len == 0 {
            break;
        }
        let env_str = String::from_utf8_lossy(&data[cursor..cursor + len]).into_owned();
        envs.push(env_str);
        cursor += len + 1;
    }

    Ok((args, envs))
}

/// Check if a path on macOS indicates a Codex application bundle.
fn is_codex_app_bundle(path: &Path) -> bool {
    let lossy = path.to_string_lossy();
    lossy.contains("/Codex.app/Contents/MacOS/")
        || lossy.contains("/ChatGPT.app/Contents/MacOS/")
        || lossy.contains("/Codex Desktop.app/Contents/MacOS/")
        || lossy.contains("codex-desktop")
}

pub fn scan_processes(p: &Paths) -> Result<ProcessDiscoveryReport> {
    let pids = list_all_pids()?;
    let managed = accounts::list(p).unwrap_or_default();
    let desktop = crate::tool::desktop();
    let configured_desktop = desktop.file_name().and_then(|s| s.to_str());

    let mut processes = Vec::new();
    let mut warnings = Vec::new();
    let mut is_complete = true;

    for pid in pids {
        let exe_path = match query_pid_path(pid) {
            Ok(Some(path)) => path,
            Ok(None) => continue, // exited mid-scan: ordinary churn
            Err(e) if e.raw_os_error() == Some(EPERM) || e.raw_os_error() == Some(EACCES) => {
                // If permission is denied querying path, check proc_name for candidate identity:
                if let Some(short_name) = query_proc_name(pid) {
                    if is_candidate_codex_name(&short_name)
                        || configured_desktop == Some(short_name.as_str())
                    {
                        warnings.push(format!(
                            "permission denied reading path for candidate process pid {pid}"
                        ));
                        is_complete = false;
                        processes.push(DiscoveredProcess {
                            pid: pid as u32,
                            exe_path: None,
                            identity: ProcessIdentity::CandidateDesktop,
                            attribution: ProfileAttribution::Unknown,
                            user_data_dir: None,
                        });
                    }
                }
                continue;
            }
            Err(_) => continue,
        };

        let file_name = exe_path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        let is_candidate = is_codex_app_bundle(&exe_path)
            || is_candidate_codex_name(file_name)
            || configured_desktop == Some(file_name);

        if !is_candidate {
            continue;
        }

        // Try reading arguments & environment:
        let (args, envs) = match query_proc_args(pid) {
            Ok(pair) => pair,
            Err(e) if e.raw_os_error() == Some(ESRCH) => {
                // Process terminated during scan: routine churn
                continue;
            }
            Err(e) if e.raw_os_error() == Some(EPERM) || e.raw_os_error() == Some(EACCES) => {
                // Candidate Codex process exists, but permission denied inspecting command line
                warnings.push(format!(
                    "permission denied inspecting command-line arguments for candidate Codex process pid {pid}"
                ));
                is_complete = false;
                processes.push(DiscoveredProcess {
                    pid: pid as u32,
                    exe_path: Some(exe_path),
                    identity: ProcessIdentity::CandidateDesktop,
                    attribution: ProfileAttribution::Unknown,
                    user_data_dir: None,
                });
                continue;
            }
            Err(e) => {
                warnings.push(format!(
                    "error reading arguments for candidate Codex process pid {pid}: {e}"
                ));
                is_complete = false;
                processes.push(DiscoveredProcess {
                    pid: pid as u32,
                    exe_path: Some(exe_path),
                    identity: ProcessIdentity::CandidateDesktop,
                    attribution: ProfileAttribution::Unknown,
                    user_data_dir: None,
                });
                continue;
            }
        };

        let identity = classify_process(Some(&exe_path), file_name, &args, configured_desktop);
        if identity == ProcessIdentity::ElectronHelper
            || identity == ProcessIdentity::Cli
            || identity == ProcessIdentity::NonCodex
        {
            continue;
        }

        let user_data_dir = super::extract_user_data_dir(&args);
        let codex_home = envs
            .iter()
            .find_map(|s| s.strip_prefix("CODEX_HOME="))
            .map(PathBuf::from);

        let attribution =
            attribute_process(p, user_data_dir.as_deref(), codex_home.as_deref(), &managed);

        processes.push(DiscoveredProcess {
            pid: pid as u32,
            exe_path: Some(exe_path),
            identity,
            attribution,
            user_data_dir,
        });
    }

    processes.sort_by_key(|p| p.pid);
    Ok(ProcessDiscoveryReport {
        processes,
        warnings,
        is_complete,
    })
}
