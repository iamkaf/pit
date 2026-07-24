use crate::error::{PitError, Result};
use crate::git;
use crate::json_out;
use crate::workspace;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct CloneArgs {
    pub public_url: String,
    pub private_url: Option<String>,
    pub directory: Option<String>,
    pub no_setup: bool,
    pub yes: bool,
    pub json: bool,
}

pub fn run(cwd: &Path, args: CloneArgs) -> Result<()> {
    let dir_name = args.directory.clone().unwrap_or_else(|| {
        let base = args
            .public_url
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .rsplit('/')
            .next()
            .unwrap_or("repo")
            .to_string();
        base
    });
    let dest = if Path::new(&dir_name).is_absolute() {
        PathBuf::from(&dir_name)
    } else {
        cwd.join(&dir_name)
    };
    if dest.exists() {
        return Err(PitError::msg(format!(
            "destination already exists: {}",
            dest.display()
        )));
    }

    // Clone public without executing hooks from remote (standard git clone still may run)
    let status = Command::new("git")
        .args(["clone", "--", &args.public_url, &dest.to_string_lossy()])
        .status()
        .map_err(|e| PitError::msg(format!("git clone failed: {e}")))?;
    if !status.success() {
        return Err(PitError::msg("git clone failed"));
    }

    if args.no_setup {
        if args.json {
            json_out::print_ok(
                "clone",
                serde_json::json!({
                    "path": dest,
                    "setup": false,
                }),
            );
        } else {
            println!("Cloned public repository to {}", dest.display());
            println!("Run `pit setup` inside to connect a private companion.");
        }
        return Ok(());
    }

    let private = args.private_url.clone().ok_or_else(|| {
        PitError::msg("pit clone without --no-setup requires --private <url> (or use --no-setup)")
    })?;

    // Hydrate / setup
    crate::commands::setup::run(
        &dest,
        crate::commands::setup::SetupArgs {
            private: Some(private.clone()),
            create_github: false,
            yes: args.yes,
            visibility_attested: args.yes,
        },
    )?;

    // Fetch private and checkout private files
    hydrate_private(&dest, &private)?;

    if args.json {
        json_out::print_ok(
            "clone",
            serde_json::json!({
                "path": dest,
                "setup": true,
                "private": private,
            }),
        );
    } else {
        println!("Cloned and set up Pit workspace at {}", dest.display());
    }
    Ok(())
}

/// Fetch private remote and materialize private-tracked files into the work tree.
pub fn hydrate_private(work_tree: &Path, _private_url: &str) -> Result<()> {
    let ws = workspace::Workspace::discover(work_tree)?;
    // fetch private
    let _ = ws.private_git(&["fetch", &ws.config.private_remote_name]);
    // try checkout private branch content without touching public-tracked paths
    let branch = ws.public_branch().unwrap_or_else(|_| "main".into());
    // If remote has branch, reset private index/worktree files from it carefully
    let remote_ref = format!("refs/remotes/{}/{}", ws.config.private_remote_name, branch);
    let has_remote = ws
        .private_git(&["rev-parse", "--verify", &remote_ref])
        .is_ok()
        || ws
            .private_git(&[
                "rev-parse",
                "--verify",
                &format!("{}/{}", ws.config.private_remote_name, branch),
            ])
            .is_ok();

    if has_remote {
        // Create local private branch tracking remote if needed
        let _ = ws.private_git(&[
            "checkout",
            "-B",
            &branch,
            &format!("{}/{}", ws.config.private_remote_name, branch),
        ]);
    } else {
        // try fetch all and checkout
        let _ = ws.private_git(&["fetch", &ws.config.private_remote_name, "+refs/heads/*:refs/remotes/private/*"]);
        let _ = ws.private_git(&[
            "checkout",
            "-B",
            &branch,
            &format!("private/{branch}"),
        ]);
    }

    // Checkout private index into work tree (private files only). Use restore from HEAD.
    if git::has_commits(work_tree, Some(&ws.private_git_dir)) {
        let files = ws.private_tracked()?;
        for f in files {
            let _ = ws.private_git(&["checkout", "HEAD", "--", &f]);
        }
    }

    crate::exclude::update_managed_exclude(
        &ws.exclude_path(),
        &ws.policy.effective_private_patterns(),
    )?;
    Ok(())
}
