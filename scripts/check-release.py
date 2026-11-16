"""Validate versioned release input without executing code or creating tags."""
import json
from pathlib import Path
import re
import sys

if len(sys.argv) != 2 or not re.fullmatch(r"v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", sys.argv[1]):
    raise SystemExit("Expected a version tag such as v0.1.0")

version = sys.argv[1][1:]
versions = {}
for path in [Path("core/Cargo.toml"), Path("src-tauri/Cargo.toml")]:
    # Both package manifests put their version in [package]; do not pick a
    # dependency's version or require tomllib on Ubuntu 22.04's Python 3.10.
    package = re.search(r"(?ms)^\[package\]\s*\n(.*?)(?=^\[|\Z)", path.read_text())
    found = re.search(r'^version\s*=\s*"([^"]+)"', package[1], re.MULTILINE) if package else None
    if not found:
        raise SystemExit(f"Missing package version in {path}")
    versions[str(path)] = found[1]
versions["src-tauri/tauri.conf.json"] = json.loads(Path("src-tauri/tauri.conf.json").read_text())["version"]
arch_version = re.search(r"^pkgver=(.+)$", Path("packaging/arch/PKGBUILD").read_text(), re.MULTILINE)
if not arch_version:
    raise SystemExit("Missing Arch pkgver")
# Arch forbids hyphens in pkgver; map SemVer prerelease separators to dots.
expected_arch = version.replace("-", ".")
if arch_version[1] != expected_arch:
    raise SystemExit(f"Arch pkgver {arch_version[1]} does not match {expected_arch}")
for path, actual in versions.items():
    if actual != version:
        raise SystemExit(f"{path}: version {actual} does not match tag {sys.argv[1]}")
print(f"Release metadata matches {sys.argv[1]}")
