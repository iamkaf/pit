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

    // Dirty check
    let pub_dirty = !git::status_porcelain(&ws.work_tree, Some(&ws.public_git_dir))?.is_empty();
    let priv_dirty = git::status_porcelain(&ws.work_tree, Some(&ws.private_git_dir))
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    if (pub_dirty || priv_dirty) && !args.yes {
        return Err(PitError::msg(
            "uncommitted changes present; commit/stash first or re-run with --yes to attempt pull anyway",
        ));
    }

    let pub_remote = ws.config.public_remote_name.clone();
    let priv_remote = ws.config.private_remote_name.clone();
    let branch = ws.public_branch()?;

    // Fetch both
    ws.public_git(&["fetch", &pub_remote])?;
    let _ = ws.private_git(&["fetch", &priv_remote]);

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

    // Update private branch
    let priv_result = ws.private_git(&["pull", "--ff-only", &priv_remote, &branch]);
    if let Err(e) = &priv_result {
        // try checkout tracking
        let _ = ws.private_git(&[
            "checkout",
            "-B",
            &branch,
            &format!("{priv_remote}/{branch}"),
        ]);
        let _ = e;
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
