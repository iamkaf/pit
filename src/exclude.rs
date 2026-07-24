use crate::error::Result;
use std::path::Path;

pub const BEGIN_MARKER: &str = "# BEGIN PIT MANAGED — DO NOT EDIT BY HAND";
pub const END_MARKER: &str = "# END PIT MANAGED";

/// Update the Pit-managed block in `.git/info/exclude` without touching user lines.
pub fn update_managed_exclude(exclude_path: &Path, private_patterns: &[String]) -> Result<()> {
    if let Some(parent) = exclude_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let existing = if exclude_path.exists() {
        std::fs::read_to_string(exclude_path)?
    } else {
        String::new()
    };

    let user_part = strip_managed_block(&existing);
    let mut block = String::new();
    block.push_str(BEGIN_MARKER);
    block.push('\n');
    for p in private_patterns {
        block.push_str(p);
        block.push('\n');
    }
    // Always exclude pit internal metadata from public staging
    if !private_patterns.iter().any(|p| p.contains("pit-worktree")) {
        block.push_str(".git/pit-worktree-metadata/**\n");
    }
    block.push_str(END_MARKER);
    block.push('\n');

    let mut out = user_part;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&block);
    atomic_write(exclude_path, &out)?;
    Ok(())
}

pub fn strip_managed_block(content: &str) -> String {
    let mut result = String::new();
    let mut in_block = false;
    for line in content.lines() {
        if line.trim() == BEGIN_MARKER {
            in_block = true;
            continue;
        }
        if line.trim() == END_MARKER {
            in_block = false;
            continue;
        }
        if !in_block {
            result.push_str(line);
            result.push('\n');
        }
    }
    // trim trailing blank lines for cleanliness but preserve user content
    while result.ends_with("\n\n") {
        result.pop();
    }
    result
}

pub fn has_managed_block(content: &str) -> bool {
    content.contains(BEGIN_MARKER) && content.contains(END_MARKER)
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("exclude.pit-tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn preserves_user_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("exclude");
        std::fs::write(&path, "*.local-scratch\nfoo.bar\n").unwrap();
        update_managed_exclude(&path, &["private/**".into(), ".env".into()]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("*.local-scratch"));
        assert!(text.contains("foo.bar"));
        assert!(text.contains(BEGIN_MARKER));
        assert!(text.contains("private/**"));
        assert!(text.contains(END_MARKER));
        // user lines before managed block
        let user_idx = text.find("*.local-scratch").unwrap();
        let begin_idx = text.find(BEGIN_MARKER).unwrap();
        assert!(user_idx < begin_idx);
    }

    #[test]
    fn update_is_idempotent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("exclude");
        std::fs::write(&path, "keep-me\n").unwrap();
        update_managed_exclude(&path, &["private/**".into()]).unwrap();
        update_managed_exclude(&path, &["private/**".into(), ".env".into()]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches(BEGIN_MARKER).count(), 1);
        assert!(text.contains("keep-me"));
        assert!(text.contains(".env"));
    }
}
