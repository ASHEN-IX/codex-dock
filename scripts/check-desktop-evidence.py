#!/usr/bin/env python3
"""Validate synthetic desktop evidence fixtures and exercise rejection cases."""

from copy import deepcopy
import json
from pathlib import Path
import unittest

from jsonschema import Draft202012Validator, FormatChecker


ROOT = Path(__file__).resolve().parents[1] / "docs" / "testing"


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


class DesktopEvidence(unittest.TestCase):
    def test_report_fixtures(self):
        schema = read_json(ROOT / "desktop-evidence.schema.json")
        Draft202012Validator.check_schema(schema)
        validator = Draft202012Validator(schema, format_checker=FormatChecker())
        windows, macos, baseline = [
            read_json(ROOT / "fixtures" / f"{name}.json")
            for name in ("windows", "macos", "not-executed")
        ]
        for name, report in (("Windows", windows), ("macOS", macos), ("NOT EXECUTED", baseline)):
            with self.subTest(name=name):
                validator.validate(report)
                print(f"Accepted {name}", flush=True)

        def check(name, source, changes, valid=False):
            report = deepcopy(source)
            for path, value in changes:
                *parents, field = path
                target = report
                for parent in parents:
                    target = target[parent]
                target[field] = value
            with self.subTest(name=name):
                if valid:
                    validator.validate(report)
                else:
                    self.assertFalse(validator.is_valid(report))
                print(f"{'Accepted' if valid else 'Rejected'} {name}", flush=True)

        isolation = ("scenarios", "profile_isolation")
        verdict = ("isolation_verdict",)
        failed = {"status": "FAIL", "reason": "Both instances used the same store.", "evidence": []}
        blocked = {"status": "BLOCKED", "reason": "Store observations were unavailable.", "evidence": []}
        check("isolation FAIL with activation PASS", windows, [(isolation, failed), (verdict, "NO-GO")])
        check("isolation FAIL with UNVERIFIED", baseline, [(isolation, failed)])
        check("isolation FAIL with NO-GO", baseline, [(isolation, failed), (verdict, "NO-GO")], True)
        check("isolation BLOCKED with activation PASS", windows, [(isolation, blocked), (verdict, "UNVERIFIED")])
        check("isolation BLOCKED with NO-GO", baseline, [(isolation, blocked), (verdict, "NO-GO")])
        check("isolation BLOCKED with UNVERIFIED", baseline, [(isolation, blocked)], True)
        check("NOT EXECUTED isolation with activation PASS", baseline, [
            (("scenarios", "account_focus"), windows["scenarios"]["account_focus"]),
        ])
        check("NOT EXECUTED with GO", baseline, [(verdict, "GO")])
        check("Windows with a macOS probe", windows, [(("probe_samples",), macos["probe_samples"])])
        check("Windows with a mixed probe array", windows, [
            (("probe_samples",), windows["probe_samples"] + macos["probe_samples"]),
        ])
        check("macOS with a Windows probe", macos, [(("probe_samples",), windows["probe_samples"])])
        check("Windows probe with macOS permission", windows, [
            (("probe_samples", 0, "activation_permission"), macos["probe_samples"][0]["activation_permission"]),
        ])
        check("Windows x64 host with ARM64 host sample", windows, [
            (("probe_samples", 0, "os", "architecture"), "arm64"),
        ])
        for product in ("chatgpt-classic", "codex-cli"):
            check(f"{product} with GO", windows, [(("product",), product)])
        for index, method in enumerate(("official provenance", "product UI", "running identity")):
            items = windows["scenarios"]["identity"]["evidence"]
            check(f"GO without {method}", windows, [
                (("scenarios", "identity", "evidence"), items[:index] + items[index + 1:]),
            ])
        check("GO with one isolation PID", windows, [(isolation + ("evidence", 0, "pids"), [101])])
        check("GO with probe-only isolation evidence", windows, [
            (isolation + ("evidence", 0, "method"), "local-os-metadata"),
        ])
        check("known execution mode without process evidence", baseline, [(("execution_mode",), "native")])
        check("known execution mode with null evidence", windows, [(("execution_evidence",), None)])
        check("Windows with translated execution", windows, [(("execution_mode",), "translated")])
        check("Windows ARM64 claiming native x64 execution", windows, [
            (("host",), "windows-arm64"),
            (("probe_samples", 0, "os", "architecture"), "arm64"),
        ])
        check("Windows ARM64 with observed x64 emulation", windows, [
            (("host",), "windows-arm64"),
            (("probe_samples", 0, "os", "architecture"), "arm64"),
            (("execution_mode",), "emulated"),
        ], True)
        check("macOS execution claim with Windows evidence", macos, [
            (("execution_evidence", "method"), "windows-process-machine"),
        ])
        check("Rosetta claim with an ARM64 running process", macos, [
            (("execution_evidence", "process_architecture"), "arm64"),
        ])
        for field in ("command_line", "environment", "account_id", "conversation", "token"):
            check(f"extra process field {field}", windows, [
                (("probe_samples", 0, "discovery", "processes", 0, field), "synthetic"),
            ])


if __name__ == "__main__":
    unittest.main()
