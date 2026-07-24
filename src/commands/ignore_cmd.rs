use crate::error::Result;
use crate::json_out;
use crate::workspace::Workspace;
use std::path::Path;

pub struct IgnoreArgs {
    pub path: String,
    pub json: bool,
}

pub fn run(cwd: &Path, args: IgnoreArgs) -> Result<()> {
    let mut ws = Workspace::discover(cwd)?;
    let path = args.path.trim_start_matches("./").replace('\\', "/");

    let pub_had = ws.public_tracked()?.iter().any(|p| p == &path);
    let priv_had = ws.private_tracked()?.iter().any(|p| p == &path);

    if pub_had {
        let _ = ws.public_git(&["rm", "--cached", "--ignore-unmatch", "-q", "--", &path]);
        eprintln!("warning: `{path}` removed from public index; history may still contain it.");
    }
    if priv_had {
        let _ = ws.private_git(&["rm", "--cached", "--ignore-unmatch", "-q", "--", &path]);
    }

    // Add to ignored patterns
    if !ws.policy.ignored.patterns.iter().any(|p| p == &path) {
        ws.policy.ignored.patterns.push(path.clone());
    }
    // Remove from private exact patterns
    ws.policy.private.patterns.retain(|p| p != &path);
    ws.policy.public.patterns.retain(|p| p != &path);
    ws.save_policy()?;
    crate::exclude::update_managed_exclude(
        &ws.exclude_path(),
        &ws.policy.effective_private_patterns(),
    )?;

    // Also append path to managed exclude via private list? ignored paths can go in exclude
    // by adding to a side list — for simplicity add as private pattern that is only ignored
    // Classification: ignored patterns already handled by matcher.

    if args.json {
        json_out::print_ok(
            "ignore",
            serde_json::json!({
                "path": path,
                "was_public": pub_had,
                "was_private": priv_had,
                "worktree_preserved": true,
            }),
        );
    } else {
        println!("Ignoring `{path}` (worktree preserved; untracked in both repos).");
    }
    Ok(())
}
