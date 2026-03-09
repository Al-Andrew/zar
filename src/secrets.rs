use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub trait SecretStore {
    fn ftp_password(&self, id: &str) -> Option<&str>;
    fn smb_password(&self, id: &str) -> Option<&str>;
    fn ssh_password(&self, id: &str) -> Option<&str>;
    fn ssh_key_passphrase(&self, id: &str) -> Option<&str>;
    fn set_ftp_password(&mut self, id: &str, password: Option<String>) -> Result<()>;
    fn set_smb_password(&mut self, id: &str, password: Option<String>) -> Result<()>;
    fn set_ssh_password(&mut self, id: &str, password: Option<String>) -> Result<()>;
    fn set_ssh_key_passphrase(&mut self, id: &str, passphrase: Option<String>) -> Result<()>;
    fn save_to_dir(&self, dir: &Path) -> Result<()>;
}

#[derive(Debug, Default, Clone)]
pub struct PlaintextSecretStore {
    ftp: BTreeMap<String, FtpSecret>,
    smb: BTreeMap<String, SmbSecret>,
    ssh: BTreeMap<String, SshSecret>,
}

impl PlaintextSecretStore {
    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        Self::load_from_path(&dir.join("secrets.toml"))
    }

    pub fn load_from_path(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let file: SecretsFile =
            toml::from_str(&contents).context("failed to parse secrets.toml")?;
        Ok(Self {
            ftp: file.secrets.ftp,
            smb: file.secrets.smb,
            ssh: file.secrets.ssh,
        })
    }

    fn remove_empty_ssh_entry(&mut self, id: &str) {
        let should_remove = self
            .ssh
            .get(id)
            .is_some_and(|secret| secret.password.is_none() && secret.key_passphrase.is_none());
        if should_remove {
            self.ssh.remove(id);
        }
    }
}

impl SecretStore for PlaintextSecretStore {
    fn ftp_password(&self, id: &str) -> Option<&str> {
        self.ftp
            .get(id)
            .and_then(|secret| secret.password.as_deref())
    }

    fn smb_password(&self, id: &str) -> Option<&str> {
        self.smb
            .get(id)
            .and_then(|secret| secret.password.as_deref())
    }

    fn ssh_password(&self, id: &str) -> Option<&str> {
        self.ssh
            .get(id)
            .and_then(|secret| secret.password.as_deref())
    }

    fn ssh_key_passphrase(&self, id: &str) -> Option<&str> {
        self.ssh
            .get(id)
            .and_then(|secret| secret.key_passphrase.as_deref())
    }

    fn set_ftp_password(&mut self, id: &str, password: Option<String>) -> Result<()> {
        match password {
            Some(password) if !password.is_empty() => {
                self.ftp.insert(
                    id.to_string(),
                    FtpSecret {
                        password: Some(password),
                    },
                );
            }
            _ => {
                self.ftp.remove(id);
            }
        }
        Ok(())
    }

    fn set_smb_password(&mut self, id: &str, password: Option<String>) -> Result<()> {
        match password {
            Some(password) if !password.is_empty() => {
                self.smb.insert(
                    id.to_string(),
                    SmbSecret {
                        password: Some(password),
                    },
                );
            }
            _ => {
                self.smb.remove(id);
            }
        }
        Ok(())
    }

    fn set_ssh_password(&mut self, id: &str, password: Option<String>) -> Result<()> {
        if let Some(secret) = self.ssh.get_mut(id) {
            secret.password = password.filter(|value| !value.is_empty());
        } else if let Some(password) = password.filter(|value| !value.is_empty()) {
            self.ssh.insert(
                id.to_string(),
                SshSecret {
                    password: Some(password),
                    key_passphrase: None,
                },
            );
        }
        self.remove_empty_ssh_entry(id);
        Ok(())
    }

    fn set_ssh_key_passphrase(&mut self, id: &str, passphrase: Option<String>) -> Result<()> {
        if let Some(secret) = self.ssh.get_mut(id) {
            secret.key_passphrase = passphrase.filter(|value| !value.is_empty());
        } else if let Some(passphrase) = passphrase.filter(|value| !value.is_empty()) {
            self.ssh.insert(
                id.to_string(),
                SshSecret {
                    password: None,
                    key_passphrase: Some(passphrase),
                },
            );
        }
        self.remove_empty_ssh_entry(id);
        Ok(())
    }

    fn save_to_dir(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
        let path = dir.join("secrets.toml");
        let body = toml::to_string_pretty(&SecretsFile {
            secrets: SecretsConfig {
                ftp: self.ftp.clone(),
                smb: self.smb.clone(),
                ssh: self.ssh.clone(),
            },
        })
        .context("failed to serialize secrets.toml")?;
        fs::write(&path, body).with_context(|| format!("failed to write {}", path.display()))
    }
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct SecretsFile {
    #[serde(default)]
    secrets: SecretsConfig,
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct SecretsConfig {
    #[serde(default)]
    ftp: BTreeMap<String, FtpSecret>,
    #[serde(default)]
    smb: BTreeMap<String, SmbSecret>,
    #[serde(default)]
    ssh: BTreeMap<String, SshSecret>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
struct FtpSecret {
    #[serde(default)]
    password: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
struct SmbSecret {
    #[serde(default)]
    password: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
struct SshSecret {
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    key_passphrase: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::secrets::{PlaintextSecretStore, SecretStore};

    #[test]
    fn missing_secrets_file_is_empty() {
        let temp = TempDir::new().expect("temp dir");
        let store =
            PlaintextSecretStore::load_from_path(&temp.path().join("missing.toml")).expect("store");

        assert_eq!(store.ftp_password("archive"), None);
    }

    #[test]
    fn parses_plaintext_secrets() {
        let temp = TempDir::new().expect("temp dir");
        let path = temp.path().join("secrets.toml");
        fs::write(
            &path,
            r#"
[secrets.ftp.archive]
password = "ftp-pass"

[secrets.smb.media]
password = "smb-pass"

[secrets.ssh.prod]
password = "ssh-pass"
key_passphrase = "key-pass"
"#,
        )
        .expect("write secrets");

        let store = PlaintextSecretStore::load_from_path(&path).expect("store");
        assert_eq!(store.ftp_password("archive"), Some("ftp-pass"));
        assert_eq!(store.smb_password("media"), Some("smb-pass"));
        assert_eq!(store.ssh_password("prod"), Some("ssh-pass"));
        assert_eq!(store.ssh_key_passphrase("prod"), Some("key-pass"));
    }

    #[test]
    fn updates_and_persists_plaintext_secrets() {
        let temp = TempDir::new().expect("temp dir");
        let mut store = PlaintextSecretStore::default();

        store
            .set_ftp_password("archive", Some("ftp-pass".to_string()))
            .expect("set ftp password");
        store
            .set_ssh_password("prod", Some("ssh-pass".to_string()))
            .expect("set ssh password");
        store.save_to_dir(temp.path()).expect("save secrets");

        let reloaded = PlaintextSecretStore::load_from_dir(temp.path()).expect("reload");
        assert_eq!(reloaded.ftp_password("archive"), Some("ftp-pass"));
        assert_eq!(reloaded.ssh_password("prod"), Some("ssh-pass"));
    }
}
