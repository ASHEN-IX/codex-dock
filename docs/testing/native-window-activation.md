# T2.4A native desktop smoke guide

Official-runtime testing starts at NOT EXECUTED on all platforms. The [target matrix](official-desktop-compatibility.md) describes which products to identify. Use one [evidence report](desktop-evidence-checklist.md) per OS, architecture and installed build.

## Set up a disposable session

- Use an isolated OS user/session and two disposable accounts, called alpha and beta in the report. Use the official download channel or an already-installed build whose provenance you can verify. Keep other desktop instances closed initially and keep an unrelated editor open as a negative control.
- Choose an absolute local sandbox root outside any normal or shared store. Create empty `base`, `profiles/alpha` and `profiles/beta` directories. Avoid symlinks or junctions. Never copy authentication files, link stores, or use Dock repair or account removal.
- Run the probe as the same unprivileged user in the interactive session. Launch the verified app normally in that disposable session first to identify the running main executable and verify that it offers Codex. Quit it before starting any isolation experiment. Record only sanitized identity, paths, PIDs, window IDs, results and permissions.

## Select and probe the installed build

### Windows x64

Use an x64 Windows host and Windows PowerShell 5.1. Find the main executable for the verified app using the app's installation information or the main process in Task Manager (Details → Open file location, where available). For a packaged app also corroborate the package identity using installed package metadata. Pass the full executable path, including its filename. Do not use a Start-menu alias, installer, updater or bundled CLI.

```powershell
$desktop = 'C:\path\to\installed\main.exe'
.\scripts\desktop-compatibility.ps1 -ConfirmDisposable -Executable $desktop
```

Use the x64 Rust host target for the harness (`x86_64-pc-windows-msvc`). Compare the host and executable architectures in the probe. If package policy prevents locating or inspecting the real image, record the blocker and avoid permission changes or ownership workarounds.

### Windows ARM64

Use Windows on ARM and Windows PowerShell 5.1. Follow the x64 inventory procedure only when the official channel provides a compatible build. Use `aarch64-pc-windows-msvc` for the harness. Record the PE architecture and whether the installed build runs natively, includes hybrid code, or runs under x64 emulation if verified. Report native and emulated runtime scenarios separately. If the official build is unavailable for the host, record BLOCKED with the observed distribution constraint. If availability itself was never checked, record NOT EXECUTED.

### macOS Apple Silicon

Use macOS 14 or later and a trusted Python 3.9+ interpreter. Select the official Apple Silicon download or verify the existing installation. Pass the installed bundle path from Finder. The probe resolves the main executable via the bundle plist and reports its Mach-O architectures.

```sh
python3 scripts/desktop-compatibility-macos.py --confirm-disposable '/Applications/ChatGPT.app'
```

Substitute the actual bundle path, which may differ on an upgraded installation. Use `aarch64-apple-darwin` for the harness. Record any Rosetta translation separately if observed. A binary's architecture slices do not establish which slice is running.

### macOS Intel

Use macOS 14 or later and the official Intel download, or an existing official bundle with an x86_64 slice. Repeat the macOS inventory on the actual Intel host and use `x86_64-apple-darwin` for the harness. Record a distribution or OS restriction as BLOCKED if observed. A Rosetta result from Apple Silicon cannot stand in for an Intel run.

## Interpret the probe

Run once with the verified official app open, and again after each intentional instance change. Each invocation prints one JSON object for the report's `probe_samples` array. The Windows snapshot covers matching image basenames and marks missing matching image paths partial. The macOS probe queries visible PIDs and groups resolved paths by the selected basename. Unreadable macOS paths are counted even when they may belong to unrelated processes. Completeness is relative to the stated scope, with no guarantee about alternate executable names, other sessions, launchers, helpers or processes that start or exit during the sample. Compare against independent same-session process observations and document any gaps. Metadata and path matches are evidence to corroborate with provenance and product UI.

The probe reads installed metadata and process image identifiers. It does not query process arguments, environments, auth files, profiles, or window/chat titles. It leaves process-to-profile attribution unverified. Record independent attribution observations in the isolation scenario. On macOS the prompt-free Accessibility query applies to the probe's execution context; permission for the Rust smoke binary must be tested separately. Windows foreground and integrity restrictions depend on the window and caller context, so permission remains unverified until an actual activation attempt.

## Verify isolation before activation

1. Within the disposable OS session, use an official, documented instance/profile mechanism if available for the installed build. If you explicitly experiment with Dock's proposed direct-launch settings, treat them as hypotheses. Use the verified image and point both settings into the disposable root. Avoid a direct executable experiment if the official package requires normal OS activation or the real image is inaccessible.
2. Dock's hypothesis for alpha is `CODEX_HOME=<root>/profiles/alpha` and `--user-data-dir=<root>/AppData/Roaming/Codex-alpha` on Windows or `--user-data-dir=<root>/Library/Application Support/Codex-alpha` on macOS. Use beta in both paths for the second launch. Supply variables only to the disposable invocation. Start with empty sandbox data directories. Never place test arguments in an existing shortcut or change system-wide variables.
3. Verify two different **main-window PIDs** and distinct disposable data roots actually in use. Use trusted local OS file-activity metadata filtered to the disposable processes and paths, or equivalent direct evidence that each PID uses its own expected roots. Check the disposable OS user's default store locations for unexpected fallback activity without inspecting file contents. Corroborate each window and PID using an OS window inspector and the disposable session. Record only aliases such as `alpha`, `beta` and `<sandbox>`, expected-versus-observed root use and sanitized PID/window IDs. A passed-through flag, created directory, or extra helper PID is insufficient.
4. If either setting is ignored in the direct-launch experiment, writes fall back to shared/default locations, or the instances consolidate, record **FAIL / NO-GO** for isolation and **BLOCKED** for dependent activation scenarios. If evidence is unavailable or the OS prohibits the experiment, record **BLOCKED** and stop before activation. If a different supported isolation mechanism works but Dock cannot attribute the process, record that separate limitation and stop. Sign in manually with disposable accounts only once isolation is verified.

## Build the activation harness

```sh
cargo build --locked --release -p codex-dock-core --features synthetic-tests --bin activation-smoke
```

Use the native Rust host and run from the repository root (`rustc -vV` reports the host). The harness requires `--confirm-disposable` and explicit `base` and `profiles` paths. It can focus existing windows using Dock's standard process/managed-profile checks, which inspect candidate process metadata and existing managed account metadata. It cannot spawn Desktop or grant permissions. If it refuses because it cannot attribute the installed build, record the refusal and the sanitized reason. Keep the production gates in effect throughout.

## Exercising activation

Windows:

```powershell
.\target\release\activation-smoke.exe --confirm-disposable "$root\base" "$root\profiles" alpha
```

macOS:

```sh
./target/release/activation-smoke --confirm-disposable "$root/base" "$root/profiles" alpha
```

Use the corresponding scenario IDs in the evidence checklist. Repeat applicable scenarios for both accounts. On every successful result, independently confirm the foreground PID and the expected account window. Confirm process count and PIDs did not increase. A printed success without the expected foreground window is a failure.

1. Activate alpha while beta and the unrelated editor are open. Only alpha may become active. Repeat for beta. Repeat with the target already focused; no extra process may appear.
2. Minimize the target, then activate it. Hide it as well (macOS Hide; Windows hidden-window behavior only if reproducible through the official UI). It must become visible and focused, or return an explicit failure. No success may be inferred from a restore request alone. On Windows, test a window on another virtual desktop as well: cloaked windows cannot be reported as visible foreground windows.
3. Open two ordinary windows within one account/PID. An already-focused eligible window should be reused; otherwise the first visible native-order eligible window may be selected. Neither another account nor a dialog/helper may be targeted. On macOS, identical bounds across multiple AX/Quartz windows are intentionally indeterminate; separate their geometry before retrying. Also test a target on another Space: success requires on-screen Quartz corroboration, otherwise expect refusal.
4. Open a modal dialog or sheet in the target. Activation must refuse or fail verification, never report a dialog as the account window. Close the dialog and retry.
5. Request a nonexistent/wrong account, or close its main window while its process remains alive. Expect refusal without launch. Leave an unmanaged official instance open and verify unknown-process discovery blocks activation.
6. Terminate the disposable target during enumeration/activation, repeating enough times to exercise the race. Expect failure when ownership/focus cannot be verified; never another account, a stale window, or a replacement unrelated PID.
7. Windows: activate from a background terminal without a recent user foreground grant, and from a different integrity level if safely available in the disposable session. Foreground-lock/UIPI restrictions must produce failure unless the correct HWND is independently confirmed foreground. No input-queue attachment, simulated input, or foreground-lock changes are permitted.
8. macOS: begin without Accessibility permission; expect a clear error and no automatic permission prompt. Grant access to the exact smoke binary manually in System Settings and rerun; revoke it and rerun. Permission/AX metadata failures must be errors. No image capture or title access is requested. If OS policy withholds the required Quartz owner/bounds metadata, expect refusal and record the restriction; the harness never requests Screen Recording permission automatically.
9. Retry failed activation repeatedly. Check that both original desktop PIDs and total desktop-process count remain unchanged. Record any expected refusals with the independent foreground observation. Production gate checks belong in the CI evidence.

## Results and cleanup

Capture the exact Dock commit, the installed build and observed results in the [evidence checklist](desktop-evidence-checklist.md). CI may demonstrate policy, compilation and probe behavior on non-OpenAI fixtures. Keep it outside official-runtime reports.

Quit only the manually started disposable instances and confirm their exact PIDs exited. Remove only the disposable sandbox and restore permissions granted for the test. Keep the production gates disabled.
