#!/usr/bin/env python3
"""Exercise macOS probe path handling with synthetic files only."""

import importlib.util
from pathlib import Path
import plistlib
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "desktop_probe", Path(__file__).with_name("desktop-compatibility-macos.py")
)
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class ProbePaths(unittest.TestCase):
    def test_redacts_standard_and_data_volume_home_paths(self):
        for prefix in ("/Users/", "/System/Volumes/Data/Users/", "/users/"):
            with self.subTest(prefix=prefix):
                self.assertEqual(
                    probe.protect_path(f"{prefix}test-person/Apps/Test.app"),
                    f"{prefix}<user>/Apps/Test.app",
                )
        self.assertEqual(probe.protect_path("/Applications/Test.app"), "/Applications/Test.app")

    def test_discards_invalid_path_text(self):
        for path in ("relative.exe", "/Users/test-person/a\nsecret", "/Users/test-person/\udcff"):
            with self.subTest(path=path):
                self.assertIsNone(probe.protect_path(path))

    def test_bundle_resolves_only_internal_plist_and_executable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bundle = root / "Test.app"
            plist = bundle / "Contents" / "Info.plist"
            executable = plist.parent / "MacOS" / "Test"
            executable.parent.mkdir(parents=True)
            executable.touch()
            info = {"CFBundleExecutable": "Test"}
            plist.write_bytes(plistlib.dumps(info))
            self.assertEqual(probe.resolve_application(bundle), (bundle.resolve(), executable.resolve(), info))

            for invalid in ("../private", "bad\x00name", ".", ["Test"]):
                with self.subTest(executable=invalid):
                    plist.write_bytes(plistlib.dumps({"CFBundleExecutable": invalid}, fmt=plistlib.FMT_BINARY))
                    with self.assertRaises(ValueError):
                        probe.resolve_application(bundle)

            outside = root / "private"
            outside.write_bytes(plistlib.dumps(info))
            plist.unlink()
            plist.symlink_to(outside)
            with self.assertRaises(ValueError):
                probe.resolve_application(bundle)
            plist.unlink()
            plist.write_bytes(plistlib.dumps(info))
            executable.unlink()
            executable.symlink_to(outside)
            with self.assertRaises(ValueError):
                probe.resolve_application(bundle)


if __name__ == "__main__":
    unittest.main()
