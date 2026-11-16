# Licensing decision and third-party notice review

Status: **OWNER DECISION REQUIRED — distribution blocked**. No project license is
selected by this change. Neither Cargo manifest declares one, and root `LICENSE`
is absent. The release workflow now refuses to build a draft without that file;
its existence alone is not evidence of license approval or notice compliance.

Recommendation for owner approval: **Apache-2.0**, for its express patent grant
and patent-litigation termination terms. MIT is a reasonable alternative when
minimal license text and notice administration are the owner's priority. MIT
requires retaining its copyright and permission notice. Apache-2.0 also requires
preserving applicable attribution notices, indicating changed files, and carrying
forward applicable upstream NOTICE content. Neither grants trademark rights to
Codex branding. These are engineering recommendations, not an assignment of rights.

Primary texts: [MIT](https://opensource.org/license/mit) and
[Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0.html).
The maintainer must approve the choice and copyright holder/year, confirm rights
to existing contributions and artwork, add the approved root text and matching
Cargo metadata, and review distribution notices before any public RC.

## Cargo inventory and prepared notices

`cargo-license-inventory.tsv` records all 448 registry dependencies in the locked
all-target metadata graph (including build tools and non-Linux dependencies).
This deliberately overcounts the code in a particular Linux binary. License
expressions are declarations, not evidence that every redistribution obligation
has been satisfied. Dominant expressions are MIT and Apache-2.0 alternatives.
Five crates declare MPL-2.0: cssparser, cssparser-macros, dtoa-short, option-ext,
and selectors. Unicode-3.0 also appears. Do not describe the whole binary as
containing only MIT/Apache code. No dependency update or license waiver is made.

Prepare full upstream notice texts without embedding workstation paths:

```sh
cargo metadata --locked --format-version 1 |
  python3 scripts/prepare-license-notices.py dist/license-review
```

The script produces `THIRD_PARTY_NOTICES.txt`, an inventory, and
`missing-notice-texts.txt`. The initial output copied upstream
terms into a 3.6 MB review draft; it is generated rather than committed here.
35 crates have no top-level license/notice text in their registry archive,
including Linux-relevant alloc-stdlib, dlopen2, dlopen2_derive,
libappindicator-sys and selectors. Their metadata does declare a license, but the
owner must retrieve the applicable upstream copyright/license notices, inspect
nested license files and build-generated inputs, and approve the final notice
bundle. Retain the exact source versions and MPL source-availability information
for recipients. This generator is a review aid, not compliance clearance.

## Native libraries and other executables

Portable packages contain only Dock and its CLI as executables. Codex Desktop,
Codex CLI, sqlite3, qdbus6, wmctrl, xdotool and xdg-open are host tools, not
redistributed by the portable/Arch scripts. Native system-linked GTK, GLib,
WebKitGTK, JavaScriptCore, libsoup and their transitive dependencies have separate
license obligations. Debian/RPM normally resolve these from the system.
AppImage bundling may redistribute native libraries and runtime helpers: inventory
its actual extracted filesystem, preserve their notices and any applicable LGPL
relinking/source obligations before release. Cargo metadata alone cannot audit it.

Artwork origin and rights, all final package notice placement, native library
source offers where required, and the final release SBOM remain owner/reviewer
blockers. Do not ship this draft as a substitute for approved distribution notices.
