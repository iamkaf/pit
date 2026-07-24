use crate::error::Result;
use crate::git;
use crate::policy::{Class, PolicyMatcher};
use crate::workspace::Workspace;
use std::collections::HashSet;
use std::path::Path;

pub fn run(cwd: &Path, json: bool) -> Result<()> {
    let ws = Workspace::discover(cwd)?;
    let matcher = ws.policy.matcher()?;

    let public_branch = ws.public_branch().unwrap_or_else(|_| "(none)".into());
    let private_branch = ws.private_branch().unwrap_or_else(|_| "(none)".into());

    let dual = ws.dual_tracked()?;
    let pending = ws.tx_store().list_pending()?;

    // Collect working tree changes from both views
    let public_status = git::status_porcelain(&ws.work_tree, Some(&ws.public_git_dir))?;
    let private_status = if ws.private_git_dir.join("HEAD").exists() {
        git::status_porcelain(&ws.work_tree, Some(&ws.private_git_dir)).unwrap_or_default()
    } else {
        Vec::new()
    };

    let public_tracked: HashSet<String> = ws.public_tracked()?.into_iter().collect();
    let private_tracked: HashSet<String> = ws.private_tracked()?.into_iter().collect();

    let mut public_staged = Vec::new();
    let mut public_unstaged = Vec::new();
    let mut private_staged = Vec::new();
    let mut private_unstaged = Vec::new();
    let mut unclassified = Vec::new();
    let mut ignored_visible = Vec::new();

    // Walk public porcelain
    for e in &public_status {
        let path = &e.path;
        let class = classify_existing(&matcher, path, &public_tracked, &private_tracked);
        let staged = !e.xy.starts_with(' ') && !e.xy.starts_with('?');
        let unstaged = e.xy.chars().nth(1).map(|c| c != ' ').unwrap_or(false)
            || e.xy.starts_with('?');

        match class {
            Class::Private => {
                // private paths shouldn't appear staged in public
                if staged && !e.xy.starts_with('?') {
                    public_staged.push(format!("{path} (LEAK RISK)"));
                }
            }
            Class::Public => {
                if staged {
                    public_staged.push(path.clone());
                }
                if unstaged {
                    public_unstaged.push(path.clone());
                }
            }
            Class::Ignored => {
                if e.xy.starts_with('?') {
                    ignored_visible.push(path.clone());
                }
            }
            Class::Unclassified => {
                if e.xy.contains('?') || e.xy.starts_with('?') {
                    unclassified.push(path.clone());
                } else if staged {
                    public_staged.push(path.clone());
                } else {
                    public_unstaged.push(path.clone());
                }
            }
        }
    }

    // Private porcelain for private-tracked / private-class paths
    for e in &private_status {
        let path = &e.path;
        let class = classify_existing(&matcher, path, &public_tracked, &private_tracked);
        if class != Class::Private && !private_tracked.contains(path) {
            continue;
        }
        let staged = !e.xy.starts_with(' ') && !e.xy.starts_with('?');
        let unstaged = e.xy.chars().nth(1).map(|c| c != ' ').unwrap_or(false)
            || e.xy.starts_with('?');
        if staged {
            private_staged.push(path.clone());
        }
        if unstaged {
            private_unstaged.push(path.clone());
        }
    }

    // Also surface untracked private-class files that public git may exclude
    scan_untracked_private(&ws, &matcher, &public_tracked, &private_tracked, &mut private_unstaged, &mut unclassified)?;

    public_staged.sort();
    public_staged.dedup();
    public_unstaged.sort();
    public_unstaged.dedup();
    private_staged.sort();
    private_staged.dedup();
    private_unstaged.sort();
    private_unstaged.dedup();
    unclassified.sort();
    unclassified.dedup();

    let health = if !dual.is_empty() {
        "ERROR (dual-tracked paths)"
    } else if !unclassified.is_empty() {
        "WARN (unclassified paths)"
    } else if pending.iter().any(|t| t.needs_resume()) {
        "WARN (pending transaction)"
    } else {
        "OK"
    };

    if json {
        crate::json_out::print_ok(
            "status",
            serde_json::json!({
                "public_branch": public_branch,
                "private_branch": private_branch,
                "health": health,
                "public_staged": public_staged,
                "public_unstaged": public_unstaged,
                "private_staged": private_staged,
                "private_unstaged": private_unstaged,
                "unclassified": unclassified,
                "dual_tracked": dual,
                "branch_mapping_stale": ws.state.branch_mapping_stale,
                "transactions": pending.iter().map(|t| serde_json::json!({
                    "id": t.id,
                    "state": format!("{:?}", t.state),
                })).collect::<Vec<_>>(),
            }),
        );
        return Ok(());
    }

    println!("On branch {public_branch}");
    println!("Public:  {}/{public_branch}", ws.config.public_remote_name);
    println!("Private: {}/{private_branch}", ws.config.private_remote_name);
    println!("Health:  {health}");
    println!();
    println!("Public changes:");
    print_list("  staged:    ", &public_staged);
    print_list("  unstaged:  ", &public_unstaged);
    println!();
    println!("Private changes:");
    print_list("  staged:    ", &private_staged);
    print_list("  unstaged:  ", &private_unstaged);
    println!();
    if !unclassified.is_empty() {
        println!("Unclassified:");
        for p in &unclassified {
            println!("  {p}");
        }
        println!();
    }
    if !dual.is_empty() {
        println!("Dual-tracked (must repair):");
        for p in &dual {
            println!("  {p}");
        }
        println!();
    }
    println!("Transactions:");
    if pending.is_empty() {
        println!("  none pending");
    } else {
        for t in &pending {
            println!("  {} state={:?}", t.id, t.state);
            if t.needs_resume() {
                println!("    run: pit push --resume");
            }
        }
    }
    Ok(())
}

fn print_list(label: &str, items: &[String]) {
    if items.is_empty() {
        println!("{label}(none)");
    } else {
        println!("{label}{}", items.join(", "));
    }
}

fn classify_existing(
    matcher: &PolicyMatcher,
    path: &str,
    public_tracked: &HashSet<String>,
    private_tracked: &HashSet<String>,
) -> Class {
    if private_tracked.contains(path) {
        return Class::Private;
    }
    if public_tracked.contains(path) {
        return Class::Public;
    }
    matcher.classify(path)
}

fn scan_untracked_private(
    ws: &Workspace,
    matcher: &PolicyMatcher,
    public_tracked: &HashSet<String>,
    private_tracked: &HashSet<String>,
    private_unstaged: &mut Vec<String>,
    unclassified: &mut Vec<String>,
) -> Result<()> {
    use walkdir::WalkDir;
    for entry in WalkDir::new(&ws.work_tree)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            name != ".git" && name != "target" && name != "node_modules"
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = match entry.path().strip_prefix(&ws.work_tree) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };
        if public_tracked.contains(&rel) || private_tracked.contains(&rel) {
            continue;
        }
        // skip if already known staged in private
        match matcher.classify(&rel) {
            Class::Private => {
                if !private_unstaged.contains(&rel) {
                    // check if private index already has it staged
                    private_unstaged.push(rel);
                }
            }
            Class::Unclassified => {
                if !unclassified.contains(&rel) {
                    unclassified.push(rel);
                }
            }
            _ => {}
        }
    }
    Ok(())
}
