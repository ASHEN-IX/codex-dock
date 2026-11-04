"""Keep informational RustSec warnings visible even when audit exits zero."""
import json
from pathlib import Path
import sys

path = Path(sys.argv[1])
if not path.exists() or not path.stat().st_size:
    print("## Dependency audit\n\nNo report was produced. Check the failed audit step.")
    raise SystemExit(0)
report = json.loads(path.read_text())
print("## Dependency audit\n")
print(f"Blocking vulnerability findings: **{report['vulnerabilities']['count']}**\n")
warnings = report.get("warnings", {})
print(f"Additional warnings requiring review: **{sum(len(items) for items in warnings.values())}**\n")
print("| Category | Package | Version | Advisory |\n| --- | --- | --- | --- |")
for category, items in warnings.items():
    for item in items:
        package = item.get("package", {})
        advisory = item.get("advisory") or {}
        cells = [category, package.get("name", "unknown"), package.get("version", "unknown"), advisory.get("id", "yanked package; review lockfile")]
        print("| " + " | ".join(str(cell).replace("|", "\\|").replace("\n", " ") for cell in cells) + " |")
print("\nA successful audit is not proof of safety. Review the full artifact and CodeQL findings.")
