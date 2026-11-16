# Third-party notices and Linux redistribution status

Codex Dock's project license is Apache-2.0. Dependency licenses remain unchanged.
[THIRD-PARTY-NOTICES.txt](THIRD-PARTY-NOTICES.txt) reproduces the retained Cargo
license/notice texts for the Linux dependency graph, including build tools as a
conservative superset. This is notice preservation, not public release clearance.

The [inventory](docs/release/cargo-license-inventory.tsv) classifies all 448 locked
registry entries: 329 in the Linux resolved graph, of which 249 are normal,
non-proc-macro binary candidates and 80 are build/procedural-macro dependencies;
119 are excluded from the Linux graph. Linker elimination, code generation and
feature-specific use prevent this graph from proving exact code inclusion.
Source checkouts contain Cargo manifests/lockfile and notices, not vendored crate
source. Binary packages embed compiled code and the notices below. No Codex CLI
or Desktop executable is redistributed by this project.

## The original 35 missing-root cases

[missing-notice-review.json](docs/release/missing-notice-review.json) investigates
every case and records its exact VCS revision/path where published. Thirty are
non-Linux dependencies: their exclusion from Linux is not a finding of license
noncompliance, and does not clear future Windows/macOS distribution. Five affect
the Linux graph. Their exact upstream/workspace notices or standard MPL text are
preserved under [third-party/licenses](third-party/licenses) with provenance and
SHA-256 in [license-overrides.json](third-party/license-overrides.json):

- alloc-stdlib: BSD notice from the parent repository at its published VCS commit.
- dlopen2 and dlopen2_derive: MIT workspace notice at their shared VCS commit.
- libappindicator-sys: both parent libappindicator notices; parent and sys crates
  report the same VCS commit. This retains both alternatives without inventing a grant.
- selectors: source headers explicitly apply MPL-2.0; the standard MPL text is
  supplied with exact-version source access. No copyright holder is invented.

## MPL, Unicode and source availability

MPL-2.0 packages in this graph are cssparser 0.37.0, cssparser-macros 0.7.1,
dtoa-short 0.3.5, option-ext 0.2.0 and selectors 0.38.0. Their license terms and
per-file source notices remain intact. Exact unmodified source archives are linked
for every version in the inventory and notice file. For example, selectors source
is available at https://static.crates.io/crates/selectors/selectors-0.38.0.crate.
A distributor must ensure recipients have timely, reasonable access to the
corresponding MPL source, including any modifications, and tell recipients how
to obtain it. A release should retain its exact archives/source manifest alongside
its binary artifacts; an upstream URL alone is not an enduring availability plan.
No dependency source modifications are introduced here.

Unicode-3.0 texts, including their original copyright and permission notices,
are preserved from the exact crate distributions. Dual-license alternatives,
BSD, ISC, zlib and Apache notices are also retained; the root Apache license does
not replace them. Check the actual generated file, not just SPDX metadata.

Primary references: [MPL FAQ](https://www.mozilla.org/en-US/MPL/2.0/FAQ/),
[MPL text](https://www.mozilla.org/en-US/MPL/2.0/),
[Unicode license](https://www.unicode.org/license.txt).

## Native AppImage libraries and helpers — public release blocked

Cargo notices do not cover native libraries, pixbuf/GTK/GIO plugins, WebKit helper
executables, AppRun, linuxdeploy hooks, or the AppImage runtime. The native
inventory and retained distribution copyright texts live under
`third-party/native/`. The inventory's package mapping is an evidence-backed
candidate by file basename; bundlers patch ELF paths, so it must not be treated
as proof of exact package/source identity without build-ID/version verification.

LGPL components generally require preserving notices, providing the applicable
corresponding library source and permitting replacement/relinking as required by
the actual license version. GPL components/helpers can impose additional source
and build/install-script obligations. Distro copyright files may reference shared
license texts; retain those too. Font/data, OpenSSL and other component-specific
terms must be reviewed separately. Extraction/replacement capability alone does
not satisfy all source obligations. Obtain the exact build sources and recipes,
review bundler modifications, and deliver a compliant source mechanism before
public distribution. No blanket legal clearance or source offer is made here.

`docs/release/redistribution-status.json` records the outstanding release blockers.
The draft-release workflow checks it and fails closed. Normal PR checks validate
the implementation and preserved notices while reporting these open obligations.

Regenerate Cargo materials with:

```sh
cargo metadata --locked --format-version 1 |
  python3 scripts/prepare-license-notices.py /tmp/codex-dock-license-review
```

Compare the generated inventory, missing-case review and notices with committed
files. Do not remove or waive an unresolved entry merely to make CI green.
