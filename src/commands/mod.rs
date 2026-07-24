pub mod add;
pub mod clone_cmd;
pub mod commit;
pub mod config_cmd;
pub mod diff_cmd;
pub mod doctor;
pub mod hook;
pub mod hooks_cmd;
pub mod ignore_cmd;
pub mod protect;
pub mod pull_cmd;
pub mod push;
pub mod restore;
pub mod reveal;
pub mod setup;
pub mod status;
pub mod switch_cmd;
pub mod tx_cmd;

use crate::error::Result;
use crate::workspace::Workspace;
use std::path::Path;

pub fn require_workspace(cwd: &Path) -> Result<Workspace> {
    Workspace::discover(cwd)
}
