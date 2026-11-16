# SPDX-License-Identifier: Apache-2.0
"""Validate notice reproducibility and license integration; --release fails on open obligations."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
OFFICIAL_APACHE_SHA256 = 'cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30'


def require_release_clearance(status):
    if status['unresolved']:
        raise ValueError('Public redistribution remains blocked: ' + '; '.join(status['unresolved']))


def check(release=False):
    if hashlib.sha256((ROOT / 'LICENSE').read_bytes()).hexdigest() != OFFICIAL_APACHE_SHA256:
        raise ValueError('LICENSE is not the unmodified official Apache-2.0 text')
    for name in ('core/Cargo.toml', 'src-tauri/Cargo.toml'):
        if not re.search(r'^license = "Apache-2.0"$', (ROOT / name).read_text(), re.MULTILINE):
            raise ValueError(f'Missing Apache metadata: {name}')
    for folder, pattern in [('core', '*.rs'), ('src-tauri/src', '*.rs'), ('scripts', '*.py'), ('packaging', '*.sh')]:
        for path in (ROOT / folder).rglob(pattern):
            if not any('SPDX-License-Identifier: Apache-2.0' in line for line in path.read_text().splitlines()[:3]):
                raise ValueError(f'Missing source SPDX: {path.relative_to(ROOT)}')
    metadata = subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1'], cwd=ROOT)
    with tempfile.TemporaryDirectory(prefix='dock-license-check-') as temp:
        subprocess.run([sys.executable, str(ROOT / 'scripts/prepare-license-notices.py'), temp],
                       input=metadata, cwd=ROOT, check=True)
        for generated, committed in [('THIRD-PARTY-NOTICES.txt', 'THIRD-PARTY-NOTICES.txt'),
                                     ('cargo-license-inventory.tsv', 'docs/release/cargo-license-inventory.tsv'),
                                     ('missing-notice-review.json', 'docs/release/missing-notice-review.json'),
                                     ('unresolved-linux-notices.json', 'docs/release/unresolved-linux-notices.json')]:
            if (Path(temp) / generated).read_bytes() != (ROOT / committed).read_bytes():
                raise ValueError(f'Stale generated notice material: {committed}')
    for package, record in json.loads((ROOT / 'third-party/native/inventory.json').read_text())['package_candidates'].items():
        path = ROOT / 'third-party/native' / record['copyright_file']
        if hashlib.sha256(path.read_bytes()).hexdigest() != record['copyright_sha256']:
            raise ValueError(f'Changed native copyright text: {package}')
    config = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text())
    for kind in ('deb', 'rpm', 'appimage'):
        mapping = config['bundle']['linux'][kind]['files']
        for name in ('LICENSE', 'NOTICE', 'THIRD-PARTY-NOTICES.md', 'THIRD-PARTY-NOTICES.txt'):
            if mapping[f'/usr/share/licenses/codex-dock/{name}'] != '../' + name:
                raise ValueError(f'Missing {kind} notice mapping: {name}')
    status = json.loads((ROOT / 'docs/release/redistribution-status.json').read_text())
    if release:
        require_release_clearance(status)
    print(f'PASS: license, SPDX, generated Cargo notices, retained native texts, package mappings; {len(status["unresolved"])} public-release obligations remain open')


if __name__ == '__main__':
    check('--release' in sys.argv)
