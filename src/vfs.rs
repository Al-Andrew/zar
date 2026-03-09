use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use remotefs::fs::{Metadata, RemoteFs, UnixPex};
use remotefs_ftp::FtpFs;
use remotefs_ssh::{SftpFs, SshConfigParseRule, SshOpts};
use tempfile::NamedTempFile;

use crate::config::{
    Config, FtpSourceProfile, LocalSourceProfile, SmbSourceProfile, SshAuthMethod, SshSourceProfile,
};
use crate::secrets::SecretStore;
use crate::source::{
    EntryKind, FileEntry, LocationPath, SourceKind, SourceRef, normalize_remote, sort_entries,
};

pub struct ConnectedSource {
    pub kind: SourceKind,
    pub label: String,
    pub source_ref: SourceRef,
    pub session: Box<dyn VfsSession>,
    pub default_path: LocationPath,
}

pub trait SessionFactory {
    fn connect(
        &self,
        config: &Config,
        secrets: &dyn SecretStore,
        source: &SourceRef,
    ) -> Result<ConnectedSource>;
}

pub trait VfsSession {
    fn source_kind(&self) -> SourceKind;
    fn source_label(&self) -> &str;
    fn pwd(&mut self) -> Result<LocationPath>;
    fn change_dir(&mut self, path: &LocationPath) -> Result<LocationPath>;
    fn list_dir(&mut self, path: &LocationPath) -> Result<Vec<FileEntry>>;
    fn entry_kind(&mut self, path: &LocationPath) -> Result<EntryKind>;
    fn exists(&mut self, path: &LocationPath) -> Result<bool>;
    fn read_text_file(&mut self, path: &LocationPath) -> Result<String>;
    fn copy_file_within_source(
        &mut self,
        source: &LocationPath,
        destination: &LocationPath,
    ) -> Result<()>;
    fn move_entry_within_source(
        &mut self,
        source: &LocationPath,
        destination: &LocationPath,
    ) -> Result<()>;
    fn create_dir(&mut self, path: &LocationPath) -> Result<()>;
    fn delete_entry(&mut self, path: &LocationPath) -> Result<()>;
    fn copy_file_to_writer(&mut self, path: &LocationPath, writer: &mut dyn Write) -> Result<u64>;
    fn create_file_from_reader(
        &mut self,
        path: &LocationPath,
        reader: &mut dyn Read,
        size_hint: u64,
    ) -> Result<u64>;
    fn disconnect(&mut self) -> Result<()>;
}

pub struct DefaultSessionFactory;

impl SessionFactory for DefaultSessionFactory {
    fn connect(
        &self,
        config: &Config,
        secrets: &dyn SecretStore,
        source: &SourceRef,
    ) -> Result<ConnectedSource> {
        match source {
            SourceRef::InlineLocal { path, label } => Ok(ConnectedSource {
                kind: SourceKind::Local,
                label: label.clone(),
                source_ref: source.clone(),
                session: Box::new(LocalSession::new(label.clone(), path.clone())?),
                default_path: LocationPath::Local(path.clone()),
            }),
            SourceRef::SavedLocal { id } => {
                let profile = config
                    .sources
                    .local
                    .get(id)
                    .with_context(|| format!("unknown local source profile: {id}"))?;
                connect_local_profile(id, profile, source.clone())
            }
            SourceRef::SavedFtp { id } => {
                let profile = config
                    .sources
                    .ftp
                    .get(id)
                    .with_context(|| format!("unknown ftp source profile: {id}"))?;
                connect_ftp_profile(id, profile, secrets, source.clone())
            }
            SourceRef::SavedSmb { id } => {
                let profile = config
                    .sources
                    .smb
                    .get(id)
                    .with_context(|| format!("unknown smb source profile: {id}"))?;
                connect_smb_profile(id, profile, secrets, source.clone())
            }
            SourceRef::SavedSsh { id } => {
                let profile = config
                    .sources
                    .ssh
                    .get(id)
                    .with_context(|| format!("unknown ssh source profile: {id}"))?;
                connect_ssh_profile(id, profile, secrets, source.clone())
            }
        }
    }
}

fn connect_local_profile(
    _id: &str,
    profile: &LocalSourceProfile,
    source_ref: SourceRef,
) -> Result<ConnectedSource> {
    Ok(ConnectedSource {
        kind: SourceKind::Local,
        label: profile.label.clone(),
        source_ref,
        session: Box::new(LocalSession::new(
            profile.label.clone(),
            profile.path.clone(),
        )?),
        default_path: LocationPath::Local(profile.path.clone()),
    })
}

fn connect_ftp_profile(
    id: &str,
    profile: &FtpSourceProfile,
    secrets: &dyn SecretStore,
    source_ref: SourceRef,
) -> Result<ConnectedSource> {
    let password = secrets
        .ftp_password(id)
        .with_context(|| format!("missing ftp password for profile {id}"))?;
    let client = FtpFs::new(&profile.host, profile.port)
        .username(&profile.username)
        .password(password);
    Ok(ConnectedSource {
        kind: SourceKind::Ftp,
        label: profile.label.clone(),
        source_ref,
        session: Box::new(RemoteSession::connect(
            SourceKind::Ftp,
            profile.label.clone(),
            Box::new(client),
        )?),
        default_path: LocationPath::Remote(normalize_remote(&profile.initial_path)),
    })
}

fn connect_smb_profile(
    _id: &str,
    _profile: &SmbSourceProfile,
    _secrets: &dyn SecretStore,
    _source_ref: SourceRef,
) -> Result<ConnectedSource> {
    bail!("SMB support is unavailable in this build")
}

fn connect_ssh_profile(
    id: &str,
    profile: &SshSourceProfile,
    secrets: &dyn SecretStore,
    source_ref: SourceRef,
) -> Result<ConnectedSource> {
    let mut opts = SshOpts::new(&profile.host)
        .port(profile.port)
        .username(&profile.username)
        .connection_timeout(Duration::from_secs(30));

    let ssh_config = dirs_home().join(".ssh").join("config");
    if ssh_config.exists() {
        opts = opts.config_file(&ssh_config, SshConfigParseRule::ALLOW_UNKNOWN_FIELDS);
    }

    match profile.auth {
        SshAuthMethod::Password => {
            let password = secrets
                .ssh_password(id)
                .with_context(|| format!("missing ssh password for profile {id}"))?;
            opts = opts.password(password);
        }
        SshAuthMethod::KeyFile => {
            struct StaticKeyStorage(PathBuf);

            impl remotefs_ssh::SshKeyStorage for StaticKeyStorage {
                fn resolve(&self, _host: &str, _username: &str) -> Option<PathBuf> {
                    Some(self.0.clone())
                }
            }

            let key_path = profile
                .key_path
                .clone()
                .with_context(|| format!("ssh profile {id} is missing key_path"))?;
            if let Some(passphrase) = secrets.ssh_key_passphrase(id) {
                opts = opts.password(passphrase);
            }
            opts = opts.key_storage(Box::new(StaticKeyStorage(key_path)));
        }
    }

    let client = SftpFs::libssh2(opts);
    Ok(ConnectedSource {
        kind: SourceKind::Ssh,
        label: profile.label.clone(),
        source_ref,
        session: Box::new(RemoteSession::connect(
            SourceKind::Ssh,
            profile.label.clone(),
            Box::new(client),
        )?),
        default_path: LocationPath::Remote(normalize_remote(&profile.initial_path)),
    })
}

pub struct LocalSession {
    label: String,
    cwd: PathBuf,
}

impl LocalSession {
    pub fn new(label: String, cwd: PathBuf) -> Result<Self> {
        if !cwd.is_dir() {
            bail!("not a directory: {}", cwd.display());
        }
        Ok(Self { label, cwd })
    }
}

impl VfsSession for LocalSession {
    fn source_kind(&self) -> SourceKind {
        SourceKind::Local
    }

    fn source_label(&self) -> &str {
        &self.label
    }

    fn pwd(&mut self) -> Result<LocationPath> {
        Ok(LocationPath::Local(self.cwd.clone()))
    }

    fn change_dir(&mut self, path: &LocationPath) -> Result<LocationPath> {
        let path = expect_local(path)?;
        if !path.is_dir() {
            bail!("not a directory: {}", path.display());
        }
        self.cwd = path.to_path_buf();
        Ok(LocationPath::Local(self.cwd.clone()))
    }

    fn list_dir(&mut self, path: &LocationPath) -> Result<Vec<FileEntry>> {
        let path = expect_local(path)?;
        read_local_dir(path)
    }

    fn entry_kind(&mut self, path: &LocationPath) -> Result<EntryKind> {
        local_entry_kind(expect_local(path)?)
    }

    fn exists(&mut self, path: &LocationPath) -> Result<bool> {
        Ok(expect_local(path)?.exists())
    }

    fn read_text_file(&mut self, path: &LocationPath) -> Result<String> {
        let path = expect_local(path)?;
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
    }

    fn copy_file_within_source(
        &mut self,
        source: &LocationPath,
        destination: &LocationPath,
    ) -> Result<()> {
        let source = expect_local(source)?;
        let destination = expect_local(destination)?;
        if !source.is_file() {
            bail!("not a file: {}", source.display());
        }
        if destination.exists() {
            bail!("destination already exists: {}", destination.display());
        }
        fs::copy(source, destination).with_context(|| {
            format!(
                "failed to copy {} to {}",
                source.display(),
                destination.display()
            )
        })?;
        Ok(())
    }

    fn move_entry_within_source(
        &mut self,
        source: &LocationPath,
        destination: &LocationPath,
    ) -> Result<()> {
        let source = expect_local(source)?;
        let destination = expect_local(destination)?;
        if destination.exists() {
            bail!("destination already exists: {}", destination.display());
        }
        match fs::rename(source, destination) {
            Ok(()) => Ok(()),
            Err(_) => {
                if source.is_file() {
                    fs::copy(source, destination).with_context(|| {
                        format!(
                            "failed to move {} to {}",
                            source.display(),
                            destination.display()
                        )
                    })?;
                    fs::remove_file(source)
                        .with_context(|| format!("failed to remove {}", source.display()))?;
                    Ok(())
                } else {
                    bail!("move supports files only")
                }
            }
        }
    }

    fn create_dir(&mut self, path: &LocationPath) -> Result<()> {
        let path = expect_local(path)?;
        if path.exists() {
            bail!("destination already exists: {}", path.display());
        }
        fs::create_dir(path).with_context(|| format!("failed to create {}", path.display()))
    }

    fn delete_entry(&mut self, path: &LocationPath) -> Result<()> {
        let path = expect_local(path)?;
        if !path.exists() {
            bail!("path does not exist: {}", path.display());
        }
        let metadata = fs::symlink_metadata(path)
            .with_context(|| format!("failed to inspect {}", path.display()))?;
        if metadata.is_dir() {
            fs::remove_dir_all(path).with_context(|| format!("failed to delete {}", path.display()))
        } else {
            fs::remove_file(path).with_context(|| format!("failed to delete {}", path.display()))
        }
    }

    fn copy_file_to_writer(&mut self, path: &LocationPath, writer: &mut dyn Write) -> Result<u64> {
        let path = expect_local(path)?;
        let mut file =
            fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        io::copy(&mut file, writer).context("failed to stream local file")
    }

    fn create_file_from_reader(
        &mut self,
        path: &LocationPath,
        reader: &mut dyn Read,
        _size_hint: u64,
    ) -> Result<u64> {
        let path = expect_local(path)?;
        if path.exists() {
            bail!("destination already exists: {}", path.display());
        }
        let mut file = fs::File::create(path)
            .with_context(|| format!("failed to create {}", path.display()))?;
        io::copy(reader, &mut file).context("failed to write local file")
    }

    fn disconnect(&mut self) -> Result<()> {
        Ok(())
    }
}

pub struct RemoteSession {
    kind: SourceKind,
    label: String,
    cwd: String,
    client: Box<dyn RemoteFs>,
}

impl RemoteSession {
    pub fn connect(kind: SourceKind, label: String, mut client: Box<dyn RemoteFs>) -> Result<Self> {
        client.connect().map_err(remote_error)?;
        let cwd = client.pwd().map_err(remote_error)?.display().to_string();
        Ok(Self {
            kind,
            label,
            cwd: normalize_remote(&cwd),
            client,
        })
    }

    fn remote_path<'a>(&self, path: &'a LocationPath) -> Result<&'a str> {
        path.as_remote_path()
            .ok_or_else(|| anyhow!("remote session cannot use local path"))
    }
}

impl VfsSession for RemoteSession {
    fn source_kind(&self) -> SourceKind {
        self.kind
    }

    fn source_label(&self) -> &str {
        &self.label
    }

    fn pwd(&mut self) -> Result<LocationPath> {
        let cwd = self.client.pwd().map_err(remote_error)?;
        self.cwd = normalize_remote(&cwd.display().to_string());
        Ok(LocationPath::Remote(self.cwd.clone()))
    }

    fn change_dir(&mut self, path: &LocationPath) -> Result<LocationPath> {
        let path = self.remote_path(path)?;
        let changed = self
            .client
            .change_dir(Path::new(path))
            .map_err(remote_error)?;
        self.cwd = normalize_remote(&changed.display().to_string());
        Ok(LocationPath::Remote(self.cwd.clone()))
    }

    fn list_dir(&mut self, path: &LocationPath) -> Result<Vec<FileEntry>> {
        let path = self.remote_path(path)?;
        let mut entries = self
            .client
            .list_dir(Path::new(path))
            .map_err(remote_error)?
            .into_iter()
            .map(|entry| {
                let kind = if entry.is_dir() {
                    EntryKind::Directory
                } else if entry.is_file() {
                    EntryKind::File
                } else if entry.is_symlink() {
                    EntryKind::Symlink
                } else {
                    EntryKind::Other
                };
                FileEntry {
                    name: entry.name(),
                    path: LocationPath::Remote(normalize_remote(&entry.path.display().to_string())),
                    kind,
                    is_hidden: entry.is_hidden(),
                }
            })
            .collect::<Vec<_>>();
        sort_entries(&mut entries);
        Ok(entries)
    }

    fn entry_kind(&mut self, path: &LocationPath) -> Result<EntryKind> {
        let path = self.remote_path(path)?;
        let entry = self.client.stat(Path::new(path)).map_err(remote_error)?;
        Ok(if entry.is_dir() {
            EntryKind::Directory
        } else if entry.is_file() {
            EntryKind::File
        } else if entry.is_symlink() {
            EntryKind::Symlink
        } else {
            EntryKind::Other
        })
    }

    fn exists(&mut self, path: &LocationPath) -> Result<bool> {
        let path = self.remote_path(path)?;
        self.client.exists(Path::new(path)).map_err(remote_error)
    }

    fn read_text_file(&mut self, path: &LocationPath) -> Result<String> {
        let path = self.remote_path(path)?;
        let mut stream = self.client.open(Path::new(path)).map_err(remote_error)?;
        let mut bytes = Vec::new();
        stream
            .read_to_end(&mut bytes)
            .context("failed to read remote file")?;
        self.client.on_read(stream).map_err(remote_error)?;
        String::from_utf8(bytes).context("file is not valid UTF-8")
    }

    fn copy_file_within_source(
        &mut self,
        source: &LocationPath,
        destination: &LocationPath,
    ) -> Result<()> {
        let source = self.remote_path(source)?;
        let destination = self.remote_path(destination)?;
        match self.client.copy(Path::new(source), Path::new(destination)) {
            Ok(()) => Ok(()),
            Err(err) => {
                if err.kind == remotefs::RemoteErrorType::UnsupportedFeature {
                    copy_via_temp(self, source, destination)
                } else {
                    Err(remote_error(err))
                }
            }
        }
    }

    fn move_entry_within_source(
        &mut self,
        source: &LocationPath,
        destination: &LocationPath,
    ) -> Result<()> {
        let source = self.remote_path(source)?;
        let destination = self.remote_path(destination)?;
        match self.client.mov(Path::new(source), Path::new(destination)) {
            Ok(()) => Ok(()),
            Err(err) => {
                if err.kind == remotefs::RemoteErrorType::UnsupportedFeature {
                    copy_via_temp(self, source, destination)?;
                    self.client
                        .remove_dir_all(Path::new(source))
                        .map_err(remote_error)
                } else {
                    Err(remote_error(err))
                }
            }
        }
    }

    fn create_dir(&mut self, path: &LocationPath) -> Result<()> {
        let path = self.remote_path(path)?;
        self.client
            .create_dir(Path::new(path), UnixPex::from(0o755))
            .map_err(remote_error)
    }

    fn delete_entry(&mut self, path: &LocationPath) -> Result<()> {
        let path = self.remote_path(path)?;
        self.client
            .remove_dir_all(Path::new(path))
            .map_err(remote_error)
    }

    fn copy_file_to_writer(&mut self, path: &LocationPath, writer: &mut dyn Write) -> Result<u64> {
        let path = self.remote_path(path)?;
        let mut stream = self.client.open(Path::new(path)).map_err(remote_error)?;
        let bytes = io::copy(&mut stream, writer).context("failed to stream remote file")?;
        self.client.on_read(stream).map_err(remote_error)?;
        Ok(bytes)
    }

    fn create_file_from_reader(
        &mut self,
        path: &LocationPath,
        reader: &mut dyn Read,
        size_hint: u64,
    ) -> Result<u64> {
        let path = self.remote_path(path)?;
        let metadata = Metadata::default()
            .size(size_hint)
            .mode(UnixPex::from(0o644));
        let mut stream = self
            .client
            .create(Path::new(path), &metadata)
            .map_err(remote_error)?;
        let bytes = io::copy(reader, &mut stream).context("failed to write remote file")?;
        self.client.on_written(stream).map_err(remote_error)?;
        Ok(bytes)
    }

    fn disconnect(&mut self) -> Result<()> {
        self.client.disconnect().map_err(remote_error)
    }
}

fn copy_via_temp(session: &mut dyn VfsSession, source: &str, destination: &str) -> Result<()> {
    let mut temp = NamedTempFile::new().context("failed to create temporary file")?;
    let size = session.copy_file_to_writer(
        &LocationPath::Remote(source.to_string()),
        temp.as_file_mut(),
    )?;
    temp.as_file_mut()
        .seek(SeekFrom::Start(0))
        .context("failed to rewind temporary file")?;
    session.create_file_from_reader(
        &LocationPath::Remote(destination.to_string()),
        temp.as_file_mut(),
        size,
    )?;
    Ok(())
}

fn read_local_dir(path: &Path) -> Result<Vec<FileEntry>> {
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
        let name = entry.file_name().to_string_lossy().into_owned();
        entries.push(FileEntry {
            path: LocationPath::Local(entry.path()),
            kind,
            is_hidden: name.starts_with('.'),
            name,
        });
    }
    sort_entries(&mut entries);
    Ok(entries)
}

fn local_entry_kind(path: &Path) -> Result<EntryKind> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("failed to inspect {}", path.display()))?;
    let file_type = metadata.file_type();
    Ok(if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::Other
    })
}

fn expect_local(path: &LocationPath) -> Result<&Path> {
    path.as_local_path()
        .ok_or_else(|| anyhow!("local session cannot use remote path"))
}

fn remote_error(err: impl std::fmt::Display) -> anyhow::Error {
    anyhow!(err.to_string())
}

fn dirs_home() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
#[derive(Default, Clone)]
pub struct MockSessionFactory {
    remotes: Arc<Mutex<BTreeMap<String, MockRemoteSpec>>>,
}

#[cfg(test)]
#[derive(Clone)]
struct MockRemoteSpec {
    kind: SourceKind,
    label: String,
    root: PathBuf,
}

#[cfg(test)]
impl MockSessionFactory {
    pub fn add_remote(
        &self,
        source: SourceRef,
        kind: SourceKind,
        label: impl Into<String>,
        root: PathBuf,
    ) {
        self.remotes.lock().expect("lock").insert(
            source.stable_key(),
            MockRemoteSpec {
                kind,
                label: label.into(),
                root,
            },
        );
    }
}

#[cfg(test)]
impl SessionFactory for MockSessionFactory {
    fn connect(
        &self,
        config: &Config,
        _secrets: &dyn SecretStore,
        source: &SourceRef,
    ) -> Result<ConnectedSource> {
        match source {
            SourceRef::InlineLocal { path, label } => Ok(ConnectedSource {
                kind: SourceKind::Local,
                label: label.clone(),
                source_ref: source.clone(),
                session: Box::new(LocalSession::new(label.clone(), path.clone())?),
                default_path: LocationPath::Local(path.clone()),
            }),
            SourceRef::SavedLocal { id } => {
                let profile = config
                    .sources
                    .local
                    .get(id)
                    .with_context(|| format!("unknown local source profile: {id}"))?;
                connect_local_profile(id, profile, source.clone())
            }
            _ => {
                let spec = self
                    .remotes
                    .lock()
                    .expect("lock")
                    .get(&source.stable_key())
                    .cloned()
                    .with_context(|| format!("missing mock remote for {}", source.stable_key()))?;
                Ok(ConnectedSource {
                    kind: spec.kind,
                    label: spec.label.clone(),
                    source_ref: source.clone(),
                    session: Box::new(MockRemoteSession {
                        kind: spec.kind,
                        label: spec.label.clone(),
                        root: spec.root.clone(),
                        cwd: "/".to_string(),
                        disconnected: false,
                    }),
                    default_path: LocationPath::Remote("/".to_string()),
                })
            }
        }
    }
}

#[cfg(test)]
struct MockRemoteSession {
    kind: SourceKind,
    label: String,
    root: PathBuf,
    cwd: String,
    disconnected: bool,
}

#[cfg(test)]
impl MockRemoteSession {
    fn resolve(&self, path: &LocationPath) -> Result<PathBuf> {
        let remote = path
            .as_remote_path()
            .ok_or_else(|| anyhow!("expected remote path"))?;
        let normalized = normalize_remote(remote);
        let relative = normalized.trim_start_matches('/');
        Ok(if relative.is_empty() {
            self.root.clone()
        } else {
            self.root.join(relative)
        })
    }
}

#[cfg(test)]
impl VfsSession for MockRemoteSession {
    fn source_kind(&self) -> SourceKind {
        self.kind
    }

    fn source_label(&self) -> &str {
        &self.label
    }

    fn pwd(&mut self) -> Result<LocationPath> {
        Ok(LocationPath::Remote(self.cwd.clone()))
    }

    fn change_dir(&mut self, path: &LocationPath) -> Result<LocationPath> {
        let local = self.resolve(path)?;
        if !local.is_dir() {
            bail!("not a directory: {}", path.display());
        }
        self.cwd = normalize_remote(&path.display());
        Ok(LocationPath::Remote(self.cwd.clone()))
    }

    fn list_dir(&mut self, path: &LocationPath) -> Result<Vec<FileEntry>> {
        let local = self.resolve(path)?;
        let base = normalize_remote(&path.display());
        let read_dir =
            fs::read_dir(&local).with_context(|| format!("failed to read {}", local.display()))?;
        let mut entries = Vec::new();
        for entry in read_dir {
            let entry = entry.with_context(|| format!("failed to read {}", local.display()))?;
            let metadata = entry.file_type().expect("file type");
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = if metadata.is_dir() {
                EntryKind::Directory
            } else if metadata.is_file() {
                EntryKind::File
            } else if metadata.is_symlink() {
                EntryKind::Symlink
            } else {
                EntryKind::Other
            };
            entries.push(FileEntry {
                name: name.clone(),
                path: LocationPath::Remote(normalize_remote(&format!("{base}/{name}"))),
                kind,
                is_hidden: name.starts_with('.'),
            });
        }
        sort_entries(&mut entries);
        Ok(entries)
    }

    fn entry_kind(&mut self, path: &LocationPath) -> Result<EntryKind> {
        local_entry_kind(&self.resolve(path)?)
    }

    fn exists(&mut self, path: &LocationPath) -> Result<bool> {
        Ok(self.resolve(path)?.exists())
    }

    fn read_text_file(&mut self, path: &LocationPath) -> Result<String> {
        fs::read_to_string(self.resolve(path)?).context("failed to read mock remote file")
    }

    fn copy_file_within_source(
        &mut self,
        source: &LocationPath,
        destination: &LocationPath,
    ) -> Result<()> {
        let source = self.resolve(source)?;
        let destination = self.resolve(destination)?;
        fs::copy(&source, &destination).with_context(|| {
            format!(
                "failed to copy {} to {}",
                source.display(),
                destination.display()
            )
        })?;
        Ok(())
    }

    fn move_entry_within_source(
        &mut self,
        source: &LocationPath,
        destination: &LocationPath,
    ) -> Result<()> {
        fs::rename(self.resolve(source)?, self.resolve(destination)?).context("failed to move")
    }

    fn create_dir(&mut self, path: &LocationPath) -> Result<()> {
        fs::create_dir(self.resolve(path)?).context("failed to create directory")
    }

    fn delete_entry(&mut self, path: &LocationPath) -> Result<()> {
        let local = self.resolve(path)?;
        let metadata =
            fs::symlink_metadata(&local).context("failed to inspect mock remote path")?;
        if metadata.is_dir() {
            fs::remove_dir_all(local).context("failed to delete mock remote directory")
        } else {
            fs::remove_file(local).context("failed to delete mock remote file")
        }
    }

    fn copy_file_to_writer(&mut self, path: &LocationPath, writer: &mut dyn Write) -> Result<u64> {
        let mut file = fs::File::open(self.resolve(path)?).context("failed to open mock file")?;
        io::copy(&mut file, writer).context("failed to copy mock file")
    }

    fn create_file_from_reader(
        &mut self,
        path: &LocationPath,
        reader: &mut dyn Read,
        _size_hint: u64,
    ) -> Result<u64> {
        let mut file =
            fs::File::create(self.resolve(path)?).context("failed to create mock file")?;
        io::copy(reader, &mut file).context("failed to write mock file")
    }

    fn disconnect(&mut self) -> Result<()> {
        self.disconnected = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::source::SourceRef;
    use crate::vfs::{LocalSession, VfsSession};

    #[test]
    fn local_session_sorts_directories_before_files() {
        let temp = TempDir::new().expect("temp dir");
        fs::create_dir(temp.path().join("Zoo")).expect("dir");
        fs::create_dir(temp.path().join("alpha")).expect("dir");
        fs::write(temp.path().join("Beta.txt"), b"b").expect("file");
        fs::write(temp.path().join("aardvark.txt"), b"a").expect("file");

        let mut session =
            LocalSession::new("Tmp".to_string(), temp.path().to_path_buf()).expect("session");
        let entries = session
            .list_dir(&crate::source::LocationPath::Local(
                temp.path().to_path_buf(),
            ))
            .expect("list");
        let names: Vec<_> = entries.into_iter().map(|entry| entry.name).collect();

        assert_eq!(names, vec!["alpha", "Zoo", "aardvark.txt", "Beta.txt"]);
    }

    #[test]
    fn stable_keys_match_saved_profiles() {
        assert_eq!(
            SourceRef::SavedFtp {
                id: "archive".to_string()
            }
            .stable_key(),
            "ftp:archive"
        );
    }
}
