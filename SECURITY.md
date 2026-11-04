# Security policy

Codex Dock handles local sign-in credentials and shared chat data. Account
mix-ups, unsafe file operations, token disclosure, and unintended process or
network execution are security-sensitive.

## Report privately

Use this repository's **Security → Report a vulnerability** button when available.
Include affected versions, reproduction steps using synthetic data, expected
behavior, and the impact you observed. Never send live credentials or chat data.

If private reporting is not enabled, ask a maintainer for a private contact
channel without posting the vulnerability details publicly. Do not open a public
issue containing an exploit, credential, or another person's private data.

There is no guaranteed response or patch deadline. Please reproduce against the
latest release or current `main` and identify the exact commit you tested.

## Maintainer handling

Keep sensitive reproduction material private. Establish impact, affected
versions and a regression test using synthetic fixtures. Coordinate disclosure
and a patch before publishing exploit details. If credentials were exposed,
remove public copies and revoke or rotate the affected credentials; deleting a
line from the latest commit does not remove it from repository history.

Dependency audit and CodeQL jobs supplement review; a green build is not a
guarantee that account handling or migration is secure. Never add a broad audit
exception to make CI green. A narrowly scoped exception needs an advisory ID,
reason, owner, review date, and a link to the tracking work.
