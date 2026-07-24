use crate::error::{PitError, Result};
use crate::json_out;
use crate::transaction::TxState;
use crate::workspace::Workspace;
use std::path::Path;
use uuid::Uuid;

pub enum TxAction {
    List,
    Show { id: String },
    Resume { id: Option<String> },
    Abort { id: String },
}

pub fn run(cwd: &Path, action: TxAction, json: bool) -> Result<()> {
    let ws = Workspace::discover(cwd)?;
    let store = ws.tx_store();
    match action {
        TxAction::List => {
            let all = store.list_all()?;
            if json {
                json_out::print_ok(
                    "transaction",
                    serde_json::json!({
                        "transactions": all.iter().map(tx_json).collect::<Vec<_>>(),
                    }),
                );
            } else if all.is_empty() {
                println!("No transactions recorded.");
            } else {
                for t in &all {
                    println!(
                        "{}  {:?}  private_push={} public_push={}  {}",
                        t.id,
                        t.state,
                        t.private_push_ok,
                        t.public_push_ok,
                        t.message
                    );
                }
            }
        }
        TxAction::Show { id } => {
            let id = Uuid::parse_str(&id)
                .map_err(|e| PitError::msg(format!("invalid transaction id: {e}")))?;
            let t = store.load(id)?;
            if json {
                json_out::print_ok("transaction", tx_json(&t));
            } else {
                println!("id: {}", t.id);
                println!("state: {:?}", t.state);
                println!("message: {}", t.message);
                println!("public:  {} -> {:?}", t.public_branch, t.public_after);
                println!("private: {} -> {:?}", t.private_branch, t.private_after);
                println!(
                    "push: private_ok={} public_ok={}",
                    t.private_push_ok, t.public_push_ok
                );
                if let Some(h) = &t.recovery_hint {
                    println!("recovery: {h}");
                }
                if let Some(e) = &t.last_error {
                    println!("last_error: {e}");
                }
            }
        }
        TxAction::Resume { id } => {
            if let Some(id) = id {
                let id = Uuid::parse_str(&id)
                    .map_err(|e| PitError::msg(format!("invalid transaction id: {e}")))?;
                let t = store.load(id)?;
                // point CURRENT at this tx if resumable
                if !t.needs_resume() && t.state != TxState::LocalComplete {
                    return Err(PitError::msg(format!(
                        "transaction {id} is not resumable (state={:?})",
                        t.state
                    )));
                }
                store.save(&t)?; // refreshes CURRENT
            }
            crate::commands::push::run(
                cwd,
                crate::commands::push::PushArgs {
                    resume: true,
                    dry_run: false,
                    json,
                },
            )?;
            if json {
                json_out::print_ok("transaction", serde_json::json!({ "resumed": true }));
            }
        }
        TxAction::Abort { id } => {
            let id = Uuid::parse_str(&id)
                .map_err(|e| PitError::msg(format!("invalid transaction id: {e}")))?;
            let t = store.abort(id)?;
            if json {
                json_out::print_ok(
                    "transaction",
                    serde_json::json!({ "aborted": id, "state": format!("{:?}", t.state) }),
                );
            } else {
                println!("Aborted transaction {id}");
            }
        }
    }
    Ok(())
}

fn tx_json(t: &crate::transaction::Transaction) -> serde_json::Value {
    serde_json::json!({
        "id": t.id,
        "state": format!("{:?}", t.state),
        "message": t.message,
        "public_branch": t.public_branch,
        "private_branch": t.private_branch,
        "public_after": t.public_after,
        "private_after": t.private_after,
        "private_push_ok": t.private_push_ok,
        "public_push_ok": t.public_push_ok,
        "recovery_hint": t.recovery_hint,
        "last_error": t.last_error,
    })
}
