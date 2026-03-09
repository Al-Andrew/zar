use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Local,
    Ftp,
    Smb,
    Ssh,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Ftp => "ftp",
            Self::Smb => "smb",
            Self::Ssh => "ssh",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceCategory {
    History,
    Local,
    Ftp,
    Smb,
    Ssh,
}

impl SourceCategory {
    pub fn title(self) -> &'static str {
        match self {
            Self::History => "History",
            Self::Local => "Saved Local",
            Self::Ftp => "Saved FTP",
            Self::Smb => "Saved SMB",
            Self::Ssh => "Saved SSH",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SourceRef {
    InlineLocal { path: PathBuf, label: String },
    SavedLocal { id: String },
    SavedFtp { id: String },
    SavedSmb { id: String },
    SavedSsh { id: String },
}

impl SourceRef {
    pub fn stable_key(&self) -> String {
        match self {
            Self::InlineLocal { path, .. } => format!("inline:{}", path.display()),
            Self::SavedLocal { id } => format!("local:{id}"),
            Self::SavedFtp { id } => format!("ftp:{id}"),
            Self::SavedSmb { id } => format!("smb:{id}"),
            Self::SavedSsh { id } => format!("ssh:{id}"),
        }
    }

    pub fn kind(&self) -> SourceKind {
        match self {
            Self::InlineLocal { .. } | Self::SavedLocal { .. } => SourceKind::Local,
            Self::SavedFtp { .. } => SourceKind::Ftp,
            Self::SavedSmb { .. } => SourceKind::Smb,
            Self::SavedSsh { .. } => SourceKind::Ssh,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LocationPath {
    Local(PathBuf),
    Remote(String),
}

impl LocationPath {
    pub fn display(&self) -> String {
        match self {
            Self::Local(path) => path.display().to_string(),
            Self::Remote(path) => path.clone(),
        }
    }

    pub fn file_name(&self) -> Option<String> {
        match self {
            Self::Local(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
            Self::Remote(path) => {
                let trimmed = path.trim_end_matches('/');
                if trimmed.is_empty() || trimmed == "/" {
                    None
                } else {
                    trimmed.rsplit('/').next().map(ToOwned::to_owned)
                }
            }
        }
    }

    pub fn parent(&self) -> Option<Self> {
        match self {
            Self::Local(path) => path
                .parent()
                .map(|parent| Self::Local(parent.to_path_buf())),
            Self::Remote(path) => remote_parent(path).map(Self::Remote),
        }
    }

    pub fn join_child(&self, child: &str) -> Self {
        match self {
            Self::Local(path) => Self::Local(path.join(child)),
            Self::Remote(path) => Self::Remote(remote_join(path, child)),
        }
    }

    pub fn is_absolute_input_for_kind(kind: SourceKind, raw: &str) -> bool {
        match kind {
            SourceKind::Local => Path::new(raw).is_absolute(),
            SourceKind::Ftp | SourceKind::Smb | SourceKind::Ssh => raw.starts_with('/'),
        }
    }

    pub fn from_input(kind: SourceKind, cwd: &Self, raw: &str) -> Self {
        match kind {
            SourceKind::Local => {
                let candidate = PathBuf::from(raw);
                if candidate.is_absolute() {
                    Self::Local(candidate)
                } else {
                    match cwd {
                        Self::Local(cwd) => Self::Local(cwd.join(candidate)),
                        Self::Remote(_) => Self::Local(candidate),
                    }
                }
            }
            SourceKind::Ftp | SourceKind::Smb | SourceKind::Ssh => {
                if raw.starts_with('/') {
                    Self::Remote(normalize_remote(raw))
                } else {
                    match cwd {
                        Self::Remote(cwd) => Self::Remote(remote_join(cwd, raw)),
                        Self::Local(_) => Self::Remote(normalize_remote(raw)),
                    }
                }
            }
        }
    }

    pub fn as_local_path(&self) -> Option<&Path> {
        match self {
            Self::Local(path) => Some(path.as_path()),
            Self::Remote(_) => None,
        }
    }

    pub fn as_remote_path(&self) -> Option<&str> {
        match self {
            Self::Remote(path) => Some(path.as_str()),
            Self::Local(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

impl EntryKind {
    pub fn is_directory(self) -> bool {
        matches!(self, Self::Directory)
    }

    pub fn is_file(self) -> bool {
        matches!(self, Self::File)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub path: LocationPath,
    pub kind: EntryKind,
    pub is_hidden: bool,
}

impl FileEntry {
    pub fn display_name(&self) -> String {
        self.name.clone()
    }
}

pub fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by(compare_entries);
}

fn compare_entries(left: &FileEntry, right: &FileEntry) -> Ordering {
    left.kind
        .is_directory()
        .cmp(&right.kind.is_directory())
        .reverse()
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        .then_with(|| left.name.cmp(&right.name))
}

pub fn normalize_remote(raw: &str) -> String {
    let mut normalized = raw.replace('\\', "/");
    if normalized.is_empty() {
        return "/".to_string();
    }
    if !normalized.starts_with('/') {
        normalized = format!("/{normalized}");
    }
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

pub fn remote_join(base: &str, child: &str) -> String {
    normalize_remote(&format!("{}/{}", base.trim_end_matches('/'), child))
}

pub fn remote_parent(path: &str) -> Option<String> {
    let normalized = normalize_remote(path);
    if normalized == "/" {
        None
    } else {
        let mut parts: Vec<_> = normalized
            .trim_start_matches('/')
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        parts.pop();
        if parts.is_empty() {
            Some("/".to_string())
        } else {
            Some(format!("/{}", parts.join("/")))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{LocationPath, normalize_remote, remote_join, remote_parent};

    #[test]
    fn remote_paths_normalize_and_join() {
        assert_eq!(normalize_remote("/a//b/../c"), "/a/c");
        assert_eq!(remote_join("/srv/data", "logs"), "/srv/data/logs");
        assert_eq!(
            remote_parent("/srv/data/logs"),
            Some("/srv/data".to_string())
        );
        assert_eq!(remote_parent("/"), None);
    }

    #[test]
    fn local_and_remote_input_resolution_is_backend_aware() {
        let local = LocationPath::from_input(
            super::SourceKind::Local,
            &LocationPath::Local(PathBuf::from("/tmp")),
            "child",
        );
        assert_eq!(local, LocationPath::Local(PathBuf::from("/tmp/child")));

        let remote = LocationPath::from_input(
            super::SourceKind::Ssh,
            &LocationPath::Remote("/var".to_string()),
            "log",
        );
        assert_eq!(remote, LocationPath::Remote("/var/log".to_string()));
    }
}
