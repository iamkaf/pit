use crate::error::{PitError, Result};
use crate::git;
use crate::json_out;
use crate::transaction::{TxState, Transaction};
use crate::workspace::Workspace;
use std::fs;
use std::path::Path;

pub struct PushArgs {
    pub resume: bool,
    pub dry_run: bool,
    pub json: bool,
}

pub fn run(cwd: &Path, args: PushArgs) -> Result<()> {
    let ws = Workspace::discover(cwd)?;
    let lock_path = ws.pit_dir.join("locks").join("push.lock");
    fs::create_dir_all(lock_path.parent().unwrap())?;
    if lock_path.exists() {
        // stale lock: if process dead, allow override after check
        let content = fs::read_to_string(&lock_path).unwrap_or_default();
        return Err(PitError::msg(format!(
            "push lock held ({content}). Remove {} if no push is running.",
            lock_path.display()
        )));
    }
    fs::write(&lock_path, format!("{}", std::process::id()))?;
    let result = push_inner(&ws, &args);
    let _ = fs::remove_file(&lock_path);
    result
}

fn push_inner(ws: &Workspace, args: &PushArgs) -> Result<()> {
    if ws.state.branch_mapping_stale {
        return Err(PitError::msg(
            "branch mapping is stale; run `pit switch` or `pit doctor --repair` before push",
        ));
    }
    let dual = ws.dual_tracked()?;
    if !dual.is_empty() {
        return Err(PitError::DualTracked(dual));
    }

    // Visibility gate for private remote
    if ws.config.private_visibility == "unverified" {
        return Err(PitError::msg(
            "private remote visibility is unverified; re-run setup with --yes to attest",
        ));
    }

    let store = ws.tx_store();
    let mut tx = if args.resume {
        store
            .load_current()?
            .ok_or_else(|| PitError::msg("no pending transaction to resume"))?
    } else if let Some(pending) = store.load_current()? {
        if pending.needs_resume() {
            return Err(PitError::PendingTransaction(format!(
                "{} needs resume — run `pit push --resume`",
                pending.id
            )));
        }
        if pending.state == TxState::LocalComplete || pending.is_pending_push() {
            pending
        } else {
            make_push_tx(ws)?
        }
    } else {
        make_push_tx(ws)?
    };

    // CRITICAL: git push sends the branch tip (HEAD), not a stale journal tip.
    // Always sync public_after/private_after to the tips that the refspecs will
    // actually publish, then walk remote..that tip before any public push.
    let public_branch = ws.public_branch().unwrap_or_else(|_| tx.public_branch.clone());
    let private_branch = ws.private_branch().unwrap_or_else(|_| tx.private_branch.clone());
    tx.public_branch = public_branch.clone();
    tx.private_branch = private_branch.clone();

    let public_tip =
        git::rev_parse(&ws.work_tree, Some(&ws.public_git_dir), "HEAD").ok();
    let private_tip =
        git::rev_parse(&ws.work_tree, Some(&ws.private_git_dir), "HEAD").ok();

    if let Some(ref tip) = public_tip {
        if tx.public_after.as_ref() != Some(tip) {
            // HEAD moved since the journaled commit — still must validate everything pushed.
            tx.public_after = Some(tip.clone());
            store.save(&tx)?;
        }
    }
    if let Some(ref tip) = private_tip {
        if tx.private_after.as_ref() != Some(tip) {
            tx.private_after = Some(tip.clone());
            store.save(&tx)?;
        }
    }

    if tx.public_after.is_none() && tx.private_after.is_none() {
        return Err(PitError::msg("nothing to push"));
    }

    // --- Validate exact outgoing public range (remote tip .. HEAD) BEFORE any push ---
    if let Some(ref public_head) = tx.public_after.clone() {
        validate_public_outbound(ws, public_head, &tx)?;
    }

    if args.dry_run {
        if args.json {
            json_out::print_ok(
                "push",
                serde_json::json!({
                    "dry_run": true,
                    "id": tx.id,
                    "private": tx.private_after,
                    "public": tx.public_after,
                }),
            );
        } else {
            println!("Dry-run push for transaction {}", tx.id);
            println!("  private first: {:?}", tx.private_after);
            println!("  public second: {:?}", tx.public_after);
        }
        return Ok(());
    }

    // --- Private push first ---
    if !tx.private_push_ok {
        if tx.private_after.is_some() && git::has_commits(&ws.work_tree, Some(&ws.private_git_dir)) {
            tx.touch(TxState::PrivatePushStarted);
            store.save(&tx)?;

            let remote = &ws.config.private_remote_name;
            ensure_private_remote(ws)?;

            let refspec = format!("refs/heads/{private_branch}:refs/heads/{private_branch}");
            unsafe {
                std::env::set_var("PIT_PUSH_IN_PROGRESS", "1");
            }
            let priv_result = ws.private_git(&["push", "-u", remote, &refspec]);
            unsafe {
                std::env::remove_var("PIT_PUSH_IN_PROGRESS");
            }
            match priv_result {
                Ok(out) => {
                    if !out.is_empty() {
                        eprintln!("{out}");
                    }
                    tx.private_push_ok = true;
                    tx.touch(TxState::PrivatePushed);
                    store.save(&tx)?;
                    println!("Private repository: pushed successfully");
                }
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("Everything up-to-date") || msg.contains("up to date") {
                        tx.private_push_ok = true;
                        tx.touch(TxState::PrivatePushed);
                        store.save(&tx)?;
                        println!("Private repository: already up-to-date");
                    } else {
                        tx.last_error = Some(msg.clone());
                        tx.touch(TxState::FailedRecoverable);
                        store.save(&tx)?;
                        return Err(PitError::msg(format!(
                            "private push failed (public not attempted): {msg}"
                        )));
                    }
                }
            }
        } else {
            tx.private_push_ok = true;
            tx.touch(TxState::PrivatePushed);
            store.save(&tx)?;
            println!("Private repository: nothing to push");
        }
    } else {
        println!("Private repository: already pushed (resuming)");
    }

    // --- Public push second ---
    if !tx.public_push_ok {
        if let Some(ref public_head) = tx.public_after.clone() {
            // Re-validate the exact tip that will be published (HEAD), not a stale journal only.
            validate_public_outbound(ws, public_head, &tx)?;

            tx.touch(TxState::PublicPushStarted);
            store.save(&tx)?;

            let remote = &ws.config.public_remote_name;
            let has_origin = ws.public_git(&["remote", "get-url", remote]).is_ok();
            if !has_origin {
                tx.last_error = Some("public remote not configured".into());
                tx.touch(TxState::FailedRecoverable);
                store.save(&tx)?;
                return Err(PitError::msg(format!(
                    "Private repository: pushed successfully\n\
                     Public repository: push failed (no remote `{remote}`)\n\
                     Transaction {} is pending public publication.\n\
                     Run: pit push --resume",
                    tx.id
                )));
            }

            let refspec = format!("refs/heads/{public_branch}:refs/heads/{public_branch}");
            // Bypass pre-push hook block for coordinated pit push (validation already done).
            unsafe {
                std::env::set_var("PIT_PUSH_IN_PROGRESS", "1");
            }
            let push_result = ws.public_git(&["push", "-u", remote, &refspec]);
            unsafe {
                std::env::remove_var("PIT_PUSH_IN_PROGRESS");
            }
            match push_result {
                Ok(out) => {
                    if !out.is_empty() {
                        eprintln!("{out}");
                    }
                    tx.public_push_ok = true;
                    tx.touch(TxState::Complete);
                    store.save(&tx)?;
                    store.clear_current_if(tx.id)?;
                    println!("Public repository: pushed successfully");
                }
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("Everything up-to-date") || msg.contains("up to date") {
                        tx.public_push_ok = true;
                        tx.touch(TxState::Complete);
                        store.save(&tx)?;
                        store.clear_current_if(tx.id)?;
                        println!("Public repository: already up-to-date");
                    } else {
                        tx.last_error = Some(msg.clone());
                        tx.touch(TxState::FailedRecoverable);
                        store.save(&tx)?;
                        return Err(PitError::msg(format!(
                            "Private repository: pushed successfully\n\
                             Public repository: push rejected\n\
                             \n\
                             Transaction {} is pending public publication.\n\
                             No private data was sent to the public remote.\n\
                             Run: pit push --resume\n\
                             \n\
                             Detail: {msg}",
                            tx.id
                        )));
                    }
                }
            }
        } else {
            tx.public_push_ok = true;
            tx.touch(TxState::Complete);
            store.save(&tx)?;
            store.clear_current_if(tx.id)?;
            println!("Public repository: nothing to push");
        }
    }

    if args.json {
        json_out::print_ok(
            "push",
            serde_json::json!({
                "id": tx.id,
                "state": "complete",
                "private_push_ok": tx.private_push_ok,
                "public_push_ok": tx.public_push_ok,
                "public": tx.public_after,
                "private": tx.private_after,
            }),
        );
    } else {
        println!("Transaction {} complete", tx.id);
    }
    Ok(())
}

fn make_push_tx(ws: &Workspace) -> Result<Transaction> {
    let public_branch = ws.public_branch().unwrap_or_else(|_| "main".into());
    let private_branch = ws.private_branch().unwrap_or_else(|_| public_branch.clone());
    let mut tx = Transaction::new("(push)", &public_branch, &private_branch);
    tx.public_after = git::rev_parse(&ws.work_tree, Some(&ws.public_git_dir), "HEAD").ok();
    tx.private_after = git::rev_parse(&ws.work_tree, Some(&ws.private_git_dir), "HEAD").ok();
    tx.touch(TxState::LocalComplete);
    ws.tx_store().save(&tx)?;
    Ok(tx)
}

fn ensure_private_remote(ws: &Workspace) -> Result<()> {
    let name = &ws.config.private_remote_name;
    let url = &ws.config.private_remote;
    if url.is_empty() {
        return Err(PitError::msg("private remote URL not configured"));
    }
    match ws.private_git(&["remote", "get-url", name]) {
        Ok(existing) if existing == *url => Ok(()),
        Ok(_) => {
            ws.private_git(&["remote", "set-url", name, url])?;
            Ok(())
        }
        Err(_) => {
            ws.private_git(&["remote", "add", name, url])?;
            Ok(())
        }
    }
}

/// Walk exact outgoing public commit range; reject protected/private/dual paths.
pub fn validate_public_outbound(ws: &Workspace, public_head: &str, tx: &Transaction) -> Result<()> {
    let remote = &ws.config.public_remote_name;
    // Determine remote tip if any
    let remote_tip = ws
        .public_git(&["rev-parse", &format!("{remote}/{public_branch}", public_branch = tx.public_branch)])
        .ok()
        .or_else(|| {
            ws.public_git(&["ls-remote", "--heads", remote, &tx.public_branch])
                .ok()
                .and_then(|s| s.split_whitespace().next().map(|x| x.to_string()))
        });

    let paths = git::all_paths_in_range(
        &ws.public_git_dir,
        remote_tip.as_deref(),
        public_head,
    )?;

    let matcher = ws.policy.matcher()?;
    let mut violations = Vec::new();
    for p in &paths {
        if matcher.is_private_pattern_match(p) {
            violations.push(format!("protected/private path in public history: {p}"));
        }
        // Hard-coded: Pit metadata must never appear in public history even if
        // a workspace's policy cache was stripped of these patterns.
        if p == ".pit"
            || p.starts_with(".pit/")
            || p.starts_with(".git/pit")
            || p == ".git/pit/config.toml"
        {
            violations.push(format!("pit private metadata in public history: {p}"));
        }
    }

    // Dual-tracked
    for d in ws.dual_tracked()? {
        violations.push(format!("dual-tracked path: {d}"));
    }

    // Content canary-style: search private patterns' staged content is N/A;
    // scan blobs for known private file basenames already covered by path walk.

    if !violations.is_empty() {
        return Err(PitError::PrivacyValidation(violations.join("\n")));
    }
    Ok(())
}

/// Scan a bare (or regular) public git dir for path or content canary.
pub fn public_repo_contains(git_dir: &Path, path_needle: &str, content_needle: &str) -> Result<(bool, bool)> {
    let objects = git::run_git(None, Some(git_dir), &["rev-list", "--all", "--objects"], None)?;
    let mut path_hit = false;
    for line in objects.lines() {
        if let Some((_, path)) = line.split_once(' ') {
            if path == path_needle || path.contains(path_needle) {
                path_hit = true;
            }
        }
    }

    let mut content_hit = false;
    if !content_needle.is_empty() {
        let commits = git::run_git(None, Some(git_dir), &["rev-list", "--all"], None)?;
        for commit in commits.lines() {
            if commit.is_empty() {
                continue;
            }
            match git::run_git(
                None,
                Some(git_dir),
                &["grep", "-a", "-F", "-e", content_needle, commit],
                None,
            ) {
                Ok(s) if !s.is_empty() => {
                    content_hit = true;
                    break;
                }
                _ => {}
            }
        }
        // Also try git log -S
        if !content_hit {
            match git::run_git(
                None,
                Some(git_dir),
                &["log", "-S", content_needle, "--all", "--oneline"],
                None,
            ) {
                Ok(s) if !s.is_empty() => content_hit = true,
                _ => {}
            }
        }
    }
    Ok((path_hit, content_hit))
}
