//! AppKit activation with Accessibility window roles and AXUIElementGetPid.
//! AX supplies both an actionable window reference and its owner, unlike a
//! Quartz CGWindowID (which has no public, reliable mapping to an AX element).
//! Permission denial or missing AX metadata is an error, never guessed from a
//! bundle name. No private AX APIs or AppleScript are used.
use super::*;
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
use std::ffi::{c_void, CString};
type Cf = *const c_void;
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(value: Cf);
    fn CFRetain(value: Cf) -> Cf;
    fn CFEqual(a: Cf, b: Cf) -> u8;
    fn CFGetTypeID(value: Cf) -> usize;
    fn CFArrayGetTypeID() -> usize;
    fn CFArrayGetCount(array: Cf) -> isize;
    fn CFArrayGetValueAtIndex(array: Cf, index: isize) -> Cf;
    fn CFStringGetTypeID() -> usize;
    fn CFStringCreateWithCString(allocator: Cf, text: *const std::ffi::c_char, encoding: u32)
        -> Cf;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(value: Cf) -> u8;
    static kCFBooleanFalse: Cf;
    static kCFBooleanTrue: Cf;
}
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> Cf;
    fn AXUIElementCreateSystemWide() -> Cf;
    fn AXUIElementGetTypeID() -> usize;
    fn AXUIElementGetPid(element: Cf, pid: *mut i32) -> i32;
    fn AXUIElementCopyAttributeValue(element: Cf, attribute: Cf, value: *mut Cf) -> i32;
    fn AXUIElementSetAttributeValue(element: Cf, attribute: Cf, value: Cf) -> i32;
    fn AXUIElementPerformAction(element: Cf, action: Cf) -> i32;
    fn AXUIElementSetMessagingTimeout(element: Cf, seconds: f32) -> i32;
}

struct Owned(Cf);
impl Owned {
    fn new(value: Cf) -> Result<Self> {
        if value.is_null() {
            Err(refusal("macOS returned a null object"))
        } else {
            Ok(Self(value))
        }
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0);
        }
    }
}
fn string(text: &str) -> Result<Owned> {
    let text = CString::new(text).map_err(|_| refusal("invalid AX attribute"))?;
    Owned::new(unsafe { CFStringCreateWithCString(std::ptr::null(), text.as_ptr(), 0x0800_0100) })
}
fn check(code: i32) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(refusal(&format!(
            "macOS Accessibility failed ({code}); check permission and process state"
        )))
    }
}
fn attribute(element: Cf, name: &str) -> Result<Owned> {
    let key = string(name)?;
    let mut value = std::ptr::null();
    check(unsafe { AXUIElementCopyAttributeValue(element, key.0, &mut value) })?;
    Owned::new(value)
}
fn matches_string(element: Cf, name: &str, expected: &str) -> Result<bool> {
    let value = attribute(element, name)?;
    let expected = string(expected)?;
    if unsafe { CFGetTypeID(value.0) != CFStringGetTypeID() } {
        return Err(refusal("invalid AX string metadata"));
    }
    Ok(unsafe { CFEqual(value.0, expected.0) != 0 })
}
fn boolean(element: Cf, name: &str) -> Result<bool> {
    let value = attribute(element, name)?;
    if unsafe { CFGetTypeID(value.0) != CFBooleanGetTypeID() } {
        return Err(refusal("invalid AX boolean metadata"));
    }
    Ok(unsafe { CFBooleanGetValue(value.0) != 0 })
}
fn owner(element: Cf) -> Result<u32> {
    if unsafe { CFGetTypeID(element) != AXUIElementGetTypeID() } {
        return Err(refusal("invalid AX window reference"));
    }
    let mut pid = 0;
    check(unsafe { AXUIElementGetPid(element, &mut pid) })?;
    u32::try_from(pid)
        .ok()
        .filter(|p| *p != 0)
        .ok_or_else(|| refusal("invalid AX process owner"))
}
fn application(pid: u32) -> Result<objc2::rc::Retained<NSRunningApplication>> {
    let pid = i32::try_from(pid).map_err(|_| refusal("invalid macOS PID"))?;
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .filter(|app| !app.isTerminated())
        .ok_or_else(|| refusal("managed application terminated"))
}
fn ax_application(pid: u32) -> Result<Owned> {
    if !unsafe { AXIsProcessTrusted() } {
        return Err(refusal(
            "Accessibility permission is required; no permission prompt was opened",
        ));
    }
    let app = Owned::new(unsafe {
        AXUIElementCreateApplication(i32::try_from(pid).map_err(|_| refusal("invalid PID"))?)
    })?;
    check(unsafe { AXUIElementSetMessagingTimeout(app.0, 0.25) })?;
    Ok(app)
}
fn inspect(element: Cf, id: u64, hidden: bool) -> Result<NativeWindow> {
    check(unsafe { AXUIElementSetMessagingTimeout(element, 0.25) })?;
    let pid = owner(element)?;
    let normal = matches_string(element, "AXRole", "AXWindow")?
        && matches_string(element, "AXSubrole", "AXStandardWindow")?;
    let modal = boolean(element, "AXModal")?;
    let minimized = boolean(element, "AXMinimized")?;
    Ok(NativeWindow {
        id,
        pid,
        kind: if modal || !normal {
            WindowKind::Dialog
        } else {
            WindowKind::Standard
        },
        visible: !hidden && !minimized,
        minimized,
        enabled: true,
    })
}

#[derive(Default)]
pub(super) struct MacAdapter {
    windows: Vec<Owned>,
}
impl MacAdapter {
    fn reference(&self, window: &NativeWindow) -> Result<Cf> {
        self.windows
            .get(
                window
                    .id
                    .checked_sub(1)
                    .ok_or_else(|| refusal("invalid window handle"))? as usize,
            )
            .map(|w| w.0)
            .ok_or_else(|| refusal("unknown AX window handle"))
    }
}
impl WindowAdapter for MacAdapter {
    fn enumerate(&mut self, pid: u32) -> Result<Vec<NativeWindow>> {
        objc2::rc::autoreleasepool(|_| {
            let app = application(pid)?;
            let ax = ax_application(pid)?;
            let list = attribute(ax.0, "AXWindows")?;
            if unsafe { CFGetTypeID(list.0) != CFArrayGetTypeID() } {
                return Err(refusal("invalid AX window list"));
            }
            let count = unsafe { CFArrayGetCount(list.0) };
            if !(0..=256).contains(&count) {
                return Err(refusal("AX window count exceeds safety bound"));
            }
            let mut result = Vec::new();
            for index in 0..count {
                let element = unsafe { CFArrayGetValueAtIndex(list.0, index) };
                if element.is_null() {
                    return Err(refusal("null AX window"));
                }
                let slot = self
                    .windows
                    .iter()
                    .position(|w| unsafe { CFEqual(w.0, element) != 0 });
                let slot = match slot {
                    Some(slot) => slot,
                    None => {
                        if self.windows.len() >= 512 {
                            return Err(refusal("too many changing AX windows"));
                        }
                        self.windows.push(Owned::new(unsafe { CFRetain(element) })?);
                        self.windows.len() - 1
                    }
                };
                result.push(inspect(element, slot as u64 + 1, app.isHidden())?);
            }
            Ok(result)
        })
    }
    fn is_foreground(&mut self, window: &NativeWindow) -> Result<bool> {
        objc2::rc::autoreleasepool(|_| {
            let element = self.reference(window)?;
            let app = application(window.pid)?;
            let current = inspect(element, window.id, app.isHidden())?;
            if current.pid != window.pid || current.kind != WindowKind::Standard {
                return Err(refusal("AX window identity changed"));
            }
            // NSRunningApplication properties can be cached for a run-loop turn.
            // Verify live AX system focus as well, never trust AppKit alone.
            if !current.visible || current.minimized || !app.isActive() {
                return Ok(false);
            }
            let system = Owned::new(unsafe { AXUIElementCreateSystemWide() })?;
            check(unsafe { AXUIElementSetMessagingTimeout(system.0, 0.25) })?;
            let focused_app = attribute(system.0, "AXFocusedApplication")?;
            let ax = ax_application(window.pid)?;
            let focused_window = attribute(ax.0, "AXFocusedWindow")?;
            Ok(app.isActive()
                && current.visible
                && !current.minimized
                && owner(focused_app.0)? == window.pid
                && unsafe { CFEqual(focused_window.0, element) != 0 })
        })
    }
    fn activate(&mut self, window: &NativeWindow) -> Result<()> {
        objc2::rc::autoreleasepool(|_| {
            let element = self.reference(window)?;
            let app = application(window.pid)?;
            let current = inspect(element, window.id, app.isHidden())?;
            if current.pid != window.pid || current.kind != WindowKind::Standard {
                return Err(refusal("AX window ownership changed"));
            }
            if current.minimized {
                let key = string("AXMinimized")?;
                check(unsafe { AXUIElementSetAttributeValue(element, key.0, kCFBooleanFalse) })?;
            }
            if app.isHidden() && !app.unhide() {
                return Err(refusal("AppKit refused to unhide the managed application"));
            }
            if owner(element)? != window.pid {
                return Err(refusal("AX owner changed during restore"));
            }
            let raise = string("AXRaise")?;
            check(unsafe { AXUIElementPerformAction(element, raise.0) })?;
            let main = string("AXMain")?;
            check(unsafe { AXUIElementSetAttributeValue(element, main.0, kCFBooleanTrue) })?;
            if !app.activateWithOptions(NSApplicationActivationOptions::empty()) {
                return Err(refusal("AppKit refused activation"));
            }
            Ok(())
        })
    }
}
