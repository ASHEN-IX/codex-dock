# SPDX-License-Identifier: Apache-2.0
"""Generate deterministic Linux Cargo notices and classify all-target dependencies.

cargo metadata --locked --format-version 1 | python3 scripts/prepare-license-notices.py OUTPUT
Reads a second, Linux-filtered graph. No network calls beyond Cargo's locked metadata.
Normal non-proc-macro reachability is a conservative binary candidate set, not proof
that a linker retained every package. Build dependencies are included in notices.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent


def read_notice(path):
    # Universal-newline conversion would alter some upstream license files.
    with path.open(encoding='utf-8', newline='') as stream:
        return stream.read()


def write_notice(path, text):
    with path.open('w', encoding='utf-8', newline='') as stream:
        stream.write(text)


def generate(metadata, linux, output):
    packages = {p['id']: p for p in metadata['packages']}
    nodes = {n['id']: n for n in linux['resolve']['nodes']}
    runtime, pending = set(), list(metadata['workspace_members'])
    while pending:
        current = pending.pop()
        if current in runtime:
            continue
        runtime.add(current)
        for dep in nodes[current]['deps']:
            if any(k['kind'] is None for k in dep['dep_kinds']) and not any(
                'proc-macro' in target['kind'] for target in packages[dep['pkg']]['targets']
            ):
                pending.append(dep['pkg'])
    overrides = json.loads((ROOT / 'third-party/license-overrides.json').read_text())
    rows = ['package\tversion\tdeclared-license\tlinux-scope\tsource-url\tnotice-files']
    texts = ['THIRD-PARTY NOTICES — LINUX CARGO GRAPH\n'
             'Project code: Apache-2.0; third-party terms below remain unchanged.\n'
             'Includes Linux build tools conservatively; not native AppImage clearance.\n'
             'Exact unmodified crate sources: https://static.crates.io/crates/NAME/NAME-VERSION.crate\n'
             'See THIRD-PARTY-NOTICES.md for source availability and unresolved native obligations.\n']
    missing, original_missing = [], []
    for package in sorted(packages.values(), key=lambda p: (p['name'], p['version'])):
        if not package['source']:
            continue
        name, version = package['name'], package['version']
        key = f'{name}@{version}'
        scope = 'binary-candidate' if package['id'] in runtime else 'build-or-proc-macro' if package['id'] in nodes else 'excluded-non-linux'
        root = Path(package['manifest_path']).parent
        notices = sorted(p for p in root.iterdir() if p.is_file() and
                         p.name.lower().startswith(('license', 'licence', 'notice', 'copying', 'copyright')))
        if not notices:
            original_missing.append({'package': key, 'scope': scope,
                'repository': package.get('repository'),
                'vcs': json.loads((root / '.cargo_vcs_info.json').read_text()) if (root / '.cargo_vcs_info.json').exists() else None,
                'finding': 'Excluded from Linux; upstream terms require separate review before another target ships.' if scope == 'excluded-non-linux' else overrides.get(key, {}).get('reason', 'UNRESOLVED')})
        provenance = []
        if key in overrides:
            for item in overrides[key]['files']:
                path = ROOT / item['path']
                if hashlib.sha256(path.read_bytes()).hexdigest() != item['sha256']:
                    raise ValueError(f'Override checksum mismatch: {key}')
                notices.append(path)
                provenance.append(item['source'])
        url = f'https://static.crates.io/crates/{name}/{name}-{version}.crate'
        rows.append('\t'.join([name, version, package['license'] or 'UNDECLARED', scope, url,
                               ', '.join(p.name for p in notices) or 'not distributed on Linux']))
        if scope == 'excluded-non-linux':
            continue
        texts.append(f'\n=== {name} {version} | {package["license"]} | {scope} ===\nSource: {url}\n')
        if provenance:
            texts.append('Notice provenance: ' + '; '.join(provenance) + '\n')
        if not notices:
            missing.append(key)
            texts.append('UNRESOLVED NOTICE TEXT: redistribution review required.\n')
        for notice in notices:
            texts.append(f'\n--- {notice.name} ---\n' + read_notice(notice) + '\n')
    output.mkdir(parents=True, exist_ok=True)
    (output / 'cargo-license-inventory.tsv').write_text('\n'.join(rows) + '\n')
    write_notice(output / 'THIRD-PARTY-NOTICES.txt', ''.join(texts))
    (output / 'missing-notice-review.json').write_text(json.dumps(original_missing, indent=2, sort_keys=True) + '\n')
    (output / 'unresolved-linux-notices.json').write_text(json.dumps(missing, indent=2) + '\n')
    print(f'{len(rows)-1} registry entries; {len(original_missing)} original missing-root cases; {len(missing)} unresolved Linux cases')
    if missing:
        raise SystemExit('Linux notice text unresolved: ' + ', '.join(missing))


if __name__ == '__main__':
    linux = json.loads(subprocess.check_output(['cargo', 'metadata', '--locked', '--format-version', '1',
                                              '--filter-platform', 'x86_64-unknown-linux-gnu'], cwd=ROOT))
    generate(json.load(sys.stdin), linux, Path(sys.argv[1]))
