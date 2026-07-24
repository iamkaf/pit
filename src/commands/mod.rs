pub mod add;
pub mod commit;
pub mod doctor;
pub mod hook;
pub mod push;
pub mod setup;
pub mod status;

use crate::error::Result;
use crate::workspace::Workspace;
use std::path::Path;

pub fn require_workspace(cwd: &Path) -> Result<Workspace> {
    Workspace::discover(cwd)
}
