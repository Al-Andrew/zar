use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: OsString,
    pub path: PathBuf,
    pub kind: EntryKind,
    pub is_hidden: bool,
}

impl FileEntry {
    pub fn display_name(&self) -> String {
        self.name.to_string_lossy().into_owned()
    }
}

pub fn read_directory(path: &Path) -> Result<Vec<FileEntry>> {
    let read_dir =
        fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))?;

    let mut entries = Vec::new();
    for entry in read_dir {
        let entry = entry.with_context(|| format!("failed to read {}", path.display()))?;
        let file_type = entry.file_type().ok();
        let kind = match file_type {
            Some(ft) if ft.is_dir() => EntryKind::Directory,
            Some(ft) if ft.is_file() => EntryKind::File,
            Some(ft) if ft.is_symlink() => EntryKind::Symlink,
            _ => EntryKind::Other,
        };

        let name = entry.file_name();
        let is_hidden = name.to_string_lossy().starts_with('.');

        entries.push(FileEntry {
            path: entry.path(),
            name,
            kind,
            is_hidden,
        });
    }

    entries.sort_by(compare_entries);
    Ok(entries)
}

fn compare_entries(left: &FileEntry, right: &FileEntry) -> Ordering {
    left.kind
        .is_directory()
        .cmp(&right.kind.is_directory())
        .reverse()
        .then_with(|| {
            left.display_name()
                .to_lowercase()
                .cmp(&right.display_name().to_lowercase())
        })
        .then_with(|| left.display_name().cmp(&right.display_name()))
}

impl EntryKind {
    pub fn is_directory(&self) -> bool {
        matches!(self, Self::Directory)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::read_directory;

    #[test]
    fn sorts_directories_before_files_case_insensitively() {
        let temp = TempDir::new().expect("temp dir");
        fs::create_dir(temp.path().join("Zoo")).expect("dir");
        fs::create_dir(temp.path().join("alpha")).expect("dir");
        fs::write(temp.path().join("Beta.txt"), b"b").expect("file");
        fs::write(temp.path().join("aardvark.txt"), b"a").expect("file");

        let entries = read_directory(temp.path()).expect("read dir");
        let names: Vec<_> = entries.iter().map(|entry| entry.display_name()).collect();

        assert_eq!(names, vec!["alpha", "Zoo", "aardvark.txt", "Beta.txt"]);
    }
}
