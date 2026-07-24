use serde::Serialize;
use serde_json::{json, Value};

pub const SCHEMA_VERSION: u32 = 1;

/// Stable versioned JSON envelope for all commands.
pub fn envelope(command: &str, ok: bool, data: Value) -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "command": command,
        "ok": ok,
        "data": data,
    })
}

pub fn print_ok(command: &str, data: impl Serialize) {
    let v = envelope(command, true, serde_json::to_value(data).unwrap_or(json!({})));
    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".into()));
}

pub fn print_err(command: &str, err: &str) {
    let v = envelope(command, false, json!({ "error": err }));
    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".into()));
}
