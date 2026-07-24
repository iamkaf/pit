use crate::error::{PitError, Result};
use crate::git;
use crate::json_out;
use crate::policy::Class;
use crate::workspace::Workspace;
use std::path::Path;

pub struct RestoreArgs {
    pub paths: Vec<String>,
    pub staged: bool,
    pub json: bool,
}

pub fn run(cwd: &Path, args: RestoreArgs) -> Result<()> {
    if !args.staged {
        return Err(PitError::msg(
            "only `pit restore --staged` is supported in this release",
        ));
    }
    let ws = Workspace::discover(cwd)?;
    if args.paths.is_empty() {
        return Err(PitError::msg("pathspec required: pit restore --staged <path>…"));
    }

    let matcher = ws.policy.matcher()?;
    let public_tracked: std::collections::HashSet<_> =
        ws.public_tracked()?.into_iter().collect();
    let private_tracked: std::collections::HashSet<_> =
        ws.private_tracked()?.into_iter().collect();

    let mut public_paths = Vec::new();
    let mut private_paths = Vec::new();

    for path in &args.paths {
        let class = if private_tracked.contains(path) {
            Class::Private
        } else if public_tracked.contains(path) {
            Class::Public
        } else {
            // staged-only paths: check staged lists
            let pub_staged = git::staged_paths(&ws.work_tree, Some(&ws.public_git_dir))?;
            let priv_staged =
                git::staged_paths(&ws.work_tree, Some(&ws.private_git_dir)).unwrap_or_default();
            if priv_staged.iter().any(|p| p == path) {
                Class::Private
            } else if pub_staged.iter().any(|p| p == path) {
                Class::Public
            } else {
                matcher.classify(path)
            }
        };
        match class {
            Class::Private => private_paths.push(path.clone()),
            Class::Public => public_paths.push(path.clone()),
            Class::Ignored | Class::Unclassified => {
                // try both indexes
                public_paths.push(path.clone());
                private_paths.push(path.clone());
            }
        }
    }

    for p in &public_paths {
        let _ = ws.public_git(&["restore", "--staged", "--", p]);
    }
    for p in &private_paths {
        let _ = ws.private_git(&["restore", "--staged", "--", p]);
    }

    if args.json {
        json_out::print_ok(
            "restore",
            serde_json::json!({
                "public_unstaged": public_paths,
                "private_unstaged": private_paths,
            }),
        );
    } else {
        println!(
            "Unstaged {} public, {} private path(s).",
            public_paths.len(),
            private_paths.len()
        );
    }
    Ok(())
}
