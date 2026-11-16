"""Inspect the portable archive and test file-only installation in a disposable HOME.

This never launches Dock, Codex, a browser, or a login. It is not GUI evidence.
"""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify(archive):
    expected = {
        'bin/codex-dock', 'bin/codex-dock-cli', 'install.sh', 'INSTALL.md',
        'share/applications/codex-dock.desktop',
        *(f'share/icons/hicolor/{size}x{size}/apps/codex-dock.png'
          for size in (32, 48, 64, 128, 256)),
    }
    allowed_dirs = {str(parent) for name in expected for parent in Path('codex-dock', name).parents}
    checksum = archive.with_name(archive.name + '.sha256').read_text().split()
    assert checksum == [digest(archive), archive.name], 'Archive checksum mismatch'
    with tempfile.TemporaryDirectory(prefix='dock-package-check-') as temp:
        root = Path(temp)
        with tarfile.open(archive) as bundle:
            files = set()
            members = bundle.getmembers()
            assert len({m.name.rstrip('/') for m in members}) == len(members), 'Duplicate member'
            for member in members:
                path = Path(member.name)
                assert not path.is_absolute() and '..' not in path.parts, 'Unsafe path'
                assert path.parts[0] == 'codex-dock', 'Unexpected archive root'
                assert member.isdir() or member.isfile(), 'Links/devices are forbidden'
                assert member.mode & 0o7022 == 0, 'Unsafe file mode'
                if member.isdir():
                    assert str(path) in allowed_dirs, f'Unexpected directory: {path}'
                if member.isfile():
                    relative = str(path.relative_to('codex-dock'))
                    files.add(relative)
                    wanted = 0o755 if relative.startswith('bin/') or relative == 'install.sh' else 0o644
                    assert member.mode & 0o777 == wanted, f'Wrong mode: {relative}'
            assert files == expected, f'Unexpected package contents: {files ^ expected}'
            # Only regular files and directories with validated relative paths remain.
            bundle.extractall(root)
        home = root / 'home'
        home.mkdir()
        prefix = home / '.local'
        # Mimic sensitive stores without using any real credential or database.
        sentinels = []
        for name in ('.codex/state_5.sqlite', '.codex-profiles/test/auth.json',
                     '.config/Codex-test/Cookies'):
            path = home / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'disposable-data-preservation-sentinel\n')
            sentinels.append(path)
        link = home / '.codex-profiles/test/state_5.sqlite'
        link.symlink_to('../../.codex/state_5.sqlite')
        before = [(p, digest(p), p.stat().st_mode) for p in sentinels]
        env = {'HOME': str(home), 'PATH': '/usr/bin:/bin',
               'XDG_CONFIG_HOME': str(home / '.config'),
               'XDG_DATA_HOME': str(home / '.local/share')}
        installer = root / 'codex-dock/install.sh'
        invalid = subprocess.run(['sh', str(installer), 'relative'], env=env, capture_output=True)
        assert invalid.returncode != 0 and b'absolute path' in invalid.stderr
        subprocess.run(['sh', str(installer)], env=env, check=True)
        original = {name: digest(prefix / 'bin' / name) for name in ('codex-dock', 'codex-dock-cli')}
        assert not list((prefix / 'share/codex-dock').glob('previous-*'))
        # A distinct older executable pair makes backup/rollback verification meaningful.
        for name in original:
            (prefix / 'bin' / name).write_bytes(b'old-disposable-executable-' + name.encode())
        old = {name: digest(prefix / 'bin' / name) for name in original}
        subprocess.run(['sh', str(installer)], env=env, check=True)
        previous, = (prefix / 'share/codex-dock').glob('previous-*')
        for name in original:
            assert digest(prefix / 'bin' / name) == original[name]
            assert digest(previous / name) == old[name]
            shutil.copy2(previous / name, prefix / 'bin' / name)
            assert digest(prefix / 'bin' / name) == old[name]
        # Remove precisely application-owned paths; preserve backups and user data.
        for name in original:
            (prefix / 'bin' / name).unlink()
        (prefix / 'share/applications/codex-dock.desktop').unlink()
        for size in (32, 48, 64, 128, 256):
            (prefix / f'share/icons/hicolor/{size}x{size}/apps/codex-dock.png').unlink()
        for path, checksum, mode in before:
            assert digest(path) == checksum and path.stat().st_mode == mode
        assert link.is_symlink() and os.readlink(link) == '../../.codex/state_5.sqlite'
        assert not list((prefix / 'bin').iterdir())
        print(f'PASS {archive.name}: checksum, allowlist, modes, install, upgrade, rollback, removal, data preservation')


if __name__ == '__main__':
    if len(sys.argv) != 2:
        raise SystemExit('Usage: check-linux-package.py archive.tar.gz')
    verify(Path(sys.argv[1]).resolve())
