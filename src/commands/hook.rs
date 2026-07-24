use crate::error::{PitError, Result};
use crate::git;
use crate::workspace::Workspace;
use std::io::{self, BufRead};
use std::path::Path;

/// `pit hook <name>` — called from installed dispatchers.
pub fn run(cwd: &Path, hook_name: &str) -> Result<()> {
    let ws = match Workspace::discover(cwd) {
        Ok(ws) => ws,
        Err(_) => {
            // Not a pit workspace — allow
            return Ok(());
        }
    };

    match hook_name {
        "pre-commit" => pre_commit(&ws),
        "pre-push" => pre_push(&ws),
        _ => Ok(()),
    }
}

fn pre_commit(ws: &Workspace) -> Result<()> {
    let staged = git::staged_paths(&ws.work_tree, Some(&ws.public_git_dir))?;
    let matcher = ws.policy.matcher()?;
    let mut bad = Vec::new();
    for p in &staged {
        if matcher.is_private_pattern_match(p) {
            bad.push(p.clone());
        }
    }
    let dual = ws.dual_tracked()?;
    if !dual.is_empty() {
        eprintln!("pit: dual-tracked paths block commit:");
        for p in &dual {
            eprintln!("  {p}");
        }
        return Err(PitError::DualTracked(dual));
    }
    if !bad.is_empty() {
        eprintln!("pit: refusing public commit of private/protected paths:");
        for p in &bad {
            eprintln!("  {p}");
        }
        eprintln!("Use `pit add` / `pit commit` for private paths.");
        return Err(PitError::PrivacyValidation(format!(
            "private paths in public index: {}",
            bad.join(", ")
        )));
    }
    Ok(())
}

fn pre_push(ws: &Workspace) -> Result<()> {
    // Block direct git push; instruct to use pit push.
    // pre-push receives refs on stdin
    let stdin = io::stdin();
    let mut lines = Vec::new();
    for line in stdin.lock().lines() {
        lines.push(line?);
    }
    // Allow if PIT_ALLOW_GIT_PUSH=1 for emergencies
    if std::env::var("PIT_ALLOW_GIT_PUSH").ok().as_deref() == Some("1") {
        return Ok(());
    }
    // If this is invoked from within `pit push`, allow
    if std::env::var("PIT_PUSH_IN_PROGRESS").ok().as_deref() == Some("1") {
        return Ok(());
    }

    eprintln!("pit: direct `git push` is blocked in Pit workspaces.");
    eprintln!("Use `pit push` to publish private then public safely.");
    eprintln!("Emergency override: PIT_ALLOW_GIT_PUSH=1 git push ...");
    let _ = lines;
    let _ = ws;
    Err(PitError::msg("direct git push blocked; use pit push"))
}
