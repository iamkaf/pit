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

    crate::commands::setup::run(
        &dest,
        crate::commands::setup::SetupArgs {
            private: Some(private.clone()),
            create_github: false,
            yes: args.yes,
            visibility_attested: args.yes,
            json: false,
        },
    )?;

    // setup already hydrates; ensure again for clone reporting
    let _ = hydrate_private(&dest, &private);

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
///
/// Used by `pit clone` and `pit setup --private` so connecting an already-populated
/// private companion hydrates private paths into the public checkout.
pub fn hydrate_private(work_tree: &Path, _private_url: &str) -> Result<()> {
    let ws = workspace::Workspace::discover(work_tree)?;
    let remote = ws.config.private_remote_name.clone();
    let branch = ws.public_branch().unwrap_or_else(|_| {
        // fall back to common defaults
        "main".into()
    });
    let branch = if branch.is_empty() {
        "main".to_string()
    } else {
        branch
    };

    // Ensure remote URL is configured
    if ws.config.private_remote.is_empty() {
        return Err(PitError::msg("private remote URL not configured"));
    }

    // Fetch all heads from private remote
    ws.private_git(&[
        "fetch",
        &remote,
        "+refs/heads/*:refs/remotes/private/*",
    ])
    .or_else(|_| ws.private_git(&["fetch", &remote]))?;

    // Resolve remote tip for branch (try main/master aliases)
    let candidates = [
        format!("refs/remotes/private/{branch}"),
        format!("{remote}/{branch}"),
        "refs/remotes/private/main".into(),
        "refs/remotes/private/master".into(),
        format!("{remote}/main"),
        format!("{remote}/master"),
    ];
    let mut tip: Option<String> = None;
    for c in &candidates {
        if let Ok(sha) = ws.private_git(&["rev-parse", "--verify", c]) {
            if !sha.is_empty() {
                tip = Some(c.clone());
                break;
            }
        }
    }

    let Some(remote_tip) = tip else {
        // Empty private remote — nothing to hydrate
        return Ok(());
    };

    // Point local private branch at remote tip and check out private files
    let local_branch = branch.clone();
    ws.private_git(&["checkout", "-B", &local_branch, &remote_tip])?;

    if git::has_commits(work_tree, Some(&ws.private_git_dir)) {
        // Materialize all paths from private HEAD into the work tree
        let tree = ws.private_git(&["ls-tree", "-r", "--name-only", "HEAD"])?;
        for f in tree.lines().filter(|l| !l.is_empty()) {
            let _ = ws.private_git(&["checkout", "HEAD", "--", f]);
        }
    }

    crate::exclude::update_managed_exclude(
        &ws.exclude_path(),
        &ws.policy.effective_private_patterns(),
    )?;
    Ok(())
}
