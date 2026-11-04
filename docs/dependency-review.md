# Dependency maintenance baseline

The initial audit for this CI contribution used cargo-audit 0.22.2 against the
root workspace lockfile on 2026-11-04. It reported zero blocking vulnerability
findings and six additional warnings. CI preserves these warnings in its job
summary and JSON artifact; no advisory has been ignored to make a check pass.

| Package | Warning | Tracking reference |
| --- | --- | --- |
| glib 0.18.5 | Unsound iterator implementation | [RUSTSEC-2024-0429](https://rustsec.org/advisories/RUSTSEC-2024-0429.html) |
| proc-macro-error 1.0.4 | Unmaintained | [RUSTSEC-2024-0370](https://rustsec.org/advisories/RUSTSEC-2024-0370.html) |
| zvariant 5.15.1 | Unmaintained | [RUSTSEC-2026-0370](https://rustsec.org/advisories/RUSTSEC-2026-0370.html) |
| zbus_names 4.3.5 | Unmaintained | [RUSTSEC-2026-0371](https://rustsec.org/advisories/RUSTSEC-2026-0371.html) |
| zvariant_derive 5.15.1 | Unmaintained | [RUSTSEC-2026-0372](https://rustsec.org/advisories/RUSTSEC-2026-0372.html) |
| async-executor 1.14.0 | Yanked release | Review its registry notice and compatible replacement |

This snapshot is not a finding that this application reaches an affected code
path, and it is not a reason to dismiss the advisories. Trace dependencies with
`cargo tree -i <package>`, review upstream fixes, and test supported replacements.
The GTK3/Tauri dependency stack must be considered before attempting a standalone
major-version update to GLib. Keep dependency remediation separate from the
mechanical CI baseline so both changes can be reviewed clearly.

Re-run the audit before a release. New vulnerability entries fail the job;
informational warnings remain visible and require a maintainer decision.
