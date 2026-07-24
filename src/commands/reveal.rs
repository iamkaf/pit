use crate::error::{PitError, Result};
use crate::json_out;
use crate::workspace::Workspace;
use std::io::{self, Write};
use std::path::Path;

pub struct RevealArgs {
    pub path: String,
    pub yes: bool,
    pub json: bool,
}

pub fn run(cwd: &Path, args: RevealArgs) -> Result<()> {
    let mut ws = Workspace::discover(cwd)?;
    let path = args.path.trim_start_matches("./").replace('\\', "/");

    if !args.yes {
        if atty_stdin() {
            eprint!("Reveal `{path}` to the public repository? [y/N] ");
            let _ = io::stderr().flush();
            let mut line = String::new();
            io::stdin().read_line(&mut line)?;
            if !matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
                return Err(PitError::msg("reveal cancelled"));
            }
        } else {
            return Err(PitError::msg(
                "reveal requires --yes in non-interactive mode",
            ));
        }
    }

    // Remove from private index
    let _ = ws.private_git(&["rm", "--cached", "--ignore-unmatch", "-q", "--", &path]);

    // Drop exact path from private policy patterns
    ws.policy.private.patterns.retain(|p| p != &path);
    ws.save_policy()?;
    crate::exclude::update_managed_exclude(
        &ws.exclude_path(),
        &ws.policy.effective_private_patterns(),
    )?;

    // Stage into public index
    if ws.work_tree.join(&path).exists() {
        // must not be private-pattern anymore
        ws.public_git(&["add", "--", &path])?;
    }

    if args.json {
        json_out::print_ok(
            "reveal",
            serde_json::json!({
                "path": path,
                "staged_public": true,
                "confirmed": true,
            }),
        );
    } else {
        println!("Revealed `{path}` (staged public).");
        println!("Run: pit commit -m \"Reveal {path}\" && pit push");
    }
    Ok(())
}

fn atty_stdin() -> bool {
    std::io::IsTerminal::is_terminal(&io::stdin())
}
