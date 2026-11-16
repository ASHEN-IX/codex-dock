# Official desktop tester evidence checklist

Copy this checklist for one installed build on one host. Use the [smoke guide](native-window-activation.md) for procedure and the [JSON Schema](desktop-evidence.schema.json) for the machine-readable report. Put each sanitized probe object in `probe_samples` in capture order. Use the JSON scenario IDs below as keys under `scenarios`.

- [ ] Record Dock commit, test date, host architecture, app version, bundle/package identity, and distribution provenance. Where relevant, record whether execution was native, translated, emulated or unverified.
- [ ] Confirm the selected main executable belongs to the official product under test and identify whether the app offers Codex. Keep every signed path observation local until reviewed.
- [ ] Replace local usernames, private directories, and default-store paths with aliases such as `<user>` and `<sandbox>`. Review all paths and text before sharing. Omit auth data, process arguments and environment values, account identifiers, email addresses, titles, content, window captures and raw OS traces. Use alpha and beta for the two disposable profiles.
- [ ] Record independent evidence for every PASS. Include the observation method, expected result, actual result, and relevant PIDs/window IDs. A negative-control PASS requires an observed safe refusal with the expected unchanged foreground and process state. Evidence from the probe alone cannot pass isolation or activation.
- [ ] Keep synthetic CI links and results in the test handoff or PR. Submit only official-runtime observations in this report.

Start with every row marked **NOT EXECUTED**. Use **PASS** when observed behavior met the expected result. Use **FAIL** for a verified mismatch. Use **BLOCKED** when a prerequisite, platform applicability, or sufficient observation prevented a decision. For an OS-specific row intentionally skipped on the other OS, use **NOT EXECUTED** and note applicability if helpful. For FAIL or BLOCKED, record the observed failure or blocker.

| JSON scenario ID | Test and evidence to capture | Status |
| --- | --- | --- |
| `identity` | Official source, explicit product choice, installed package/bundle and executable, observed running image, Codex visible in primary target. Note Classic and CLI identity controls if available. | NOT EXECUTED |
| `inventory` | Independent inventory agrees for visible main images and relevant controls. Record completeness, collisions, and permission gaps. | NOT EXECUTED |
| `profile_isolation` | Two main PIDs with distinct, independently observed profile/store use and no fallback. Map PIDs to alpha/beta. Ignored isolation settings, fallback and singleton consolidation are FAIL / NO-GO. | NOT EXECUTED |
| `account_focus` | Guide step 1. Switching either way and already-focused activation. Expected versus actual target and foreground PID/window. | NOT EXECUTED |
| `minimized_hidden` | Guide step 2. Report minimize, hide where reproducible, and virtual desktop results. | NOT EXECUTED |
| `multiple_windows` | Guide step 3. Main-window eligibility and selection, including macOS Spaces and indistinguishable bounds where applicable. | NOT EXECUTED |
| `modal` | Guide step 4. Safe modal refusal and recovery after dismissing it. | NOT EXECUTED |
| `wrong_missing_unmanaged` | Guide step 5. Wrong or missing account/window and an unmanaged official instance. | NOT EXECUTED |
| `process_churn` | Guide step 6. Target exits during enumeration/activation. Record whether the race was observed. | NOT EXECUTED |
| `windows_foreground_policy` | Guide step 7. Foreground-lock and integrity restrictions in the tested contexts. | NOT EXECUTED |
| `macos_accessibility` | Guide step 8. Denied, granted and revoked permission for the exact harness. Record any Quartz visibility restriction. | NOT EXECUTED |
| `retry_no_spawn` | Guide step 9. PIDs and instance counts across repeated failed activation. | NOT EXECUTED |

For a row covering multiple cases, list each expected and actual result in `evidence`. Use FAIL if any case violates expectations. Use BLOCKED if a case required for that row lacks sufficient evidence and no case failed. Include an explicit isolation verdict in `isolation_verdict`. Use `NO-GO` for observed isolation failure, `GO` only for an evidenced isolation PASS, and `UNVERIFIED` otherwise. GO permits proceeding with the disposable smoke protocol only.
