use crate::error::{PitError, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Authoritative privacy policy (versioned in private mirror; local cache under .git/pit/).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub version: u32,
    #[serde(default)]
    pub classification: ClassificationConfig,
    #[serde(default)]
    pub private: PatternSection,
    #[serde(default)]
    pub ignored: PatternSection,
    #[serde(default)]
    pub public: PatternSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationConfig {
    /// prompt | public | private | reject
    #[serde(default = "default_new_files")]
    pub new_files: String,
}

fn default_new_files() -> String {
    "reject".to_string()
}

impl Default for ClassificationConfig {
    fn default() -> Self {
        Self {
            new_files: default_new_files(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PatternSection {
    #[serde(default)]
    pub patterns: Vec<String>,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            version: 1,
            classification: ClassificationConfig::default(),
            private: PatternSection {
                patterns: vec![
                    ".env".into(),
                    ".env.*".into(),
                    "private/**".into(),
                    "notes/internal/**".into(),
                    "config/*.secret".into(),
                    ".git/pit-worktree-metadata/**".into(),
                ],
            },
            ignored: PatternSection {
                patterns: vec![
                    ".DS_Store".into(),
                    "tmp/**".into(),
                    "dist/**".into(),
                    "target/**".into(),
                    "*.o".into(),
                ],
            },
            public: PatternSection {
                patterns: vec![
                    "README.md".into(),
                    "LICENSE".into(),
                    "src/**".into(),
                    "docs/public/**".into(),
                    "Cargo.toml".into(),
                    "Cargo.lock".into(),
                    "*.rs".into(),
                    "tests/**".into(),
                    ".gitignore".into(),
                ],
            },
        }
    }
}

impl Policy {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let p: Policy = toml::from_str(&text)?;
        Ok(p)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn matcher(&self) -> Result<PolicyMatcher> {
        PolicyMatcher::new(self)
    }

    pub fn all_private_patterns(&self) -> &[String] {
        &self.private.patterns
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Public,
    Private,
    Ignored,
    Unclassified,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Public => "public",
            Class::Private => "private",
            Class::Ignored => "ignored",
            Class::Unclassified => "unclassified",
        }
    }
}

pub struct PolicyMatcher {
    private: GlobSet,
    ignored: GlobSet,
    public: GlobSet,
    new_files: String,
    private_patterns: Vec<String>,
}

impl PolicyMatcher {
    pub fn new(policy: &Policy) -> Result<Self> {
        Ok(Self {
            private: build_set(&policy.private.patterns)?,
            ignored: build_set(&policy.ignored.patterns)?,
            public: build_set(&policy.public.patterns)?,
            new_files: policy.classification.new_files.clone(),
            private_patterns: policy.private.patterns.clone(),
        })
    }

    /// Classify a path relative to the work tree.
    /// Order: private → ignored → public → default.
    pub fn classify(&self, path: &str) -> Class {
        let path = normalize_path(path);
        if self.private.is_match(&path) {
            return Class::Private;
        }
        if self.ignored.is_match(&path) {
            return Class::Ignored;
        }
        if self.public.is_match(&path) {
            return Class::Public;
        }
        match self.new_files.as_str() {
            "public" => Class::Public,
            "private" => Class::Private,
            "prompt" | "reject" => Class::Unclassified,
            _ => Class::Unclassified,
        }
    }

    pub fn is_private_pattern_match(&self, path: &str) -> bool {
        self.private.is_match(normalize_path(path))
    }

    pub fn private_patterns(&self) -> &[String] {
        &self.private_patterns
    }

    pub fn fail_closed(&self) -> bool {
        matches!(self.new_files.as_str(), "prompt" | "reject")
    }
}

fn normalize_path(path: &str) -> String {
    path.trim_start_matches("./").replace('\\', "/")
}

fn build_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for p in patterns {
        // Support git-style ** patterns via globset
        let g = Glob::new(p).map_err(|e| PitError::msg(format!("invalid pattern '{p}': {e}")))?;
        builder.add(g);
        // Also match without trailing /**
        if let Some(stripped) = p.strip_suffix("/**") {
            if let Ok(g2) = Glob::new(stripped) {
                builder.add(g2);
            }
            if let Ok(g3) = Glob::new(&format!("{stripped}/**/*")) {
                builder.add(g3);
            }
        }
    }
    builder
        .build()
        .map_err(|e| PitError::msg(format!("globset build: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_private_paths() {
        let m = Policy::default().matcher().unwrap();
        assert_eq!(m.classify("private/notes.txt"), Class::Private);
        assert_eq!(m.classify("private/deep/a.md"), Class::Private);
        assert_eq!(m.classify(".env"), Class::Private);
        assert_eq!(m.classify(".env.local"), Class::Private);
    }

    #[test]
    fn classifies_public_paths() {
        let m = Policy::default().matcher().unwrap();
        assert_eq!(m.classify("src/index.ts"), Class::Public);
        assert_eq!(m.classify("README.md"), Class::Public);
        assert_eq!(m.classify("Cargo.toml"), Class::Public);
    }

    #[test]
    fn fail_closed_unclassified() {
        let m = Policy::default().matcher().unwrap();
        assert!(m.fail_closed());
        assert_eq!(m.classify("mystery/file.bin"), Class::Unclassified);
    }

    #[test]
    fn public_default_mode() {
        let mut p = Policy::default();
        p.classification.new_files = "public".into();
        let m = p.matcher().unwrap();
        assert_eq!(m.classify("mystery/file.bin"), Class::Public);
    }

    #[test]
    fn ignored_patterns() {
        let m = Policy::default().matcher().unwrap();
        assert_eq!(m.classify("target/debug/pit"), Class::Ignored);
        assert_eq!(m.classify("tmp/x"), Class::Ignored);
    }
}
