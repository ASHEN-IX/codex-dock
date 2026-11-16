# Licensing implementation status — T4.2

The maintainer selected Apache-2.0 on 2026-11-16 and confirmed permission for
existing contributions and rights to artwork. The official root LICENSE is
unmodified; project package metadata and source SPDX identifiers now apply it.
No copyright ownership transfer, fabricated holder/year, or third-party relicense
is asserted. See [rights and branding](rights-and-branding.md).

[THIRD-PARTY-NOTICES.md](../../THIRD-PARTY-NOTICES.md) describes the reviewed
448-entry inventory, Linux graph, all 35 missing-root cases, preserved license
texts and remaining native source/trademark obligations. Five Linux missing-root
cases have retained notices; 30 are outside Linux and remain out of scope for
other platform redistribution. Public release clearance is still blocked.

`python3 scripts/check-licenses.py` validates reproducibility and package mappings.
`--release` additionally rejects every open item in redistribution-status.json.
A passing PR check does not authorize creating a public release or waive legal
obligations. Native copyright text preservation is not exact source verification.
