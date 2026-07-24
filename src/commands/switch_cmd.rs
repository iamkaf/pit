use crate::error::{PitError, Result};
use crate::git;
use crate::json_out;
use crate::transaction::{Transaction, TxState};
use crate::workspace::Workspace;
use std::path::Path;

pub struct SwitchArgs {
    pub branch: String,
    pub create: bool,
    pub json: bool,
}

pub fn run(cwd: &Path, args: SwitchArgs) -> Result<()> {
    let mut ws = Workspace::discover(cwd)?;
    let branch = args.branch.trim().to_string();
    if branch.is_empty() {
        return Err(PitError::msg("branch name required"));
    }

    if has_tracked_changes(&ws, true)? || has_tracked_changes(&ws, false)? {
        return Err(PitError::msg(
            "uncommitted changes in public or private tracked files; commit or stash first",
        ));
    }

    let mut tx = Transaction::new(&format!("switch {branch}"), &branch, &branch);
    tx.public_before = git::rev_parse(&ws.work_tree, Some(&ws.public_git_dir), "HEAD").ok();
    tx.private_before = git::rev_parse(&ws.work_tree, Some(&ws.private_git_dir), "HEAD").ok();
    tx.touch(TxState::Prepared);
    ws.tx_store().save(&tx)?;

    // Switch public first
    let pub_switch = if args.create {
        ws.public_git(&["switch", "-c", &branch])
    } else {
        ws.public_git(&["switch", &branch])
    };
    if let Err(e) = pub_switch {
        tx.last_error = Some(e.to_string());
        tx.touch(TxState::FailedManual);
        ws.tx_store().save(&tx)?;
        return Err(PitError::msg(format!("public switch failed: {e}")));
    }
    tx.touch(TxState::LocalPublicCommitted);
    ws.tx_store().save(&tx)?;

    // Switch private (create if needed)
    let priv_switch = if args.create {
        // create private branch from current private HEAD or orphan
        match ws.private_git(&["switch", "-c", &branch]) {
            Ok(_) => Ok(()),
            Err(_) => {
                // maybe exists
                ws.private_git(&["switch", &branch]).map(|_| ())
            }
        }
    } else {
        match ws.private_git(&["switch", &branch]) {
            Ok(_) => Ok(()),
            Err(_) => {
                // create private branch matching public if missing
                match ws.private_git(&["switch", "-c", &branch]) {
                    Ok(_) => Ok(()),
                    Err(e) => Err(e),
                }
            }
        }
    };

    if let Err(e) = priv_switch {
        // attempt rollback public
        if let Some(ref before) = tx.public_before {
            let _ = ws.public_git(&["switch", "--detach", before]);
            // try restore previous branch name unknown — best effort detach message
        }
        tx.last_error = Some(e.to_string());
        tx.touch(TxState::FailedRecoverable);
        tx.recovery_hint = Some(
            "public branch may have switched; private failed — reconcile with pit switch / doctor"
                .into(),
        );
        ws.tx_store().save(&tx)?;
        ws.state.branch_mapping_stale = true;
        ws.save_state()?;
        return Err(PitError::msg(format!(
            "private switch failed after public switched: {e}. Mapping marked stale."
        )));
    }

    tx.public_after = git::rev_parse(&ws.work_tree, Some(&ws.public_git_dir), "HEAD").ok();
    tx.private_after = git::rev_parse(&ws.work_tree, Some(&ws.private_git_dir), "HEAD").ok();
    tx.touch(TxState::Complete);
    ws.tx_store().save(&tx)?;
    ws.tx_store().clear_current_if(tx.id)?;

    ws.state.branch_mapping_stale = false;
    ws.state.last_public_head = tx.public_after.clone();
    ws.state.last_private_head = tx.private_after.clone();
    ws.save_state()?;

    crate::exclude::update_managed_exclude(
        &ws.exclude_path(),
        &ws.policy.effective_private_patterns(),
    )?;

    if args.json {
        json_out::print_ok(
            "switch",
            serde_json::json!({
                "branch": branch,
                "created": args.create,
                "public_head": tx.public_after,
                "private_head": tx.private_after,
            }),
        );
    } else {
        println!("Switched public and private to `{branch}`.");
    }
    Ok(())
}

/// True if index or tracked worktree has modifications (ignores untracked).
fn has_tracked_changes(ws: &Workspace, public: bool) -> Result<bool> {
    let entries = if public {
        git::status_porcelain(&ws.work_tree, Some(&ws.public_git_dir))?
    } else if ws.private_git_dir.exists() {
        git::status_porcelain(&ws.work_tree, Some(&ws.private_git_dir)).unwrap_or_default()
    } else {
        return Ok(false);
    };
    Ok(entries.iter().any(|e| !e.xy.starts_with('?')))
}
