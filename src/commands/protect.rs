use crate::error::{PitError, Result};
use crate::git;
use crate::json_out;
use crate::workspace::Workspace;
use std::path::Path;

pub struct ProtectArgs {
    pub path: String,
    pub yes: bool,
    pub json: bool,
}

pub fn run(cwd: &Path, args: ProtectArgs) -> Result<()> {
    let mut ws = Workspace::discover(cwd)?;
    let path = args.path.trim_start_matches("./").replace('\\', "/");

    // History exposure check on public side
    let exposed = public_history_has_path(&ws, &path)?;
    if exposed {
        let warn = format!(
            "WARNING: `{path}` appears in local public history. \
             Moving it to private does NOT erase prior public copies, remotes, forks, or caches."
        );
        eprintln!("{warn}");
        if !args.yes && !args.json {
            eprintln!("Continue with protect? re-run with --yes to confirm.");
            return Err(PitError::msg(
                "protect requires --yes when path has public history exposure (non-interactive)",
            ));
        }
    }

    // Remove from public index without deleting worktree file
    let _ = ws.public_git(&["rm", "--cached", "--ignore-unmatch", "-q", "--", &path]);

    // Add to private policy patterns if not already
    if !ws
        .policy
        .private
        .patterns
        .iter()
        .any(|p| p == &path || p == &format!("{path}"))
    {
        // Prefer exact path pattern
        ws.policy.private.patterns.push(path.clone());
        ws.save_policy()?;
    }
    crate::exclude::update_managed_exclude(
        &ws.exclude_path(),
        &ws.policy.effective_private_patterns(),
    )?;

    // Stage into private index
    if ws.work_tree.join(&path).exists() {
        ws.private_git(&["add", "-f", "--", &path])?;
    }

    if args.json {
        json_out::print_ok(
            "protect",
            serde_json::json!({
                "path": path,
                "public_history_exposure": exposed,
                "erasure_claimed": false,
                "staged_private": true,
                "message": "Path moved toward private tracking; commit with pit commit. Prior public history is not erased.",
            }),
        );
    } else {
        println!("Protected `{path}` (staged private; not erased from public history).");
        println!("Run: pit commit -m \"Protect {path}\" && pit push");
        if exposed {
            println!("Note: previously public content remains previously public.");
        }
    }
    Ok(())
}

fn public_history_has_path(ws: &Workspace, path: &str) -> Result<bool> {
    if !git::has_commits(&ws.work_tree, Some(&ws.public_git_dir)) {
        return Ok(false);
    }
    let head = git::rev_parse(&ws.work_tree, Some(&ws.public_git_dir), "HEAD")?;
    let paths = git::all_paths_in_range(&ws.public_git_dir, None, &head)?;
    Ok(paths.iter().any(|p| p == path))
}
