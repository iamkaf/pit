# Pit — Product Requirements Document

**Working name:** Pit (`pit`, short for “private Git”)  
**Status:** Draft v0.1  
**Date:** 2026-07-24  
**Audience:** Product, CLI engineering, VS Code extension engineering, and security reviewers  
**Tagline:** One working tree, two repositories, one safe workflow.

---

## 1. Executive summary

Pit is a local-first developer tool that lets a project contain public and private files in one working directory while storing them in separate Git repositories.

A normal public Git repository remains fully usable by anyone. Authorized developers connect a second private companion repository that tracks protected files and Pit metadata. Pit coordinates classification, staging, commits, branches, pushes, pulls, recovery, and editor integration across both repositories.

The intended experience is:

```bash
pit clone https://github.com/iamkaf/my-awesome-repo.git
cd my-awesome-repo
pit setup

# Work on public and private files in the same directory.
pit add .
pit commit -m "Add the new feature and private deployment notes"
pit push
```

The result is:

- Public changes are committed and pushed to `iamkaf/my-awesome-repo`.
- Private changes are committed and pushed to an authorized private companion repository.
- Private files are generated into `.git/info/exclude`, so ordinary `git add .` does not stage them.
- Local Git hooks block common accidental leaks.
- `pit push` independently validates the outgoing public history before sending it.
- A VS Code extension exposes setup, classification, staging, commits, diffs, pushes, recovery, and privacy warnings in the editor.

### Core product decision

The MVP uses a **public-first private overlay** architecture:

- The existing root `.git/` remains the ordinary public repository.
- A second Git directory under `.git/pit/` tracks only private-classified paths and private Pit metadata.
- Both repositories operate over the same working tree.
- Pit commands are the supported porcelain for operations that may affect both repositories.

In this document, **private mirror** means this companion private overlay repository. It is not required to be a byte-for-byte mirror of the public repository in the MVP. A full private mirror mode may be added later.

### Command compatibility decision

For the MVP:

- `git status`, `git diff`, `git log`, and `git blame` continue to show the public repository normally.
- `git add` and `git commit` operate on public files only.
- `pit add`, `pit commit`, `pit push`, `pit pull`, and `pit switch` coordinate both repositories.
- Direct `git push` is blocked by default in Pit-enabled workspaces to prevent incomplete or unsafe publication. The hook explains how to use `pit push`.
- A later native-integration phase may add a `git-remote-pit` helper so plain `git push` can safely route through Pit.

This choice keeps the MVP explicit, recoverable, and understandable. Hooks must not silently create hidden commits or pushes.

---

## 2. Problem statement

Git hosting platforms generally apply visibility at the repository boundary. A repository is public or private; individual paths do not receive independent access control.

Developers often want to keep a public project together with related private material such as:

- Internal design notes
- Private benchmarks and datasets
- Customer-specific fixtures
- Deployment configuration
- Security research notes
- Proprietary modules
- Unpublished roadmap material
- Local notes, scratchpads, or tooling context
- Private documentation that must evolve with public source code

The usual workarounds are fragmented and error-prone:

- Keep private files outside the repository and lose project-relative organization.
- Use a separate private repository and manually synchronize it.
- Put local paths in `.git/info/exclude`, which prevents ordinary staging but provides no private version history or coordinated workflow.
- Use hooks, which are useful guardrails but are bypassable and do not create an access-control boundary.
- Encrypt files in the public repository, which still exposes filenames, metadata, encrypted blobs, and operational complexity.
- Make the entire project private, which prevents public collaboration and distribution.

Pit should make the secure two-repository approach feel like one coherent local project without pretending that Git itself supports per-file permissions.

---

## 3. Product goals

Pit must:

1. Keep private file contents out of the public repository’s index, object database, commits, refs, and remotes.
2. Let authorized users edit public and private files in one normal working directory.
3. Preserve the public repository as an ordinary Git repository for contributors who do not use Pit.
4. Make classification, staging, committing, pushing, pulling, branching, and recovery understandable and predictable.
5. Fail closed when Pit cannot safely classify or publish a path.
6. Use existing Git authentication mechanisms instead of storing credentials.
7. Provide strong protection against accidental disclosure while clearly documenting what Pit cannot protect against.
8. Expose a stable machine-readable CLI contract for editor integrations and automation.
9. Provide a first-party VS Code extension that makes the dual-repository state visible.
10. Remain local-first and require no Pit-hosted cloud service.

---

## 4. Non-goals

The MVP is not intended to:

- Add real per-file authorization inside one Git repository.
- Encrypt the local working tree.
- Protect private files from another user or process that can read the local filesystem.
- Prevent a determined user from bypassing hooks or invoking Git plumbing commands directly.
- Make previously published data private again.
- Guarantee atomic transactions across two independent Git servers.
- Replace a secrets manager for credentials used at runtime.
- Automatically rewrite public history or erase data from forks, caches, releases, artifacts, package registries, or clones.
- Transparently support every advanced Git operation in the first release.
- Hide the existence of Pit from an authorized local user.
- Prevent non-Git tools, build scripts, archives, backups, IDEs, search indexes, or shell commands from reading or copying private files.

Pit is a privacy-oriented workflow and leak-prevention tool, not a new cryptographic storage system.

---

## 5. Personas and primary user stories

### 5.1 Solo maintainer

A maintainer has a public open-source repository and wants private notes, fixtures, configuration, or roadmap files beside the public code.

**Success condition:** The maintainer can clone, edit, commit, and push both classes of files without manually changing directories or juggling two worktrees.

### 5.2 Authorized collaborator

A collaborator has access to both the public project and its private companion repository.

**Success condition:** `pit clone` and `pit setup` hydrate the complete authorized working tree and keep both histories aligned.

### 5.3 Public contributor

A contributor clones only the public repository and does not install Pit.

**Success condition:** The repository behaves like a normal public Git project, with no required proprietary tooling and no broken tracked files.

### 5.4 Security-conscious maintainer

A maintainer wants proof that protected paths and known canary content are not present in outbound public history.

**Success condition:** `pit push` performs an independent outbound validation and refuses publication on any violation.

### 5.5 VS Code user

A developer wants to see public, private, staged, unclassified, and conflicted changes without leaving the editor.

**Success condition:** The extension displays both repositories, offers one coordinated commit and push workflow, and gives clear privacy-boundary warnings.

---

## 6. Terminology

| Term | Meaning |
|---|---|
| Public repository | The ordinary root Git repository stored at `.git/` and pushed to the public remote. |
| Private mirror | The companion private overlay repository managed under `.git/pit/`. It tracks private paths and private Pit metadata. |
| Working tree | The shared project directory containing both public and private files. |
| Public path | A path tracked only by the public repository. |
| Private path | A path tracked only by the private mirror and excluded from the public repository. |
| Ignored path | A path tracked by neither repository. |
| Unclassified path | A new path that Pit cannot safely assign to public, private, or ignored based on current policy. |
| Privacy boundary change | Moving a path from public to private or private to public. |
| Logical transaction | One user operation represented by zero or one public commit and zero or one private commit. |
| Pending transaction | A logical transaction that has not completed all required local or remote steps. |
| Hydration | Materializing authorized private files into an existing public working tree. |

---

## 7. Security invariants

These invariants are mandatory and should be enforced in code and tests.

1. A path may not be tracked by both repositories at the same logical revision.
2. Private file contents must never be written to the public Git object database by a Pit command.
3. Protected path names must not appear in public commits, trees, tags, or generated tracked configuration.
4. Private policy and private remote configuration must not be committed to the public repository by default.
5. Pit must not store personal access tokens, passwords, SSH private keys, or reusable credentials.
6. `pit push` must validate the exact outgoing public commit range even when hooks were bypassed.
7. No public push may begin while a known unresolved classification or local transaction error exists.
8. A private push must occur before its related public push.
9. A failed second push must be resumable without silently rewriting successful remote state.
10. Moving an already-public path to private must never be described as making its historical contents private.
11. Setup must not overwrite existing hooks, hook paths, user exclude rules, remotes, branches, or configuration without explicit handling.
12. Diagnostics and telemetry must never include file contents. Private filenames and remote URLs must be redacted by default in shareable diagnostic output.

---

## 8. Repository and local-state model

A Pit-enabled checkout should have a layout conceptually similar to:

```text
my-awesome-repo/
├── .git/                         # Ordinary public repository
│   ├── info/
│   │   └── exclude              # User rules plus a Pit-managed block
│   └── pit/
│       ├── config.toml           # Local Pit configuration; never tracked
│       ├── state.json            # Current state machine snapshot
│       ├── policy.toml           # Local cache of private policy
│       ├── private.git/          # Private Git directory and index
│       ├── hooks/                # Pit hook dispatcher/shims
│       ├── transactions/         # Durable transaction journals
│       ├── locks/                # Process locks
│       └── logs/                 # Redacted operational logs
├── src/                          # Public
├── README.md                     # Public
├── private/                      # Private
├── notes/internal.md             # Private
└── .env                          # Private or ignored, per policy
```

### 8.1 Public repository

The root `.git/` remains an ordinary Git repository. Pit must not change its object format, refs, commit shape, or remote protocol.

### 8.2 Private mirror

The private mirror uses a separate Git directory and index while sharing the root working tree. It tracks only:

- Private-classified project paths
- Private policy metadata required to hydrate another authorized checkout
- Private transaction metadata, when needed

The private mirror must never require public contributors to install Pit.

### 8.3 Policy storage

The authoritative privacy policy should be versioned in the private mirror. A local cache may live under `.git/pit/`.

The public repository must not contain the list of private paths unless a user explicitly chooses a public policy mode.

A policy example:

```toml
version = 1

[classification]
new_files = "prompt"   # prompt | public | private | reject

[private]
patterns = [
  ".env",
  ".env.*",
  "private/**",
  "notes/internal/**",
  "config/*.secret"
]

[ignored]
patterns = [
  ".DS_Store",
  "tmp/**",
  "dist/**"
]

[public]
patterns = [
  "README.md",
  "LICENSE",
  "src/**",
  "docs/public/**"
]
```

Pattern semantics should follow Git wildmatch behavior closely enough to be familiar, while Pit owns the final parser and test suite. Negation and precedence rules must be explicitly documented.

### 8.4 Generated `.git/info/exclude` block

Pit should generate a clearly delimited section while preserving all user entries:

```gitignore
# Existing user entries remain untouched.
*.local-scratch

# BEGIN PIT MANAGED — DO NOT EDIT BY HAND
.env
.env.*
private/**
notes/internal/**
config/*.secret
.git/pit-worktree-metadata/**
# END PIT MANAGED
```

Pit must update this block atomically and must not reorder or delete user-managed lines.

---

## 9. Classification model

### 9.1 Existing paths

- A path already tracked by the public repository is public unless the user explicitly runs `pit protect`.
- A path already tracked by the private mirror is private unless the user explicitly runs `pit reveal`.
- A path tracked by both is an invariant violation and blocks commits and pushes until repaired.

### 9.2 New paths

New untracked paths should be resolved in this order:

1. Explicit command flag, such as `pit add --private path`.
2. Exact or pattern match in the private policy.
3. Exact or pattern match in the ignored policy.
4. Exact or pattern match in the public policy.
5. Configured default behavior.

The safe default is `prompt` or `reject`, not silently public.

In an interactive terminal, `pit add .` may ask:

```text
Unclassified files:
  docs/customer-rollout.md
  src/new-module.ts

Classify docs/customer-rollout.md:
  [p]rivate  [u]public  [i]gnore  [s]kip
```

In non-interactive mode, unresolved paths must fail with a nonzero exit code and a machine-readable error.

### 9.3 Boundary transitions

#### `pit protect <path>`

Moves a path from public tracking to private tracking.

Required behavior:

- Detect whether the path exists anywhere in public local or remote history.
- Display a prominent warning when prior public exposure is detected.
- Remove the path from the public index without deleting the working file.
- Add the path to private policy and the private index.
- Update `.git/info/exclude`.
- Require a coordinated Pit commit.
- Never claim that prior public copies have been erased.

#### `pit reveal <path>`

Moves a path from private tracking to public tracking.

Required behavior:

- Show the exact path and a summary of outgoing content risk.
- Run configured secret/content scanners before allowing the transition.
- Require explicit confirmation unless `--yes` is used in an approved automation context.
- Remove the path from private tracking and policy.
- Add the path to the public index.
- Update `.git/info/exclude`.
- Create a coordinated transaction.

#### `pit ignore <path>`

Stops tracking a path in either repository while preserving the working file, with a clear warning if the path has existing history.

---

## 10. Primary workflows

### 10.1 Clone and setup

```bash
pit clone https://github.com/iamkaf/my-awesome-repo.git
cd my-awesome-repo
pit setup
```

`pit clone` should:

1. Clone the public repository using the installed Git client.
2. Enter or identify the checkout.
3. Run the setup flow unless `--no-setup` is passed.
4. Never execute repository-provided scripts or hooks automatically.

`pit setup` should:

1. Verify that the directory is a supported Git working tree.
2. Detect an existing Pit configuration and offer resume/repair behavior.
3. Ask whether to connect an existing private repository or create one through a supported provider integration.
4. Reuse Git, SSH, credential-helper, or provider CLI authentication.
5. Verify that the private destination is not publicly readable before sending private content.
6. Initialize or fetch the secondary Git directory.
7. Load or create private policy.
8. Hydrate private files without overwriting conflicting public files.
9. Generate the managed exclude block.
10. Install non-destructive hook dispatchers.
11. Run `pit doctor`.
12. Present a concise success summary and next commands.

Example setup flow:

```text
Public repository: github.com/iamkaf/my-awesome-repo

Private repository:
  1. Create iamkaf/my-awesome-repo-private on GitHub
  2. Connect an existing Git remote
  3. Initialize locally and add a remote later

Private visibility verified: yes
Default handling for new files: ask before staging
Hooks installed: pre-commit, pre-push, post-checkout, post-rewrite
Workspace health: OK
```

### 10.2 Daily edit, stage, commit, and push

```bash
pit status
pit add .
pit commit -m "Implement billing export"
pit push
```

Expected result:

- Public changes are staged in the public index.
- Private changes are staged in the private index.
- One logical transaction is created.
- Private commits contain private paths only.
- Public commits contain public paths only.
- The private remote is pushed first.
- The public remote is pushed only after outbound validation succeeds.

### 10.3 Pull and hydrate

```bash
pit pull
```

`pit pull` should:

1. Refuse or offer an explicit autostash when either repository has uncommitted changes.
2. Fetch both remotes.
3. Update the public branch.
4. Update the mapped private branch.
5. Rehydrate private paths.
6. Detect path overlap, boundary conflicts, or policy changes.
7. Resume or flag any pending transaction.
8. Refresh editor state and hook configuration.

### 10.4 Branch switching

```bash
pit switch feature/new-export
pit switch -c feature/new-export
```

The default private branch name should match the public branch name.

Pit must:

- Switch both repositories as one operation.
- Create a private branch when needed according to policy.
- Detect uncommitted state in both repositories.
- Prevent a partial switch from being presented as successful.
- Journal and recover from a failure after one side switches.

A direct `git switch` or `git checkout` may still occur. The post-checkout hook must detect drift and mark Pit state as requiring reconciliation before the next coordinated commit or push.

### 10.5 Failure recovery

If the private push succeeds and the public push fails:

```text
Private repository: pushed successfully
Public repository: push rejected

Transaction 7fca1b9d is pending public publication.
No private data was sent to the public remote.
Run: pit push --resume
```

Pit must preserve enough durable state to retry safely after process termination, network failure, machine restart, or editor crash.

---

## 11. CLI requirements

### 11.1 Global behavior

Every command must support, where applicable:

- `--json` for stable machine-readable output
- `--no-color`
- `--verbose`
- `--quiet`
- `--dry-run`
- `--yes` for explicitly approved non-interactive flows
- Deterministic nonzero exit codes

The CLI must never require a shell-specific runtime for normal operation.

### 11.2 Required MVP commands

| Command | Requirement |
|---|---|
| `pit clone <public-url>` | Clone the public repository and optionally begin setup. |
| `pit setup` | Connect or create the private mirror, load policy, hydrate files, install hooks, and validate health. |
| `pit status` | Show public, private, ignored, unclassified, conflicted, and transaction state. |
| `pit add <pathspec...>` | Classify and stage changes in the correct index. |
| `pit restore --staged <pathspec...>` | Unstage from the correct index without changing the working file. |
| `pit diff [<pathspec...>]` | Show public or private diffs without leaking private contents into public Git state. |
| `pit commit` | Create one logical transaction across zero, one, or two physical commits. |
| `pit push` | Preflight, validate, push private first, push public second, and journal recovery state. |
| `pit pull` | Fetch and update both repositories safely. |
| `pit switch` | Switch or create mapped public/private branches. |
| `pit protect <path>` | Move a path from public to private with history warnings. |
| `pit reveal <path>` | Move a path from private to public with explicit review and scanning. |
| `pit ignore <path>` | Stop tracking a path in either repository while preserving it locally. |
| `pit doctor` | Validate invariants, hooks, excludes, branches, remotes, policy, history exposure, and pending transactions. |
| `pit hooks install\|status\|repair\|uninstall` | Manage hook integration without destroying user hooks. |
| `pit transaction list\|show\|resume\|abort` | Inspect and recover logical transactions. |
| `pit config get\|set\|list` | Manage local configuration. |

### 11.3 Useful post-MVP commands

- `pit stash`
- `pit clean`
- `pit merge`
- `pit rebase`
- `pit mv --private|--public`
- `pit verify-public`
- `pit history check`
- `pit scrub-plan`
- `pit ci hydrate`
- `pit aliases install`
- `pit remote verify`

### 11.4 `pit status`

Human-readable output should group state clearly:

```text
On branch main
Public:  origin/main
Private: private/main
Health:  OK

Public changes:
  staged:     src/export.ts
  unstaged:   README.md

Private changes:
  staged:     notes/internal-export-plan.md
  unstaged:   .env.local

Unclassified:
  docs/customer-example.md

Transactions:
  none pending
```

The command must not require network access.

### 11.5 `pit add`

Required forms:

```bash
pit add .
pit add src/app.ts
pit add --private docs/internal.md
pit add --public examples/demo.json
pit add --ignore tmp/output.log
pit add -A
```

Behavior:

- Use NUL-safe path handling internally.
- Respect pathspecs and filesystem case behavior.
- Classify before staging.
- Refuse ambiguous or conflicting paths.
- Never use the public object database to hash private content.
- Display a concise classification summary before mutating state when interactive.
- Support an atomic rollback if staging one index fails after staging the other.

Interactive patch staging may be deferred, but the design must leave room for `pit add -p`.

### 11.6 `pit commit`

Required forms:

```bash
pit commit -m "Message"
pit commit
pit commit --amend
```

Behavior:

- Use one commit message for both physical commits by default.
- Open the configured Git editor when no message flag is supplied.
- Run public and private validation before creating either commit.
- Snapshot refs and index state before mutation.
- Create the public commit and private commit as a recoverable local transaction.
- Roll back local refs and restore staging state if commit creation fails before the transaction is finalized.
- Preserve author identity and a consistent timestamp where practical.
- Never put a private commit identifier, private path, or Pit transaction identifier into the public commit unless explicitly configured.
- Store public linkage in the private commit or private transaction metadata only.

Suggested private commit trailers:

```text
Pit-Transaction: 7fca1b9d-2ad4-4a30-a520-cc875d25b833
Pit-Public-Commit: a8be019b0b3f...
```

Commit cases:

| Staged state | Result |
|---|---|
| Public only | One public commit; private state records the new public base. |
| Private only | One private commit linked to current public `HEAD`. |
| Both | One public commit and one private commit in one logical transaction. |
| Neither | Fail without creating a commit. |

`--amend` should be limited to unpushed transactions in the MVP. Pushed-history rewriting requires explicit later design.

### 11.7 `pit push`

`pit push` is the critical publication boundary.

Required sequence:

1. Acquire an exclusive workspace lock.
2. Load and validate local transaction state.
3. Fetch or otherwise verify current remote tips.
4. Confirm both pushes are fast-forward unless an explicit lease-protected override is used.
5. Calculate the exact outgoing public commit range.
6. Walk all outgoing public commit trees and reject any protected or dual-tracked path.
7. Run configured public-content scanners.
8. Verify private remote visibility when the provider supports it.
9. Push the private branch or refs.
10. Persist private-push success durably.
11. Push the public branch or refs.
12. Mark the transaction complete.
13. Refresh state and report both remote outcomes.

Rules:

- If private push fails, do not attempt the public push.
- If public push fails after private succeeds, preserve a resumable pending state.
- Never automatically force-push.
- `--force-with-lease` must validate both remotes separately and display a high-severity warning.
- A successful public push must mean the exact outbound range passed Pit’s independent privacy validation.

### 11.8 `pit doctor`

`pit doctor` should check:

- Required Git capabilities
- Local state schema and migrations
- Public and private Git directory health
- Remote configuration and reachability
- Private remote visibility, when verifiable
- Branch mapping
- Dual-tracked paths
- Missing private files
- Policy parse errors
- Managed exclude-block integrity
- Hook dispatcher integrity
- Existing hooks that are not being chained
- Pending or orphaned transactions
- Protected paths found in local public history
- Protected paths found in outbound public history
- Public files that depend on absent private files, when `verify-public` integration is enabled
- Case-collision and path-normalization hazards
- Unsupported Git LFS, submodule, nested-repository, worktree, or sparse-checkout combinations

`pit doctor --repair` may repair only reversible local configuration. It must not rewrite history, delete data, change remote visibility, or force-push without a separate explicit command.

---

## 12. Logical transaction model

Two Git servers cannot provide one atomic cross-repository commit or push. Pit must therefore implement a durable, explicit transaction state machine.

Suggested local states:

```text
new
prepared
local-public-committed
local-private-committed
local-complete
private-push-started
private-pushed
public-push-started
complete
failed-recoverable
failed-manual
```

Each journal should include:

- Transaction UUID
- Creation time
- Public and private branch names
- Before and after refs
- Public and private commit IDs, when created
- Public base linked from private state
- Staged path summaries
- Validation results
- Push attempts and remote results
- Recovery instructions
- State schema version

Journals must be written atomically and fsynced where supported before advancing to an irreversible remote step.

Public commits should remain ordinary and free of private linkage. Private state may reference public commits.

---

## 13. Hooks integration

Hooks are defense in depth, not the security boundary.

### 13.1 Required hooks

| Hook | Pit behavior |
|---|---|
| `pre-commit` | Reject protected or dual-tracked paths staged in the public index. Reject commits while Pit state is inconsistent. |
| `pre-push` | Validate outbound public refs or block direct pushes and instruct the user to run `pit push`. |
| `post-checkout` | Detect public/private branch drift and refresh generated excludes. |
| `post-merge` | Revalidate path classification and policy. |
| `post-rewrite` | Mark commit mappings stale after amend or rebase and require reconciliation. |

### 13.2 Hook requirements

- Preserve and chain existing hooks.
- Detect and respect an existing `core.hooksPath`.
- Use small dispatchers that call `pit hook <hook-name> ...`.
- Provide install, repair, status, and uninstall commands.
- Never silently stage files, create commits, or push from a hook.
- Fail closed for `pre-commit` and `pre-push` when a Pit workspace is active but the Pit binary or state is unavailable.
- Make bypass limitations explicit. A user can bypass client-side hooks, so `pit push` must repeat critical checks independently.

### 13.3 Direct Git commands

Default policy in the MVP:

- Plain `git add` and `git commit` are allowed for public-only work.
- Plain `git push` is blocked in a Pit-enabled repository unless a local configuration explicitly allows public-only direct pushes.
- A direct public commit updates Pit’s observed public base. The next `pit status` must detect it.
- A direct branch switch marks the private branch mapping as potentially stale.
- Pit should explain state changes instead of treating ordinary Git use as corruption.

---

## 14. Authentication and private-repository setup

### 14.1 Authentication principles

Pit must delegate authentication to existing mechanisms:

- SSH agent and SSH configuration
- Git credential helpers
- Operating-system credential storage used by Git
- Provider CLIs such as GitHub CLI, when available

Pit must not write raw tokens into `.git/config`, `.git/pit/config.toml`, logs, command history suggestions, or extension settings.

### 14.2 Provider-neutral core

The core product must accept any Git remote URL supported by the installed Git client.

```bash
pit setup --private git@github.com:iamkaf/my-awesome-repo-private.git
```

### 14.3 GitHub-assisted flow

An optional GitHub integration may:

- Detect the owner and repository name from the public remote.
- Suggest `<public-name>-private`.
- Create a private repository after explicit confirmation.
- Verify the repository visibility and current user access.
- Refuse to upload private content if visibility cannot be verified as private.

The generic flow must remain available without GitHub-specific dependencies.

### 14.4 Remote verification

Before the first private push, Pit should record one of:

- `verified-private`: provider API confirmed private visibility.
- `user-attested-private`: generic remote; user explicitly confirmed access restrictions.
- `unverified`: publication blocked by default.

A visibility change detected later must block private pushes and show a high-severity error.

---

## 15. Public outbound validation

Before every public push, Pit must validate the outgoing commit graph, not only the current index.

Minimum checks:

1. Enumerate commits reachable from the proposed public update but not the current remote tip.
2. Enumerate every tree entry in those commits.
3. Reject protected paths, private metadata paths, or dual-tracked paths.
4. Reject unresolved merge entries.
5. Detect paths that were added and later deleted within the same outgoing range.
6. Inspect symlink targets for references to known private paths and warn or reject according to policy.
7. Run configured content scanners over new public blobs.
8. Produce a redacted audit result stored in the transaction journal.

The scanner interface should support external tools without requiring one in the core MVP:

```toml
[scanners.gitleaks]
command = ["gitleaks", "protect", "--stdin"]
required = false
```

Pit’s core guarantee is path and repository-boundary enforcement. Secret scanning is additional defense in depth and must not be marketed as complete.

---

## 16. Collaboration and CI

### 16.1 Public contributors

A public-only clone must:

- Require no Pit configuration.
- Contain no broken symlinks or tracked references that are required solely for private users, unless intentionally documented.
- Build and test independently when the project promises that capability.
- Never expose private repository URLs or path policy by default.

### 16.2 Authorized collaborators

An authorized collaborator should be able to run:

```bash
pit clone <public-url> --private <private-url>
```

Pit should fetch both repositories, verify policy, and hydrate the working tree.

### 16.3 CI hydration

A later MVP-complete or near-v1 feature should support:

```bash
pit ci hydrate --private-url "$PIT_PRIVATE_URL"
```

Requirements:

- Use a deploy key or token supplied by the CI environment.
- Never print credentials.
- Checkout public content first, then private overlay content.
- Verify the private remote.
- Support read-only hydration for build and test jobs.
- Avoid requiring the public workflow to disclose the private repository’s existence.

A first-party GitHub Action may wrap the CLI later, but the CLI remains the source of truth.

---

## 17. VS Code extension requirements

### 17.1 Product role

The VS Code extension is a thin client over the Pit CLI. It must not independently reimplement classification, transaction, validation, or Git mutation logic.

The CLI’s `--json` output is the contract between the extension and the core product.

### 17.2 Activation

The extension should activate when:

- A workspace contains `.git/pit/config.toml`, or
- `pit status --json` identifies the workspace as Pit-enabled, or
- The user explicitly runs `Pit: Set Up Workspace`.

The extension must respect VS Code Workspace Trust. It must not install hooks, invoke mutating commands, or hydrate private files in an untrusted workspace without explicit approval.

### 17.3 Source-control experience

The default UX should coexist with VS Code’s built-in Git extension:

- Built-in Git continues to represent public changes.
- Pit registers a **Pit Private** source-control provider for private changes.
- A **Pit Overview** view displays combined state, unclassified files, health issues, branch mapping, and pending transactions.
- Coordinated commands such as Commit, Push, Pull, and Switch are provided by Pit and operate on both repositories.

Suggested layout:

```text
SOURCE CONTROL
  Git (Public)
    Staged Changes
    Changes

  Pit Private
    Staged Changes
    Changes

PIT OVERVIEW
  Unclassified (2)
  Branches: main ↔ main
  Pending Transactions: none
  Health: OK
```

### 17.4 File decorations

The extension should add optional Explorer decorations:

- Shield/lock: private
- Globe: public
- Question mark: unclassified
- Eye or arrow: pending reveal
- Warning: public-history exposure or invariant violation

Decorations must be accessible, not color-only, and disableable through settings.

### 17.5 Required commands

The extension should contribute:

- `Pit: Set Up Workspace`
- `Pit: Connect Private Repository`
- `Pit: Refresh Status`
- `Pit: Stage Selected`
- `Pit: Unstage Selected`
- `Pit: Commit`
- `Pit: Push`
- `Pit: Pull`
- `Pit: Switch Branch`
- `Pit: Mark as Private`
- `Pit: Mark as Public`
- `Pit: Ignore Locally`
- `Pit: Show Diff`
- `Pit: Run Doctor`
- `Pit: Repair Local Configuration`
- `Pit: Resume Pending Transaction`
- `Pit: Open Transaction Details`
- `Pit: Copy Redacted Diagnostics`

Explorer context menus should include Mark as Private, Mark as Public, and Ignore Locally.

### 17.6 Staging and commit UX

- Public and private files must be stageable individually or together.
- Unclassified files must display a classification picker.
- One commit message should create a coordinated Pit transaction.
- Commit output must show whether zero, one, or two physical commits were created.
- The UI must clearly distinguish “committed locally” from “pushed to both remotes.”
- Direct use of the built-in Git commit button should not silently commit private changes. When private staged changes exist, the extension should display a visible reminder to use `Pit: Commit`.

### 17.7 Diff UX

- Public diffs may use the built-in Git API or `git diff` through Pit.
- Private diffs must be requested through the Pit CLI.
- The extension must not write private contents into public Git objects, temporary public paths, telemetry, or logs.
- Temporary diff content must use VS Code virtual documents or securely managed temporary files with cleanup.

### 17.8 Setup wizard

The setup wizard should:

1. Detect the public repository.
2. Check for the Pit CLI.
3. Offer existing-private or create-private flows.
4. Delegate authentication to the CLI.
5. Display verified private visibility.
6. Configure classification behavior.
7. Install hooks with a clear summary.
8. Run doctor and surface any blockers.

The extension should not collect credentials directly.

### 17.9 Notifications and errors

Notifications must be specific and actionable.

Good example:

```text
Private push succeeded, but the public push was rejected because origin/main advanced.
Transaction 7fca1b9d is safe and resumable.
[Pull and Reconcile] [View Details]
```

Bad example:

```text
Push failed.
```

High-severity privacy warnings must not disappear automatically.

### 17.10 Settings

Suggested settings:

```json
{
  "pit.cliPath": "pit",
  "pit.autoRefresh": true,
  "pit.showFileDecorations": true,
  "pit.confirmReveal": true,
  "pit.statusRefreshDebounceMs": 250,
  "pit.telemetry.enabled": false,
  "pit.diagnostics.redactPrivatePaths": true
}
```

### 17.11 Multi-root workspaces

The extension should treat each Git root independently and show repository-specific state. One failing Pit repository must not disable all other workspace folders.

### 17.12 CLI compatibility

The extension must:

- Check CLI semantic version and JSON schema version.
- Explain incompatible versions.
- Avoid parsing human-readable CLI output.
- Degrade gracefully when the CLI is absent.
- Provide installation guidance or a later signed binary installer.

---

## 18. Machine-readable CLI contract

The CLI should expose a versioned JSON schema from the first release.

Example `pit status --json` response:

```json
{
  "schemaVersion": 1,
  "pitVersion": "0.1.0",
  "workspace": "/home/kaf/my-awesome-repo",
  "health": {
    "status": "ok",
    "issues": []
  },
  "branches": {
    "public": "main",
    "private": "main",
    "aligned": true
  },
  "remotes": {
    "public": {
      "name": "origin",
      "ahead": 1,
      "behind": 0
    },
    "private": {
      "name": "origin",
      "verification": "verified-private",
      "ahead": 1,
      "behind": 0
    }
  },
  "changes": [
    {
      "path": "src/export.ts",
      "visibility": "public",
      "indexStatus": "modified",
      "worktreeStatus": "clean"
    },
    {
      "path": "notes/internal-export-plan.md",
      "visibility": "private",
      "indexStatus": "added",
      "worktreeStatus": "clean"
    },
    {
      "path": "docs/customer-example.md",
      "visibility": "unclassified",
      "indexStatus": "untracked",
      "worktreeStatus": "untracked"
    }
  ],
  "transaction": null
}
```

Requirements:

- Paths are workspace-relative.
- Output uses stable enums.
- Errors include a machine code, human message, remediation commands, and optional structured details.
- Sensitive fields are omitted or redacted unless a local trusted caller explicitly requests them.
- Schema changes are versioned and backwards-compatible within a documented support window.

A future `pit watch --json-lines` command may stream state changes to editors, but polling the status command is acceptable for the MVP.

---

## 19. Edge cases and compatibility requirements

### 19.1 `git clean`

Private files are ignored from the public repository’s perspective. Therefore, `git clean -fdx` can delete them, including uncommitted private work.

Pit cannot reliably intercept every direct `git clean` invocation.

Requirements:

- Document this limitation prominently.
- Provide `pit clean` later, preserving private and ignored policy paths by default.
- Warn in the VS Code extension before invoking Git clean actions when private changes exist.
- Encourage committing or stashing private changes before destructive cleanup.
- Ensure committed private files can be rehydrated from the private remote.

### 19.2 Stash

Plain `git stash` affects public tracked state and generally does not preserve ignored private changes.

A coordinated `pit stash` is needed before declaring full compatibility with stash-based workflows.

### 19.3 History exposure

When a newly protected path exists in any public commit, tag, branch, reflog, or remote history:

- Flag it as historically exposed.
- Block any claim that the path is now secret.
- Offer remediation guidance.
- Do not automatically rewrite or force-push.

### 19.4 Git LFS

The MVP should either:

- Explicitly support separate public and private LFS configurations, or
- Detect LFS-managed private paths and block unsupported operations with a clear message.

Pit must account for LFS objects as a separate leakage channel.

### 19.5 Submodules and nested repositories

Treat a submodule or nested Git repository as an atomic path. Mixing public and private paths inside one submodule is out of scope for the MVP.

### 19.6 Symlinks

- Git stores symlink targets, not target contents.
- Public symlink targets may still reveal private names or structure.
- Pit should warn or reject public symlinks that resolve into known private paths.

### 19.7 Case-insensitive filesystems

Pit must normalize paths carefully and detect collisions such as `Secret.txt` versus `secret.txt` before staging or checkout.

### 19.8 Worktrees, sparse checkout, and partial clones

These may be unsupported in the first release. `pit setup` and `pit doctor` must detect unsupported combinations and fail clearly rather than behaving unpredictably.

### 19.9 Rebase, cherry-pick, amend, and rewrite

The MVP may support only limited unpushed amend. Full dual-repository rebase, cherry-pick, and rewrite coordination should be a later milestone. Direct rewrites must mark transaction mappings stale and block push until reconciled.

### 19.10 Build and packaging leakage

Pit cannot prevent a build script, archive command, container build context, or publishing tool from packaging private files. Documentation should recommend explicit public build contexts and a `pit verify-public` workflow.

---

## 20. Non-functional requirements

### 20.1 Platforms

The CLI should support current macOS, Linux, and Windows environments where Git is installed. The VS Code extension should support desktop VS Code on those platforms.

### 20.2 Performance targets

Targets, subject to benchmark refinement:

- Warm `pit status` under 500 ms for a 10,000-file repository with no network access.
- Cold `pit status` under 2 seconds for a 100,000-file repository on typical developer hardware.
- Incremental status and extension refresh should avoid full history scans.
- Outbound history validation may take longer but must show progress for large pushes.

### 20.3 Reliability

- All local state writes must be atomic.
- Mutating commands must use an exclusive lock.
- Read-only status commands should avoid unnecessary locks.
- Interrupted operations must be detectable and recoverable.
- Schema migrations must create a backup and be reversible when possible.

### 20.4 Privacy

- No telemetry by default.
- No file contents in telemetry under any setting.
- No credentials in logs.
- Shareable diagnostics redact private path names and remote URLs by default.
- Crash reports must be local unless the user explicitly submits them.

### 20.5 Accessibility

- CLI output must remain understandable without color.
- VS Code state must not rely only on color.
- Commands and warnings must be keyboard accessible.
- Icons need accessible labels.

### 20.6 Documentation

Required documentation:

- Five-minute setup guide
- Mental model: one working tree, two repositories
- Daily workflow
- Native Git compatibility table
- Threat model and limitations
- Recovery guide
- History-exposure guide
- `git clean` and stash warnings
- VS Code extension guide
- CI hydration guide
- Troubleshooting and diagnostic guide

---

## 21. Acceptance criteria

The MVP is acceptable when all of the following pass in automated end-to-end tests.

### 21.1 Setup

- A normal public repository can be converted into a Pit workspace without modifying tracked public files.
- An existing private repository can be connected and hydrated.
- A new private repository can be initialized locally.
- Existing `.git/info/exclude` content is preserved.
- Existing hooks or `core.hooksPath` configuration are preserved and chained.

### 21.2 Classification and staging

- `pit add .` stages public files in the public index and private files in the private index.
- `git add .` does not stage configured private files.
- Unclassified files are never silently staged as public when the policy is `prompt` or `reject`.
- No path can be staged in both indexes.

### 21.3 Commit

- A mixed change set creates one public commit and one private commit under one transaction.
- A public-only change creates no empty private commit.
- A private-only change creates no empty public commit.
- A failed second local commit restores the pre-command refs and staging state.
- Public commit messages and metadata contain no private path names or private commit identifiers by default.

### 21.4 Push and leakage prevention

Using a unique canary string in a private file:

- `pit push` sends the private path and canary to the private remote.
- The public remote contains neither the private path nor the canary in any outgoing commit, tree, or blob.
- A private file added and deleted within the same unpushed public history is still detected.
- A forced public staging attempt is blocked by the hook.
- Bypassing the hook does not bypass `pit push` outbound validation.
- If the private push succeeds and public push fails, `pit push --resume` completes without duplicating or losing state.

### 21.5 History warnings

- `pit protect` detects that a path was previously public and displays a durable exposure warning.
- The tool does not claim that removal from the current branch erases prior public copies.

### 21.6 Branches and pull

- `pit switch -c feature` creates and switches mapped branches.
- A direct `git switch` is detected as drift.
- `pit pull` updates both repositories or reports a recoverable conflict without silent partial success.

### 21.7 VS Code

- The extension shows public and private changes in distinguishable source-control groups.
- A user can classify, stage, unstage, diff, commit, push, and resume a failed transaction from VS Code.
- Private diffs do not enter public Git state or telemetry.
- The extension detects missing or incompatible CLI versions.
- Workspace Trust is respected.

### 21.8 Cross-platform

- Core end-to-end tests pass on Windows, macOS, and Linux.
- Paths containing spaces, Unicode, and shell metacharacters work correctly.
- Case-collision tests behave safely on case-insensitive filesystems.

---

## 22. Testing strategy

### 22.1 Unit tests

- Pattern parsing and precedence
- Path normalization
- Classification state machine
- Transaction state transitions
- JSON schema serialization
- Hook dispatch and chaining
- Redaction
- Remote verification state

### 22.2 Property and fuzz tests

- Pathspec handling
- Unicode and unusual filenames
- Pattern matching against Git behavior
- Transaction recovery after interruption at every state transition
- No private blob writes to the public object database

### 22.3 Integration tests

Use temporary local bare repositories as public and private remotes.

Test:

- Initial setup
- Hydration
- Mixed staging and commits
- Push ordering
- Remote race conditions
- Non-fast-forward rejection
- Hook bypass followed by `pit push`
- Existing hooks
- Branch drift
- Policy changes
- Failure injection after each durable step

### 22.4 Security regression tests

- Unique canary path and content searches across all public objects
- Private file added then removed before push
- Symlink references
- LFS pointer handling
- Credentials in remote URLs
- Diagnostic redaction
- Public-history exposure detection

### 22.5 VS Code tests

- Command registration
- SCM groups
- File decorations
- Setup wizard
- CLI schema mismatch
- Workspace Trust
- Multi-root behavior
- Transaction recovery notifications
- Private diff lifecycle and cleanup

---

## 23. Suggested implementation architecture

The implementation language is not a product requirement, but a practical starting point is:

- **CLI:** Rust or Go, compiled as a single native binary
- **VS Code extension:** TypeScript
- **Shared contract:** Versioned JSON Schemas checked into the repository
- **Tests:** Local bare Git remotes for deterministic end-to-end coverage

Suggested monorepo structure:

```text
pit/
├── cli/
│   ├── src/
│   └── tests/
├── vscode/
│   ├── src/
│   └── test/
├── schemas/
│   ├── status-v1.schema.json
│   ├── error-v1.schema.json
│   └── transaction-v1.schema.json
├── docs/
├── fixtures/
└── scripts/
```

### 23.1 Internal CLI modules

Suggested modules:

- Workspace discovery
- Public repository adapter
- Private repository adapter
- Policy parser and classifier
- Exclude-file manager
- Hook manager
- Transaction journal
- Commit coordinator
- Push coordinator
- Pull and branch coordinator
- Remote verifier
- Outbound validator
- Diagnostics and redaction
- JSON output layer

### 23.2 Git invocation rules

- Prefer invoking the installed Git binary over implementing Git object protocols in the MVP.
- Use stable plumbing or porcelain v2 output where available.
- Use NUL-delimited output for filenames.
- Pass arguments as process argument arrays, never shell-concatenated commands.
- Set explicit `--git-dir` and `--work-tree` for private operations.
- Ensure private content is never passed to a public `hash-object`, `add`, `commit`, or object-writing command.

### 23.3 No daemon in MVP

The MVP should remain command-driven. A watcher or daemon may be introduced later if extension performance requires it. The CLI state model must work without a long-running process.

---

## 24. Delivery phases

### Phase 0 — Technical spike

- Prove that two Git directories can safely share one working tree.
- Prove private staging and commits do not write to the public object database.
- Implement canary-based leakage tests.
- Exercise Windows path behavior.
- Validate hook chaining strategy.

### Phase 1 — CLI core alpha

- `pit setup`
- `pit status`
- `pit add`
- `pit restore --staged`
- `pit commit`
- `pit push`
- `pit doctor`
- Private policy
- Managed excludes
- Hook installation
- Durable transaction journal
- Local bare-remote E2E suite

### Phase 2 — Collaboration beta

- `pit clone`
- `pit pull`
- `pit switch`
- `pit protect`
- `pit reveal`
- Existing private mirror hydration
- GitHub-assisted private repository creation and verification
- Better recovery and diagnostics

### Phase 3 — VS Code alpha

- CLI JSON schema stabilization
- Setup wizard
- Pit Private SCM provider
- Pit Overview
- Classification and file decorations
- Coordinated commit, push, and pull
- Transaction recovery UI

### Phase 4 — v1 hardening

- Cross-platform installers
- Signed releases
- Performance benchmarks
- CI hydration
- Full documentation
- Security review
- Compatibility matrix
- Upgrade and migration testing

### Later possibilities

- `git-remote-pit` for safe native `git push`
- Coordinated stash, merge, rebase, cherry-pick, and worktree support
- Full private mirror mode
- JetBrains and Neovim integrations
- First-party GitHub Action
- Public projection mode from a private canonical repository
- Policy-as-code review workflows
- Optional encrypted private remotes

---

## 25. Key risks and mitigations

| Risk | Mitigation |
|---|---|
| Users assume hooks are a security boundary | Repeat critical checks inside `pit push`; document bypass behavior. |
| Private files enter public local history before deletion | Validate the complete outgoing commit range, not only the index or final tree. |
| Two remote pushes cannot be atomic | Push private first and use a durable resumable transaction journal. |
| Plain Git commands create drift | Detect drift, block unsafe publication, and provide reconciliation commands. |
| `.git/info/exclude` causes private work to disappear from normal status | Make `pit status` and the VS Code extension the primary visibility layer. |
| `git clean -fdx` deletes private files | Document prominently, provide `pit clean`, warn in editor, and rely on committed private recovery. |
| Existing hook configuration is overwritten | Build a non-destructive dispatcher and test chaining. |
| Private repository is accidentally public | Verify visibility through provider APIs or block unverified first pushes. |
| Private filenames leak through public config | Keep policy and remote data under `.git/pit/` and in the private mirror only. |
| Public project silently depends on private files | Add `pit verify-public` and CI checks in a sanitized public-only clone. |
| VS Code extension diverges from CLI behavior | Make the extension a thin client using a versioned JSON contract. |
| Advanced Git operations rewrite mappings | Limit MVP support, detect rewrites, and require reconciliation before push. |

---

## 26. Implementation build brief

Implementers should treat the following as the initial implementation contract:

1. Build the CLI before the editor extension.
2. Use a normal public `.git/` plus a secondary private Git directory under `.git/pit/`.
3. Keep private policy and remote configuration out of tracked public files.
4. Make new unclassified files fail closed by default.
5. Implement `pit setup`, `status`, `add`, `commit`, `push`, and `doctor` first.
6. Add durable transaction journaling before implementing network pushes.
7. Push private first, then public.
8. Validate the entire outgoing public commit range immediately before public push.
9. Preserve existing hooks and `.git/info/exclude` entries.
10. Expose stable versioned JSON before starting VS Code integration.
11. Make the extension a thin TypeScript client over the CLI.
12. Build every feature against local temporary public/private bare remotes.
13. Include canary tests proving private path names and contents never reach the public object database or remote.
14. Do not add a cloud backend, credential store, or hidden automatic commits in the MVP.
15. Prefer explicit recoverable behavior over magical behavior.

### First demonstrable milestone

The first demo should prove this exact flow:

```bash
# Start from a normal public repository.
pit setup --private /tmp/private-remote.git

mkdir -p src private
printf 'export const answer = 42;\n' > src/index.ts
printf 'PIT-CANARY-7fca1b9d\n' > private/notes.txt

pit add .
pit status
pit commit -m "Add public implementation and private notes"
pit push
```

The demo passes only when:

- The public remote contains `src/index.ts`.
- The private remote contains `private/notes.txt`.
- The public remote contains neither `private/notes.txt` nor `PIT-CANARY-7fca1b9d` in any reachable object.
- A fresh public-only clone works without Pit.
- A fresh authorized Pit clone hydrates both files.
- The VS Code extension can later display and commit the same split state through the CLI contract.

---

## 27. Product principles

1. **Separate repositories are the security boundary.** Excludes and hooks are guardrails.
2. **No surprises.** Pit must show what will be public, private, ignored, or unresolved before publication.
3. **Private first, public second.** Failure should favor confidentiality over convenience.
4. **Ordinary public Git remains ordinary.** Public contributors should not inherit Pit complexity.
5. **Recoverability beats pretend atomicity.** Every partial operation needs a durable explanation and resume path.
6. **The CLI is the source of truth.** Editor integrations call it rather than duplicate it.
7. **Previously public means previously public.** Pit must be honest about historical exposure.
8. **Fail closed at the publication boundary.** Uncertainty blocks public push.
9. **Credentials stay with Git and the operating system.** Pit does not become a secrets vault.
10. **Seamless does not mean invisible.** Privacy state should be easy to see and hard to misunderstand.

