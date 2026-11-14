# T2.3 opt-in native activation smoke protocol

Status: NOT EXECUTED against official Desktop builds. All Windows/macOS production capabilities remain disabled. Synthetic CI results do not authorize changing them.

## Setup and boundaries

Use a disposable OS user/session and two disposable Codex accounts, alpha and beta. Close other Codex Desktop/ChatGPT instances so an unknown desktop process cannot invalidate the process inventory. Keep an unrelated editor open as a negative control. Never use your normal profile, share authentication files, or link profile stores.

Record OS/build/architecture, official Desktop version and executable path, the tested Dock commit, process PIDs, selected window IDs, permissions, expected/observed result, and pass/fail. Do not record credentials, chat titles, environment contents, or screenshots containing account data.

Build the harness explicitly:

```sh
cargo build --locked --release -p codex-dock-core --features synthetic-tests --bin activation-smoke
```

The binary is excluded from default builds. It only activates existing windows; it never spawns Desktop, changes permissions, repairs a store, or enables capabilities. `--confirm-disposable` is mandatory. The native test entry bypasses the activation capability only, while keeping the complete managed-process and window checks. No environment variable enables it in ordinary releases.

Choose an absolute temporary sandbox ROOT containing empty `base`, `profiles/alpha`, and `profiles/beta` directories. Pass BASE=ROOT/base and PROFILES=ROOT/profiles. No file is implicitly read from the normal account home by the harness. Native discovery still inspects the system process inventory and refuses unknown desktop candidates.

## Starting official Desktop manually

Determine the real executable from the installed official build; do not substitute the standalone CLI. Start each account directly with CODEX_HOME set to ROOT/profiles/ACCOUNT and its independent `--user-data-dir` below. Use two separate processes and confirm distinct PIDs, both command lines, and account attribution before testing. If the official runtime ignores either path or consolidates accounts into one PID, stop: this is a validation failure, not permission to focus by name or title.

Windows (PowerShell, after creating the directories):

```powershell
# Supply your disposable ROOT and verified official executable path.
$root = 'C:\absolute\disposable-sandbox'
$desktop = 'C:\absolute\official\Codex.exe'
$previousCodexHome = $env:CODEX_HOME
try {
  $env:CODEX_HOME = "$root\profiles\alpha"
  & $desktop "--user-data-dir=$root\AppData\Roaming\Codex-alpha"
  # Start beta the same way in another terminal with beta in both paths.
} finally {
  $env:CODEX_HOME = $previousCodexHome
}
```

macOS (Terminal; executable name must match the actual installed bundle):

```sh
root='/absolute/disposable-sandbox'
CODEX_HOME="$root/profiles/alpha" '/Applications/Codex.app/Contents/MacOS/Codex' \
  "--user-data-dir=$root/Library/Application Support/Codex-alpha"
# Start beta in another terminal with beta in both paths.
```

Sign in only with disposable accounts if required. Do not change production capability constants. `open -a`, broad process-name focusing, and automatic launches by Dock are not part of this protocol.

## Exercising activation

Windows:

```powershell
.\target\release\activation-smoke.exe --confirm-disposable "$root\base" "$root\profiles" alpha
```

macOS:

```sh
./target/release/activation-smoke --confirm-disposable "$root/base" "$root/profiles" alpha
```

Run each scenario for both accounts. On every successful result, independently confirm the foreground PID and the expected account window. Confirm process count and PIDs did not increase. A printed success without the expected foreground window is a failure.

1. Activate alpha while beta and the unrelated editor are open. Only alpha may become active. Repeat for beta. Repeat with the target already focused; no extra process may appear.
2. Minimize the target, then activate it. Hide it as well (macOS Hide; Windows hidden-window behavior only if reproducible through the official UI). It must become visible and focused, or return an explicit failure. No success may be inferred from a restore request alone.
3. Open two ordinary windows within one account/PID. An already-focused eligible window should be reused; otherwise the first visible native-order eligible window may be selected. Neither another account nor a dialog/helper may be targeted.
4. Open a modal dialog or sheet in the target. Activation must refuse or fail verification, never report a dialog as the account window. Close the dialog and retry.
5. Request a nonexistent/wrong account, or close its main window while its process remains alive. Expect refusal without launch. Leave an unmanaged official instance open and verify unknown-process discovery blocks activation.
6. Terminate the disposable target during enumeration/activation, repeating enough times to exercise the race. Expect failure when ownership/focus cannot be verified; never another account, a stale window, or a replacement unrelated PID.
7. Windows: activate from a background terminal without a recent user foreground grant, and from a different integrity level if safely available in the disposable session. Foreground-lock/UIPI restrictions must produce failure unless the correct HWND is independently confirmed foreground. No input-queue attachment, simulated input, or foreground-lock changes are permitted.
8. macOS: begin without Accessibility permission; expect a clear error and no automatic permission prompt. Grant access to the exact smoke binary manually in System Settings and rerun; revoke it and rerun. Permission/AX metadata failures must be errors. Screen Recording is not required because the adapter uses AX window references, not window images or titles.
9. Retry failed activation repeatedly. Check that both original desktop PIDs and total desktop-process count remain unchanged. Confirm normal production entry points still refuse Windows/macOS activation/launch with default-feature release builds.

## Result and cleanup

Keep separate implementation/CI and official-runtime verdicts. Until both operating systems have recorded passing evidence for their official build, capability status stays No. An inconclusive restricted session is not a pass. This protocol does not authorize enabling capabilities, merging, releasing, or starting T3.

Quit only the two manually started disposable instances, confirm their exact PIDs exited, then remove the disposable sandbox. Restore any permission changes made solely for this test. Do not use process-name-wide termination or delete the persistent directories of ordinary accounts.
