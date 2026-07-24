# Pit

**One working tree, two repositories, one safe workflow.**

Pit keeps public and private files in the same project directory while storing them in separate Git repositories. The ordinary root `.git/` remains a normal public repository. A private overlay under `.git/pit/` tracks protected paths and Pit metadata.

Public contributors clone the public repo with ordinary Git and never need Pit. Authorized users connect a private companion repository for protected material.

## Requirements

- [Git](https://git-scm.com/) installed and on `PATH`
- Rust toolchain (to build from source)
- Optional: [GitHub CLI](https://cli.github.com/) (`gh`) for assisted private companion creation

## Install

```bash
cargo install --path .
# or
cargo build --release
# binary: target/release/pit
```

## Quick start

```bash
# In an existing Git repo:
pit setup --private /path/to/private-remote.git --yes
# or create a private GitHub companion:
pit setup --create-github --yes

mkdir -p src private
printf 'export const answer = 42;\n' > src/index.ts
printf 'internal notes\n' > private/notes.txt

pit add .
pit status
pit commit -m "Add public implementation and private notes"
pit push
```

Result:

- Public changes go to the public remote only.
- Private paths go to the private companion only.
- Private patterns are written into a managed block in `.git/info/exclude`, so plain `git add .` skips them.
- `pit push` validates the outgoing public history before publishing (private remote first).

## Commands

| Command | Purpose |
|---|---|
| `pit setup` | Connect private remote, policy, excludes, hooks; hydrate if companion has content |
| `pit clone` | Clone public repo; optional private setup/hydrate |
| `pit status` | Public / private / unclassified / transactions |
| `pit add` / `restore --staged` | Classify/stage; unstage correct index |
| `pit diff` | Public and/or private diffs |
| `pit commit` | Logical transaction (0–1 public + 0–1 private) |
| `pit push` / `pull` / `switch` | Publish, fetch both, mapped branches |
| `pit protect` / `reveal` / `ignore` | Boundary transitions |
| `pit hooks` / `doctor` | Hook lifecycle; health (`--repair` for reversible fixes) |
| `pit transaction` / `config` | Journals; local config |

Global flags: `--json`, `--yes`, `--dry-run`, `--verbose`, `--quiet`.

Machine-readable output uses a stable envelope: `schema_version`, `command`, `ok`, `data`.

## Security model

- **Repository boundary** is the access-control boundary (not hooks alone).
- Fail closed on unclassified new paths (`reject` / non-interactive `prompt`).
- Private push before public push.
- Independent outbound public commit-range walk before every public push.
- Durable transaction journals under `.git/pit/transactions/`.
- `pit protect` warns when a path exists in public history and **never claims erasure**.

Pit does **not** encrypt the working tree, stop local filesystem readers, or rewrite already-published public history. Hooks are bypassable; `pit push` re-checks.

See [SECURITY.md](./SECURITY.md) for supported versions, in-scope issues, and private reporting. See [CONTRIBUTING.md](./CONTRIBUTING.md) for development setup and pull-request expectations.

## Development

```bash
cargo test
cargo build --release
./target/release/pit doctor
```

Integration tests use temporary local bare public/private remotes and drive the real `pit` binary (including canary non-leakage and private `pull` hydration).

## License

MIT. See [LICENSE](./LICENSE).
