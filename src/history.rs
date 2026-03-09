use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::source::{LocationPath, SourceKind, SourceRef};

pub trait HistoryStore {
    fn entries(&self) -> &[HistoryEntry];
    fn last_path_for(&self, source: &SourceRef) -> Option<LocationPath>;
    fn record(&mut self, source: &SourceRef, label: &str, path: &LocationPath) -> Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoryEntry {
    pub source_kind: SourceKind,
    pub source_key: String,
    pub label: String,
    pub last_path: LocationPath,
    pub last_used_at: u64,
}

#[derive(Debug, Clone, Default)]
pub struct TomlHistoryStore {
    path: Option<std::path::PathBuf>,
    entries: Vec<HistoryEntry>,
}

impl TomlHistoryStore {
    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        Self::load_from_path(&dir.join("history.toml"))
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                path: Some(path.to_path_buf()),
                entries: Vec::new(),
            });
        }

        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let file: HistoryFile =
            toml::from_str(&contents).context("failed to parse history.toml")?;
        let mut entries = file.history;
        entries.sort_by(|left, right| right.last_used_at.cmp(&left.last_used_at));
        Ok(Self {
            path: Some(path.to_path_buf()),
            entries,
        })
    }

    fn persist(&self) -> Result<()> {
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let body = toml::to_string_pretty(&HistoryFile {
            history: self.entries.clone(),
        })
        .context("failed to serialize history")?;
        fs::write(path, body).with_context(|| format!("failed to write {}", path.display()))
    }
}

impl HistoryStore for TomlHistoryStore {
    fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    fn last_path_for(&self, source: &SourceRef) -> Option<LocationPath> {
        let key = source.stable_key();
        self.entries
            .iter()
            .find(|entry| entry.source_key == key)
            .map(|entry| entry.last_path.clone())
    }

    fn record(&mut self, source: &SourceRef, label: &str, path: &LocationPath) -> Result<()> {
        let key = source.stable_key();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.entries.retain(|entry| entry.source_key != key);
        self.entries.insert(
            0,
            HistoryEntry {
                source_kind: source.kind(),
                source_key: key,
                label: label.to_string(),
                last_path: path.clone(),
                last_used_at: now,
            },
        );
        if self.entries.len() > 50 {
            self.entries.truncate(50);
        }
        self.persist()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HistoryFile {
    #[serde(default)]
    history: Vec<HistoryEntry>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::TempDir;

    use crate::history::{HistoryStore, TomlHistoryStore};
    use crate::source::{LocationPath, SourceRef};

    #[test]
    fn records_and_restores_last_path() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("history.toml");
        let mut store = TomlHistoryStore::load_from_path(&path).expect("history");
        let source = SourceRef::SavedSsh {
            id: "prod".to_string(),
        };

        store
            .record(
                &source,
                "Prod",
                &LocationPath::Remote("/var/www".to_string()),
            )
            .expect("record");

        let reloaded = TomlHistoryStore::load_from_path(&path).expect("reload");
        assert_eq!(
            reloaded.last_path_for(&source),
            Some(LocationPath::Remote("/var/www".to_string()))
        );
    }

    #[test]
    fn history_is_capped_and_latest_first() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("history.toml");
        let mut store = TomlHistoryStore::load_from_path(&path).expect("history");

        for index in 0..55 {
            store
                .record(
                    &SourceRef::InlineLocal {
                        path: PathBuf::from(format!("/tmp/{index}")),
                        label: format!("Tmp {index}"),
                    },
                    &format!("Tmp {index}"),
                    &LocationPath::Local(PathBuf::from(format!("/tmp/{index}"))),
                )
                .expect("record");
        }

        assert_eq!(store.entries().len(), 50);
        assert_eq!(store.entries()[0].label, "Tmp 54");
    }
}
