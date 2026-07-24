# Security policy

## What Pit protects

Pit is a **workflow and leak-prevention tool** around two Git repositories:

- Private file contents should not enter the **public** index, object database, commits, or remotes when using Pit commands.
- `pit push` validates the exact outbound public commit range (`remote..HEAD`) before any public push.
- Managed excludes and client hooks reduce accidental staging/pushes; they are **not** a hard security boundary.

## What Pit does not protect

- Local filesystem readers (other users/processes with disk access).
- Bypassing hooks or invoking raw Git plumbing against the public repo.
- Content that was **already published** publicly (protect does not erase history).
- Credentials: Pit does not store tokens; use Git/SSH/OS credential helpers.
- Build tools, IDE indexes, archives, or backups that scan the work tree.

## Reporting a vulnerability

If you believe you have found a security issue in Pit (for example a way for a Pit command to write private content into the public object database or remote):

1. Prefer a **private** report via GitHub Security Advisories on this repository, or email the maintainer listed in `Cargo.toml` / GitHub profile.
2. Include a minimal reproduction with local bare remotes if possible.
3. Do **not** open a public issue with exploit details until a fix is available.

We aim to acknowledge reports promptly and prioritize boundary-integrity bugs.

## Safe dogfooding tips

- Keep instruction files and secrets under private classification / policy patterns.
- Run `pit doctor` before publishing.
- Prefer `pit push` over direct `git push` in Pit workspaces.
- After `pit protect`, assume historical public exposure still exists.
