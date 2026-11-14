//! Win32 top-level windows, scoped to a verified PID. No input-queue attachment,
//! synthetic keystrokes, foreground-lock changes, or process-name focusing.
use super::*;
use std::ffi::c_void;
type Hwnd = *mut c_void;
#[link(name = "user32")]
extern "system" {
    fn EnumWindows(callback: unsafe extern "system" fn(Hwnd, isize) -> i32, data: isize) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
    fn IsWindow(hwnd: Hwnd) -> i32;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn IsWindowEnabled(hwnd: Hwnd) -> i32;
    fn IsIconic(hwnd: Hwnd) -> i32;
    fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
    fn SetForegroundWindow(hwnd: Hwnd) -> i32;
    fn GetForegroundWindow() -> Hwnd;
    fn GetWindow(hwnd: Hwnd, command: u32) -> Hwnd;
    fn GetWindowLongW(hwnd: Hwnd, index: i32) -> i32;
    fn GetClassNameW(hwnd: Hwnd, buffer: *mut u16, count: i32) -> i32;
    fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn SetLastError(code: u32);
    fn GetLastError() -> u32;
}

fn window_style(hwnd: Hwnd, index: i32) -> Result<u32> {
    // Zero is also a legitimate style; distinguish it from inspection failure.
    unsafe {
        SetLastError(0);
        let value = GetWindowLongW(hwnd, index);
        if value == 0 && GetLastError() != 0 {
            return Err(refusal("window styles could not be inspected"));
        }
        Ok(value as u32)
    }
}

#[repr(C)]
#[derive(Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

pub(super) struct WindowsAdapter;

fn inspect(hwnd: Hwnd) -> Result<NativeWindow> {
    // SAFETY: all handles are opaque OS values; buffers have the declared size.
    unsafe {
        let mut pid = 0;
        if IsWindow(hwnd) == 0 || GetWindowThreadProcessId(hwnd, &mut pid) == 0 || pid == 0 {
            return Err(refusal("window ownership could not be inspected"));
        }
        let mut class = [0u16; 256];
        let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
        let mut rect = Rect::default();
        if len == 0 || GetWindowRect(hwnd, &mut rect) == 0 {
            return Err(refusal("window metadata is unavailable"));
        }
        let class = String::from_utf16_lossy(&class[..len as usize]);
        let style = window_style(hwnd, -16)?; // GWL_STYLE
        let extended = window_style(hwnd, -20)?; // GWL_EXSTYLE
        let owned = !GetWindow(hwnd, 4).is_null(); // GW_OWNER
        let kind = if owned || class == "#32770" {
            WindowKind::Dialog
        } else if style & 0x4000_0000 != 0 // WS_CHILD
                || extended & (0x80 | 0x0800_0000) != 0 // TOOLWINDOW / NOACTIVATE
                || !class.starts_with("Chrome_WidgetWin_")
                || ((rect.right as i64 - rect.left as i64).max(0)
                    * (rect.bottom as i64 - rect.top as i64).max(0) < 100_000 && IsIconic(hwnd) == 0)
        {
            WindowKind::Helper
        } else {
            WindowKind::Standard
        };
        Ok(NativeWindow {
            id: hwnd as usize as u64,
            pid,
            kind,
            visible: IsWindowVisible(hwnd) != 0,
            minimized: IsIconic(hwnd) != 0,
            enabled: IsWindowEnabled(hwnd) != 0,
        })
    }
}

struct Enumeration {
    pid: u32,
    windows: Vec<NativeWindow>,
    error: Option<Error>,
}
unsafe extern "system" fn collect(hwnd: Hwnd, data: isize) -> i32 {
    // SAFETY: EnumWindows is synchronous; data points to its live stack context.
    let state = unsafe { &mut *(data as *mut Enumeration) };
    let mut pid = 0;
    if unsafe { GetWindowThreadProcessId(hwnd, &mut pid) } == 0 {
        state.error = Some(refusal("window enumeration raced process termination"));
        return 0;
    }
    if pid == state.pid {
        match inspect(hwnd) {
            Ok(w) => state.windows.push(w),
            Err(e) => {
                state.error = Some(e);
                return 0;
            }
        }
    }
    1
}

impl WindowAdapter for WindowsAdapter {
    fn enumerate(&mut self, pid: u32) -> Result<Vec<NativeWindow>> {
        let mut state = Enumeration {
            pid,
            windows: Vec::new(),
            error: None,
        };
        let success = unsafe { EnumWindows(collect, &mut state as *mut Enumeration as isize) };
        if let Some(error) = state.error {
            return Err(error);
        }
        if success == 0 {
            return Err(refusal("EnumWindows failed or access was denied"));
        }
        Ok(state.windows)
    }
    fn is_foreground(&mut self, window: &NativeWindow) -> Result<bool> {
        let current = inspect(window.id as usize as Hwnd)?;
        if current.pid != window.pid || current.kind != WindowKind::Standard {
            return Err(refusal("window identity changed"));
        }
        Ok(
            unsafe { GetForegroundWindow() } == window.id as usize as Hwnd
                && current.visible
                && !current.minimized,
        )
    }
    fn activate(&mut self, window: &NativeWindow) -> Result<()> {
        let hwnd = window.id as usize as Hwnd;
        let current = inspect(hwnd)?;
        if current.pid != window.pid || current.kind != WindowKind::Standard || !current.enabled {
            return Err(refusal("window ownership or eligibility changed"));
        }
        unsafe {
            // ShowWindow returns previous visibility, not success.
            ShowWindow(hwnd, if current.minimized { 9 } else { 5 }); // RESTORE / SHOW
        }
        let current = inspect(hwnd)?;
        if current.pid != window.pid || current.kind != WindowKind::Standard {
            return Err(refusal("window changed during restore"));
        }
        if unsafe { SetForegroundWindow(hwnd) } == 0 {
            return Err(refusal("Windows denied foreground activation"));
        }
        Ok(()) // The policy engine separately verifies actual foreground ownership.
    }
}
