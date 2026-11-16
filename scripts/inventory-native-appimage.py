# SPDX-License-Identifier: Apache-2.0
"""Inventory extracted AppImage ELF files against the build host's dpkg database.

Run on the Ubuntu build/audit host: script APPDIR OUTPUT.
Package names are candidates by basename, not exact source/build-ID verification.
Copies actual package copyright and shared license texts without modifying them.
"""
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys


def inventory(appdir, output):
    output.mkdir(parents=True, exist_ok=True)
    copyrights = output / 'copyright'
    copyrights.mkdir(exist_ok=True)
    ownership = {}
    for listing in sorted(Path('/var/lib/dpkg/info').glob('*.list')):
        package = listing.name[:-5]
        for line in listing.read_text(errors='replace').splitlines():
            ownership.setdefault(Path(line).name, set()).add(package)
    installed = {}
    query = subprocess.check_output(['dpkg-query', '-W', '-f=${binary:Package}\t${Version}\t${source:Package}\t${source:Version}\n'], text=True)
    for line in query.splitlines():
        package, version, source, source_version = line.split('\t')
        installed[package] = {'binary_version': version, 'source_package': source, 'source_version': source_version}
    records, package_records = [], {}
    for path in sorted(appdir.rglob('*')):
        if not path.is_file():
            continue
        data = path.read_bytes()
        if not data.startswith(b'\x7fELF'):
            continue
        relative = str(path.relative_to(appdir))
        if path.name == 'codex-dock':
            records.append({'path': relative, 'sha256': hashlib.sha256(data).hexdigest(), 'project_owned': True})
            continue
        candidates = []
        owners = set(ownership.get(path.name, []))
        if not owners and path.name.endswith('.so'):
            for basename, package_owners in ownership.items():
                if basename.startswith(path.name + '.'):
                    owners.update(package_owners)
        for package in sorted(owners):
            doc = Path('/usr/share/doc') / package.split(':')[0] / 'copyright'
            if package not in installed or not doc.is_file():
                continue
            target = copyrights / (package.replace(':', '_') + '.txt')
            shutil.copyfile(doc, target)
            labels = sorted(set(re.findall(r'^License:\s*(.+)', doc.read_text(errors='replace'), re.MULTILINE)))
            package_records[package] = {**installed[package], 'license_labels': labels or ['REVIEW_COPYRIGHT_TEXT'],
                'copyright_file': 'copyright/' + target.name,
                'copyright_sha256': hashlib.sha256(doc.read_bytes()).hexdigest(),
                'clearance': 'PENDING exact identity and corresponding-source review'}
            candidates.append(package)
        records.append({'path': relative, 'sha256': hashlib.sha256(data).hexdigest(),
                        'package_candidates': candidates,
                        'clearance': 'PENDING' if candidates else 'UNMAPPED — investigate upstream helper/runtime provenance'})
    common = Path('/usr/share/common-licenses')
    if common.is_dir():
        shutil.copytree(common, output / 'common-licenses', dirs_exist_ok=True)
    result = {'schema': 1, 'mapping_method': 'dpkg installed basename or shared-object version suffix; not build-ID proof',
              'elf_files': records, 'package_candidates': package_records,
              'additional_review': ['Outer AppImage runtime', 'AppRun wrapper and wrapped helper',
                                    'linuxdeploy hooks/scripts', 'Non-ELF data/font/plugin notices',
                                    'Exact corresponding source, bundler patches and build/install scripts']}
    (output / 'inventory.json').write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
    (output / 'README.txt').write_text('NATIVE NOTICE EVIDENCE — NOT REDISTRIBUTION CLEARANCE\n'
        'See inventory.json, retained package copyright files and common-licenses.\n'
        'Package identity/source verification and helper/data review remain open.\n'
        'Shared license texts referenced by distro copyright files are in common-licenses/.\n')
    print(f'{len(records)} ELF paths; {len(package_records)} candidate packages; public release clearance PENDING')


if __name__ == '__main__':
    inventory(Path(sys.argv[1]), Path(sys.argv[2]))
