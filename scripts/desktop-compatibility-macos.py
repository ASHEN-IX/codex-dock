#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Read-only inventory of a tester-selected macOS application bundle."""

import argparse
import ctypes
import json
from pathlib import Path
import plistlib
import re
import subprocess
import sys
from xml.parsers.expat import ExpatError


def system_tool(*command, signing_details=False):
    try:
        result = subprocess.run(command, capture_output=True, timeout=20, check=False)
    except (OSError, subprocess.TimeoutExpired):
        return None, ""
    output = result.stderr if signing_details else result.stdout
    return result.returncode, output.decode(errors="replace").strip()


def protect_path(path):
    value = str(path)
    if not value.startswith("/") or re.search(r"[\x00-\x1f\x7f\ud800-\udfff]", value):
        return None
    return re.sub(r"^(/(?:System/Volumes/Data/)?Users/)[^/]+", r"\1<user>", value,
                  flags=re.IGNORECASE)


def plist_text(info, key):
    value = info.get(key)
    return value if isinstance(value, str) and value.strip() else None


def application_metadata(bundle, executable, info):
    _, signing = system_tool("/usr/bin/codesign", "-dv", "--verbose=4", str(bundle),
                             signing_details=True)
    team = next((line.removeprefix("TeamIdentifier=") for line in signing.splitlines()
                 if line.startswith("TeamIdentifier=")), None)
    signer = next((line.removeprefix("Authority=") for line in signing.splitlines()
                   if line.startswith("Authority=")), None)
    verify, _ = system_tool("/usr/bin/codesign", "--verify", "--strict", str(bundle))
    file_result, image_format = system_tool("/usr/bin/file", "-b", str(executable))
    architectures = sorted(set(re.findall(r"\b(?:arm64e?|x86_64h?|i386)\b", image_format)))
    return {
        "executable_path": protect_path(executable),
        "architectures": architectures if file_result == 0 and "Mach-O" in image_format else [],
        "declared_name": plist_text(info, "CFBundleDisplayName") or plist_text(info, "CFBundleName"),
        "application_id": plist_text(info, "CFBundleIdentifier"),
        "package_lookup": "bundle-plist",
        "package_id": None,
        "publisher": team if team != "not set" else None,
        "signer": signer,
        "signature_status": None if verify is None else ("valid" if verify == 0 else "unverified"),
        "version": plist_text(info, "CFBundleShortVersionString"),
        "package_or_build_version": plist_text(info, "CFBundleVersion"),
    }


def inventory(executable):
    result = {
        "scope": "visible-pids-and-executable-paths",
        "completeness": "unavailable",
        "unresolved_paths": None,
        "processes": [],
    }
    try:
        libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
        list_pids = libproc.proc_listallpids
        list_pids.argtypes = (ctypes.c_void_p, ctypes.c_int)
        pid_path = libproc.proc_pidpath
        pid_path.argtypes = (ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32)
    except (OSError, AttributeError):
        return result

    estimate = list_pids(None, 0)
    if estimate <= 0:
        return result
    pids = (ctypes.c_int * (estimate * 2 + 64))()
    count = list_pids(pids, ctypes.sizeof(pids))
    if count <= 0:
        return result

    unresolved = 0
    path_buffer = ctypes.create_string_buffer(4096)
    for pid in sorted(set(pids[:count]) - {0}):
        if pid_path(pid, path_buffer, len(path_buffer)) <= 0:
            unresolved += 1
            continue
        path = Path(path_buffer.value.decode("utf-8", errors="surrogateescape"))
        if path.name.casefold() != executable.name.casefold():
            continue
        protected_path = protect_path(path)
        if protected_path is None:
            unresolved += 1
        result["processes"].append({
            "pid": pid,
            "identity": (
                "path-unavailable" if protected_path is None else
                "selected-executable" if path == executable else "same-name-different-path"
            ),
            "executable_path": protected_path,
            "profile_attribution": "unverified",
        })
    result["completeness"] = "partial" if unresolved or count >= len(pids) else "complete"
    result["unresolved_paths"] = unresolved
    return result


def accessibility_permission():
    try:
        library = ctypes.CDLL(
            "/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices"
        )
        trusted = library.AXIsProcessTrusted
        trusted.argtypes = ()
        trusted.restype = ctypes.c_bool
        return "available" if trusted() else "unavailable"
    except (OSError, AttributeError):
        return "unverified"


def resolve_application(path):
    if not path.is_absolute() or path.suffix.lower() != ".app" or protect_path(path) is None:
        raise ValueError
    bundle = path.resolve(strict=True)
    contents = bundle / "Contents"
    plist = (contents / "Info.plist").resolve(strict=True)
    if not plist.is_relative_to(contents) or not plist.is_file():
        raise ValueError
    with plist.open("rb") as file:
        info = plistlib.load(file)
    if not isinstance(info, dict):
        raise ValueError
    name = info["CFBundleExecutable"]
    if not isinstance(name, str) or Path(name).name != name or name in ("", ".", ".."):
        raise ValueError
    executable = (contents / "MacOS" / name).resolve(strict=True)
    if (not executable.is_relative_to(contents / "MacOS") or not executable.is_file()
            or protect_path(executable) is None):
        raise ValueError
    return bundle, executable, info


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--confirm-disposable", required=True, action="store_true")
    parser.add_argument("bundle", type=Path)
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("this probe requires macOS")
    try:
        bundle, executable, info = resolve_application(args.bundle)
    except (OSError, ValueError, KeyError, RuntimeError, ExpatError, plistlib.InvalidFileException):
        parser.error("supply an installed .app bundle with a readable main executable and plist")

    _, version = system_tool("/usr/bin/sw_vers", "-productVersion")
    _, build = system_tool("/usr/bin/sw_vers", "-buildVersion")
    arm_result, arm = system_tool("/usr/sbin/sysctl", "-n", "hw.optional.arm64")
    host_architecture = None
    if arm_result == 0 and arm == "1":
        host_architecture = "arm64"
    elif arm_result == 0 and arm == "0":
        host_architecture = "x86_64"
    discovery = inventory(executable)
    print(json.dumps({
        "os": {
            "name": "macos", "version": version or None, "build": build or None,
            "architecture": host_architecture,
        },
        "application": application_metadata(bundle, executable, info),
        "discovery": discovery,
        "activation_permission": {
            "kind": "macos-accessibility-probe-context", "availability": accessibility_permission(),
        },
        "operations": {
            "installed_metadata": "supported",
            "process_inventory": "supported" if discovery["completeness"] == "complete" else "unverified",
            "managed_profile_attribution": "unverified",
            "separate_instances": "unverified",
            "activation_smoke": "unverified",
            "dock_production_launch": "unsupported",
            "dock_production_activation": "unsupported",
        },
    }, indent=2))


if __name__ == "__main__":
    main()
