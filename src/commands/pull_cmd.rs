use crate::error::{PitError, Result};
use crate::git;
use crate::json_out;
use crate::workspace::Workspace;
use std::path::Path;

pub struct PullArgs {
    pub yes: bool,
    pub json: bool,
}

pub fn run(cwd: &Path, args: PullArgs) -> Result<()> {
    let mut ws = Workspace::discover(cwd)?;
    if ws.state.branch_mapping_stale {
        return Err(PitError::msg(
            "branch mapping is stale; run `pit switch <branch>` or `pit doctor --repair` before pull",
        ));
    }

    // Dirty check — ignore untracked for pull decision
    let pub_dirty = has_tracked_dirty(&ws, true)?;
    let priv_dirty = has_tracked_dirty(&ws, false)?;
    if (pub_dirty || priv_dirty) && !args.yes {
        return Err(PitError::msg(
            "uncommitted changes present; commit/stash first or re-run with --yes to attempt pull anyway",
        ));
    }

    let pub_remote = ws.config.public_remote_name.clone();
    let priv_remote = ws.config.private_remote_name.clone();
    let branch = ws.public_branch()?;

    // Fetch both — private fetch failure is fatal if remote is configured
    ws.public_git(&["fetch", &pub_remote])?;
    if let Err(e) = ws.private_git(&["fetch", &priv_remote]) {
        ws.state.branch_mapping_stale = true;
        ws.save_state()?;
        return Err(PitError::msg(format!(
            "private fetch failed (public not pulled): {e}. Mapping marked stale."
        )));
    }

    // Pull public (ff-only by default)
    match ws.public_git(&["pull", "--ff-only", &pub_remote, &branch]) {
        Ok(o) => {
            if !o.is_empty() {
                eprintln!("{o}");
            }
        }
        Err(e) => {
            return Err(PitError::msg(format!(
                "public pull failed (private not updated): {e}"
            )));
        }
    }

    // Update private branch — fail closed if this fails after public succeeded
    let priv_ok = update_private_branch(&ws, &priv_remote, &branch);
    if let Err(e) = priv_ok {
        ws.state.branch_mapping_stale = true;
        let _ = ws.save_state();
        return Err(PitError::msg(format!(
            "public pull succeeded but private update failed: {e}. \
             Mapping marked stale; run `pit doctor` / `pit switch {branch}` to reconcile. \
             Private was not silently treated as success."
        )));
    }

    // Rehydrate private files
    if git::has_commits(&ws.work_tree, Some(&ws.private_git_dir)) {
        for f in ws.private_tracked()? {
            let _ = ws.private_git(&["checkout", "HEAD", "--", &f]);
        }
    }

    crate::exclude::update_managed_exclude(
        &ws.exclude_path(),
        &ws.policy.effective_private_patterns(),
    )?;
    ws.state.last_public_head =
        git::rev_parse(&ws.work_tree, Some(&ws.public_git_dir), "HEAD").ok();
    ws.state.last_private_head =
        git::rev_parse(&ws.work_tree, Some(&ws.private_git_dir), "HEAD").ok();
    ws.state.branch_mapping_stale = false;
    ws.save_state()?;

    if args.json {
        json_out::print_ok(
            "pull",
            serde_json::json!({
                "branch": branch,
                "public_head": ws.state.last_public_head,
                "private_head": ws.state.last_private_head,
            }),
        );
    } else {
        println!("Pulled public and private on branch `{branch}`.");
    }
    Ok(())
}

fn update_private_branch(ws: &Workspace, priv_remote: &str, branch: &str) -> Result<()> {
    // Prefer ff-only pull when local branch exists
    match ws.private_git(&["pull", "--ff-only", priv_remote, branch]) {
        Ok(_) => return Ok(()),
        Err(e) => {
            // Fall back to checkout of remote-tracking branch if that is the issue
            let remote_ref = format!("{priv_remote}/{branch}");
            match ws.private_git(&["rev-parse", "--verify", &remote_ref]) {
                Ok(_) => {
                    ws.private_git(&["checkout", "-B", branch, &remote_ref])
                        .map_err(|e2| {
                            PitError::msg(format!(
                                "private pull failed ({e}); checkout of {remote_ref} also failed: {e2}"
                            ))
                        })?;
                    Ok(())
                }
                Err(_) => {
                    // No remote branch yet — only OK if private has no commits (new empty companion)
                    if !git::has_commits(&ws.work_tree, Some(&ws.private_git_dir)) {
                        Ok(())
                    } else {
                        Err(PitError::msg(format!(
                            "private pull failed and remote branch {remote_ref} missing: {e}"
                        )))
                    }
                }
            }
        }
    }
}

fn has_tracked_dirty(ws: &Workspace, public: bool) -> Result<bool> {
    let entries = if public {
        git::status_porcelain(&ws.work_tree, Some(&ws.public_git_dir))?
    } else if ws.private_git_dir.exists() {
        git::status_porcelain(&ws.work_tree, Some(&ws.private_git_dir)).unwrap_or_default()
    } else {
        return Ok(false);
    };
    Ok(entries.iter().any(|e| !e.xy.starts_with('?')))
}
