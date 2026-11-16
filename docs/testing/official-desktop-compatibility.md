# T2.4A official desktop compatibility targets

All Windows and macOS Dock production launch and activation gates stay disabled. The product information below determines what to test. Dock compatibility requires observations from each installed build and architecture.

| Product | How to identify the installed product | Dock test disposition |
| --- | --- | --- |
| Current ChatGPT desktop with Codex | Obtain it through the official download or official Codex updater. Correlate the installed bundle or package, signing information, and actual running image with the Codex view in the app. An upgraded install can present the Codex icon. | Primary target. Windows x64, Windows ARM64 when a compatible build is available, macOS Apple Silicon and macOS Intel each need their own report. All official-runtime scenarios are NOT EXECUTED at baseline. |
| Former standalone Codex desktop | Use provenance, installed version, signing metadata and observed product UI to distinguish a retained pre-transition build from an updated build. Historical Dock fallbacks expect names such as `Codex.exe` or `Codex.app`. | Optional legacy comparison when a trusted build is already installed. Results apply to that build only. |
| ChatGPT Classic | The previous ChatGPT desktop app can remain alongside the new app. Use provenance, package or bundle, and UI to distinguish the two even if executable names overlap. | Negative control for target selection. Out of scope for managed launch and activation. |
| Standalone Codex CLI | Terminal product. The command on PATH can be a shim or a native `codex` / `codex.exe`; a desktop bundle may also contain a CLI executable. | Negative control for target selection. Out of scope for desktop launch and activation. |

OpenAI documents the update from standalone Codex to ChatGPT with Codex, the coexistence of Classic, and macOS 14+ installers for both Apple Silicon and Intel. Check official distribution and the installed executable architecture on Windows. Older Windows help pages and storefront architecture labels alone do not establish the architecture of the current app's running image. [Migration guide](https://help.openai.com/en/articles/20001276-moving-to-the-new-chatgpt-desktop-app) · [macOS requirements](https://help.openai.com/en/articles/9275200-downloading-the-chatgpt-macos-app) · [Official downloads](https://chatgpt.com/download/) · [Codex CLI](https://github.com/openai/codex)

## Dock audit at the T2.4A baseline

| Area | Behavior and compatibility risk |
| --- | --- |
| [Executable discovery](../../core/src/tool.rs) | `CODEX_DESKTOP_BIN` wins. Otherwise Dock takes the first executable candidate on PATH or in hard-coded locations, then falls back to a legacy path. Windows guesses unpackaged locations and does not resolve installed package registrations. macOS guesses bundle **and** inner executable names. PATH can resolve a CLI ahead of the intended app. |
| [Launch resolution and spawn](../../core/src/platform/launch.rs) | A macOS bundle override uses a simple XML lookup, bundle-stem matching, then the first executable in `Contents/MacOS`. The executable and CLI checks do not verify bundle ID, package family, publisher or signature. When enabled on a supported path, Dock prepares profile data and spawns the binary directly with both `CODEX_HOME` and `--user-data-dir`. The current installed app may use a different entry point, ignore one or both settings, or route launch through a singleton. |
| [Shared process classifier](../../core/src/platform/process.rs) | Candidate names are `codex`, `chatgpt` and `codex-desktop` (including Windows `.exe`) plus a configured basename and selected macOS path hints. Helper flags and CLI arguments/path patterns are exclusions. A recognized bundle substring, a legacy Codex install directory, or a user-data flag can yield the internal label `ConfirmedDesktop`. None validates publisher, package, product generation, or actual store use. A name reused by Classic or a different binary can match, and a differently named main process can be missed. |
| [Windows process adapter](../../core/src/platform/process/windows.rs) | Toolhelp snapshot filtering happens before image and command-line queries. Attribution uses a matching managed user-data argument; it has no `CODEX_HOME` evidence. Command-line or path lookup can fail even when the snapshot reports complete. |
| [macOS process adapter](../../core/src/platform/process/macos.rs) | Uses executable path, candidate name/bundle checks, and process argument/environment inspection. Attribution accepts a matching managed path and rejects conflicting values when both are present. Path/name visibility and argument permissions can make candidates indeterminate. |
| [Activation smoke harness](../../core/src/bin/activation-smoke.rs) | Explicitly opt-in and only activates existing managed windows. It uses Dock's existing process inspection and attribution, which can establish that a process advertised a path. Independent evidence is required that the runtime actually used the intended separate stores. |

## Open compatibility questions

- Exact bundle IDs, package identities, signatures, image paths, versions and architecture slices across fresh installs and upgrades.
- Whether Windows ARM64 has a native official build, an official x64 build running under emulation, or no compatible build on the test host.
- Whether official startup supports per-instance data roots and keeps two independent processes in one OS session, including where profile, login and local app state land.
- Whether process visibility and Dock attribution stay reliable with helpers, Classic, the CLI, restricted permissions and process churn.
- Whether foreground activation is independently verifiable on each platform across minimized/hidden windows, modal dialogs, virtual desktops or Spaces, and permission boundaries.

Follow the [native smoke guide](native-window-activation.md) and submit the [evidence checklist](desktop-evidence-checklist.md) and [report](desktop-evidence.schema.json).
