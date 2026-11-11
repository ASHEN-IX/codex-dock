//! Windows native process discovery using Toolhelp32 snapshot and QueryFullProcessImageNameW.

use crate::accounts;
use crate::error::{Error, Result};
use crate::paths::Paths;
use std::path::{Path, PathBuf};

use super::{
    attribute_process, is_candidate_codex_name, is_electron_helper, DiscoveredProcess,
    ProcessDiscoveryReport, ProfileAttribution,
};

const TH32CS_SNAPPROCESS: u32 = 0x00000002;
const INVALID_HANDLE_VALUE: isize = -1;

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_INVALID_PARAMETER: u32 = 87;

#[repr(C)]
struct PROCESSENTRY32W {
    dwSize: u32,
    cntUsage: u32,
    th32ProcessID: u32,
    th32DefaultHeapID: usize,
    th32ModuleID: u32,
    cntThreads: u32,
    th32ParentProcessID: u32,
    pcPriClassBase: i32,
    dwFlags: u32,
    szExeFile: [u16; 260],
}

struct AutoHandle(*mut std::ffi::c_void);

impl AutoHandle {
    fn new(h: *mut std::ffi::c_void) -> Option<Self> {
        if h.is_null() || h as isize == INVALID_HANDLE_VALUE {
            None
        } else {
            Some(Self(h))
        }
    }

    fn raw(&self) -> *mut std::ffi::c_void {
        self.0
    }
}

impl Drop for AutoHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 as isize != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

extern "system" {
    fn CreateToolhelp32Snapshot(dwFlags: u32, th32ProcessID: u32) -> *mut std::ffi::c_void;
    fn Process32FirstW(hSnapshot: *mut std::ffi::c_void, lppe: *mut PROCESSENTRY32W) -> i32;
    fn Process32NextW(hSnapshot: *mut std::ffi::c_void, lppe: *mut PROCESSENTRY32W) -> i32;
    fn OpenProcess(
        dwDesiredAccess: u32,
        bInheritHandle: i32,
        dwProcessId: u32,
    ) -> *mut std::ffi::c_void;
    fn QueryFullProcessImageNameW(
        hProcess: *mut std::ffi::c_void,
        dwFlags: u32,
        lpExeName: *mut u16,
        lpdwSize: *mut u32,
    ) -> i32;
    fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    fn GetLastError() -> u32;
    fn GetModuleHandleW(lpModuleName: *const u16) -> *mut std::ffi::c_void;
    fn GetProcAddress(
        hModule: *mut std::ffi::c_void,
        lpProcName: *const u8,
    ) -> *mut std::ffi::c_void;
}

type FnNtQueryInformationProcess = unsafe extern "system" fn(
    process_handle: *mut std::ffi::c_void,
    process_information_class: u32,
    process_information: *mut std::ffi::c_void,
    process_information_length: u32,
    return_length: *mut u32,
) -> i32;

/// Dynamically resolve `NtQueryInformationProcess` from `ntdll.dll`.
fn resolve_nt_query_information_process() -> Option<FnNtQueryInformationProcess> {
    let ntdll_name: [u16; 10] = [
        b'n' as u16,
        b't' as u16,
        b'd' as u16,
        b'l' as u16,
        b'l' as u16,
        b'.' as u16,
        b'd' as u16,
        b'l' as u16,
        b'l' as u16,
        0,
    ];
    let h_ntdll = unsafe { GetModuleHandleW(ntdll_name.as_ptr()) };
    if h_ntdll.is_null() {
        return None;
    }
    let proc_name = b"NtQueryInformationProcess\0";
    let addr = unsafe { GetProcAddress(h_ntdll, proc_name.as_ptr()) };
    if addr.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute(addr) })
    }
}

/// Query command-line string for a process using `ProcessCommandLineInformation` (class 60).
fn query_process_command_line(
    h_process: *mut std::ffi::c_void,
    nt_query: Option<FnNtQueryInformationProcess>,
) -> Option<String> {
    let query_fn = nt_query?;

    // Class 60 is ProcessCommandLineInformation (supported on Windows 8.1 / 10 / 11)
    const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;

    let mut buf = vec![0u8; 4096];
    let mut return_len: u32 = 0;

    let mut status = unsafe {
        query_fn(
            h_process,
            PROCESS_COMMAND_LINE_INFORMATION,
            buf.as_mut_ptr() as *mut std::ffi::c_void,
            buf.len() as u32,
            &mut return_len,
        )
    };

    // If buffer was too small (STATUS_BUFFER_TOO_SMALL / STATUS_INFO_LENGTH_MISMATCH):
    if status != 0 && return_len > buf.len() as u32 {
        buf.resize(return_len as usize, 0);
        status = unsafe {
            query_fn(
                h_process,
                PROCESS_COMMAND_LINE_INFORMATION,
                buf.as_mut_ptr() as *mut std::ffi::c_void,
                buf.len() as u32,
                &mut return_len,
            )
        };
    }

    if status != 0 || return_len < 8 {
        return None;
    }

    // Layout of UNICODE_STRING returned:
    // Length: u16 (bytes)
    // MaximumLength: u16 (bytes)
    // Buffer: *mut u16
    let len_bytes = u16::from_ne_bytes(buf[0..2].try_into().ok()?) as usize;
    let char_count = len_bytes / 2;
    if char_count == 0 {
        return None;
    }

    // Pointer offset:
    // In 64-bit systems, Buffer is at offset 8; in 32-bit, at offset 4.
    // The actual UTF-16 characters follow the UNICODE_STRING header in the buffer.
    let header_size = if std::mem::size_of::<usize>() == 8 {
        16
    } else {
        8
    };
    if buf.len() >= header_size + len_bytes {
        let u16_slice = unsafe {
            std::slice::from_raw_parts(buf[header_size..].as_ptr() as *const u16, char_count)
        };
        return Some(String::from_utf16_lossy(u16_slice));
    }

    None
}

/// Tokenize a Windows command line into arguments respecting double quotes.
fn parse_windows_command_line(cmd: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for ch in cmd.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
            }
            ' ' | '\t' if !in_quotes => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

pub fn scan_processes(p: &Paths) -> Result<ProcessDiscoveryReport> {
    let h_snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let snap_guard = AutoHandle::new(h_snap).ok_or_else(|| {
        let err = unsafe { GetLastError() };
        Error::Msg(format!("CreateToolhelp32Snapshot failed with code {err}"))
    })?;

    let managed = accounts::list(p).unwrap_or_default();
    let desktop = crate::tool::desktop();
    let configured_desktop = desktop.file_name().and_then(|s| s.to_str());
    let nt_query = resolve_nt_query_information_process();

    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut processes = Vec::new();
    let mut warnings = Vec::new();

    let mut ok = unsafe { Process32FirstW(snap_guard.raw(), &mut entry) };
    while ok != 0 {
        let pid = entry.th32ProcessID;
        // Skip System Idle (0) and System (4):
        if pid > 4 {
            let null_pos = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let exe_name = String::from_utf16_lossy(&entry.szExeFile[..null_pos]);

            let is_candidate =
                is_candidate_codex_name(&exe_name) || configured_desktop == Some(exe_name.as_str());

            if is_candidate {
                let h_proc = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };

                if let Some(proc_guard) = AutoHandle::new(h_proc) {
                    let mut path_buf = [0u16; 1024];
                    let mut path_len = path_buf.len() as u32;
                    let img_ok = unsafe {
                        QueryFullProcessImageNameW(
                            proc_guard.raw(),
                            0,
                            path_buf.as_mut_ptr(),
                            &mut path_len,
                        )
                    };

                    let exe_path = if img_ok != 0 && path_len > 0 {
                        Some(PathBuf::from(String::from_utf16_lossy(
                            &path_buf[..path_len as usize],
                        )))
                    } else {
                        None
                    };

                    // Try to inspect command line:
                    let cmd_line = query_process_command_line(proc_guard.raw(), nt_query);
                    let args = cmd_line
                        .as_deref()
                        .map(parse_windows_command_line)
                        .unwrap_or_default();

                    if !is_electron_helper(&exe_name, &args) {
                        let user_data_dir = super::extract_user_data_dir(&args);
                        let attribution =
                            attribute_process(user_data_dir.as_deref(), None, &managed);

                        processes.push(DiscoveredProcess {
                            pid,
                            exe_path,
                            attribution,
                            user_data_dir,
                        });
                    }
                } else {
                    let err = unsafe { GetLastError() };
                    if err == ERROR_ACCESS_DENIED {
                        warnings.push(format!(
                            "access denied opening candidate Codex process pid {pid}"
                        ));
                        processes.push(DiscoveredProcess {
                            pid,
                            exe_path: None,
                            attribution: ProfileAttribution::Unknown,
                            user_data_dir: None,
                        });
                    } else if err != ERROR_INVALID_PARAMETER {
                        // ERROR_INVALID_PARAMETER (87) is process termination during scan
                        warnings.push(format!(
                            "error opening candidate process pid {pid}: code {err}"
                        ));
                    }
                }
            }
        }

        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        ok = unsafe { Process32NextW(snap_guard.raw(), &mut entry) };
    }

    processes.sort_by_key(|p| p.pid);
    Ok(ProcessDiscoveryReport {
        processes,
        warnings,
    })
}
