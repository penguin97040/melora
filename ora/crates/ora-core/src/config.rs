//! The Spotify Client ID, kept in `spotify.json` in the client's settings folder. It isn't a
//! secret, but it is the owner's own app, so it never enters a repository.

use crate::fsio::{read_bounded, write_atomic};
use ora_types::OraError;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const FILE_NAME: &str = "spotify.json";
const MAX_BYTES: u64 = 4096;

#[derive(Clone, PartialEq, Eq)]
pub struct ClientId(String);

impl ClientId {
    /// Spotify Client IDs are 32 hexadecimal characters; accept 20 to 64 letters and digits.
    pub fn parse(value: &str) -> Result<Self, OraError> {
        let value = value.trim();
        if (20..=64).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_alphanumeric()) {
            Ok(Self(value.to_owned()))
        } else {
            Err(OraError::InvalidClientId)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Enough to recognise the ID without showing all of it.
    pub fn hint(&self) -> String {
        format!("{}…{}", &self.0[..4], &self.0[self.0.len() - 4..])
    }
}

impl std::fmt::Debug for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ClientId({})", self.hint())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigFile {
    client_id: String,
}

#[derive(Debug, Default)]
pub struct LoadedConfig {
    pub client_id: Option<ClientId>,
    pub from_environment: bool,
    pub error: Option<OraError>,
}

pub struct ConfigStore {
    directory: PathBuf,
    environment_variable: &'static str,
}

impl ConfigStore {
    pub fn new(directory: PathBuf, environment_variable: &'static str) -> Self {
        Self {
            directory,
            environment_variable,
        }
    }

    /// A valid development override wins over the file. A missing file means "not set up".
    pub fn load(&self) -> LoadedConfig {
        if let Ok(value) = std::env::var(self.environment_variable) {
            if !value.trim().is_empty() {
                return match ClientId::parse(&value) {
                    Ok(id) => LoadedConfig {
                        client_id: Some(id),
                        from_environment: true,
                        error: None,
                    },
                    Err(error) => LoadedConfig {
                        error: Some(error),
                        from_environment: true,
                        ..LoadedConfig::default()
                    },
                };
            }
        }
        match read_bounded(&self.directory.join(FILE_NAME), MAX_BYTES) {
            Ok(bytes) => match serde_json::from_slice::<ConfigFile>(&bytes)
                .ok()
                .map(|file| ClientId::parse(&file.client_id))
            {
                Some(Ok(id)) => LoadedConfig {
                    client_id: Some(id),
                    ..LoadedConfig::default()
                },
                _ => LoadedConfig {
                    error: Some(OraError::InvalidClientId),
                    ..LoadedConfig::default()
                },
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => LoadedConfig::default(),
            Err(_) => LoadedConfig {
                error: Some(OraError::SettingsUnwritable),
                ..LoadedConfig::default()
            },
        }
    }

    pub fn save(&self, id: &ClientId) -> Result<(), OraError> {
        let bytes = serde_json::to_vec_pretty(&ConfigFile {
            client_id: id.as_str().to_owned(),
        })
        .map_err(|_| OraError::SettingsUnwritable)?;
        write_atomic(&self.directory, FILE_NAME, &bytes).map_err(|_| OraError::SettingsUnwritable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ora-config-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn client_ids_are_validated_loosely() {
        assert!(ClientId::parse(ID).is_ok());
        assert!(ClientId::parse(&format!("  {ID}\n")).is_ok());
        assert!(ClientId::parse("short").is_err());
        assert!(ClientId::parse(&"a".repeat(65)).is_err());
        assert!(ClientId::parse("0123456789abcdef0123456789abcde!").is_err());
        assert_eq!(ClientId::parse(ID).unwrap().hint(), "0123…cdef");
        assert!(!format!("{:?}", ClientId::parse(ID).unwrap()).contains(ID));
    }

    #[test]
    fn saved_id_survives_reload_without_stray_files() {
        let path = temp_path("persist");
        let store = ConfigStore::new(path.clone(), "ORA_TEST_UNSET_VARIABLE");
        assert!(store.load().client_id.is_none());
        store.save(&ClientId::parse(ID).unwrap()).unwrap();
        let loaded = store.load();
        assert_eq!(loaded.client_id.unwrap().as_str(), ID);
        assert!(!loaded.from_environment);
        assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn invalid_or_oversized_files_are_reported_and_left_alone() {
        let path = temp_path("invalid");
        fs::create_dir_all(&path).unwrap();
        let store = ConfigStore::new(path.clone(), "ORA_TEST_UNSET_VARIABLE");
        fs::write(path.join(FILE_NAME), r#"{"clientId":"bad","extra":1}"#).unwrap();
        assert_eq!(store.load().error, Some(OraError::InvalidClientId));
        fs::write(path.join(FILE_NAME), vec![b' '; MAX_BYTES as usize + 1]).unwrap();
        assert_eq!(store.load().error, Some(OraError::SettingsUnwritable));
        assert_eq!(
            fs::metadata(path.join(FILE_NAME)).unwrap().len(),
            MAX_BYTES + 1
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn environment_override_wins_and_is_marked() {
        let path = temp_path("environment");
        let variable = "ORA_TEST_CLIENT_ID_OVERRIDE";
        // Only this test reads or writes this variable.
        std::env::set_var(variable, ID);
        let loaded = ConfigStore::new(path.clone(), variable).load();
        std::env::remove_var(variable);
        assert!(loaded.from_environment);
        assert_eq!(loaded.client_id.unwrap().as_str(), ID);
        assert!(!path.exists());
    }

    #[test]
    fn unwritable_folder_is_an_error_not_a_silent_fallback() {
        let path = temp_path("blocked");
        fs::write(&path, "blocking file").unwrap();
        let store = ConfigStore::new(path.join("inner"), "ORA_TEST_UNSET_VARIABLE");
        assert_eq!(
            store.save(&ClientId::parse(ID).unwrap()),
            Err(OraError::SettingsUnwritable)
        );
        fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn linked_settings_are_not_read() {
        let path = temp_path("linked");
        fs::create_dir_all(&path).unwrap();
        let target = path.join("elsewhere.json");
        fs::write(&target, format!(r#"{{"clientId":"{ID}"}}"#)).unwrap();
        std::os::unix::fs::symlink(&target, path.join(FILE_NAME)).unwrap();
        let loaded = ConfigStore::new(path.clone(), "ORA_TEST_UNSET_VARIABLE").load();
        assert!(loaded.client_id.is_none());
        assert!(loaded.error.is_some());
        fs::remove_dir_all(path).unwrap();
    }
}
