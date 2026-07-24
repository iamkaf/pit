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
        /// Private remote URL (path or git URL)
        #[arg(long)]
        private: Option<String>,
        /// Create a private GitHub companion via `gh`
        #[arg(long)]
        create_github: bool,
    },
    /// Show public, private, unclassified, and transaction state
    Status,
    /// Classify and stage paths into the correct index
    Add {
        /// Paths to add (default: none unless -A)
        paths: Vec<String>,
        /// Stage all known changes
        #[arg(short = 'A', long = "all")]
        all: bool,
        /// Force private classification
        #[arg(long)]
        private: bool,
        /// Force public classification
        #[arg(long)]
        public: bool,
        /// Force ignore
        #[arg(long)]
        ignore: bool,
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
    /// Validate workspace health and privacy invariants
    Doctor,
    /// Internal: hook dispatcher entry
    #[command(hide = true)]
    Hook {
        name: String,
        /// Remaining args from Git (e.g. pre-push remote name/url)
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
                },
            )
        }
        Commands::Commit { message } => commands::commit::run(
            &cwd,
            commands::commit::CommitArgs {
                message,
                dry_run: cli.dry_run,
            },
        ),
        Commands::Push { resume } => commands::push::run(
            &cwd,
            commands::push::PushArgs {
                resume,
                dry_run: cli.dry_run,
            },
        ),
        Commands::Doctor => commands::doctor::run(&cwd, cli.json),
        Commands::Hook { name, .. } => commands::hook::run(&cwd, &name),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if !cli.quiet {
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
