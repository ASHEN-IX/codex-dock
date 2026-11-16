"""Prepare review material from the locked Cargo graph; assigns no project license.

Usage: cargo metadata --locked --format-version 1 | python3 scripts/prepare-license-notices.py OUTPUT
Includes all target platforms and build dependencies conservatively. Native libraries
and files absent from registry packages still need distributor review.
"""
import json
from pathlib import Path
import sys

metadata = json.load(sys.stdin)
output = Path(sys.argv[1])
output.mkdir(parents=True, exist_ok=True)
rows = ['package\tversion\tdeclared-license\tsource\tnotice-files']
texts = ['THIRD-PARTY NOTICE REVIEW DRAFT\nNo license is assigned to Codex Dock by this file.\n'
         'All-target Cargo inventory; not a native-library redistribution clearance.\n']
missing = []
for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
    if not package['source']:
        continue
    root = Path(package['manifest_path']).parent
    notices = sorted(p for p in root.iterdir() if p.is_file() and
                     p.name.lower().startswith(('license', 'licence', 'notice', 'copying', 'copyright')))
    rows.append('\t'.join([package['name'], package['version'], package['license'] or 'UNDECLARED',
                           package['source'], ', '.join(p.name for p in notices) or 'MISSING: owner review required']))
    texts.append(f"\n=== {package['name']} {package['version']} | {package['license']} ===\n")
    if not notices:
        missing.append(package['name'] + ' ' + package['version'])
        texts.append('NOTICE TEXT MISSING FROM CRATE ROOT: retrieve and review upstream terms before distribution.\n')
    for notice in notices:
        texts.append(f'\n--- {notice.name} ---\n' + notice.read_text(errors='replace') + '\n')
(output / 'cargo-license-inventory.tsv').write_text('\n'.join(rows) + '\n')
(output / 'THIRD_PARTY_NOTICES.txt').write_text(''.join(texts))
(output / 'missing-notice-texts.txt').write_text('\n'.join(missing) + '\n')
print(f"Prepared {len(rows)-1} registry records; {len(missing)} need upstream text review", file=sys.stderr)
