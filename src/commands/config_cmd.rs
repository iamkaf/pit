use crate::error::{PitError, Result};
use crate::json_out;
use crate::workspace::Workspace;
use std::path::Path;

pub enum ConfigAction {
    Get { key: String },
    Set { key: String, value: String },
    List,
}

pub fn run(cwd: &Path, action: ConfigAction, json: bool) -> Result<()> {
    let mut ws = Workspace::discover(cwd)?;
    match action {
        ConfigAction::List => {
            let map = config_map(&ws);
            if json {
                json_out::print_ok("config", serde_json::json!({ "values": map }));
            } else {
                for (k, v) in &map {
                    println!("{k}={v}");
                }
            }
        }
        ConfigAction::Get { key } => {
            let map = config_map(&ws);
            let val = map
                .get(&key)
                .cloned()
                .ok_or_else(|| PitError::msg(format!("unknown config key: {key}")))?;
            if json {
                json_out::print_ok("config", serde_json::json!({ "key": key, "value": val }));
            } else {
                println!("{val}");
            }
        }
        ConfigAction::Set { key, value } => {
            set_key(&mut ws, &key, &value)?;
            ws.save_config()?;
            if key.starts_with("policy.") {
                ws.save_policy()?;
                crate::exclude::update_managed_exclude(
                    &ws.exclude_path(),
                    &ws.policy.effective_private_patterns(),
                )?;
            }
            if json {
                json_out::print_ok(
                    "config",
                    serde_json::json!({ "key": key, "value": value, "set": true }),
                );
            } else {
                println!("set {key}={value}");
            }
        }
    }
    Ok(())
}

fn config_map(ws: &Workspace) -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    m.insert(
        "private.remote".into(),
        ws.config.private_remote.clone(),
    );
    m.insert(
        "private.remote_name".into(),
        ws.config.private_remote_name.clone(),
    );
    m.insert(
        "public.remote_name".into(),
        ws.config.public_remote_name.clone(),
    );
    m.insert(
        "private.visibility".into(),
        ws.config.private_visibility.clone(),
    );
    m.insert(
        "hooks.installed".into(),
        ws.config.hooks_installed.to_string(),
    );
    m.insert(
        "policy.new_files".into(),
        ws.policy.classification.new_files.clone(),
    );
    m.insert(
        "state.branch_mapping_stale".into(),
        ws.state.branch_mapping_stale.to_string(),
    );
    m
}

fn set_key(ws: &mut Workspace, key: &str, value: &str) -> Result<()> {
    match key {
        "private.remote" => ws.config.private_remote = value.to_string(),
        "private.remote_name" => ws.config.private_remote_name = value.to_string(),
        "public.remote_name" => ws.config.public_remote_name = value.to_string(),
        "private.visibility" => {
            if !matches!(
                value,
                "verified-private" | "user-attested-private" | "unverified"
            ) {
                return Err(PitError::msg(
                    "private.visibility must be verified-private|user-attested-private|unverified",
                ));
            }
            ws.config.private_visibility = value.to_string();
        }
        "policy.new_files" => {
            if !matches!(value, "prompt" | "public" | "private" | "reject") {
                return Err(PitError::msg(
                    "policy.new_files must be prompt|public|private|reject",
                ));
            }
            ws.policy.classification.new_files = value.to_string();
        }
        "state.branch_mapping_stale" => {
            ws.state.branch_mapping_stale = matches!(value, "true" | "1" | "yes");
            ws.save_state()?;
        }
        _ => {
            return Err(PitError::msg(format!(
                "unknown or read-only config key: {key}"
            )));
        }
    }
    Ok(())
}
