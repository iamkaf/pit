use crate::error::{PitError, Result};
use crate::json_out;
use crate::workspace::{self, Workspace};
use std::path::Path;

pub enum HooksAction {
    Install,
    Status,
    Repair,
    Uninstall,
}

pub fn run(cwd: &Path, action: HooksAction, json: bool) -> Result<()> {
    let ws = Workspace::discover(cwd)?;
    match action {
        HooksAction::Install | HooksAction::Repair => {
            workspace::install_hooks(&ws.work_tree, &ws.public_git_dir, &ws.pit_dir)?;
            let mut cfg = ws.config.clone();
            cfg.hooks_installed = true;
            let mut ws = ws;
            ws.config = cfg;
            ws.save_config()?;
            // also refresh exclude
            crate::exclude::update_managed_exclude(
                &ws.exclude_path(),
                &ws.policy.effective_private_patterns(),
            )?;
            let status = workspace::hooks_status(&ws.public_git_dir);
            if json {
                json_out::print_ok(
                    "hooks",
                    serde_json::json!({
                        "action": if matches!(action, HooksAction::Install) { "install" } else { "repair" },
                        "hooks": status.iter().map(|(n,s)| serde_json::json!({"name": n, "status": s})).collect::<Vec<_>>(),
                    }),
                );
            } else {
                println!("Hooks installed/repaired:");
                for (n, s) in status {
                    println!("  {n}: {s}");
                }
            }
        }
        HooksAction::Status => {
            let status = workspace::hooks_status(&ws.public_git_dir);
            if json {
                json_out::print_ok(
                    "hooks",
                    serde_json::json!({
                        "action": "status",
                        "hooks": status.iter().map(|(n,s)| serde_json::json!({"name": n, "status": s})).collect::<Vec<_>>(),
                    }),
                );
            } else {
                println!("Hook status:");
                for (n, s) in status {
                    println!("  {n}: {s}");
                }
            }
        }
        HooksAction::Uninstall => {
            workspace::uninstall_hooks(&ws.public_git_dir)?;
            let mut ws = ws;
            ws.config.hooks_installed = false;
            ws.save_config()?;
            if json {
                json_out::print_ok(
                    "hooks",
                    serde_json::json!({ "action": "uninstall", "ok": true }),
                );
            } else {
                println!("Pit hooks uninstalled (user hooks restored where present).");
            }
        }
    }
    Ok(())
}

pub fn parse_action(s: &str) -> Result<HooksAction> {
    match s {
        "install" => Ok(HooksAction::Install),
        "status" => Ok(HooksAction::Status),
        "repair" => Ok(HooksAction::Repair),
        "uninstall" => Ok(HooksAction::Uninstall),
        _ => Err(PitError::msg(
            "usage: pit hooks install|status|repair|uninstall",
        )),
    }
}
