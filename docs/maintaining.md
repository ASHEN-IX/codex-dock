# Maintainer setup and releases

Repository files are only part of the setup. A repository administrator must
apply GitHub settings; a contribution cannot change the upstream owner's settings.

## Required decisions

- Choose and add a license, including the appropriate copyright information.
  No license file is currently present in upstream. This contribution does not
  assign a license to existing code. Resolve this before calling the repository
  a licensed open-source distribution or publishing a general release.
- Confirm who maintains the project and how to contact them privately. Consider
  adding `CODEOWNERS` when the maintainers have agreed to receive review requests.
- Enable private vulnerability reporting and give maintainers access to reports.
- Enable the dependency graph, Dependabot alerts/security updates, secret
  scanning and push protection where available.
- Keep the default Actions token read-only and disallow Actions approving PRs.
  Only the draft-release upload job needs `contents: write`.

## Protect main

Require pull requests and passing **CI gate**, resolve review conversations,
and disallow branch deletion and force pushes. Human review requirements should
match the active maintainer team: requiring a second reviewer in a solo-maintainer
fork can prevent every merge. Require an independent reviewer when one is available.

Keep status-check names stable. The `CI gate` job fails if any required quality
or Linux matrix check fails, is cancelled, or is skipped. Security jobs run
separately so findings stay visible. Review their results before merging; CodeQL
upload success means analysis completed, not that no findings exist.

Enable scheduled workflows and Dependabot after these files reach the default
branch. Forks may need Actions enabled explicitly. Dependabot config and scheduled
workflows on a feature branch alone do not activate default-branch maintenance.

## CI design

- **CI:** format, JavaScript syntax, workflow lint, core regression tests, Clippy,
  and complete release builds on Ubuntu 22.04 and 24.04. Compiled binaries are
  short-lived review artifacts, not installer packages.
- **Security:** pinned cargo-audit with a verified download checksum, plus CodeQL
  for Rust, JavaScript and Actions. Scheduled scans catch new advisories without
  a source change. Audit reports are retained briefly. Unmaintained dependency
  warnings still require review even when no known vulnerability fails the job.
  The initial findings are recorded in [dependency-review.md](dependency-review.md).
- **Draft release:** manual only, with an existing version tag. Build jobs have
  read-only permissions. A separate job downloads only package artifacts and
  creates an unpublished draft, using narrowly scoped write permission.
- Third-party actions are pinned to verified upstream commit SHAs. Dependabot
  proposes Actions updates. Review updates to pinned CLI tools and cargo-audit's
  download checksum as well; they are not covered by Actions-version updates.

No job needs a Codex login, API key, or personal data. Do not use a personal
developer machine as a runner for untrusted pull requests. The root workspace
lockfile is used for builds and auditing; `src-tauri/Cargo.lock` is not a second
workspace lockfile to build against.

## Make a release

1. Update both Cargo package versions and `src-tauri/tauri.conf.json`, then
   regenerate the root lockfile with Cargo. Run CI and review security results.
2. Manually smoke-test the desktop UI, tray, focus behavior, login and recovery
   on the distributions being claimed as supported, using disposable profiles.
3. Create and push a tag matching those versions, such as `v0.1.0`. Signed tags
   are encouraged when maintainers have signing keys configured.
4. Run **Actions → Draft release** with that tag. The tagged commit must include
   this workflow, the local Linux setup action, and the release helper scripts.
5. Review the resulting Debian package, AppImage and `SHA256SUMS`. The pipeline
   builds on Ubuntu 22.04 for an older ABI baseline, but does not promise runtime
   support for every Linux distribution, architecture or compositor.
6. Review release notes, supported platforms, remaining issues and any required
   backup steps. Publish the draft explicitly after that review.

The workflow does not create tags, automatically publish releases, run repair,
or collect real profile data. A failed rerun does not overwrite an existing
release. Delete an obsolete draft deliberately before retrying its creation.

When rolling back app binaries, retain credentials, profiles and shared chats.
Uninstall hooks and workflow cleanup must never delete user configuration or data.
