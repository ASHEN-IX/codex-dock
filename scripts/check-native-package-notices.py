# SPDX-License-Identifier: Apache-2.0
"""Verify exact notice bytes/modes in built native packages; extraction is not GUI validation."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def expected_notices():
    result = {name: ROOT / name for name in ('LICENSE', 'NOTICE', 'THIRD-PARTY-NOTICES.md', 'THIRD-PARTY-NOTICES.txt')}
    result.update({'native/' + str(p.relative_to(ROOT / 'third-party/native')): p
                   for p in (ROOT / 'third-party/native').rglob('*') if p.is_file()})
    return result


def check_tree(root):
    base = root / 'usr/share/licenses/codex-dock'
    for name, source in expected_notices().items():
        target = base / name
        if not target.is_file() or hashlib.sha256(target.read_bytes()).digest() != hashlib.sha256(source.read_bytes()).digest():
            raise ValueError(f'Missing/altered notice: {target}')
        if target.stat().st_mode & 0o111:
            raise ValueError(f'Executable license data: {target}')


def check_archive(path):
    with tempfile.TemporaryDirectory(prefix='dock-native-notice-') as temp:
        root = Path(temp)
        subprocess.run(['bsdtar', '-xf', str(path.resolve()), '-C', str(root)], check=True)
        if path.suffix == '.deb':
            data, = root.glob('data.tar.*')
            subprocess.run(['bsdtar', '-xf', str(data), '-C', str(root)], check=True)
        check_tree(root)
        if path.suffix == '.deb':
            if (root / 'usr/share/doc/codex-dock/copyright').read_bytes() != (ROOT / 'packaging/debian-copyright').read_bytes():
                raise ValueError('Missing/altered Debian copyright document')
    print(f'PASS exact notice bytes and modes: {path.name}')


def main():
    if sys.argv[1] == '--arch':
        for name in sys.argv[2:]:
            check_archive(Path(name))
        return
    bundle = Path(sys.argv[1])
    for kind in ('deb', 'rpm'):
        path, = (bundle / kind).glob('*.' + kind)
        check_archive(path)
    appimage, = (bundle / 'appimage').glob('*.AppImage')
    destination = ROOT / 'dist/appimage-inspection'
    if destination.exists():
        shutil.rmtree(destination)
    destination.mkdir(parents=True)
    (destination / 'home').mkdir()
    env = {'HOME': str(destination / 'home'), 'PATH': os.environ.get('PATH', '/usr/bin:/bin')}
    subprocess.run([str(appimage.resolve()), '--appimage-extract'], cwd=destination, env=env,
                   stdout=subprocess.DEVNULL, check=True)
    check_tree(destination / 'squashfs-root')
    print('PASS exact AppImage notices; no graphical runtime claim')


if __name__ == '__main__':
    main()
