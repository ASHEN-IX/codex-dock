//! Windows native process discovery using Toolhelp32 snapshot, QueryFullProcessImageNameW,
//! NtQueryInformationProcess(ProcessCommandLineInformation), and CommandLineToArgvW.

#![allow(non_snake_case, non_camel_case_types)]

use crate::accounts;
use crate::error::{Error, Result};
use crate::paths::Paths;
use std::path::PathBuf;

use super::{
    attribute_process, classify_process, is_candidate_codex_name, parse_unicode_string_buffer,
    parse_windows_command_line_msdn, DiscoveredProcess, ProcessDiscoveryReport, ProcessIdentity,
    ProfileAttribution,
};

const TH32CS_SNAPPROCESS: u32 = 0x00000002;
const INVALID_HANDLE_VALUE: isize = -1;

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_NO_MORE_FILES: u32 = 18;
const ERROR_INVALID_PARAMETER: u32 = 87;
const ERROR_NOT_FOUND: u32 = 1168;

#[repr(C)]
#[derive(Clone)]
pub(crate) struct PROCESSENTRY32W {
    pub(crate) dwSize: u32,
    pub(crate) cntUsage: u32,
    pub(crate) th32ProcessID: u32,
    pub(crate) th32DefaultHeapID: usize,
    pub(crate) th32ModuleID: u32,
    pub(crate) cntThreads: u32,
    pub(crate) th32ParentProcessID: u32,
    pub(crate) pcPriClassBase: i32,
    pub(crate) dwFlags: u32,
    pub(crate) szExeFile: [u16; 260],
}

#[cfg(test)]
impl PROCESSENTRY32W {
    pub(crate) fn new_mock(pid: u32, name: &str) -> Self {
        let mut entry: Self = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<Self>() as u32;
        entry.th32ProcessID = pid;
        let wide: Vec<u16> = name.encode_utf16().collect();
        let copy_len = wide.len().min(entry.szExeFile.len() - 1);
        entry.szExeFile[..copy_len].copy_from_slice(&wide[..copy_len]);
        entry
    }
}

pub(crate) trait ProcessSnapshotEnumerator {
    fn first(&mut self, entry: &mut PROCESSENTRY32W) -> Result<bool, u32>;
    fn next(&mut self, entry: &mut PROCESSENTRY32W) -> Result<bool, u32>;
}

struct LiveSnapshotEnumerator(*mut std::ffi::c_void);

impl ProcessSnapshotEnumerator for LiveSnapshotEnumerator {
    fn first(&mut self, entry: &mut PROCESSENTRY32W) -> Result<bool, u32> {
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let ok = unsafe { Process32FirstW(self.0, entry) };
        if ok != 0 {
            Ok(true)
        } else {
            let err = unsafe { GetLastError() };
            if err == ERROR_NO_MORE_FILES || err == 0 {
                Ok(false)
            } else {
                Err(err)
            }
        }
    }

    fn next(&mut self, entry: &mut PROCESSENTRY32W) -> Result<bool, u32> {
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let ok = unsafe { Process32NextW(self.0, entry) };
        if ok != 0 {
            Ok(true)
        } else {
            let err = unsafe { GetLastError() };
            if err == ERROR_NO_MORE_FILES || err == 0 {
                Ok(false)
            } else {
                Err(err)
            }
        }
    }
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

    #[cfg(windows)]
    fn LocalFree(hMem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
}

#[cfg(windows)]
#[link(name = "shell32")]
extern "system" {
    fn CommandLineToArgvW(lpCmdLine: *const u16, pNumArgs: *mut i32) -> *mut *mut u16;
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
        Some(unsafe {
            std::mem::transmute::<*mut std::ffi::c_void, FnNtQueryInformationProcess>(addr)
        })
    }
}

/// Query command-line string for a process using `ProcessCommandLineInformation` (class 60).
pub fn query_process_command_line(
    h_process: *mut std::ffi::c_void,
    nt_query: Option<FnNtQueryInformationProcess>,
) -> Option<String> {
    let query_fn = nt_query?;

    // Class 60 is ProcessCommandLineInformation (supported on Windows 8.1 / 10 / 11)
    const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;

    // Allocate an 8-byte aligned buffer using Vec<u64>:
    let mut buf_u64 = vec![0u64; 512]; // 4096 bytes
    let mut return_len: u32 = 0;

    let buf_ptr = buf_u64.as_mut_ptr() as *mut std::ffi::c_void;
    let buf_bytes_len = (buf_u64.len() * std::mem::size_of::<u64>()) as u32;

    let mut status = unsafe {
        query_fn(
            h_process,
            PROCESS_COMMAND_LINE_INFORMATION,
            buf_ptr,
            buf_bytes_len,
            &mut return_len,
        )
    };

    // If buffer was too small (STATUS_BUFFER_TOO_SMALL / STATUS_INFO_LENGTH_MISMATCH):
    if status != 0 && return_len > buf_bytes_len {
        let u64_count = (return_len as usize).div_ceil(8) + 8;
        buf_u64.resize(u64_count, 0);
        let buf_ptr = buf_u64.as_mut_ptr() as *mut std::ffi::c_void;
        let buf_bytes_len = (buf_u64.len() * std::mem::size_of::<u64>()) as u32;
        status = unsafe {
            query_fn(
                h_process,
                PROCESS_COMMAND_LINE_INFORMATION,
                buf_ptr,
                buf_bytes_len,
                &mut return_len,
            )
        };
    }

    if status != 0 {
        return None;
    }

    let byte_len = (return_len as usize).min(buf_u64.len() * std::mem::size_of::<u64>());
    let byte_slice: &[u8] =
        unsafe { std::slice::from_raw_parts(buf_u64.as_ptr() as *const u8, byte_len) };

    parse_unicode_string_buffer(byte_slice)
}

#[cfg(windows)]
pub fn parse_windows_command_line_native(cmd: &str) -> Vec<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    let wide: Vec<u16> = std::ffi::OsStr::new(cmd)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut num_args: i32 = 0;
    let argv_ptr = unsafe { CommandLineToArgvW(wide.as_ptr(), &mut num_args) };
    if argv_ptr.is_null() || num_args <= 0 {
        return parse_windows_command_line_msdn(cmd);
    }

    let mut args = Vec::with_capacity(num_args as usize);
    for i in 0..num_args as usize {
        let arg_ptr = unsafe { *argv_ptr.add(i) };
        if !arg_ptr.is_null() {
            let mut len = 0;
            while unsafe { *arg_ptr.add(len) } != 0 {
                len += 1;
            }
            let slice = unsafe { std::slice::from_raw_parts(arg_ptr, len) };
            let s = OsString::from_wide(slice).to_string_lossy().into_owned();
            args.push(s);
        }
    }

    unsafe {
        LocalFree(argv_ptr as *mut std::ffi::c_void);
    }

    args
}

pub fn parse_windows_command_line(cmd: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        parse_windows_command_line_native(cmd)
    }
    #[cfg(not(windows))]
    {
        parse_windows_command_line_msdn(cmd)
    }
}

pub fn scan_processes(p: &Paths) -> Result<ProcessDiscoveryReport> {
    let h_snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let snap_guard = AutoHandle::new(h_snap).ok_or_else(|| {
        let err = unsafe { GetLastError() };
        Error::Msg(format!("CreateToolhelp32Snapshot failed with code {err}"))
    })?;

    let nt_query = resolve_nt_query_information_process();
    let enumerator = LiveSnapshotEnumerator(snap_guard.raw());
    scan_processes_with_enumerator(p, enumerator, nt_query)
}

pub(crate) fn scan_processes_with_enumerator<E: ProcessSnapshotEnumerator>(
    p: &Paths,
    mut enumerator: E,
    nt_query: Option<FnNtQueryInformationProcess>,
) -> Result<ProcessDiscoveryReport> {
    let managed = accounts::list(p).unwrap_or_default();
    let desktop = crate::tool::desktop();
    let configured_desktop = desktop.file_name().and_then(|s| s.to_str());

    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut processes = Vec::new();
    let mut warnings = Vec::new();
    let mut is_complete = true;

    let mut has_entry = match enumerator.first(&mut entry) {
        Ok(true) => true,
        Ok(false) => false,
        Err(err) => {
            warnings.push(format!("Process32FirstW failed: error code {err}"));
            is_complete = false;
            false
        }
    };

    while has_entry {
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

                    let identity =
                        classify_process(exe_path.as_deref(), &exe_name, &args, configured_desktop);

                    if identity != ProcessIdentity::ElectronHelper
                        && identity != ProcessIdentity::Cli
                        && identity != ProcessIdentity::NonCodex
                    {
                        let user_data_dir = super::extract_user_data_dir(&args);
                        let attribution =
                            attribute_process(p, user_data_dir.as_deref(), None, &managed);

                        processes.push(DiscoveredProcess {
                            pid,
                            exe_path,
                            identity,
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
                        is_complete = false;
                        processes.push(DiscoveredProcess {
                            pid,
                            exe_path: None,
                            identity: ProcessIdentity::CandidateDesktop,
                            attribution: ProfileAttribution::Unknown,
                            user_data_dir: None,
                        });
                    } else if err != ERROR_INVALID_PARAMETER && err != ERROR_NOT_FOUND {
                        // Ordinary process-exit churn (87, 1168) does not indicate permission failure.
                        // Other unexpected errors are recorded:
                        warnings.push(format!(
                            "error opening candidate process pid {pid}: code {err}"
                        ));
                        is_complete = false;
                    }
                }
            }
        }

        match enumerator.next(&mut entry) {
            Ok(true) => {
                has_entry = true;
            }
            Ok(false) => {
                has_entry = false;
            }
            Err(err) => {
                warnings.push(format!(
                    "Process32NextW failed prematurely: error code {err}"
                ));
                is_complete = false;
                has_entry = false;
            }
        }
    }

    processes.sort_by_key(|p| p.pid);
    Ok(ProcessDiscoveryReport {
        processes,
        warnings,
        is_complete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockSnapshotEnumerator {
        first_result: Result<Option<PROCESSENTRY32W>, u32>,
        next_results: Vec<Result<Option<PROCESSENTRY32W>, u32>>,
        next_index: usize,
    }

    impl ProcessSnapshotEnumerator for MockSnapshotEnumerator {
        fn first(&mut self, entry: &mut PROCESSENTRY32W) -> Result<bool, u32> {
            match &self.first_result {
                Ok(Some(mock_entry)) => {
                    *entry = mock_entry.clone();
                    Ok(true)
                }
                Ok(None) => Ok(false),
                Err(e) => Err(*e),
            }
        }

        fn next(&mut self, entry: &mut PROCESSENTRY32W) -> Result<bool, u32> {
            if self.next_index < self.next_results.len() {
                let res = &self.next_results[self.next_index];
                self.next_index += 1;
                match res {
                    Ok(Some(mock_entry)) => {
                        *entry = mock_entry.clone();
                        Ok(true)
                    }
                    Ok(None) => Ok(false),
                    Err(e) => Err(*e),
                }
            } else {
                Ok(false)
            }
        }
    }

    #[test]
    fn enumeration_failure_on_first_process_marks_scan_incomplete() {
        let temp_dir =
            std::env::temp_dir().join(format!("cd-win-enum-test-{}", std::process::id()));
        let paths = Paths::new(temp_dir.join("base"), temp_dir.join("profiles"));

        let mock = MockSnapshotEnumerator {
            first_result: Err(ERROR_ACCESS_DENIED), // code 5
            next_results: vec![],
            next_index: 0,
        };

        let report = scan_processes_with_enumerator(&paths, mock, None).unwrap();
        assert!(report.processes.is_empty());
        assert!(
            !report.is_complete,
            "scan must not be reported complete after enumeration failure"
        );
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("Process32FirstW failed: error code 5"));
    }

    #[test]
    fn enumeration_clean_completion_with_no_more_files_is_complete() {
        let temp_dir =
            std::env::temp_dir().join(format!("cd-win-enum-empty-{}", std::process::id()));
        let paths = Paths::new(temp_dir.join("base"), temp_dir.join("profiles"));

        let mock = MockSnapshotEnumerator {
            first_result: Ok(None), // ERROR_NO_MORE_FILES immediately
            next_results: vec![],
            next_index: 0,
        };

        let report = scan_processes_with_enumerator(&paths, mock, None).unwrap();
        assert!(report.processes.is_empty());
        assert!(
            report.is_complete,
            "clean scan termination must be complete"
        );
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn enumeration_premature_termination_on_next_marks_scan_incomplete() {
        let temp_dir =
            std::env::temp_dir().join(format!("cd-win-enum-next-{}", std::process::id()));
        let paths = Paths::new(temp_dir.join("base"), temp_dir.join("profiles"));

        let mock = MockSnapshotEnumerator {
            first_result: Ok(Some(PROCESSENTRY32W::new_mock(1234, "notepad.exe"))),
            next_results: vec![Err(6)], // ERROR_INVALID_HANDLE on next
            next_index: 0,
        };

        let report = scan_processes_with_enumerator(&paths, mock, None).unwrap();
        assert!(
            !report.is_complete,
            "scan must be marked incomplete when Process32NextW fails prematurely"
        );
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("Process32NextW failed prematurely: error code 6"));
    }
}
