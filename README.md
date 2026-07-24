# Pit

**One working tree, two repositories, one safe workflow.**

Pit keeps public and private files in the same project directory while storing them in separate Git repositories. The ordinary root `.git/` remains a normal public repository. A private overlay under `.git/pit/` tracks protected paths and Pit metadata.

## Install

```bash
cargo install --path .
# or
cargo build --release
# binary: target/release/pit
```

Requires system `git`. Optional: GitHub CLI (`gh`) for assisted private companion creation.

## Quick start

```bash
# In an existing Git repo:
pit setup --private /path/to/private-remote.git --yes
# or
pit setup --create-github --yes

mkdir -p src private
printf 'export const answer = 42;\n' > src/index.ts
printf 'secret notes\n' > private/notes.txt

pit add .
pit status
pit commit -m "Add public implementation and private notes"
pit push
```

Result:

- Public changes go to the public remote only.
- Private paths go to the private companion only.
- Private patterns are written into a managed block in `.git/info/exclude`, so plain `git add .` skips them.
- `pit push` validates the outgoing public history before publishing.

## Commands (Phase 1)

| Command | Purpose |
|---|---|
| `pit setup` | Connect private remote, policy, excludes, hooks |
| `pit status` | Public / private / unclassified / transactions |
| `pit add` | Classify and stage into the correct index |
| `pit commit` | One logical transaction (0–1 public + 0–1 private commits) |
| `pit push` | Private first, then public; resumable on partial failure |
| `pit doctor` | Health and privacy invariant checks |

Global flags: `--json`, `--yes`, `--dry-run`, `--verbose`, `--quiet`.

## Security model

- **Repository boundary** is the access-control boundary (not hooks alone).
- Fail closed on unclassified new paths.
- Private push before public push.
- Independent outbound public commit-range walk before every public push.
- Durable transaction journals under `.git/pit/transactions/`.

Pit does **not** encrypt the working tree or stop local filesystem readers. Hooks are bypassable; `pit push` re-checks.

## Development

```bash
cargo test
```

## License

MIT
