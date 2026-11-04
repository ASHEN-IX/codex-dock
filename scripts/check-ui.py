"""Parse inline scripts and syntax-check them without invoking the Tauri bridge."""
from html.parser import HTMLParser
from pathlib import Path
import subprocess
import sys


class Scripts(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=False)
        self.current = None
        self.module = False
        self.scripts = []

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        kind = attrs.get("type", "text/javascript").lower()
        if tag == "script" and "src" not in attrs and kind in (
            "text/javascript", "application/javascript", "module", ""
        ):
            self.current = []
            self.module = kind == "module"

    def handle_data(self, data):
        if self.current is not None:
            self.current.append(data)

    def handle_endtag(self, tag):
        if tag == "script" and self.current is not None:
            self.scripts.append(("".join(self.current), self.module))
            self.current = None


path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[1] / "src/index.html"
parser = Scripts()
parser.feed(path.read_text())
parser.close()
if parser.current is not None:
    raise SystemExit("Unclosed inline script tag")
if not parser.scripts:
    raise SystemExit("Expected an inline frontend script")
for script, module in parser.scripts:
    command = ["node", "--check"]
    if module:
        command.append("--input-type=module")
    subprocess.run(command, input=script, text=True, check=True)
print(f"Checked {len(parser.scripts)} frontend script(s)")
