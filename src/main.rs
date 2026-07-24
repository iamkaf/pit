use clap::{Parser, Subcommand};
use pit::commands;
use pit::error::PitError;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "pit", version, about = "One working tree, two repositories, one safe workflow.")]
struct Cli {
    #[arg(long, global = true)]
    json: bool,

    #[arg(long, global = true)]
    verbose: bool,

    #[arg(long, global = true)]
    quiet: bool,

    #[arg(long, global = true)]
    dry_run: bool,

    #[arg(long, global = true)]
    yes: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Connect or create the private mirror, load policy, install hooks
    Setup {
        #[arg(long)]
        private: Option<String>,
        #[arg(long)]
        create_github: bool,
    },
    /// Clone public repo and optionally set up private companion
    Clone {
        public_url: String,
        #[arg(long)]
        private: Option<String>,
        #[arg(long)]
        directory: Option<String>,
        #[arg(long)]
        no_setup: bool,
    },
    /// Show public, private, unclassified, and transaction state
    Status,
    /// Classify and stage paths into the correct index
    Add {
        paths: Vec<String>,
        #[arg(short = 'A', long = "all")]
        all: bool,
        #[arg(long)]
        private: bool,
        #[arg(long)]
        public: bool,
        #[arg(long)]
        ignore: bool,
    },
    /// Unstage paths from the correct index
    Restore {
        paths: Vec<String>,
        #[arg(long)]
        staged: bool,
    },
    /// Show public and/or private diffs
    Diff {
        paths: Vec<String>,
        #[arg(long)]
        private: bool,
        #[arg(long)]
        public: bool,
        #[arg(long)]
        staged: bool,
    },
    /// Create a logical transaction (public and/or private commits)
    Commit {
        #[arg(short, long)]
        message: Option<String>,
    },
    /// Push private first, then public, with outbound validation
    Push {
        #[arg(long)]
        resume: bool,
    },
    /// Fetch and update both repositories
    Pull,
    /// Switch or create mapped public/private branches
    Switch {
        branch: String,
        #[arg(short = 'c', long = "create")]
        create: bool,
    },
    /// Move a path from public tracking to private
    Protect {
        path: String,
    },
    /// Move a path from private tracking to public
    Reveal {
        path: String,
    },
    /// Stop tracking a path in either repository
    Ignore {
        path: String,
    },
    /// Validate workspace health and privacy invariants
    Doctor {
        #[arg(long)]
        repair: bool,
    },
    /// Manage hook integration
    Hooks {
        action: String,
    },
    /// Inspect and recover logical transactions
    Transaction {
        action: String,
        id: Option<String>,
    },
    /// Manage local configuration
    Config {
        action: String,
        key: Option<String>,
        value: Option<String>,
    },
    /// Internal: hook dispatcher entry
    #[command(hide = true)]
    Hook {
        name: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        _rest: Vec<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let result = match cli.command {
        Commands::Setup {
            private,
            create_github,
        } => commands::setup::run(
            &cwd,
            commands::setup::SetupArgs {
                private,
                create_github,
                yes: cli.yes,
                visibility_attested: cli.yes,
                json: cli.json,
            },
        ),
        Commands::Clone {
            public_url,
            private,
            directory,
            no_setup,
        } => commands::clone_cmd::run(
            &cwd,
            commands::clone_cmd::CloneArgs {
                public_url,
                private_url: private,
                directory,
                no_setup,
                yes: cli.yes,
                json: cli.json,
            },
        ),
        Commands::Status => commands::status::run(&cwd, cli.json),
        Commands::Add {
            paths,
            all,
            private,
            public,
            ignore,
        } => {
            let force = if private {
                commands::add::ForceClass::Private
            } else if public {
                commands::add::ForceClass::Public
            } else if ignore {
                commands::add::ForceClass::Ignore
            } else {
                commands::add::ForceClass::None
            };
            let paths = if paths.is_empty() && all {
                vec![".".into()]
            } else {
                paths
            };
            commands::add::run(
                &cwd,
                commands::add::AddArgs {
                    paths,
                    all,
                    force,
                    dry_run: cli.dry_run,
                    json: cli.json,
                },
            )
        }
        Commands::Restore { paths, staged } => commands::restore::run(
            &cwd,
            commands::restore::RestoreArgs {
                paths,
                staged,
                json: cli.json,
            },
        ),
        Commands::Diff {
            paths,
            private,
            public,
            staged,
        } => commands::diff_cmd::run(
            &cwd,
            commands::diff_cmd::DiffArgs {
                paths,
                private,
                public,
                staged,
                json: cli.json,
            },
        ),
        Commands::Commit { message } => commands::commit::run(
            &cwd,
            commands::commit::CommitArgs {
                message,
                dry_run: cli.dry_run,
                json: cli.json,
            },
        ),
        Commands::Push { resume } => commands::push::run(
            &cwd,
            commands::push::PushArgs {
                resume,
                dry_run: cli.dry_run,
                json: cli.json,
            },
        ),
        Commands::Pull => commands::pull_cmd::run(
            &cwd,
            commands::pull_cmd::PullArgs {
                yes: cli.yes,
                json: cli.json,
            },
        ),
        Commands::Switch { branch, create } => commands::switch_cmd::run(
            &cwd,
            commands::switch_cmd::SwitchArgs {
                branch,
                create,
                json: cli.json,
            },
        ),
        Commands::Protect { path } => commands::protect::run(
            &cwd,
            commands::protect::ProtectArgs {
                path,
                yes: cli.yes,
                json: cli.json,
            },
        ),
        Commands::Reveal { path } => commands::reveal::run(
            &cwd,
            commands::reveal::RevealArgs {
                path,
                yes: cli.yes,
                json: cli.json,
            },
        ),
        Commands::Ignore { path } => commands::ignore_cmd::run(
            &cwd,
            commands::ignore_cmd::IgnoreArgs {
                path,
                json: cli.json,
            },
        ),
        Commands::Doctor { repair } => commands::doctor::run(&cwd, cli.json, repair),
        Commands::Hooks { action } => {
            let act = match commands::hooks_cmd::parse_action(&action) {
                Ok(a) => a,
                Err(e) => {
                    if !cli.quiet {
                        eprintln!("error: {e}");
                    }
                    return ExitCode::from(1);
                }
            };
            commands::hooks_cmd::run(&cwd, act, cli.json)
        }
        Commands::Transaction { action, id } => {
            let act = match action.as_str() {
                "list" => commands::tx_cmd::TxAction::List,
                "show" => {
                    let id = id.unwrap_or_default();
                    if id.is_empty() {
                        if !cli.quiet {
                            eprintln!("error: transaction show requires <id>");
                        }
                        return ExitCode::from(1);
                    }
                    commands::tx_cmd::TxAction::Show { id }
                }
                "resume" => commands::tx_cmd::TxAction::Resume { id },
                "abort" => {
                    let id = id.unwrap_or_default();
                    if id.is_empty() {
                        if !cli.quiet {
                            eprintln!("error: transaction abort requires <id>");
                        }
                        return ExitCode::from(1);
                    }
                    commands::tx_cmd::TxAction::Abort { id }
                }
                _ => {
                    if !cli.quiet {
                        eprintln!("error: usage: pit transaction list|show|resume|abort");
                    }
                    return ExitCode::from(1);
                }
            };
            commands::tx_cmd::run(&cwd, act, cli.json)
        }
        Commands::Config { action, key, value } => {
            let act = match action.as_str() {
                "list" => commands::config_cmd::ConfigAction::List,
                "get" => {
                    let key = key.unwrap_or_default();
                    if key.is_empty() {
                        if !cli.quiet {
                            eprintln!("error: config get requires <key>");
                        }
                        return ExitCode::from(1);
                    }
                    commands::config_cmd::ConfigAction::Get { key }
                }
                "set" => {
                    let key = key.unwrap_or_default();
                    let value = value.unwrap_or_default();
                    if key.is_empty() || value.is_empty() {
                        if !cli.quiet {
                            eprintln!("error: config set requires <key> <value>");
                        }
                        return ExitCode::from(1);
                    }
                    commands::config_cmd::ConfigAction::Set { key, value }
                }
                _ => {
                    if !cli.quiet {
                        eprintln!("error: usage: pit config get|set|list");
                    }
                    return ExitCode::from(1);
                }
            };
            commands::config_cmd::run(&cwd, act, cli.json)
        }
        Commands::Hook { name, .. } => commands::hook::run(&cwd, &name),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if cli.json {
                pit::json_out::print_err("pit", &e.to_string());
            } else if !cli.quiet {
                eprintln!("error: {e}");
            }
            let code = match &e {
                PitError::Unclassified(_) => 2,
                PitError::DualTracked(_) => 3,
                PitError::PrivacyValidation(_) => 4,
                PitError::PendingTransaction(_) => 5,
                _ => 1,
            };
            ExitCode::from(code)
        }
    }
}
