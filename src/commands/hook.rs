use crate::error::{PitError, Result};
use crate::git;
use crate::workspace::Workspace;
use std::io::{self, BufRead};
use std::path::Path;

/// `pit hook <name>` — called from installed dispatchers.
pub fn run(cwd: &Path, hook_name: &str) -> Result<()> {
    let mut ws = match Workspace::discover(cwd) {
        Ok(ws) => ws,
        Err(_) => return Ok(()),
    };

    match hook_name {
        "pre-commit" => pre_commit(&ws),
        "pre-push" => pre_push(&ws),
        "post-checkout" | "post-merge" | "post-rewrite" => mark_drift(&mut ws, hook_name),
        _ => Ok(()),
    }
}

fn pre_commit(ws: &Workspace) -> Result<()> {
    if ws.state.branch_mapping_stale {
        eprintln!("pit: branch mapping is stale; run `pit switch` or `pit doctor --repair` before committing.");
        return Err(PitError::msg("stale branch mapping blocks commit"));
    }
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
    if ws.state.branch_mapping_stale {
        eprintln!("pit: branch mapping is stale; reconcile with `pit switch` before publish.");
        return Err(PitError::msg("stale branch mapping blocks push"));
    }
    let stdin = io::stdin();
    let mut lines = Vec::new();
    for line in stdin.lock().lines() {
        lines.push(line?);
    }
    if std::env::var("PIT_ALLOW_GIT_PUSH").ok().as_deref() == Some("1") {
        return Ok(());
    }
    if std::env::var("PIT_PUSH_IN_PROGRESS").ok().as_deref() == Some("1") {
        return Ok(());
    }

    eprintln!("pit: direct `git push` is blocked in Pit workspaces.");
    eprintln!("Use `pit push` to publish private then public safely.");
    eprintln!("Emergency override: PIT_ALLOW_GIT_PUSH=1 git push ...");
    let _ = lines;
    Err(PitError::msg("direct git push blocked; use pit push"))
}

fn mark_drift(ws: &mut Workspace, hook_name: &str) -> Result<()> {
    // Detect public/private branch name drift
    let pub_b = ws.public_branch().unwrap_or_default();
    let priv_b = ws.private_branch().unwrap_or_default();
    if !pub_b.is_empty() && !priv_b.is_empty() && pub_b != priv_b {
        ws.state.branch_mapping_stale = true;
        ws.save_state()?;
        eprintln!(
            "pit: {hook_name}: public branch `{pub_b}` != private `{priv_b}` — mapping marked stale"
        );
        eprintln!("Run `pit switch {pub_b}` (or doctor --repair) before commit/push.");
    } else {
        // refresh excludes after checkout/merge
        let _ = crate::exclude::update_managed_exclude(
            &ws.exclude_path(),
            &ws.policy.effective_private_patterns(),
        );
        if hook_name == "post-rewrite" {
            ws.state.branch_mapping_stale = true;
            ws.save_state()?;
            eprintln!("pit: post-rewrite: commit mappings may be stale; run pit status / doctor");
        }
    }
    Ok(())
}
