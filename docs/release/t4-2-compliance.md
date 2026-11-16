# T4.2 Apache-2.0 licensing implementation review

## Scope and prerequisite

PR #17 merged on 2026-11-16 at **5430a811c89702cf95adcf45eb864fc031a4a082**.
This branch starts from that exact baseline. The maintainer selected Apache-2.0
and confirmed permission for existing contributions and rights to artwork. The
original working tree and its uncommitted launch changes remain untouched.

This is an implementation review, not legal title verification or public release
clearance. Exact final commit and CI results are recorded in the PR/export after
freezing the branch; a Git commit cannot contain its own SHA.

## Compliance matrix

- **PASS — project license:** complete unmodified official Apache-2.0 root LICENSE,
  SHA-256 cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30;
  both Rust package manifests and Tauri/Arch metadata declare Apache-2.0.
- **PASS with recorded representation — contribution/artwork authorization:**
  explicit maintainer confirmation is recorded in rights-and-branding.md. Git
  authorship is provenance, not assumed ownership. NOTICE has no invented holder
  or year. Human reviewers should retain the underlying permission/provenance records.
- **PASS — source identifiers:** SPDX headers added to project-owned Rust, Python,
  shell, frontend and Makefile source. Rust source differs only by those comments;
  generated schemas and third-party terms are not relabelled or reformatted.
- **PASS — Cargo inventory:** all 448 registry entries classified: 329 Linux,
  249 conservative normal/non-proc-macro binary candidates, 80 build/proc-macro,
  119 excluded from Linux. This is graph reachability, not exact retained-symbol
  or binary provenance proof. Source checkouts do not vendor these crate sources.
- **PASS — missing-root investigation:** every original 35 case is recorded with
  scope, published VCS metadata and reasoning. Thirty are non-Linux; five Linux
  cases have retained workspace/upstream/standard-license texts. Absence of a
  crate-root license is not treated as automatic noncompliance.
- **PASS — Cargo notice preservation:** generated notice text includes MPL,
  Unicode, BSD, MIT, Apache, ISC, zlib and other applicable declarations/texts.
  Generation is deterministic and checks curated provenance hashes. Original
  upstream legal whitespace is retained; targeted .gitattributes exclusions avoid
  reformatting those texts while project source still receives normal checks. Exact-version
  source links are retained. Long-term MPL source availability remains a release duty.
- **PASS mechanics / HUMAN REVIEW — native inventory:** reference AppImage
  c40c167035bea4e002369f1fd45cecba37616c5fdc7c9ff6c6a7562509c83e7b from
  [PR #17 package CI](https://github.com/ASHEN-IX/codex-dock/actions/runs/43908206759)
  has 209 ELF paths mapped to 113 candidate distro packages, with original
  copyright files and shared license texts preserved. Only AppRun.wrapped remains
  unmapped after accounting for versioned shared-library names. Mapping by name
  is not exact source/build-ID verification. The outer runtime and bundler helper/
  data terms are explicitly unresolved in helper-review.json.
- **PASS locally / CI confirmation pending — all five package formats:** AppImage,
  Debian, RPM, portable and Arch builds passed local notice-byte/mode checks.
  Each carries LICENSE, NOTICE, both third-party documents and retained native
  copyright evidence under share/licenses/codex-dock. Native distro paths are
  under /usr. A source Makefile install also carries the notices.
- **PASS locally — user data preservation:** portable installation, simulated
  upgrade/rollback and removal preserve synthetic profile/auth/store data and links.
  The installer performs no profile migration or repair. Native CI installation,
  notice comparison and removal checks remain part of package validation.
- **PASS locally / CI pending — engineering checks:** 255 serial workspace tests
  in an unprivileged PID namespace, strict Clippy, formatting, frontend syntax,
  Python/shell syntax, workflow lint, notice reproducibility, three Tauri bundle
  builds, portable/Arch builds and all package notice checks passed. This is not
  GUI/login validation. No synthetic-tests feature is enabled in production builds.
- **BLOCKED for public release — corresponding source and exact native identity:**
  verify native/library/helper versions, bundler patches, all notice obligations,
  and a durable corresponding-source/build-script delivery mechanism, including
  MPL and native copyleft recipients. Do not claim that copied copyright files
  or extractable shared libraries alone satisfy every source obligation.
- **HUMAN REVIEW / public release blocked — branding:** independent-project
  disclaimer added to README/NOTICE. OpenAI, ChatGPT and Codex usage is documented
  as product interoperability. Trademark/name/icon review or permission is still
  required; Apache-2.0 and the disclaimer grant no third-party trademark rights.

## Validation and safety boundaries

`python3 scripts/check-licenses.py` passes only when the official project text,
metadata, generated Cargo materials, native copyright hashes and package mappings
match. `--release` was tested and rejects the three open redistribution obligations.
The draft-release workflow invokes this stronger check and remains draft-first,
with the existing maintainer approval environment. No release workflow was dispatched.

Package checks compare notice contents to the committed originals; they do not
just look for filenames. The portable allowlist includes only the exact application
and license payload. The existing lifecycle test verifies data/permission/link
preservation. New CI inspects Debian/RPM/Arch archives and the extracted AppImage,
and publishes a per-build native review inventory for human inspection.

No application behavior, Windows/macOS production capability gate, platform
support promise, login or real credential/profile data is changed. No source
license of an upstream component is overwritten. No tag or release is created,
and this licensing PR must not be merged automatically.

## Verdict

**CONDITIONAL GO for merging the licensing implementation**, subject to independent
review and successful CI on the frozen commit. Known public release blockers are
explicitly preserved and enforced; they need not be misrepresented as resolved to
merge notice-preservation tooling. **NO-GO for public release** until the native
identity/source/helper and trademark obligations are actually resolved, in addition
to the runtime/release conditions from PR #17.

See THIRD-PARTY-NOTICES.md, rights-and-branding.md, redistribution-status.json,
missing-notice-review.json, third-party/license-overrides.json and
third-party/native/ for the reviewable evidence and remaining work.
