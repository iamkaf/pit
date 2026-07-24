use crate::error::Result;
use crate::json_out;
use crate::workspace::Workspace;
use std::path::Path;

pub struct DiffArgs {
    pub paths: Vec<String>,
    pub private: bool,
    pub public: bool,
    pub staged: bool,
    pub json: bool,
}

pub fn run(cwd: &Path, args: DiffArgs) -> Result<()> {
    let ws = Workspace::discover(cwd)?;
    let show_public = args.public || !args.private;
    let show_private = args.private || !args.public;

    let mut public_diff = String::new();
    let mut private_diff = String::new();

    if show_public {
        public_diff = run_diff(&ws, true, args.staged, &args.paths)?;
    }
    if show_private {
        private_diff = run_diff(&ws, false, args.staged, &args.paths)?;
    }

    if args.json {
        json_out::print_ok(
            "diff",
            serde_json::json!({
                "public": public_diff,
                "private": private_diff,
            }),
        );
    } else {
        if show_public {
            println!("# public diff");
            if public_diff.is_empty() {
                println!("(no public changes)");
            } else {
                println!("{public_diff}");
            }
        }
        if show_private {
            println!("# private diff");
            if private_diff.is_empty() {
                println!("(no private changes)");
            } else {
                println!("{private_diff}");
            }
        }
    }
    Ok(())
}

fn run_diff(ws: &Workspace, public: bool, staged: bool, paths: &[String]) -> Result<String> {
    let mut args: Vec<String> = vec!["diff".into()];
    if staged {
        args.push("--cached".into());
    }
    if !paths.is_empty() {
        args.push("--".into());
        args.extend(paths.iter().cloned());
    }
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let r = if public {
        ws.public_git(&refs)
    } else {
        ws.private_git(&refs)
    };
    match r {
        Ok(s) => Ok(s),
        Err(crate::error::PitError::Git { .. }) => Ok(String::new()),
        Err(e) => Err(e),
    }
}
