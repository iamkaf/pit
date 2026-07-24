use crate::error::{PitError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TxState {
    New,
    Prepared,
    LocalPublicCommitted,
    LocalPrivateCommitted,
    LocalComplete,
    PrivatePushStarted,
    PrivatePushed,
    PublicPushStarted,
    Complete,
    FailedRecoverable,
    FailedManual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub schema_version: u32,
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub state: TxState,
    pub public_branch: String,
    pub private_branch: String,
    pub public_before: Option<String>,
    pub public_after: Option<String>,
    pub private_before: Option<String>,
    pub private_after: Option<String>,
    pub message: String,
    pub public_paths: Vec<String>,
    pub private_paths: Vec<String>,
    pub private_push_ok: bool,
    pub public_push_ok: bool,
    pub recovery_hint: Option<String>,
    pub last_error: Option<String>,
}

impl Transaction {
    pub fn new(message: &str, public_branch: &str, private_branch: &str) -> Self {
        let now = Utc::now();
        Self {
            schema_version: 1,
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            state: TxState::New,
            public_branch: public_branch.to_string(),
            private_branch: private_branch.to_string(),
            public_before: None,
            public_after: None,
            private_before: None,
            private_after: None,
            message: message.to_string(),
            public_paths: Vec::new(),
            private_paths: Vec::new(),
            private_push_ok: false,
            public_push_ok: false,
            recovery_hint: None,
            last_error: None,
        }
    }

    pub fn touch(&mut self, state: TxState) {
        self.state = state;
        self.updated_at = Utc::now();
    }

    pub fn is_pending_push(&self) -> bool {
        matches!(
            self.state,
            TxState::LocalComplete
                | TxState::PrivatePushStarted
                | TxState::PrivatePushed
                | TxState::PublicPushStarted
                | TxState::FailedRecoverable
        ) && !self.public_push_ok
    }

    pub fn needs_resume(&self) -> bool {
        self.private_push_ok && !self.public_push_ok
            && matches!(
                self.state,
                TxState::PrivatePushed | TxState::PublicPushStarted | TxState::FailedRecoverable
            )
    }
}

pub struct TxStore {
    dir: PathBuf,
}

impl TxStore {
    pub fn new(pit_dir: &Path) -> Self {
        Self {
            dir: pit_dir.join("transactions"),
        }
    }

    pub fn ensure(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        Ok(())
    }

    fn path_for(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    pub fn save(&self, tx: &Transaction) -> Result<()> {
        self.ensure()?;
        let path = self.path_for(tx.id);
        let tmp = path.with_extension("json.tmp");
        let data = serde_json::to_string_pretty(tx)?;
        fs::write(&tmp, data)?;
        // fsync best-effort
        if let Ok(f) = fs::File::open(&tmp) {
            let _ = f.sync_all();
        }
        fs::rename(&tmp, &path)?;
        // Also write "current" pointer for resume
        let current = self.dir.join("CURRENT");
        fs::write(&current, tx.id.to_string())?;
        Ok(())
    }

    pub fn load(&self, id: Uuid) -> Result<Transaction> {
        let path = self.path_for(id);
        let data = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&data)?)
    }

    pub fn load_current(&self) -> Result<Option<Transaction>> {
        let current = self.dir.join("CURRENT");
        if !current.exists() {
            return Ok(None);
        }
        let id_str = fs::read_to_string(&current)?;
        let id = Uuid::parse_str(id_str.trim())
            .map_err(|e| PitError::msg(format!("invalid CURRENT transaction id: {e}")))?;
        let tx = self.load(id)?;
        if tx.state == TxState::Complete {
            return Ok(None);
        }
        Ok(Some(tx))
    }

    pub fn list_pending(&self) -> Result<Vec<Transaction>> {
        self.ensure()?;
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.ends_with(".json") {
                continue;
            }
            let data = fs::read_to_string(entry.path())?;
            if let Ok(tx) = serde_json::from_str::<Transaction>(&data) {
                if tx.state != TxState::Complete && tx.is_pending_push() {
                    out.push(tx);
                }
            }
        }
        out.sort_by_key(|t| t.created_at);
        Ok(out)
    }

    pub fn clear_current_if(&self, id: Uuid) -> Result<()> {
        let current = self.dir.join("CURRENT");
        if current.exists() {
            let cur = fs::read_to_string(&current)?;
            if cur.trim() == id.to_string() {
                let _ = fs::remove_file(&current);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn journal_roundtrip_and_resume() {
        let dir = tempdir().unwrap();
        let store = TxStore::new(dir.path());
        let mut tx = Transaction::new("test", "main", "main");
        tx.public_after = Some("abc".into());
        tx.touch(TxState::LocalComplete);
        store.save(&tx).unwrap();

        let loaded = store.load_current().unwrap().unwrap();
        assert_eq!(loaded.id, tx.id);
        assert_eq!(loaded.state, TxState::LocalComplete);

        let mut tx2 = loaded;
        tx2.private_push_ok = true;
        tx2.touch(TxState::PrivatePushed);
        store.save(&tx2).unwrap();
        assert!(store.load_current().unwrap().unwrap().needs_resume());
    }
}
