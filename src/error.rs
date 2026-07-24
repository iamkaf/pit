use thiserror::Error;

#[derive(Error, Debug)]
pub enum PitError {
    #[error("{0}")]
    Message(String),

    #[error("not a git repository: {0}")]
    NotAGitRepo(String),

    #[error("not a pit workspace: {0}")]
    NotAPitWorkspace(String),

    #[error("unclassified paths (fail-closed):\n{}", format_paths(.0))]
    Unclassified(Vec<String>),

    #[error("dual-tracked paths (invariant violation):\n{}", format_paths(.0))]
    DualTracked(Vec<String>),

    #[error("privacy validation failed: {0}")]
    PrivacyValidation(String),

    #[error("git command failed: {cmd}\n{stderr}")]
    Git { cmd: String, stderr: String },

    #[error("pending transaction requires resume: {0}")]
    PendingTransaction(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("toml error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("toml serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

fn format_paths(paths: &[String]) -> String {
    paths
        .iter()
        .map(|p| format!("  {p}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub type Result<T> = std::result::Result<T, PitError>;

impl PitError {
    pub fn msg(s: impl Into<String>) -> Self {
        PitError::Message(s.into())
    }
}
