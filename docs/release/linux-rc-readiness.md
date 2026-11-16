# Linux release candidate readiness — 2026-11-16

## Executive summary and scope

Verdict: **NO-GO for a public Linux release candidate**. Build and synthetic
safety checks provide useful evidence, but independent graphical validation,
license approval, final redistribution notices, and approval settings remain
release blockers. This is a release-readiness contribution, not a production
support declaration or authorization to publish.

Repository: ASHEN-IX/codex-dock. `git fetch origin` verified remote `main` at the
expected baseline **99852d7efc8abbc839923f10eac678cc9664392c** before work.
Both Rust packages, Tauri and Arch metadata currently say **0.1.0**.
Work is isolated on `codex/linux-rc-readiness`; the original checkout's dirty
launch changes were neither included nor modified. No application Rust source,
Windows/macOS production capability gate, T3 feature, or real profile was changed.

The final submission SHA and immutable CI links are recorded in the submitted PR
and the exported copy of this report after the branch is frozen. A Git commit
cannot contain its own SHA; the repository copy records the tested source and the
exported report records the exact final commit, without a self-referential amend.

## Blockers and severity

- **P0 / distribution:** no root LICENSE or owner-approved grant. Owner must
  approve license/copyright details and rights to contributions/artwork. This PR
  recommends Apache-2.0, compares MIT, and deliberately selects neither.
- **P0 / redistribution:** Cargo and AppImage native dependency notices have not
  received final review. The generated all-target inventory has 448 registry
  packages, including MPL-2.0 and Unicode-3.0; 35 lack top-level notice texts in
  their crate archive. Native AppImage libraries need a separate source/notice
  inventory. Draft notice generation is not compliance clearance.
- **P1 / runtime:** no independent GUI evidence for first launch, tray, relaunch,
  KDE restoration, two actual Codex Desktop windows, cleanup, interrupted launch,
  or X11 versus Wayland. Compilation, extraction, and headless mocks do not close
  this blocker. Never run these checks against the maintainer's active sessions.
- **P1 / approval:** reading GitHub's `release-review` environment returned HTTP
  404. Protection is not established. An administrator must configure required
  maintainer reviewers and prevent self-review. The draft job now checks these
  settings and fails closed. No repository settings were changed here.
- **P1 / security acceptance:** cargo-audit reports zero blocking vulnerabilities
  and six warnings: GLib unsoundness (RUSTSEC-2024-0429), four unmaintained crates,
  and yanked async-executor 1.14.0. No advisory suppression was added. Maintainer
  must review reachability/remediation or explicitly accept documented risk.
- **P1 / final artifacts:** each claimed distribution must install and remove the
  final CI-built packages, then receive GUI validation. Baseline artifacts and
  local-host artifacts are separately identified below; they are not equivalent.
- **P2 / test reliability:** launch tests can observe host Codex processes, and a
  parallel namespace run showed synthetic-child teardown timing interference.
  The serial, unprivileged PID-namespace suite passed. Preserve both failures and
  the passing run; do not weaken production process discovery to make tests green.
- **P2 / supply chain:** Tauri CLI is pinned to 2.12.4, but its downloaded
  AppImage helpers and Rust `stable` are not independently checksum-pinned by
  this repository. Release signing/provenance and an SBOM remain follow-up work.

## Narrow release-readiness fixes

- Declare sqlite3 and Ayatana AppIndicator explicitly for Debian; declare GTK3,
  WebKitGTK 4.1, SQLite and AppIndicator for RPM. Tauri also derives ELF library
  dependencies; declarations alone do not prove availability on every distro.
- Pin Linux packaging actions, disable persisted checkout credentials, retain
  review packages for 14 days, pin Tauri CLI, and give package jobs a timeout.
- Add an archive allowlist, checksum/mode validation, and a disposable file-only
  installer lifecycle check to Linux packaging CI. It never launches the app.
- Add Arch wrapper builds using a digest-pinned disposable base-devel image.
- Draft release now builds all five formats, checks every collected SHA-256 before
  upload and again before draft creation, requires a root license file and a
  protected maintainer review environment. Only draft creation has contents write;
  actions read is required for environment inspection. No publish step exists.
- Lock `make release` dependency resolution, verify Arch version consistency,
  prepare dependency inventory/notices, and document removal and rollback limits.

## Linux package compatibility matrix

All results below are **x86_64 only**. ARM64 is a declared Arch architecture,
not tested support. No distribution/compositor receives a runtime support claim.

- **Portable tar.gz:** baseline Ubuntu 22.04 CI build and local Garuda/Arch build
  succeeded. Exact archive allowlist, SHA-256, two executable modes, desktop entry,
  five icon sizes, disposable installation, simulated upgrade, executable rollback,
  removal and profile sentinel preservation passed. Installed files live beneath
  the selected prefix (default ~/.local). GTK3/WebKitGTK/SQLite/AppIndicator and
  Codex executables remain host dependencies. Host ABI limits apply.
- **AppImage:** baseline Ubuntu 22.04 CI build succeeded. Extraction in a clean
  Ubuntu container succeeded; AppRun is executable, launcher/icon and bundled
  native libraries are present. No account database or auth.json was found by the
  filename inspection. Actual GUI launch, FUSE path, no-FUSE execution and native
  library license clearance remain unverified. Local bundling is tracked in the
  evidence addendum; a local Arch build cannot establish an Ubuntu glibc baseline.
- **Debian:** baseline Ubuntu 22.04 CI package built; inspection and disposable
  apt installation/removal are tracked in the evidence addendum. Tauri installs
  /usr/bin/codex-dock, a desktop entry, and hicolor icons; it does not include the
  separate CLI. This PR adds missing explicit runtime dependencies. Ubuntu 24.04,
  Debian and derivatives need their own final-package install/GUI runs.
- **RPM:** baseline Ubuntu 22.04 CI package built; Fedora disposable dependency
  resolution, install/removal and library checks are tracked in the addendum.
  Tauri package contains the GUI shell, launcher and icons, not the standalone
  CLI. Fedora and openSUSE dependency names/ABI compatibility need separate checks.
- **Arch pkg.tar.zst:** local makepkg wrapper built successfully with a concrete
  portable archive checksum and inspected .PKGINFO/contents. /usr/bin contains
  Dock and CLI; /usr/share contains launcher and five icons. Declared dependencies
  are gtk3, webkit2gtk-4.1, sqlite and libayatana-appindicator. Disposable pacman
  installation/removal is tracked in the addendum. No package hooks migrate data.

These are build/inspection results, not statements that all formats were installed
successfully. AppImage/native package GUI behavior remains unverified even after
a package-manager transaction succeeds.

## Test evidence and CI provenance

Baseline (immutable source 99852d7):

- [CI: passed](https://github.com/ASHEN-IX/codex-dock/actions/runs/43904445271).
- [Security: passed](https://github.com/ASHEN-IX/codex-dock/actions/runs/43904445036).
- [Linux packages: passed](https://github.com/ASHEN-IX/codex-dock/actions/runs/43904445024).
  Downloaded `codex-dock-linux-ubuntu-22.04` for isolated distribution inspection.

Local evidence is under `/home/ashenix/.cache/codex-dock-rc-evidence/`, outside
release staging. This path contains synthetic logs and generated notice material,
not real credentials. Preserve it for review; CI artifacts expire after 14 days.
Local checks completed:

- cargo fmt, frontend JavaScript syntax, desktop evidence fixtures (jsonschema
  4.26.0 in a dedicated venv), Python compilation, shell syntax and actionlint
  1.7.12 passed. The initial fixture attempt needed the missing jsonschema module;
  the rerun in the venv passed.
- cargo clippy --locked --workspace --all-targets -- -D warnings passed.
- cargo build --locked --release --workspace passed. Package collectors select
  only Dock and CLI, not Cargo's synthetic mock-desktop or activation-smoke tools.
- cargo test --locked --workspace -- --test-threads=1 passed (255 tests, 26 suites) in a private PID
  namespace with effective/inheritable/ambient capabilities removed. Launch
  discovery therefore could not inspect or interfere with active host sessions.
- Release production capability test passed; 20 release activation-policy tests
  passed. The opt-in activation-smoke harness was compile-checked separately;
  production/package build commands never enable synthetic-tests or all-features.
- Disposable CLI profiles alpha/beta were created. A fake CLI aborted device
  login without writing credentials; missing Desktop produced a clear executable
  diagnostic; production repair refused mutation. This is a fake terminal login
  test, not an authenticated GUI workflow or official Desktop compatibility test.
- Refreshed cargo-audit 0.22.2 binary checksum matched the pinned workflow value;
  audit reported zero blocking findings and six warnings. See licensing and
  dependency review documents for remaining acceptance work.

Reproduce the namespace test without enabling synthetic production features:

```sh
unshare --user --map-current-user --keep-caps --pid --fork --mount-proc \
  setpriv --bounding-set=-all --inh-caps=-all --ambient-caps=-all \
  cargo test --locked --workspace -- --test-threads=1
```

No actual GUI validation was performed in this audit. The logged-in host has a
Wayland session and Xwayland display, but sharing its display and session bus
would violate the requested runtime isolation. A headless container is not an
independent KDE/X11 or KDE/Wayland session.

## Licensing status

See [licensing.md](licensing.md) and [Cargo inventory](cargo-license-inventory.tsv).
The notice generator prepares full upstream texts for review without assigning a
root license. It does not prove all native-library obligations are satisfied.
No LICENSE file, project license metadata, or artwork licensing claim is added.

## Installation, upgrade, uninstall and rollback

The archive checker installs into a new disposable HOME and leaves synthetic
.codex, .codex-profiles and .config/Codex-* content, modes and store symlinks
unchanged. A second install backs up two distinct simulated older executables;
copying them back restores their exact hashes. Removal deletes only app-owned
files; backups and data remain. No account login, database repair or migration
runs from the installer. This is not a cross-version data compatibility test.

There is no independently validated previous release to downgrade from/to.
Preserve trusted old binaries/packages and coherent data backups before an actual
upgrade. Quit Dock and Desktop before switching packages; restoring binaries does
not undo data changes performed by Desktop. Follow [INSTALL.md](../../packaging/INSTALL.md)
for package removal and explicit downgrade commands.

## Remaining manual Linux runtime tests

Use a disposable VM account with no bind mounts of real profile directories, its
own HOME/XDG directories, bus, display, and process namespace. Record distro,
kernel, glibc, package hash, Dock commit, Desktop/CLI versions and provenance,
KDE Plasma version, display protocol, and redacted outcomes. Test X11 and Wayland
in separate sessions; Xwayland on a Wayland desktop is not a native X11 result.

1. Install each final format and launch from its installed desktop entry. Check
   icon, window sizing, dependency diagnostics and CLI discovery from the GUI PATH.
2. Create two disposable profiles, exercise device-code display/cancellation and
   error recovery using a fake CLI only. Do not enter real credentials. Authenticated
   login remains unverified unless a separately authorized disposable test account
   is supplied; fake credentials cannot validate a server login.
3. With an approved official Desktop install, prove two windows receive separate
   profile homes, cookies/config and intended chat-store links. Record actual
   window/process ownership; mock processes alone cannot establish this behavior.
4. On KDE X11 and KDE Wayland separately, minimize each window, restore the
   selected existing window, and prove no duplicate process/window appears.
   Exercise missing qdbus6/wmctrl/xdotool and unavailable compositor interfaces.
5. Close dashboard to tray, refresh tray, invoke Open Dock, launch Dock twice,
   quit through tray, and relaunch. Confirm only one tray instance remains and
   no Dock-owned login/polling helpers survive exit. Preserve existing Desktop
   windows unless explicit shutdown is intended.
6. Interrupt launch before spawn, during launcher handoff, and after startup
   record creation. Verify conservative retry/deduplication and recovery without
   deletion of data or blind clearing of uncertainty records.
7. Install a distinguishable newer candidate over the previous candidate; compare
   profile/store hashes, links and permissions before/after. Roll back and remove
   through each package manager; verify real installed paths and data preservation.
8. Repeat with missing SQLite, CLI, Desktop, AppIndicator and activation tools.
   Record actionable errors and distinguish missing host dependencies from bugs.

## Proposed version and release notes

Propose **v0.1.0-rc.1**, subject to owner approval and closure of all P0/P1 blockers.
Do not change current 0.1.0 versions or create/push a tag in this contribution.
When approved, update both manifests, root Cargo.lock, Tauri config and Arch
pkgver together; Arch represents this prerelease as 0.1.0rc.1.

Draft notes: “First Linux release candidate for independent testing. Provides
AppImage, Debian, RPM, portable archive and Arch packages with SHA-256 checksums.
Adds explicit Linux runtime dependencies, isolated installer validation and
maintainer-reviewed draft distribution. No new major application functionality.
Codex Desktop and CLI are installed separately. Back up profile/chat data before
updating. Shared-store repair remains disabled. Windows/macOS production launch,
activation and shared-store gates remain unchanged. X11/Wayland and distribution
support claims require the accompanying completed manual validation record.”

## Release integrity and independent review

Portable content is explicitly allowlisted; its only executables are Dock, CLI,
and installer. Native packages and extracted AppImage were inspected separately.
No personal profiles, credentials, API keys, account exports or test login data
were intentionally staged. Filename/content checks are bounded evidence, not a
proof that every possible secret is absent from arbitrary bytes. Review final
CI artifact manifests, generated native licenses, and repository secret-scanning
results before publishing. No synthetic capability bypass is enabled by release
commands. Source and package installers contain no automatic repair/migration.
No merge, tag, release dispatch, release creation or publication was performed.

## Distribution evidence addendum (local audit)

- All three local Tauri formats built successfully on Garuda/Arch (glibc host ABI).
  This is independent build evidence, not Ubuntu runtime compatibility. Tauri
  emitted a bundle-type patch warning during a second bundle-only invocation;
  no updater plugin is used, but clean final CI builds must be inspected.
- Baseline Debian package installed with apt on a clean Ubuntu 22.04 container,
  resolved its ELF libraries, and removed cleanly while preserving a disposable
  profile sentinel: `DEBIAN_INSTALL_REMOVE_PASS`.
- Arch wrapper also built in the pinned base-devel container, validating the new
  CI helper independently of the host makepkg installation.
- Baseline RPM installed on disposable Fedora 45, resolved its ELF libraries,
  and removed cleanly while retaining the profile sentinel:
  `RPM_INSTALL_REMOVE_PASS`. This is Fedora evidence, not an openSUSE result.
- Local Arch package installed and removed on disposable Arch with library checks
  and sentinel preservation: `ARCH_INSTALL_REMOVE_PASS`.
- Revised Debian/RPM metadata/content inspection confirmed their explicit runtime
  declarations, launcher/icons, /usr/bin placement, and absence of the CLI in
  Tauri native bundles. Modified metadata still requires final Ubuntu CI package
  installation; local host-built binaries have a newer glibc requirement.
- Negative archive cases containing auth.json, traversal, or symlinks were rejected.
- PR [#17](https://github.com/ASHEN-IX/codex-dock/pull/17) is submitted for independent
  review. Initial PR CI is running: [CI](https://github.com/ASHEN-IX/codex-dock/actions/runs/43907844753),
  [Security](https://github.com/ASHEN-IX/codex-dock/actions/runs/43907844516),
  [Linux packages](https://github.com/ASHEN-IX/codex-dock/actions/runs/43907844687),
  [Cross-platform](https://github.com/ASHEN-IX/codex-dock/actions/runs/43907844747).
  Links for the frozen final commit and completed results appear in the export.
