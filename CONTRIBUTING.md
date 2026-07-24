# Contributing

Pit welcomes bug reports, focused feature proposals, documentation improvements, and code contributions that preserve the public/private repository boundary.

## Before opening a change

- Search existing issues.
- For substantial command-surface, classification, transaction, or push-validation changes, open an issue first so the contract can be agreed before implementation.
- Keep private companion materials (`AGENTS.md`, `private/**`, local policy under `.git/pit/`) out of the public tree.
- Prefer explicit, recoverable behavior over silent dual-repo magic.

## Development setup

Install a recent stable Rust toolchain and system Git.

```bash
git clone https://github.com/iamkaf/pit.git
cd pit
cargo test
cargo build --release
./target/release/pit --help
```

Optional: GitHub CLI (`gh`) if you are exercising `pit setup --create-github`.

## Making changes

1. Fork the repository and create a branch from `main`.
2. Implement the change against the real CLI entry points (`src/main.rs` and `src/commands/`).
3. Add or extend tests that drive the shipped `pit` binary or library surface. Prefer temporary local bare public/private remotes over mocks that re-implement Git.
4. Run the full suite:

```bash
cargo test
cargo build --release
```

5. Open a pull request that explains user-visible behavior, privacy-boundary impact, and how you verified the change.

## Change expectations

- Private file contents must never enter the public object database through Pit commands.
- New unclassified paths remain fail-closed under `reject` / non-interactive `prompt`.
- `pit push` must keep private-first ordering and outbound public range validation.
- Dual-tracked paths must block unsafe commit/push until repaired.
- Public-only clones must keep working without Pit.
- Hook changes must chain existing user hooks and must not silently commit or push.
- JSON output should keep the versioned envelope: `schema_version`, `command`, `ok`, `data`.

## Code style

- Run `cargo fmt` before committing.
- Prefer small modules and explicit `git` argv arrays with `--git-dir` / `--work-tree` for private operations.
- Clear Git environment pollution (`GIT_INDEX_FILE`, `GIT_DIR`, and similar) when invoking private Git from hooks.
- Keep dependencies minimal.

## What to work on

Useful areas include:

- Remaining roadmap items that are not yet implemented (collaboration polish, CI hydration, installers).
- Boundary-transition edge cases (`protect` / `reveal` history warnings, recovery UX).
- Cross-platform path and case-collision coverage.
- Documentation and reproducible privacy-boundary examples.

If you are unsure whether a change belongs in the public CLI versus private notes, open an issue first.

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
