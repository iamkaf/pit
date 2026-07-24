use crate::error::{PitError, Result};
use crate::git;
use crate::policy::{Class, PolicyMatcher};
use crate::workspace::Workspace;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForceClass {
    None,
    Public,
    Private,
    Ignore,
}

pub struct AddArgs {
    pub paths: Vec<String>,
    pub all: bool,
    pub force: ForceClass,
    pub dry_run: bool,
    pub json: bool,
}

pub fn run(cwd: &Path, args: AddArgs) -> Result<()> {
    let ws = Workspace::discover(cwd)?;
    let matcher = ws.policy.matcher()?;

    let dual = ws.dual_tracked()?;
    if !dual.is_empty() {
        return Err(PitError::DualTracked(dual));
    }

    let candidates = collect_candidates(&ws, &args)?;
    if candidates.is_empty() {
        if args.json {
            crate::json_out::print_ok("add", serde_json::json!({ "staged": false, "reason": "nothing" }));
        } else {
            println!("Nothing specified, nothing added.");
        }
        return Ok(());
    }

    let public_tracked: HashSet<String> = ws.public_tracked()?.into_iter().collect();
    let private_tracked: HashSet<String> = ws.private_tracked()?.into_iter().collect();

    let mut public_paths = Vec::new();
    let mut private_paths = Vec::new();
    let mut ignored_paths = Vec::new();
    let mut unclassified = Vec::new();

    for path in &candidates {
        let class = if args.force == ForceClass::Public {
            Class::Public
        } else if args.force == ForceClass::Private {
            Class::Private
        } else if args.force == ForceClass::Ignore {
            Class::Ignored
        } else if private_tracked.contains(path) {
            Class::Private
        } else if public_tracked.contains(path) {
            Class::Public
        } else {
            matcher.classify(path)
        };

        match class {
            Class::Public => public_paths.push(path.clone()),
            Class::Private => private_paths.push(path.clone()),
            Class::Ignored => ignored_paths.push(path.clone()),
            Class::Unclassified => unclassified.push(path.clone()),
        }
    }

    // Interactive prompt when policy is prompt + TTY
    if !unclassified.is_empty()
        && args.force == ForceClass::None
        && ws.policy.classification.new_files == "prompt"
    {
        if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            let remaining = prompt_classify(&unclassified)?;
            for (path, class) in remaining {
                match class {
                    Class::Public => public_paths.push(path),
                    Class::Private => private_paths.push(path),
                    Class::Ignored => ignored_paths.push(path),
                    Class::Unclassified => {}
                }
            }
            unclassified.clear();
        }
    }

    if !unclassified.is_empty() && matcher.fail_closed() && args.force == ForceClass::None {
        return Err(PitError::Unclassified(unclassified));
    }

    if args.dry_run {
        println!("Would stage public ({}):", public_paths.len());
        for p in &public_paths {
            println!("  {p}");
        }
        println!("Would stage private ({}):", private_paths.len());
        for p in &private_paths {
            println!("  {p}");
        }
        if !ignored_paths.is_empty() {
            println!("Ignored ({}): {}", ignored_paths.len(), ignored_paths.join(", "));
        }
        return Ok(());
    }

    // Ensure managed exclude is current so git add doesn't pick up private
    crate::exclude::update_managed_exclude(
        &ws.exclude_path(),
        &ws.policy.effective_private_patterns(),
    )?;

    // Snapshot for rollback
    let pub_index_backup = backup_index(&ws.public_git_dir)?;
    let priv_index_backup = if ws.private_git_dir.exists() {
        backup_index(&ws.private_git_dir).ok()
    } else {
        None
    };

    let stage_result = (|| -> Result<()> {
        if !public_paths.is_empty() {
            stage_paths(&ws, true, &public_paths)?;
        }
        if !private_paths.is_empty() {
            stage_paths(&ws, false, &private_paths)?;
        }
        Ok(())
    })();

    if let Err(e) = stage_result {
        // rollback indexes
        let _ = restore_index(&ws.public_git_dir, &pub_index_backup);
        if let Some(ref b) = priv_index_backup {
            let _ = restore_index(&ws.private_git_dir, b);
        }
        return Err(e);
    }

    if args.json {
        crate::json_out::print_ok(
            "add",
            serde_json::json!({
                "public": public_paths,
                "private": private_paths,
                "ignored": ignored_paths,
            }),
        );
    } else {
        println!(
            "Staged {} public, {} private path(s).",
            public_paths.len(),
            private_paths.len()
        );
        if !public_paths.is_empty() {
            println!("Public:");
            for p in &public_paths {
                println!("  {p}");
            }
        }
        if !private_paths.is_empty() {
            println!("Private:");
            for p in &private_paths {
                println!("  {p}");
            }
        }
        if !ignored_paths.is_empty() {
            println!("Skipped ignored: {}", ignored_paths.join(", "));
        }
    }
    Ok(())
}

/// Prompt user for each unclassified path. Returns classifications (skip → dropped).
pub fn prompt_classify(paths: &[String]) -> Result<Vec<(String, Class)>> {
    use std::io::{self, Write};
    let mut out = Vec::new();
    println!("Unclassified files:");
    for p in paths {
        println!("  {p}");
    }
    for p in paths {
        eprint!("Classify {p}: [p]rivate  [u]public  [i]gnore  [s]kip  ");
        let _ = io::stderr().flush();
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        match line.trim().to_ascii_lowercase().as_str() {
            "p" | "private" => out.push((p.clone(), Class::Private)),
            "u" | "public" => out.push((p.clone(), Class::Public)),
            "i" | "ignore" => out.push((p.clone(), Class::Ignored)),
            "s" | "skip" | "" => {}
            _ => {
                return Err(PitError::msg(format!(
                    "unrecognized classification for {p}; aborting"
                )));
            }
        }
    }
    Ok(out)
}

/// Whether interactive prompt mode should activate (unit-testable).
pub fn should_prompt_interactive(new_files_mode: &str, stdin_is_tty: bool) -> bool {
    new_files_mode == "prompt" && stdin_is_tty
}

#[cfg(test)]
mod prompt_tests {
    use super::*;

    #[test]
    fn prompt_mode_requires_tty() {
        assert!(should_prompt_interactive("prompt", true));
        assert!(!should_prompt_interactive("prompt", false));
        assert!(!should_prompt_interactive("reject", true));
    }
}

fn collect_candidates(ws: &Workspace, args: &AddArgs) -> Result<Vec<String>> {
    let mut paths = Vec::new();
    if args.all || args.paths.iter().any(|p| p == ".") {
        // All changes from public status + walk for private excluded files
        let status = git::status_porcelain(&ws.work_tree, Some(&ws.public_git_dir))?;
        for e in status {
            paths.push(e.path);
        }
        // Private-class files hidden by exclude
        let matcher = ws.policy.matcher()?;
        collect_private_files(&ws.work_tree, &matcher, &mut paths)?;
        // Also private status
        if ws.private_git_dir.join("HEAD").exists()
            || ws.private_git_dir.join("index").exists()
            || ws.private_git_dir.exists()
        {
            if let Ok(ps) = git::status_porcelain(&ws.work_tree, Some(&ws.private_git_dir)) {
                for e in ps {
                    paths.push(e.path);
                }
            }
        }
    } else {
        for p in &args.paths {
            let abs = if Path::new(p).is_absolute() {
                PathBuf::from(p)
            } else {
                ws.work_tree.join(p)
            };
            if abs.is_dir() {
                let matcher = ws.policy.matcher()?;
                collect_under(&abs, &ws.work_tree, &matcher, &mut paths)?;
            } else {
                let rel = abs
                    .strip_prefix(&ws.work_tree)
                    .unwrap_or(Path::new(p))
                    .to_string_lossy()
                    .replace('\\', "/");
                paths.push(rel);
            }
        }
    }
    paths.sort();
    paths.dedup();
    // filter out .git internals
    paths.retain(|p| !p.starts_with(".git/") && p != ".git");
    Ok(paths)
}

fn collect_private_files(
    work_tree: &Path,
    matcher: &PolicyMatcher,
    out: &mut Vec<String>,
) -> Result<()> {
    use walkdir::WalkDir;
    for entry in WalkDir::new(work_tree)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            n != ".git" && n != "target" && n != "node_modules"
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(work_tree)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if matcher.classify(&rel) == Class::Private {
            out.push(rel);
        }
    }
    Ok(())
}

fn collect_under(
    dir: &Path,
    work_tree: &Path,
    matcher: &PolicyMatcher,
    out: &mut Vec<String>,
) -> Result<()> {
    use walkdir::WalkDir;
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            n != ".git" && n != "target"
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(work_tree)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if matcher.classify(&rel) != Class::Ignored {
            out.push(rel);
        }
    }
    Ok(())
}

fn stage_paths(ws: &Workspace, public: bool, paths: &[String]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    // Use git add --pathspec-from-file with NUL or sequential add
    // Force add for private (-f) to bypass private repo's lack of exclude interest
    for path in paths {
        let abs = ws.work_tree.join(path);
        if !abs.exists() {
            // allow staging deletions
            if public {
                let _ = ws.public_git(&["rm", "--cached", "--ignore-unmatch", "-q", path]);
                // if modified/deleted in worktree
                let r = ws.public_git(&["add", "-A", "--", path]);
                if r.is_err() {
                    // try plain add
                    ws.public_git(&["add", "--", path])?;
                }
            } else {
                let r = ws.private_git(&["add", "-A", "--", path]);
                if r.is_err() {
                    ws.private_git(&["add", "--", path])?;
                }
            }
            continue;
        }
        if public {
            // Never hash private content into public — double-check
            if ws.policy.matcher()?.classify(path) == Class::Private {
                return Err(PitError::msg(format!(
                    "refusing to stage private path into public index: {path}"
                )));
            }
            ws.public_git(&["add", "--", path])?;
        } else {
            // private: force in case exclude patterns exist in private repo
            ws.private_git(&["add", "-f", "--", path])?;
        }
    }
    Ok(())
}

fn backup_index(git_dir: &Path) -> Result<Vec<u8>> {
    let index = git_dir.join("index");
    if index.exists() {
        Ok(std::fs::read(&index)?)
    } else {
        Ok(Vec::new())
    }
}

fn restore_index(git_dir: &Path, data: &[u8]) -> Result<()> {
    let index = git_dir.join("index");
    if data.is_empty() {
        let _ = std::fs::remove_file(&index);
    } else {
        std::fs::write(&index, data)?;
    }
    Ok(())
}
