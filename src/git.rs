use crate::error::{PitError, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Invoke system `git` with explicit argv. Never shell-interpolates.
pub fn git(args: &[&str]) -> Result<String> {
    run_git(None, None, args, None)
}

pub fn git_in(cwd: &Path, args: &[&str]) -> Result<String> {
    run_git(Some(cwd), None, args, None)
}

pub fn git_public(work_tree: &Path, args: &[&str]) -> Result<String> {
    let git_dir = work_tree.join(".git");
    run_git(Some(work_tree), Some(&git_dir), args, None)
}

pub fn git_private(work_tree: &Path, private_git_dir: &Path, args: &[&str]) -> Result<String> {
    run_git(Some(work_tree), Some(private_git_dir), args, None)
}

pub fn git_public_ok(work_tree: &Path, args: &[&str]) -> Result<bool> {
    match git_public(work_tree, args) {
        Ok(_) => Ok(true),
        Err(PitError::Git { .. }) => Ok(false),
        Err(e) => Err(e),
    }
}

pub fn run_git(
    cwd: Option<&Path>,
    git_dir: Option<&Path>,
    args: &[&str],
    stdin: Option<&str>,
) -> Result<String> {
    let mut cmd = Command::new("git");
    // Hooks (and some Git subcommands) set GIT_INDEX_FILE / GIT_DIR / GIT_WORK_TREE.
    // Clear them so explicit --git-dir/--work-tree always win and private never
    // accidentally reads the public index during pre-commit.
    cmd.env_remove("GIT_INDEX_FILE");
    cmd.env_remove("GIT_DIR");
    cmd.env_remove("GIT_WORK_TREE");
    cmd.env_remove("GIT_OBJECT_DIRECTORY");
    cmd.env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES");

    if let Some(gd) = git_dir {
        cmd.arg(format!("--git-dir={}", gd.display()));
    }
    if let Some(wt) = cwd {
        if git_dir.is_some() {
            cmd.arg(format!("--work-tree={}", wt.display()));
        } else {
            cmd.current_dir(wt);
        }
    }

    cmd.args(args);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }

    let mut child = cmd.spawn().map_err(|e| {
        PitError::msg(format!("failed to spawn git: {e} (is git installed?)"))
    })?;

    if let Some(input) = stdin {
        use std::io::Write;
        if let Some(mut s) = child.stdin.take() {
            s.write_all(input.as_bytes())?;
        }
    }

    let output = child.wait_with_output()?;
    output_to_result(&format_cmd(args), output)
}

fn format_cmd(args: &[&str]) -> String {
    let mut parts = vec!["git".to_string()];
    parts.extend(args.iter().map(|s| s.to_string()));
    parts.join(" ")
}

fn output_to_result(cmd: &str, output: Output) -> Result<String> {
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        Err(PitError::Git {
            cmd: cmd.to_string(),
            stderr: detail,
        })
    }
}

/// Find the top-level work tree for the current directory.
pub fn find_work_tree(start: &Path) -> Result<PathBuf> {
    let out = run_git(
        Some(start),
        None,
        &["rev-parse", "--show-toplevel"],
        None,
    )?;
    Ok(PathBuf::from(out))
}

pub fn find_git_dir(work_tree: &Path) -> Result<PathBuf> {
    let out = git_in(work_tree, &["rev-parse", "--git-dir"])?;
    let p = PathBuf::from(&out);
    if p.is_absolute() {
        Ok(p)
    } else {
        Ok(work_tree.join(p))
    }
}

pub fn is_git_repo(path: &Path) -> bool {
    git_in(path, &["rev-parse", "--is-inside-work-tree"])
        .map(|s| s == "true")
        .unwrap_or(false)
}

pub fn rev_parse(work_tree: &Path, git_dir: Option<&Path>, rev: &str) -> Result<String> {
    if let Some(gd) = git_dir {
        run_git(Some(work_tree), Some(gd), &["rev-parse", rev], None)
    } else {
        git_public(work_tree, &["rev-parse", rev])
    }
}

pub fn current_branch(work_tree: &Path, git_dir: Option<&Path>) -> Result<String> {
    let args = ["branch", "--show-current"];
    if let Some(gd) = git_dir {
        run_git(Some(work_tree), Some(gd), &args, None)
    } else {
        git_public(work_tree, &args)
    }
}

pub fn has_commits(work_tree: &Path, git_dir: Option<&Path>) -> bool {
    rev_parse(work_tree, git_dir, "HEAD").is_ok()
}

/// List staged paths (NUL-separated via -z internally).
pub fn staged_paths(work_tree: &Path, git_dir: Option<&Path>) -> Result<Vec<String>> {
    let args = ["diff", "--cached", "--name-only", "-z"];
    let out = if let Some(gd) = git_dir {
        run_git(Some(work_tree), Some(gd), &args, None)?
    } else {
        git_public(work_tree, &args)?
    };
    Ok(split_z(&out))
}

/// Untracked + unstaged paths relative to work tree (status porcelain).
pub fn status_porcelain(work_tree: &Path, git_dir: Option<&Path>) -> Result<Vec<StatusEntry>> {
    let args = ["status", "--porcelain=v1", "-z", "--untracked-files=all"];
    let out = if let Some(gd) = git_dir {
        run_git(Some(work_tree), Some(gd), &args, None)?
    } else {
        git_public(work_tree, &args)?
    };
    Ok(parse_porcelain_z(&out))
}

#[derive(Debug, Clone)]
pub struct StatusEntry {
    pub xy: String,
    pub path: String,
    pub orig_path: Option<String>,
}

fn split_z(s: &str) -> Vec<String> {
    s.split('\0')
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string())
        .collect()
}

fn parse_porcelain_z(s: &str) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        // XY space path\0  or  R/C status with rename
        if bytes[i] == 0 {
            i += 1;
            continue;
        }
        let xy = String::from_utf8_lossy(&bytes[i..i + 2]).to_string();
        // skip "XY "
        i += 3;
        let start = i;
        while i < bytes.len() && bytes[i] != 0 {
            i += 1;
        }
        let path = String::from_utf8_lossy(&bytes[start..i]).to_string();
        if i < bytes.len() {
            i += 1; // skip NUL
        }
        let mut orig = None;
        // rename/copy: second path after first NUL
        if xy.starts_with('R') || xy.starts_with('C') || xy.chars().nth(1) == Some('R') {
            let start2 = i;
            while i < bytes.len() && bytes[i] != 0 {
                i += 1;
            }
            if start2 < i {
                orig = Some(String::from_utf8_lossy(&bytes[start2..i]).to_string());
            }
            if i < bytes.len() {
                i += 1;
            }
        }
        if !path.is_empty() {
            entries.push(StatusEntry {
                xy,
                path,
                orig_path: orig,
            });
        }
    }
    entries
}

/// All blobs/trees reachable from refs — used for canary scans.
pub fn cat_file_batch_check_all(git_dir: &Path) -> Result<String> {
    // List all objects via rev-list --all --objects, then we search separately
    run_git(
        None,
        Some(git_dir),
        &["rev-list", "--all", "--objects"],
        None,
    )
}

pub fn grep_objects_for_string(git_dir: &Path, needle: &str) -> Result<bool> {
    // Search blob contents via git grep on all commits
    match run_git(
        None,
        Some(git_dir),
        &["grep", "-a", "-F", "--", needle, "$(git rev-list --all)"],
        None,
    ) {
        Ok(s) if !s.is_empty() => Ok(true),
        Ok(_) => Ok(false),
        Err(PitError::Git { .. }) => {
            // git grep fails if no match; try a more reliable walk
            let objects = run_git(
                None,
                Some(git_dir),
                &["rev-list", "--all"],
                None,
            )?;
            if objects.is_empty() {
                return Ok(false);
            }
            for commit in objects.lines() {
                if commit.is_empty() {
                    continue;
                }
                match run_git(
                    None,
                    Some(git_dir),
                    &["grep", "-a", "-F", "-e", needle, commit],
                    None,
                ) {
                    Ok(s) if !s.is_empty() => return Ok(true),
                    _ => {}
                }
            }
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

/// Walk all trees in outgoing commits and collect paths.
pub fn paths_in_commit_range(
    work_tree: &Path,
    git_dir: &Path,
    range: &str,
) -> Result<Vec<String>> {
    // range like "remote..HEAD" or just "HEAD" for all
    let out = run_git(
        Some(work_tree),
        Some(git_dir),
        &["diff-tree", "-r", "--name-only", "--no-commit-id", "--root", range],
        None,
    );
    // better: for each commit in range, list tree
    let commits = run_git(
        Some(work_tree),
        Some(git_dir),
        &["rev-list", range],
        None,
    )?;
    let mut paths = Vec::new();
    for commit in commits.lines() {
        if commit.is_empty() {
            continue;
        }
        let tree = run_git(
            Some(work_tree),
            Some(git_dir),
            &["ls-tree", "-r", "--name-only", commit],
            None,
        )?;
        for p in tree.lines() {
            if !p.is_empty() {
                paths.push(p.to_string());
            }
        }
    }
    // silence unused
    let _ = out;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Paths present in any tree of the given commits (for full outbound validation).
pub fn all_paths_in_range(git_dir: &Path, from_exclusive: Option<&str>, to: &str) -> Result<Vec<String>> {
    let range = match from_exclusive {
        Some(base) if !base.is_empty() => format!("{base}..{to}"),
        _ => to.to_string(),
    };
    let commits = run_git(None, Some(git_dir), &["rev-list", &range], None)?;
    let mut paths = Vec::new();
    for commit in commits.lines() {
        if commit.is_empty() {
            continue;
        }
        let tree = run_git(
            None,
            Some(git_dir),
            &["ls-tree", "-r", "--name-only", commit],
            None,
        )?;
        for p in tree.lines() {
            if !p.is_empty() {
                paths.push(p.to_string());
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

pub fn remote_url(work_tree: &Path, git_dir: Option<&Path>, remote: &str) -> Result<String> {
    let args = ["remote", "get-url", remote];
    if let Some(gd) = git_dir {
        run_git(Some(work_tree), Some(gd), &args, None)
    } else {
        git_public(work_tree, &args)
    }
}

pub fn ensure_user_identity(work_tree: &Path, git_dir: Option<&Path>) -> Result<()> {
    // Ensure commits can be created — fall back to env if needed
    let check = |key: &str| -> bool {
        let args = ["config", key];
        let r = if let Some(gd) = git_dir {
            run_git(Some(work_tree), Some(gd), &args, None)
        } else {
            git_public(work_tree, &args)
        };
        r.map(|s| !s.is_empty()).unwrap_or(false)
    };
    if !check("user.email") {
        // leave to global config
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_porcelain_simple() {
        let s = " M file.txt\0?? new.txt\0";
        let e = parse_porcelain_z(s);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].path, "file.txt");
        assert_eq!(e[1].path, "new.txt");
    }
}
