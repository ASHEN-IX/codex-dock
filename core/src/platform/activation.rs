//! PID-scoped window activation. Native adapters never launch a process.
//! Production capability gates remain independent of synthetic test success.
use crate::error::{Error, Result};
use crate::paths::Paths;
use crate::platform::process::ProcessDiscoveryReport;
#[cfg(any(
    test,
    debug_assertions,
    feature = "synthetic-tests",
    target_os = "windows",
    target_os = "macos"
))]
use crate::platform::process::{DiscoveredProcess, ProcessIdentity, ProfileAttribution};

#[cfg(any(test, target_os = "macos"))]
mod geometry;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    Standard,
    Dialog,
    Helper,
}

/// Adapter-local handle, never a title or executable-name match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeWindow {
    pub id: u64,
    pub pid: u32,
    pub kind: WindowKind,
    pub visible: bool,
    pub minimized: bool,
    pub enabled: bool,
}

/// Enumeration errors and permission restrictions must be returned, not hidden
/// behind an empty list. Handles must be revalidated when acted on.
pub trait WindowAdapter {
    fn enumerate(&mut self, pid: u32) -> Result<Vec<NativeWindow>>;
    fn is_foreground(&mut self, window: &NativeWindow) -> Result<bool>;
    fn activate(&mut self, window: &NativeWindow) -> Result<()>;
}

pub trait ProcessAdapter {
    fn snapshot(&mut self) -> Result<ProcessDiscoveryReport>;
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
struct NativeProcesses<'a>(&'a Paths);
#[cfg(any(target_os = "windows", target_os = "macos"))]
impl ProcessAdapter for NativeProcesses<'_> {
    fn snapshot(&mut self) -> Result<ProcessDiscoveryReport> {
        crate::platform::process::scan_processes(self.0)
    }
}

fn refusal(message: &str) -> Error {
    Error::Msg(format!("cannot activate desktop: {message}"))
}

#[cfg(any(
    test,
    debug_assertions,
    feature = "synthetic-tests",
    target_os = "windows",
    target_os = "macos"
))]
fn verified_process(report: ProcessDiscoveryReport, name: &str) -> Result<DiscoveredProcess> {
    if !report.is_complete || !report.warnings.is_empty() {
        return Err(refusal("process discovery is incomplete or denied"));
    }
    if report
        .processes
        .iter()
        .any(|p| p.attribution == ProfileAttribution::Unknown)
    {
        return Err(refusal("an unknown or ambiguous desktop process exists"));
    }
    let mut matches = report
        .processes
        .into_iter()
        .filter(|p| p.attribution == ProfileAttribution::Known(name.to_owned()));
    let process = matches
        .next()
        .ok_or_else(|| refusal("no managed desktop process"))?;
    if matches.next().is_some()
        || process.pid == 0
        || process.identity != ProcessIdentity::ConfirmedDesktop
    {
        return Err(refusal(
            "managed main-process identity is ambiguous or unverified",
        ));
    }
    Ok(process)
}

#[cfg(any(
    test,
    debug_assertions,
    feature = "synthetic-tests",
    target_os = "windows",
    target_os = "macos"
))]
fn windows_for(adapter: &mut impl WindowAdapter, pid: u32) -> Result<Vec<NativeWindow>> {
    let all = adapter.enumerate(pid)?;
    let mut seen = std::collections::HashSet::new();
    if all.iter().any(|w| w.id == 0 || !seen.insert(w.id)) {
        return Err(refusal("window enumeration contains ambiguous handles"));
    }
    // Activating an application's main window while it has a modal dialog can
    // redirect focus to the dialog. Refuse rather than declaring that success.
    if all
        .iter()
        .any(|w| w.pid == pid && w.kind == WindowKind::Dialog && w.visible)
    {
        return Err(refusal("a dialog is blocking the managed desktop"));
    }
    Ok(all
        .into_iter()
        .filter(|w| w.pid == pid && w.kind == WindowKind::Standard && w.enabled)
        .collect())
}

/// Internal policy engine. Injection is exposed only by the explicit testing API.
#[cfg(any(
    test,
    debug_assertions,
    feature = "synthetic-tests",
    target_os = "windows",
    target_os = "macos"
))]
fn activate_with(
    name: &str,
    processes: &mut impl ProcessAdapter,
    windows: &mut impl WindowAdapter,
) -> Result<NativeWindow> {
    let process = verified_process(processes.snapshot()?, name)?;
    let mut candidates = windows_for(windows, process.pid)?;
    // Keep native z-order within visibility tiers. Any selected window belongs
    // to the same verified main PID/account; never fall back to another PID.
    candidates.sort_by_key(|w| (!w.visible, w.minimized));
    let mut selected = None;
    for window in &candidates {
        if windows.is_foreground(window)? {
            selected = Some(window.clone());
            break;
        }
    }
    let selected = selected
        .or_else(|| candidates.into_iter().next())
        .ok_or_else(|| refusal("no eligible account window"))?;
    if verified_process(processes.snapshot()?, name)? != process {
        return Err(refusal("managed process changed before activation"));
    }
    let current = windows_for(windows, process.pid)?
        .into_iter()
        .find(|w| w.id == selected.id)
        .ok_or_else(|| refusal("window disappeared or changed ownership"))?;
    if !windows.is_foreground(&current)? {
        windows.activate(&current)?;
    }
    // Native activation is asynchronous and may legally be denied by the OS.
    let start = std::time::Instant::now();
    loop {
        if verified_process(processes.snapshot()?, name)? != process {
            return Err(refusal("managed process changed during activation"));
        }
        let live = windows_for(windows, process.pid)?
            .into_iter()
            .find(|w| w.id == current.id)
            .ok_or_else(|| refusal("window disappeared during activation"))?;
        if live.visible && !live.minimized && windows.is_foreground(&live)? {
            return Ok(live);
        }
        if start.elapsed() >= std::time::Duration::from_millis(750) {
            return Err(refusal(
                "foreground activation was not verified (OS restrictions may apply)",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

/// Activate an existing managed account window. Never spawns or retries launch.
pub fn activate_profile(p: &Paths, name: &str) -> Result<NativeWindow> {
    crate::platform::require(
        crate::platform::capabilities().window_activation,
        "window activation",
    )?;
    let name = crate::accounts::canonical_name(p, name)?;
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        activate_native(p, &name)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = name;
        Err(refusal("use the existing Linux focus adapter"))
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
fn activate_native(p: &Paths, name: &str) -> Result<NativeWindow> {
    #[cfg(target_os = "windows")]
    let mut adapter = windows::WindowsAdapter;
    #[cfg(target_os = "macos")]
    let mut adapter = macos::MacAdapter::default();
    activate_with(name, &mut NativeProcesses(p), &mut adapter)
}

/// Explicit opt-in test surface; unavailable in default-feature release builds.
#[cfg(any(test, debug_assertions, feature = "synthetic-tests"))]
pub mod testing {
    use super::*;
    pub fn activate(
        name: &str,
        processes: &mut impl ProcessAdapter,
        windows: &mut impl WindowAdapter,
    ) -> Result<NativeWindow> {
        activate_with(name, processes, windows)
    }
    /// Manual smoke testing of an already-running, disposable managed profile.
    /// This bypasses only the activation capability, never process attribution.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub fn activate_native_profile(p: &Paths, name: &str) -> Result<NativeWindow> {
        let name = crate::accounts::canonical_name(p, name)?;
        activate_native(p, &name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    fn process(pid: u32, name: &str) -> DiscoveredProcess {
        DiscoveredProcess {
            pid,
            exe_path: Some("/disposable/Codex".into()),
            identity: ProcessIdentity::ConfirmedDesktop,
            attribution: ProfileAttribution::Known(name.into()),
            user_data_dir: Some(format!("/disposable/Codex-{name}").into()),
        }
    }
    fn report() -> ProcessDiscoveryReport {
        ProcessDiscoveryReport {
            processes: vec![process(42, "alpha"), process(43, "beta")],
            warnings: vec![],
            is_complete: true,
        }
    }
    struct Processes {
        reports: VecDeque<ProcessDiscoveryReport>,
    }
    impl ProcessAdapter for Processes {
        fn snapshot(&mut self) -> Result<ProcessDiscoveryReport> {
            if self.reports.len() > 1 {
                Ok(self.reports.pop_front().unwrap())
            } else {
                Ok(self.reports.front().unwrap().clone())
            }
        }
    }
    fn processes(reports: Vec<ProcessDiscoveryReport>) -> Processes {
        Processes {
            reports: reports.into(),
        }
    }
    fn window(id: u64, pid: u32) -> NativeWindow {
        NativeWindow {
            id,
            pid,
            kind: WindowKind::Standard,
            visible: true,
            minimized: false,
            enabled: true,
        }
    }
    struct Windows {
        views: VecDeque<Vec<NativeWindow>>,
        foreground: Option<u64>,
        acted: Vec<u64>,
        deny_enumeration: bool,
        deny_activation: bool,
        acknowledge_only: bool,
    }
    impl WindowAdapter for Windows {
        fn enumerate(&mut self, _: u32) -> Result<Vec<NativeWindow>> {
            if self.deny_enumeration {
                return Err(refusal("permission denied"));
            }
            if self.views.len() > 1 {
                Ok(self.views.pop_front().unwrap())
            } else {
                Ok(self.views.front().unwrap().clone())
            }
        }
        fn is_foreground(&mut self, w: &NativeWindow) -> Result<bool> {
            Ok(self.foreground == Some(w.id))
        }
        fn activate(&mut self, w: &NativeWindow) -> Result<()> {
            self.acted.push(w.id);
            if self.deny_activation {
                return Err(refusal("foreground denied"));
            }
            if !self.acknowledge_only {
                self.foreground = Some(w.id);
                for view in &mut self.views {
                    for live in view.iter_mut().filter(|live| live.id == w.id) {
                        live.minimized = false;
                        live.visible = true;
                    }
                }
            }
            Ok(())
        }
    }
    fn windows(view: Vec<NativeWindow>) -> Windows {
        Windows {
            views: vec![view].into(),
            foreground: None,
            acted: vec![],
            deny_enumeration: false,
            deny_activation: false,
            acknowledge_only: false,
        }
    }
    #[test]
    fn selects_only_requested_managed_pid_even_with_other_account_frontmost() {
        let mut w = windows(vec![window(1, 43), window(2, 42)]);
        w.foreground = Some(1);
        let result = activate_with("alpha", &mut processes(vec![report()]), &mut w).unwrap();
        assert_eq!((result.pid, result.id), (42, 2));
        assert_eq!(w.acted, [2]);
    }
    #[test]
    fn missing_account_and_wrong_pid_windows_never_activate() {
        for (name, view) in [
            ("unknown", vec![window(1, 42)]),
            ("alpha", vec![window(1, 43)]),
        ] {
            let mut w = windows(view);
            assert!(activate_with(name, &mut processes(vec![report()]), &mut w).is_err());
            assert!(w.acted.is_empty());
        }
    }
    #[test]
    fn incomplete_denied_unknown_or_duplicate_processes_fail_closed() {
        let mut incomplete = report();
        incomplete.is_complete = false;
        let mut warning = report();
        warning.warnings.push("access denied".into());
        let mut unknown = report();
        unknown.processes[1].attribution = ProfileAttribution::Unknown;
        let mut duplicate = report();
        duplicate.processes.push(process(44, "alpha"));
        for r in [incomplete, warning, unknown, duplicate] {
            let mut w = windows(vec![window(1, 42)]);
            assert!(activate_with("alpha", &mut processes(vec![r]), &mut w).is_err());
            assert!(w.acted.is_empty());
        }
    }
    #[test]
    fn helpers_cli_and_unconfirmed_candidates_cannot_authorize_activation() {
        for identity in [
            ProcessIdentity::ElectronHelper,
            ProcessIdentity::CandidateDesktop,
            ProcessIdentity::Cli,
            ProcessIdentity::NonCodex,
        ] {
            let mut r = report();
            r.processes[0].identity = identity;
            let mut w = windows(vec![window(1, 42)]);
            assert!(activate_with("alpha", &mut processes(vec![r]), &mut w).is_err());
            assert!(w.acted.is_empty());
        }
    }
    #[test]
    fn multiple_windows_prefer_already_focused_and_do_not_reactivate() {
        let mut w = windows(vec![window(1, 42), window(2, 42)]);
        w.foreground = Some(2);
        assert_eq!(
            activate_with("alpha", &mut processes(vec![report()]), &mut w)
                .unwrap()
                .id,
            2
        );
        assert!(w.acted.is_empty());
    }
    #[test]
    fn minimized_and_hidden_windows_are_restored_and_verified() {
        for minimized in [false, true] {
            let mut hidden = window(1, 42);
            hidden.visible = false;
            hidden.minimized = minimized;
            let mut w = windows(vec![hidden]);
            let result = activate_with("alpha", &mut processes(vec![report()]), &mut w).unwrap();
            assert!(result.visible && !result.minimized);
            assert_eq!(w.acted, [1]);
        }
    }
    #[test]
    fn visible_window_is_preferred_to_hidden_window_in_same_account() {
        let mut hidden = window(1, 42);
        hidden.visible = false;
        let mut w = windows(vec![hidden, window(2, 42)]);
        assert_eq!(
            activate_with("alpha", &mut processes(vec![report()]), &mut w)
                .unwrap()
                .id,
            2
        );
    }
    #[test]
    fn dialogs_helpers_and_disabled_windows_are_not_activated() {
        for kind in [WindowKind::Dialog, WindowKind::Helper] {
            let mut item = window(1, 42);
            item.kind = kind;
            let mut w = windows(vec![item]);
            assert!(activate_with("alpha", &mut processes(vec![report()]), &mut w).is_err());
            assert!(w.acted.is_empty());
        }
        let mut disabled = window(1, 42);
        disabled.enabled = false;
        let mut w = windows(vec![disabled]);
        assert!(activate_with("alpha", &mut processes(vec![report()]), &mut w).is_err());
        assert!(w.acted.is_empty());
    }
    #[test]
    fn a_visible_dialog_blocks_raising_the_main_window() {
        let mut dialog = window(2, 42);
        dialog.kind = WindowKind::Dialog;
        let mut w = windows(vec![window(1, 42), dialog]);
        assert!(activate_with("alpha", &mut processes(vec![report()]), &mut w).is_err());
        assert!(w.acted.is_empty());
    }
    #[test]
    fn permission_or_activation_denial_is_an_error() {
        for enumerate in [false, true] {
            let mut w = windows(vec![window(1, 42)]);
            w.deny_enumeration = enumerate;
            w.deny_activation = !enumerate;
            assert!(activate_with("alpha", &mut processes(vec![report()]), &mut w).is_err());
        }
    }
    #[test]
    fn native_acknowledgement_without_foreground_confirmation_is_not_success() {
        let mut w = windows(vec![window(1, 42)]);
        w.acknowledge_only = true;
        assert!(
            activate_with("alpha", &mut processes(vec![report()]), &mut w)
                .unwrap_err()
                .to_string()
                .contains("not verified")
        );
    }
    #[test]
    fn foreground_handle_without_visible_window_is_not_success() {
        let mut hidden = window(1, 42);
        hidden.visible = false;
        let mut w = windows(vec![hidden]);
        w.foreground = Some(1);
        w.acknowledge_only = true;
        assert!(activate_with("alpha", &mut processes(vec![report()]), &mut w).is_err());
    }

    #[test]
    fn process_termination_before_action_does_not_activate() {
        let mut gone = report();
        gone.processes.remove(0);
        let mut w = windows(vec![window(1, 42)]);
        assert!(activate_with("alpha", &mut processes(vec![report(), gone]), &mut w).is_err());
        assert!(w.acted.is_empty());
    }
    #[test]
    fn process_reassignment_or_exit_after_action_cannot_report_success() {
        for reassigned in [false, true] {
            let mut changed = report();
            if reassigned {
                changed.processes[0].attribution = ProfileAttribution::Known("beta".into());
            } else {
                changed.processes.remove(0);
            }
            let mut w = windows(vec![window(1, 42)]);
            assert!(activate_with(
                "alpha",
                &mut processes(vec![report(), report(), changed]),
                &mut w
            )
            .is_err());
        }
    }
    #[test]
    fn window_termination_or_reassignment_before_action_fails_closed() {
        for view in [vec![], vec![window(1, 43)]] {
            let mut w = windows(vec![window(1, 42)]);
            w.views.push_back(view);
            assert!(activate_with("alpha", &mut processes(vec![report()]), &mut w).is_err());
            assert!(w.acted.is_empty());
        }
    }
    #[test]
    fn window_termination_after_action_is_not_success() {
        let mut w = windows(vec![window(1, 42)]);
        w.views.extend([vec![window(1, 42)], vec![]]);
        assert!(activate_with("alpha", &mut processes(vec![report()]), &mut w).is_err());
    }
    #[test]
    fn duplicated_or_zero_window_handles_are_rejected() {
        for view in [vec![window(1, 42), window(1, 43)], vec![window(0, 42)]] {
            let mut w = windows(view);
            assert!(activate_with("alpha", &mut processes(vec![report()]), &mut w).is_err());
            assert!(w.acted.is_empty());
        }
    }
}
