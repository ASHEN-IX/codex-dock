# Contributing to Codex Dock

Bug reports, documentation, accessibility improvements, tests and code changes
are welcome. Start each focused contribution from the current `main` branch.

## Set up a development checkout

Fork this repository, clone your fork, and create a branch:

```sh
git switch main
git pull --ff-only
git switch -c fix/describe-the-change
```

Use stable Rust with the `rustfmt` and `clippy` components. Install SQLite and,
for the desktop shell, the native dependencies described in
[Tauri's Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux).
The desktop currently targets Linux; package-manager instructions for one
distribution are not a claim of support for every distribution or compositor.

```sh
make check-core  # formatting, core tests, frontend JavaScript syntax
make check      # additionally Clippy and a complete desktop/CLI build
```

Core tests use temporary directories and injected `Paths`. They require SQLite,
but do not require signing in, Codex Desktop, API keys, or real usage allowance.
Do not point tests at your own `~/.codex` or profiles. Tests that alter PATH must
remain isolated from other test processes. The root `Cargo.lock` is authoritative
for the workspace, including `src-tauri`; do not regenerate an independent
workspace under `src-tauri`.

## Before opening a pull request

- Describe the user-visible problem and the resulting behavior.
- Keep unrelated refactors and generated files out of the change. Explain
  changes to either lockfile and any unavoidable generated output.
- Add a regression test for a reproduced defect or a new behavior with material
  risk. Do not add tests that only restate implementation details.
- Run `make check`; include results and any checks you could not perform.
- For UI changes, describe keyboard behavior and provide a screenshot with
  private project names, account identifiers and tokens removed.
- For profile, login or repair changes, show that account identities stay
  separate and existing data remains recoverable, including failure paths.

Never include real `auth.json` files, tokens, cookies, chat transcripts, profile
databases, or raw private logs in an issue, PR, test fixture, or workflow artifact.
Use synthetic account identifiers and temporary data. See [SECURITY.md](SECURITY.md)
for sensitive reports and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) for conduct.

## CI and review

CI checks formatting, frontend syntax, workflow expressions, core tests, Clippy,
and desktop/CLI builds on Ubuntu 22.04 and 24.04. Security checks audit locked
dependencies and scan Rust, frontend code, Python helpers, and GitHub Actions with CodeQL.
Frontend syntax checking is not an end-to-end UI test; builds do not verify
window focusing, login, tray integration, or data migration on every desktop.

Pull requests must pass the configured checks before merging. CI never needs a
contributor's Codex credentials. Workflow changes must use read-only permissions
unless a specific job needs more, pin actions to full commit SHAs, and avoid
executing fork code with secrets or `pull_request_target` privileges.

For maintainer settings, release instructions and remaining setup decisions,
see [docs/maintaining.md](docs/maintaining.md).
