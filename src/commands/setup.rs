use crate::error::{PitError, Result};
use crate::git;
use crate::json_out;
use crate::workspace::{self, Workspace};
use std::path::Path;
use std::process::Command;

pub struct SetupArgs {
    pub private: Option<String>,
    pub create_github: bool,
    pub yes: bool,
    pub visibility_attested: bool,
    pub json: bool,
}

pub fn run(cwd: &Path, args: SetupArgs) -> Result<()> {
    if !git::is_git_repo(cwd) {
        return Err(PitError::NotAGitRepo(cwd.display().to_string()));
    }
    let work_tree = git::find_work_tree(cwd)?;

    if let Ok(existing) = Workspace::discover(&work_tree) {
        // refresh exclude + hooks
        crate::exclude::update_managed_exclude(
            &existing.exclude_path(),
            &existing.policy.effective_private_patterns(),
        )?;
        workspace::install_hooks(&existing.work_tree, &existing.public_git_dir, &existing.pit_dir)?;
        // Re-hydrate if private remote has content
        if !existing.config.private_remote.is_empty() {
            let _ = crate::commands::clone_cmd::hydrate_private(
                &work_tree,
                &existing.config.private_remote,
            );
        }
        if args.json {
            json_out::print_ok(
                "setup",
                serde_json::json!({
                    "already_configured": true,
                    "path": existing.work_tree,
                    "private_remote": existing.config.private_remote,
                    "refreshed": true,
                }),
            );
        } else {
            println!(
                "Pit workspace already configured at {}",
                existing.pit_dir.display()
            );
            println!("Private remote: {}", existing.config.private_remote);
            println!("Refreshed managed exclude and hooks.");
        }
        return Ok(());
    }

    let private_remote = if let Some(url) = args.private {
        url
    } else if args.create_github {
        create_github_private(&work_tree)?
    } else {
        return Err(PitError::msg(
            "pit setup requires --private <url> or --create-github\n\
             Example: pit setup --private /path/to/private-remote.git",
        ));
    };

    let visibility = if private_remote.starts_with('/') || private_remote.starts_with("file://") {
        "user-attested-private"
    } else if args.visibility_attested || args.yes {
        "user-attested-private"
    } else if looks_like_github(&private_remote) {
        match verify_github_private(&private_remote) {
            Ok(true) => "verified-private",
            Ok(false) => {
                return Err(PitError::msg(
                    "private remote appears publicly readable; refusing setup",
                ));
            }
            Err(e) => {
                eprintln!("warning: could not verify private visibility: {e}");
                if args.yes {
                    "user-attested-private"
                } else {
                    return Err(PitError::msg(
                        "could not verify private remote visibility; re-run with --yes to attest",
                    ));
                }
            }
        }
    } else if args.yes {
        "user-attested-private"
    } else {
        return Err(PitError::msg(
            "generic remote: re-run with --yes to attest that the private remote is access-restricted",
        ));
    };

    let ws = workspace::init_pit_workspace(&work_tree, &private_remote, visibility, None)?;

    // Hydrate from an already-populated private remote (fetch + materialize files)
    let hydrated = crate::commands::clone_cmd::hydrate_private(&work_tree, &private_remote);
    let hydrate_ok = hydrated.is_ok();
    if let Err(e) = hydrated {
        // Empty private remote is fine for brand-new companions
        eprintln!("note: private hydrate skipped or partial: {e}");
    }

    if args.json {
        json_out::print_ok(
            "setup",
            serde_json::json!({
                "path": ws.work_tree,
                "private_remote": private_remote,
                "private_visibility": visibility,
                "new_files": ws.policy.classification.new_files,
                "hooks_installed": true,
                "hydrated": hydrate_ok,
            }),
        );
    } else {
        println!("Public repository: {}", ws.work_tree.display());
        println!("Private repository: {private_remote}");
        println!("Private visibility: {visibility}");
        println!(
            "Default handling for new files: {}",
            ws.policy.classification.new_files
        );
        println!("Hooks installed: pre-commit, pre-push, post-checkout, post-merge, post-rewrite");
        println!("Managed exclude: updated");
        if hydrate_ok {
            println!("Private hydrate: attempted from remote");
        }
        println!("Workspace health: run `pit doctor` to verify");
        println!();
        println!("Next:");
        println!("  pit add .");
        println!("  pit commit -m \"...\"");
        println!("  pit push");
    }
    Ok(())
}

fn looks_like_github(url: &str) -> bool {
    url.contains("github.com")
}

fn verify_github_private(url: &str) -> Result<bool> {
    let repo = parse_github_repo(url).ok_or_else(|| PitError::msg("cannot parse GitHub URL"))?;
    let output = Command::new("gh")
        .args(["repo", "view", &repo, "--json", "isPrivate", "-q", ".isPrivate"])
        .output()
        .map_err(|e| PitError::msg(format!("gh failed: {e}")))?;
    if !output.status.success() {
        return Err(PitError::msg(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(s == "true")
}

fn parse_github_repo(url: &str) -> Option<String> {
    let url = url.trim_end_matches('/').trim_end_matches(".git");
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        return Some(rest.to_string());
    }
    if let Some(rest) = url.strip_prefix("https://github.com/") {
        return Some(rest.to_string());
    }
    if let Some(rest) = url.strip_prefix("ssh://git@github.com/") {
        return Some(rest.to_string());
    }
    None
}

fn create_github_private(work_tree: &Path) -> Result<String> {
    let public_url = git::remote_url(work_tree, None, "origin").unwrap_or_default();
    let base = parse_github_repo(&public_url).unwrap_or_else(|| {
        let name = work_tree
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".into());
        format!("iamkaf/{name}")
    });
    let private_name = format!("{base}-private");
    eprintln!("Creating private GitHub repository {private_name}...");
    let output = Command::new("gh")
        .args(["repo", "create", &private_name, "--private", "--confirm"])
        .output()
        .map_err(|e| PitError::msg(format!("gh failed: {e}")))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        if !(err.contains("already exists") || err.contains("Name already exists")) {
            return Err(PitError::msg(format!("gh repo create failed: {err}")));
        }
        eprintln!("Repository already exists; connecting.");
    }
    Ok(format!("git@github.com:{private_name}.git"))
}
