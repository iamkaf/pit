use crate::error::{PitError, Result};
use crate::git;
use crate::policy::Policy;
use crate::transaction::TxStore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Local Pit configuration — never tracked in the public repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PitConfig {
    pub version: u32,
    pub private_remote: String,
    pub private_remote_name: String,
    pub public_remote_name: String,
    /// verified-private | user-attested-private | unverified
    pub private_visibility: String,
    pub hooks_installed: bool,
}

impl Default for PitConfig {
    fn default() -> Self {
        Self {
            version: 1,
            private_remote: String::new(),
            private_remote_name: "private".into(),
            public_remote_name: "origin".into(),
            private_visibility: "unverified".into(),
            hooks_installed: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PitState {
    pub version: u32,
    pub last_public_head: Option<String>,
    pub last_private_head: Option<String>,
    pub branch_mapping_stale: bool,
}

pub struct Workspace {
    pub work_tree: PathBuf,
    pub public_git_dir: PathBuf,
    pub pit_dir: PathBuf,
    pub private_git_dir: PathBuf,
    pub config: PitConfig,
    pub policy: Policy,
    pub state: PitState,
}

impl Workspace {
    pub fn pit_dir_for(work_tree: &Path) -> PathBuf {
        work_tree.join(".git").join("pit")
    }

    pub fn discover(start: &Path) -> Result<Self> {
        let work_tree = git::find_work_tree(start)?;
        let public_git_dir = git::find_git_dir(&work_tree)?;
        let pit_dir = public_git_dir.join("pit");
        if !pit_dir.join("config.toml").exists() {
            return Err(PitError::NotAPitWorkspace(work_tree.display().to_string()));
        }
        Self::load(work_tree, public_git_dir, pit_dir)
    }

    pub fn try_discover(start: &Path) -> Result<Option<Self>> {
        match Self::discover(start) {
            Ok(ws) => Ok(Some(ws)),
            Err(PitError::NotAPitWorkspace(_)) => Ok(None),
            Err(PitError::NotAGitRepo(_)) => Ok(None),
            Err(e) => {
                // also catch git failures for non-repos
                if matches!(e, PitError::Git { .. }) {
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn load(work_tree: PathBuf, public_git_dir: PathBuf, pit_dir: PathBuf) -> Result<Self> {
        let config_path = pit_dir.join("config.toml");
        let config: PitConfig = toml::from_str(&fs::read_to_string(&config_path)?)?;
        let policy_path = pit_dir.join("policy.toml");
        let policy = if policy_path.exists() {
            Policy::load(&policy_path)?
        } else {
            Policy::default()
        };
        let state_path = pit_dir.join("state.json");
        let state = if state_path.exists() {
            serde_json::from_str(&fs::read_to_string(&state_path)?)?
        } else {
            PitState {
                version: 1,
                ..Default::default()
            }
        };
        let private_git_dir = pit_dir.join("private.git");
        Ok(Self {
            work_tree,
            public_git_dir,
            pit_dir,
            private_git_dir,
            config,
            policy,
            state,
        })
    }

    pub fn save_config(&self) -> Result<()> {
        let path = self.pit_dir.join("config.toml");
        fs::write(&path, toml::to_string_pretty(&self.config)?)?;
        Ok(())
    }

    pub fn save_policy(&self) -> Result<()> {
        self.policy.save(&self.pit_dir.join("policy.toml"))
    }

    pub fn save_state(&self) -> Result<()> {
        let path = self.pit_dir.join("state.json");
        fs::write(&path, serde_json::to_string_pretty(&self.state)?)?;
        Ok(())
    }

    pub fn tx_store(&self) -> TxStore {
        TxStore::new(&self.pit_dir)
    }

    pub fn exclude_path(&self) -> PathBuf {
        self.public_git_dir.join("info").join("exclude")
    }

    pub fn public_git<'a>(&'a self, args: &[&str]) -> Result<String> {
        git::run_git(
            Some(&self.work_tree),
            Some(&self.public_git_dir),
            args,
            None,
        )
    }

    pub fn private_git(&self, args: &[&str]) -> Result<String> {
        git::run_git(
            Some(&self.work_tree),
            Some(&self.private_git_dir),
            args,
            None,
        )
    }

    pub fn public_branch(&self) -> Result<String> {
        let b = git::current_branch(&self.work_tree, Some(&self.public_git_dir))?;
        if b.is_empty() {
            Ok("main".into())
        } else {
            Ok(b)
        }
    }

    pub fn private_branch(&self) -> Result<String> {
        match git::current_branch(&self.work_tree, Some(&self.private_git_dir)) {
            Ok(b) if !b.is_empty() => Ok(b),
            _ => self.public_branch(),
        }
    }

    /// Paths currently tracked in public index (ls-files).
    pub fn public_tracked(&self) -> Result<Vec<String>> {
        let out = self.public_git(&["ls-files", "-z"])?;
        Ok(out.split('\0').filter(|s| !s.is_empty()).map(String::from).collect())
    }

    pub fn private_tracked(&self) -> Result<Vec<String>> {
        if !self.private_git_dir.exists() {
            return Ok(Vec::new());
        }
        let out = self.private_git(&["ls-files", "-z"])?;
        Ok(out.split('\0').filter(|s| !s.is_empty()).map(String::from).collect())
    }

    pub fn dual_tracked(&self) -> Result<Vec<String>> {
        let pub_set: std::collections::HashSet<_> = self.public_tracked()?.into_iter().collect();
        let mut dual = Vec::new();
        for p in self.private_tracked()? {
            if pub_set.contains(&p) {
                dual.push(p);
            }
        }
        dual.sort();
        Ok(dual)
    }
}

/// Initialize a fresh Pit overlay on an existing public git repo.
pub fn init_pit_workspace(
    work_tree: &Path,
    private_remote: &str,
    visibility: &str,
    policy: Option<Policy>,
) -> Result<Workspace> {
    if !git::is_git_repo(work_tree) {
        return Err(PitError::NotAGitRepo(work_tree.display().to_string()));
    }
    let public_git_dir = git::find_git_dir(work_tree)?;
    let pit_dir = public_git_dir.join("pit");
    fs::create_dir_all(&pit_dir)?;
    fs::create_dir_all(pit_dir.join("transactions"))?;
    fs::create_dir_all(pit_dir.join("locks"))?;
    fs::create_dir_all(pit_dir.join("logs"))?;
    fs::create_dir_all(pit_dir.join("hooks"))?;

    // Private overlay: separate git directory under .git/pit/private.git.
    // Always pass --git-dir/--work-tree on invocations; never rewrite root .git.
    let private_git_dir = pit_dir.join("private.git");
    if !private_git_dir.join("HEAD").exists() {
        let _ = fs::remove_dir_all(&private_git_dir);
        fs::create_dir_all(&private_git_dir)?;
        git::run_git(None, Some(&private_git_dir), &["init"], None)?;
        git::run_git(
            None,
            Some(&private_git_dir),
            &["config", "core.bare", "false"],
            None,
        )?;
        // Unset core.worktree so callers always pass --work-tree explicitly
        let _ = git::run_git(
            None,
            Some(&private_git_dir),
            &["config", "--unset", "core.worktree"],
            None,
        );
    }

    // Ensure private has an initial empty commit only after first private content —
    // leave empty until first private commit.

    // Default branch name to match public
    let public_branch = git::current_branch(work_tree, Some(&public_git_dir))
        .unwrap_or_else(|_| "main".into());
    let branch_ref = if public_branch.is_empty() {
        "refs/heads/main".to_string()
    } else {
        format!("refs/heads/{public_branch}")
    };
    let _ = git::run_git(
        Some(work_tree),
        Some(&private_git_dir),
        &["symbolic-ref", "HEAD", &branch_ref],
        None,
    );

    // Inherit identity from public/global so private commits succeed
    if let Ok(name) = git::run_git(Some(work_tree), Some(&public_git_dir), &["config", "user.name"], None) {
        let _ = git::run_git(Some(work_tree), Some(&private_git_dir), &["config", "user.name", &name], None);
    }
    if let Ok(email) = git::run_git(Some(work_tree), Some(&public_git_dir), &["config", "user.email"], None) {
        let _ = git::run_git(Some(work_tree), Some(&private_git_dir), &["config", "user.email", &email], None);
    }

    let policy = policy.unwrap_or_default();
    policy.save(&pit_dir.join("policy.toml"))?;

    // Also store policy in private repo as blob path `.pit/policy.toml` later on first commit.

    let config = PitConfig {
        version: 1,
        private_remote: private_remote.to_string(),
        private_remote_name: "private".into(),
        public_remote_name: "origin".into(),
        private_visibility: visibility.to_string(),
        hooks_installed: false,
    };
    fs::write(
        pit_dir.join("config.toml"),
        toml::to_string_pretty(&config)?,
    )?;

    let state = PitState {
        version: 1,
        last_public_head: git::rev_parse(work_tree, Some(&public_git_dir), "HEAD").ok(),
        last_private_head: None,
        branch_mapping_stale: false,
    };
    fs::write(
        pit_dir.join("state.json"),
        serde_json::to_string_pretty(&state)?,
    )?;

    // Configure private remote
    if !private_remote.is_empty() {
        let _ = git::run_git(
            Some(work_tree),
            Some(&private_git_dir),
            &["remote", "remove", "private"],
            None,
        );
        git::run_git(
            Some(work_tree),
            Some(&private_git_dir),
            &["remote", "add", "private", private_remote],
            None,
        )?;
    }

    // Managed exclude
    crate::exclude::update_managed_exclude(
        &public_git_dir.join("info").join("exclude"),
        &policy.effective_private_patterns(),
    )?;

    // Install hooks (non-destructive)
    install_hooks(work_tree, &public_git_dir, &pit_dir)?;

    // Mark hooks installed
    let mut config = config;
    config.hooks_installed = true;
    fs::write(
        pit_dir.join("config.toml"),
        toml::to_string_pretty(&config)?,
    )?;

    Workspace::load(work_tree.to_path_buf(), public_git_dir, pit_dir)
}

pub fn install_hooks(work_tree: &Path, public_git_dir: &Path, pit_dir: &Path) -> Result<()> {
    let hooks_dir = public_git_dir.join("hooks");
    fs::create_dir_all(&hooks_dir)?;
    let pit_hooks = pit_dir.join("hooks");
    fs::create_dir_all(&pit_hooks)?;

    // Find pit binary path for hooks
    let pit_bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "pit".into());

    // Fail-closed hooks vs advisory drift hooks
    let fail_closed = ["pre-commit", "pre-push"];
    let advisory = ["post-checkout", "post-merge", "post-rewrite"];
    let all_hooks: Vec<&str> = fail_closed.iter().chain(advisory.iter()).copied().collect();

    for hook_name in &all_hooks {
        let fail_closed_hook = fail_closed.contains(hook_name);
        let dispatcher = if fail_closed_hook {
            format!(
                r#"#!/bin/sh
# Pit hook dispatcher — chains user hooks and enforces privacy guards.
HOOK_NAME="{hook_name}"
PIT_BIN="{pit_bin}"
USER_HOOK="{hooks_dir}/{hook_name}.user"
if [ -x "$PIT_BIN" ] || command -v "$PIT_BIN" >/dev/null 2>&1; then
  "$PIT_BIN" hook "$HOOK_NAME" "$@" || exit $?
else
  echo "pit: binary unavailable; fail-closed for $HOOK_NAME" >&2
  exit 1
fi
if [ -x "$USER_HOOK" ]; then
  exec "$USER_HOOK" "$@"
fi
exit 0
"#,
                hook_name = hook_name,
                pit_bin = pit_bin,
                hooks_dir = hooks_dir.display(),
            )
        } else {
            format!(
                r#"#!/bin/sh
# Pit hook dispatcher — drift detection (advisory; always chains user hook).
HOOK_NAME="{hook_name}"
PIT_BIN="{pit_bin}"
USER_HOOK="{hooks_dir}/{hook_name}.user"
if [ -x "$PIT_BIN" ] || command -v "$PIT_BIN" >/dev/null 2>&1; then
  "$PIT_BIN" hook "$HOOK_NAME" "$@" || true
fi
if [ -x "$USER_HOOK" ]; then
  exec "$USER_HOOK" "$@"
fi
exit 0
"#,
                hook_name = hook_name,
                pit_bin = pit_bin,
                hooks_dir = hooks_dir.display(),
            )
        };
        let hook_path = hooks_dir.join(hook_name);
        // Preserve existing non-pit hook as .user
        if hook_path.exists() {
            let existing = fs::read_to_string(&hook_path).unwrap_or_default();
            if !existing.contains("Pit hook dispatcher") {
                let user_path = hooks_dir.join(format!("{hook_name}.user"));
                if !user_path.exists() {
                    fs::rename(&hook_path, &user_path)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let mut perms = fs::metadata(&user_path)?.permissions();
                        perms.set_mode(0o755);
                        fs::set_permissions(&user_path, perms)?;
                    }
                }
            }
        }
        fs::write(&hook_path, dispatcher)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&hook_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&hook_path, perms)?;
        }
        let _ = work_tree;
    }
    Ok(())
}

pub fn uninstall_hooks(public_git_dir: &Path) -> Result<()> {
    let hooks_dir = public_git_dir.join("hooks");
    for hook_name in &[
        "pre-commit",
        "pre-push",
        "post-checkout",
        "post-merge",
        "post-rewrite",
    ] {
        let hook_path = hooks_dir.join(hook_name);
        if hook_path.exists() {
            let existing = fs::read_to_string(&hook_path).unwrap_or_default();
            if existing.contains("Pit hook dispatcher") {
                fs::remove_file(&hook_path)?;
                let user_path = hooks_dir.join(format!("{hook_name}.user"));
                if user_path.exists() {
                    fs::rename(&user_path, &hook_path)?;
                }
            }
        }
    }
    Ok(())
}

pub fn hooks_status(public_git_dir: &Path) -> Vec<(String, String)> {
    let hooks_dir = public_git_dir.join("hooks");
    let mut out = Vec::new();
    for hook_name in &[
        "pre-commit",
        "pre-push",
        "post-checkout",
        "post-merge",
        "post-rewrite",
    ] {
        let hook_path = hooks_dir.join(hook_name);
        let status = if !hook_path.exists() {
            "missing".into()
        } else {
            let text = fs::read_to_string(&hook_path).unwrap_or_default();
            if text.contains("Pit hook dispatcher") {
                let user = hooks_dir.join(format!("{hook_name}.user"));
                if user.exists() {
                    "pit+user".into()
                } else {
                    "pit".into()
                }
            } else {
                "foreign".into()
            }
        };
        out.push(((*hook_name).to_string(), status));
    }
    out
}
