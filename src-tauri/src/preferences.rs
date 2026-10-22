use ora_tauri::core::fsio::{read_bounded, write_atomic, Temporary};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

const MAX_PREFERENCES_BYTES: u64 = 4096;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub scale: u16,
    pub comfortable: bool,
    pub volume: u8,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            scale: 100,
            comfortable: false,
            volume: 70,
        }
    }
}

impl Preferences {
    pub fn validate(&self) -> Result<(), String> {
        if ![100, 125, 150, 175, 200].contains(&self.scale) || self.volume > 100 {
            return Err("Choose a supported display scale and volume between 0 and 100.".into());
        }
        Ok(())
    }
}

pub struct Store {
    pub directory: PathBuf,
    pub preferences: Preferences,
    pub error: Option<String>,
}

impl Store {
    pub fn open(directory: PathBuf) -> Self {
        let mut store = Self {
            directory,
            preferences: Preferences::default(),
            error: None,
        };
        if fs::create_dir_all(&store.directory).is_err() {
            store.error = Some(Self::write_error());
            return store;
        }
        match read_bounded(&store.directory.join("preferences.json"), MAX_PREFERENCES_BYTES) {
            Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
                Ok(value) if value.validate().is_ok() => store.preferences = value,
                _ => store.error = Some("Preferences could not be read. Choose your settings again to replace the invalid file.".into()),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => store.error = Some("Preferences could not be read. Check access to the settings folder.".into()),
        }
        // Probe writability without rewriting valid settings on every launch.
        if store.error.is_none() && store.check_writable().is_err() {
            store.error = Some(Self::write_error());
        }
        store
    }

    fn write_error() -> String {
        "Preferences cannot be saved. On Windows, extract Melora to a writable folder so it can create data beside Melora.exe. On Linux or macOS, check access to the application data folder.".into()
    }

    fn check_writable(&self) -> std::io::Result<()> {
        // The probe file is removed when it drops.
        Temporary::create(&self.directory, "preferences")?.write_synced(b"{}")
    }

    pub fn save(&mut self, value: Preferences) -> Result<Preferences, String> {
        value.validate()?;
        let bytes =
            serde_json::to_vec_pretty(&value).map_err(|_| "Preferences could not be encoded.")?;
        write_atomic(&self.directory, "preferences.json", &bytes)
            .map_err(|_| Self::write_error())?;
        self.preferences = value.clone();
        self.error = None;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "melora-test-{}-{}-{}",
            label,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn settings_survive_reopen_and_invalid_input_does_not_replace_them() {
        let path = temp_path("persist");
        let mut store = Store::open(path.clone());
        let value = Preferences {
            scale: 150,
            comfortable: true,
            volume: 25,
        };
        store.save(value.clone()).unwrap();
        assert_eq!(Store::open(path.clone()).preferences, value);
        assert!(store
            .save(Preferences {
                scale: 101,
                ..value.clone()
            })
            .is_err());
        assert_eq!(Store::open(path.clone()).preferences, value);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn blocked_directory_is_reported_without_silent_fallback() {
        let path = temp_path("blocked");
        fs::write(&path, "blocking file").unwrap();
        let store = Store::open(path.clone());
        assert!(store.error.is_some());
        assert!(!path.join("preferences.json").exists());
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn corrupt_settings_are_preserved_until_explicit_save() {
        let path = temp_path("corrupt");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("preferences.json"), "invalid").unwrap();
        let store = Store::open(path.clone());
        assert!(store.error.is_some());
        assert_eq!(
            fs::read_to_string(path.join("preferences.json")).unwrap(),
            "invalid"
        );
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn opening_valid_preferences_does_not_rewrite_them() {
        let path = temp_path("unchanged");
        fs::create_dir_all(&path).unwrap();
        let bytes = br#"{"scale":125,"comfortable":false,"volume":20}"#;
        fs::write(path.join("preferences.json"), bytes).unwrap();
        let store = Store::open(path.clone());
        assert!(store.error.is_none());
        assert_eq!(store.preferences.scale, 125);
        assert_eq!(fs::read(path.join("preferences.json")).unwrap(), bytes);
        assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn oversized_settings_are_preserved_and_can_be_replaced_explicitly() {
        let path = temp_path("large");
        fs::create_dir_all(&path).unwrap();
        let bytes = vec![b' '; MAX_PREFERENCES_BYTES as usize + 1];
        fs::write(path.join("preferences.json"), &bytes).unwrap();
        let mut store = Store::open(path.clone());
        assert!(store.error.is_some());
        assert_eq!(fs::read(path.join("preferences.json")).unwrap(), bytes);
        store.save(Preferences::default()).unwrap();
        assert!(Store::open(path.clone()).error.is_none());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn failed_replacement_preserves_memory_and_removes_temporary_file() {
        let path = temp_path("replace-failure");
        let mut store = Store::open(path.clone());
        fs::create_dir(path.join("preferences.json")).unwrap();
        let next = Preferences {
            volume: 25,
            ..Preferences::default()
        };
        assert!(store.save(next).is_err());
        assert_eq!(store.preferences, Preferences::default());
        assert_eq!(fs::read_dir(&path).unwrap().count(), 1);
        fs::remove_dir_all(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn linked_settings_are_not_read_and_explicit_save_replaces_only_the_link() {
        let path = temp_path("linked-settings");
        fs::create_dir_all(&path).unwrap();
        let target = path.join("untouched.json");
        let bytes = br#"{"scale":200,"comfortable":true,"volume":20}"#;
        fs::write(&target, bytes).unwrap();
        std::os::unix::fs::symlink(&target, path.join("preferences.json")).unwrap();
        let mut store = Store::open(path.clone());
        assert!(store.error.is_some());
        assert_eq!(store.preferences, Preferences::default());
        store.save(Preferences::default()).unwrap();
        assert_eq!(fs::read(target).unwrap(), bytes);
        assert!(!fs::symlink_metadata(path.join("preferences.json"))
            .unwrap()
            .is_symlink());
        fs::remove_dir_all(path).unwrap();
    }
}
