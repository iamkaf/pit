# Security policy

Pit coordinates public and private Git repositories over one working tree. Please do not disclose a suspected vulnerability in a public issue, discussion, pull request, or chat before it has been investigated and fixed.

## Reporting a vulnerability

Use [GitHub private vulnerability reporting](https://github.com/iamkaf/pit/security/advisories/new) to send the report directly to the maintainer. If that form is unavailable, contact [@iamkaf](https://github.com/iamkaf) without including vulnerability details so a private channel can be established.

Please include:

- The affected Pit version or commit.
- Clear reproduction steps, preferably with temporary local bare public/private remotes.
- The security impact and any conditions required to trigger it.
- Relevant logs or artifacts with credentials, tokens, and personal information removed.

You should receive an acknowledgement within seven days. Updates will continue through the private advisory while the report is reproduced, assessed, and fixed.

## Supported versions

Pit is pre-1.0 software. Security fixes target the latest published release (when one exists) and the current `main` branch. Older snapshots and superseded prereleases are not maintained separately; affected users should upgrade to the next compatible release.

## Security-sensitive areas

Reports are especially useful when they involve:

- A Pit command writing private file contents into the public index, object database, commits, tags, or remotes.
- Private path names or private policy/remote configuration landing in public tracked files by default.
- `pit push` publishing public history without validating the exact outbound public range (`remote..HEAD`).
- Unclassified or protected paths being staged or committed publicly under fail-closed policy without an explicit force flag.
- Hook or dispatcher behavior that silently creates commits, stages private content, or allows incomplete dual-repo publication without a clear error.
- Recovery or resume paths that rewrite successful remote state or drop durable transaction journals unsafely.
- Credential, token, or secret material stored by Pit outside the mechanisms Git and the OS already provide.

## Out of scope

The following are outside Pit’s supported threat model (unless a Pit command itself causes them):

- Local processes that can already read the working tree filesystem.
- Deliberate use of Git plumbing to bypass hooks or the public object database boundary.
- Content that was already published to a public remote, fork, cache, or package registry.
- Third-party tools (IDEs, archives, backups, search indexes) that scan the work tree independently of Pit.
