use crate::error::{PitError, Result};
use crate::git;
use crate::transaction::{Transaction, TxState};
use crate::workspace::Workspace;
use std::path::Path;

pub struct CommitArgs {
    pub message: Option<String>,
    pub dry_run: bool,
}

pub fn run(cwd: &Path, args: CommitArgs) -> Result<()> {
    let mut ws = Workspace::discover(cwd)?;

    let dual = ws.dual_tracked()?;
    if !dual.is_empty() {
        return Err(PitError::DualTracked(dual));
    }

    // Pending push transaction blocks new commits? Allow local commits but warn.
    if let Some(pending) = ws.tx_store().load_current()? {
        if pending.needs_resume() {
            return Err(PitError::PendingTransaction(format!(
                "{} — run `pit push --resume` before new commits",
                pending.id
            )));
        }
    }

    let message = match args.message {
        Some(m) if !m.trim().is_empty() => m,
        _ => {
            return Err(PitError::msg(
                "commit message required in non-interactive mode: pit commit -m \"...\"",
            ));
        }
    };

    let public_staged = git::staged_paths(&ws.work_tree, Some(&ws.public_git_dir))?;
    let private_staged = if ws.private_git_dir.exists() {
        git::staged_paths(&ws.work_tree, Some(&ws.private_git_dir)).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Validate no private paths being *added/modified* in public index.
    // Deletions (status D) are allowed so `pit protect` can land a public removal.
    let matcher = ws.policy.matcher()?;
    let public_name_status = ws
        .public_git(&["diff", "--cached", "--name-status", "-z"])
        .unwrap_or_default();
    for entry in parse_name_status_z(&public_name_status) {
        if entry.status != "D" && matcher.is_private_pattern_match(&entry.path) {
            return Err(PitError::PrivacyValidation(format!(
                "private path staged in public index: {}",
                entry.path
            )));
        }
    }

    if public_staged.is_empty() && private_staged.is_empty() {
        return Err(PitError::msg("nothing to commit (both indexes empty)"));
    }

    let public_branch = ws.public_branch().unwrap_or_else(|_| "main".into());
    let private_branch = ws.private_branch().unwrap_or_else(|_| public_branch.clone());

    let mut tx = Transaction::new(&message, &public_branch, &private_branch);
    tx.public_paths = public_staged.clone();
    tx.private_paths = private_staged.clone();
    tx.public_before = git::rev_parse(&ws.work_tree, Some(&ws.public_git_dir), "HEAD").ok();
    tx.private_before = git::rev_parse(&ws.work_tree, Some(&ws.private_git_dir), "HEAD").ok();
    tx.touch(TxState::Prepared);
    ws.tx_store().save(&tx)?;

    if args.dry_run {
        println!("Would create transaction {}", tx.id);
        println!("  public paths:  {}", public_staged.len());
        println!("  private paths: {}", private_staged.len());
        return Ok(());
    }

    // Snapshot refs for rollback
    let pub_head_before = tx.public_before.clone();
    let priv_head_before = tx.private_before.clone();

    let result = (|| -> Result<()> {
        // Public commit first (no private linkage in message/metadata)
        if !public_staged.is_empty() {
            // Use --no-verify to avoid recursive hook issues during pit commit;
            // we already validated. Hooks still protect plain git commit.
            ws.public_git(&["commit", "--no-verify", "-m", &message])?;
            let after = git::rev_parse(&ws.work_tree, Some(&ws.public_git_dir), "HEAD")?;
            tx.public_after = Some(after);
            tx.touch(TxState::LocalPublicCommitted);
            ws.tx_store().save(&tx)?;
        }

        // Private commit — may include trailers linking to public
        if !private_staged.is_empty() {
            // Ensure private policy is tracked
            ensure_private_policy_staged(&ws)?;

            let mut priv_msg = message.clone();
            priv_msg.push_str(&format!("\n\nPit-Transaction: {}\n", tx.id));
            if let Some(ref pub_c) = tx.public_after.as_ref().or(tx.public_before.as_ref()) {
                priv_msg.push_str(&format!("Pit-Public-Commit: {pub_c}\n"));
            }
            ws.private_git(&["commit", "--no-verify", "-m", &priv_msg])?;
            let after = git::rev_parse(&ws.work_tree, Some(&ws.private_git_dir), "HEAD")?;
            tx.private_after = Some(after);
            tx.touch(TxState::LocalPrivateCommitted);
            ws.tx_store().save(&tx)?;
        }

        tx.touch(TxState::LocalComplete);
        ws.tx_store().save(&tx)?;
        Ok(())
    })();

    if let Err(e) = result {
        // Rollback commits if partial
        if let Some(ref before) = pub_head_before {
            if tx.public_after.is_some() {
                let _ = ws.public_git(&["reset", "--soft", before]);
            }
        } else if tx.public_after.is_some() {
            // was unborn — remove ref? soft reset hard for orphan
            let _ = ws.public_git(&["update-ref", "-d", "HEAD"]);
        }
        if let Some(ref before) = priv_head_before {
            if tx.private_after.is_some() {
                let _ = ws.private_git(&["reset", "--soft", before]);
            }
        } else if tx.private_after.is_some() {
            let _ = ws.private_git(&["update-ref", "-d", "HEAD"]);
        }
        tx.last_error = Some(e.to_string());
        tx.touch(TxState::FailedManual);
        let _ = ws.tx_store().save(&tx);
        return Err(e);
    }

    ws.state.last_public_head = tx.public_after.clone().or(tx.public_before.clone());
    ws.state.last_private_head = tx.private_after.clone().or(tx.private_before.clone());
    ws.save_state()?;

    println!("Transaction {}", tx.id);
    match (&tx.public_after, &tx.private_after) {
        (Some(p), Some(v)) => {
            println!("  public:  {p}");
            println!("  private: {v}");
        }
        (Some(p), None) => {
            println!("  public:  {p}");
            println!("  private: (none)");
        }
        (None, Some(v)) => {
            println!("  public:  (none)");
            println!("  private: {v}");
        }
        (None, None) => unreachable!(),
    }
    println!("State: local-complete (run `pit push` to publish)");
    Ok(())
}

struct NameStatus {
    status: String,
    path: String,
}

fn parse_name_status_z(s: &str) -> Vec<NameStatus> {
    let mut out = Vec::new();
    let mut parts = s.split('\0').filter(|p| !p.is_empty());
    while let Some(chunk) = parts.next() {
        // format: "M\tpath" or "R100\told" then new path — simplified
        if let Some((st, path)) = chunk.split_once('\t') {
            let status = st.chars().next().unwrap_or('M').to_string();
            out.push(NameStatus {
                status,
                path: path.to_string(),
            });
        } else if chunk.len() >= 2 {
            // fallback "Mpath" unlikely with -z name-status
            let status = chunk.chars().next().unwrap_or('M').to_string();
            let path = chunk[1..].trim().to_string();
            if !path.is_empty() {
                out.push(NameStatus { status, path });
            }
        }
    }
    out
}

fn ensure_private_policy_staged(ws: &Workspace) -> Result<()> {
    // Copy authoritative policy into work-tree path `.pit/policy.toml` for the
    // private mirror only. `.pit/**` is a mandatory private pattern and is
    // always present in the managed exclude block (see Policy::effective_private_patterns).
    let pit_meta = ws.work_tree.join(".pit");
    std::fs::create_dir_all(&pit_meta)?;
    let dest = pit_meta.join("policy.toml");
    std::fs::copy(ws.pit_dir.join("policy.toml"), &dest)?;

    // Refresh managed exclude so plain `git add` cannot stage policy into public.
    crate::exclude::update_managed_exclude(
        &ws.exclude_path(),
        &ws.policy.effective_private_patterns(),
    )?;

    ws.private_git(&["add", "-f", "--", ".pit/policy.toml"])?;
    Ok(())
}
