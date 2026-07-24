use crate::error::Result;
use crate::exclude;
use crate::git;
use crate::json_out;
use crate::workspace::{self, Workspace};
use std::path::Path;

pub fn run(cwd: &Path, json: bool, repair: bool) -> Result<()> {
    let mut ws = match Workspace::discover(cwd) {
        Ok(ws) => ws,
        Err(e) => {
            if json {
                json_out::print_err("doctor", &format!("not a Pit workspace ({e})"));
            } else {
                println!("Pit doctor: not a Pit workspace ({e})");
            }
            std::process::exit(1);
        }
    };

    let mut repairs: Vec<String> = Vec::new();
    if repair {
        // Reversible local repairs only
        crate::exclude::update_managed_exclude(
            &ws.exclude_path(),
            &ws.policy.effective_private_patterns(),
        )?;
        repairs.push("managed_exclude refreshed".into());
        workspace::install_hooks(&ws.work_tree, &ws.public_git_dir, &ws.pit_dir)?;
        repairs.push("hooks repaired".into());
        if ws.state.branch_mapping_stale {
            let pub_b = ws.public_branch().unwrap_or_default();
            let priv_b = ws.private_branch().unwrap_or_default();
            if !pub_b.is_empty() && pub_b == priv_b {
                ws.state.branch_mapping_stale = false;
                ws.save_state()?;
                repairs.push("cleared stale mapping (branches already match)".into());
            } else {
                repairs.push(format!(
                    "branch_mapping_stale left set (public={pub_b} private={priv_b}); use pit switch"
                ));
            }
        }
        ws.config.hooks_installed = true;
        ws.save_config()?;
    }

    let mut checks: Vec<(String, String, String)> = Vec::new(); // name, status, detail

    // Git capability
    match git::git(&["--version"]) {
        Ok(v) => checks.push(("git", "ok", v).into_check()),
        Err(e) => checks.push(("git", "error", e.to_string()).into_check()),
    }

    // Public git dir
    if ws.public_git_dir.join("HEAD").exists() {
        checks.push(("public_git", "ok", ws.public_git_dir.display().to_string()).into_check());
    } else {
        checks.push(("public_git", "error", "HEAD missing".into()).into_check());
    }

    // Private git dir
    if ws.private_git_dir.join("HEAD").exists() {
        checks.push(("private_git", "ok", ws.private_git_dir.display().to_string()).into_check());
    } else {
        checks.push(("private_git", "error", "private git dir missing HEAD".into()).into_check());
    }

    // Config
    checks.push((
        "private_remote",
        if ws.config.private_remote.is_empty() {
            "error"
        } else {
            "ok"
        },
        redact_url(&ws.config.private_remote),
    ).into_check());

    checks.push((
        "private_visibility",
        if ws.config.private_visibility == "unverified" {
            "warn"
        } else {
            "ok"
        },
        ws.config.private_visibility.clone(),
    ).into_check());

    // Policy
    match ws.policy.matcher() {
        Ok(_) => checks.push(("policy", "ok", format!("version {}", ws.policy.version)).into_check()),
        Err(e) => checks.push(("policy", "error", e.to_string()).into_check()),
    }

    // Dual-tracked
    match ws.dual_tracked() {
        Ok(d) if d.is_empty() => checks.push(("dual_tracked", "ok", "none".into()).into_check()),
        Ok(d) => checks.push(("dual_tracked", "error", d.join(", ")).into_check()),
        Err(e) => checks.push(("dual_tracked", "error", e.to_string()).into_check()),
    }

    // Managed exclude
    let exclude_path = ws.exclude_path();
    if exclude_path.exists() {
        let text = std::fs::read_to_string(&exclude_path).unwrap_or_default();
        if exclude::has_managed_block(&text) {
            checks.push(("managed_exclude", "ok", exclude_path.display().to_string()).into_check());
        } else {
            checks.push(("managed_exclude", "warn", "block missing".into()).into_check());
        }
    } else {
        checks.push(("managed_exclude", "warn", "exclude file missing".into()).into_check());
    }

    // Hooks
    let pre_commit = ws.public_git_dir.join("hooks").join("pre-commit");
    if pre_commit.exists() {
        let text = std::fs::read_to_string(&pre_commit).unwrap_or_default();
        if text.contains("Pit hook dispatcher") {
            checks.push(("hooks_pre_commit", "ok", "pit dispatcher".into()).into_check());
        } else {
            checks.push(("hooks_pre_commit", "warn", "present but not pit dispatcher".into()).into_check());
        }
    } else {
        checks.push(("hooks_pre_commit", "warn", "not installed".into()).into_check());
    }

    let pre_push = ws.public_git_dir.join("hooks").join("pre-push");
    if pre_push.exists() {
        let text = std::fs::read_to_string(&pre_push).unwrap_or_default();
        if text.contains("Pit hook dispatcher") {
            checks.push(("hooks_pre_push", "ok", "pit dispatcher".into()).into_check());
        } else {
            checks.push(("hooks_pre_push", "warn", "present but not pit dispatcher".into()).into_check());
        }
    } else {
        checks.push(("hooks_pre_push", "warn", "not installed".into()).into_check());
    }

    // Pending transactions
    match ws.tx_store().list_pending() {
        Ok(p) if p.is_empty() => checks.push(("transactions", "ok", "none pending".into()).into_check()),
        Ok(p) => {
            let detail = p
                .iter()
                .map(|t| format!("{}({:?})", t.id, t.state))
                .collect::<Vec<_>>()
                .join(", ");
            checks.push(("transactions", "warn", detail).into_check());
        }
        Err(e) => checks.push(("transactions", "error", e.to_string()).into_check()),
    }

    // Private paths in public history (local)
    let mut history_hits = Vec::new();
    if let Ok(head) = git::rev_parse(&ws.work_tree, Some(&ws.public_git_dir), "HEAD") {
        if let Ok(paths) = git::all_paths_in_range(&ws.public_git_dir, None, &head) {
            let matcher = ws.policy.matcher().ok();
            if let Some(m) = matcher {
                for p in paths {
                    if m.is_private_pattern_match(&p) {
                        history_hits.push(p);
                    }
                }
            }
        }
    }
    if history_hits.is_empty() {
        checks.push(("public_history_private_paths", "ok", "none found".into()).into_check());
    } else {
        checks.push((
            "public_history_private_paths",
            "error",
            history_hits.join(", "),
        ).into_check());
    }

    // Policy not in public index
    let public_tracked = ws.public_tracked().unwrap_or_default();
    let policy_leaks: Vec<_> = public_tracked
        .iter()
        .filter(|p| {
            p.contains("policy.toml") && (p.contains(".git/pit") || p.starts_with(".pit/"))
                || *p == ".git/pit/config.toml"
        })
        .cloned()
        .collect();
    if policy_leaks.is_empty() {
        // also ensure .git/pit is not tracked
        let pit_tracked: Vec<_> = public_tracked.iter().filter(|p| p.starts_with(".git/pit")).cloned().collect();
        if pit_tracked.is_empty() {
            checks.push(("private_policy_public", "ok", "not in public index".into()).into_check());
        } else {
            checks.push(("private_policy_public", "error", pit_tracked.join(", ")).into_check());
        }
    } else {
        checks.push(("private_policy_public", "error", policy_leaks.join(", ")).into_check());
    }

    let errors = checks.iter().filter(|c| c.1 == "error").count();
    let warns = checks.iter().filter(|c| c.1 == "warn").count();
    let health = if errors > 0 {
        "ERROR"
    } else if warns > 0 {
        "WARN"
    } else {
        "OK"
    };

    if json {
        json_out::print_ok(
            "doctor",
            serde_json::json!({
                "health": health,
                "repairs": repairs,
                "checks": checks.iter().map(|(n,s,d)| serde_json::json!({
                    "name": n, "status": s, "detail": d
                })).collect::<Vec<_>>(),
            }),
        );
    } else {
        println!("Pit doctor — {health}");
        println!();
        for (name, status, detail) in &checks {
            let mark = match status.as_str() {
                "ok" => "ok  ",
                "warn" => "warn",
                _ => "ERR ",
            };
            println!("  [{mark}] {name}: {detail}");
        }
        if !repairs.is_empty() {
            println!();
            println!("Repairs applied:");
            for r in &repairs {
                println!("  - {r}");
            }
        }
        println!();
        if errors > 0 {
            println!("{errors} error(s), {warns} warning(s)");
        } else if warns > 0 {
            println!("0 errors, {warns} warning(s)");
        } else {
            println!("All checks passed.");
        }
    }

    if errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn redact_url(url: &str) -> String {
    // Avoid printing credentials; keep host/path shape
    if url.contains('@') && url.contains("://") {
        // https://user:pass@host -> https://***@host
        if let Some(idx) = url.find("://") {
            let scheme = &url[..idx + 3];
            let rest = &url[idx + 3..];
            if let Some(at) = rest.find('@') {
                return format!("{scheme}***@{}", &rest[at + 1..]);
            }
        }
    }
    url.to_string()
}

trait IntoCheck {
    fn into_check(self) -> (String, String, String);
}

impl IntoCheck for (&str, &str, String) {
    fn into_check(self) -> (String, String, String) {
        (self.0.to_string(), self.1.to_string(), self.2)
    }
}
