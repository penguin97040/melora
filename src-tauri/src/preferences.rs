use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

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
        match fs::read(store.directory.join("preferences.json")) {
            Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
                Ok(value) if value.validate().is_ok() => store.preferences = value,
                _ => store.error = Some("Preferences could not be read. Choose your settings again to replace the invalid file.".into()),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => store.error = Some("Preferences could not be read. Check access to the settings folder.".into()),
        }
        // Verify actual writing on first launch; permissions metadata is insufficient.
        if store.error.is_none() {
            let value = store.preferences.clone();
            if let Err(error) = store.save(value) {
                store.error = Some(error);
            }
        }
        store
    }

    fn write_error() -> String {
        "Preferences cannot be saved. On Windows, extract Melora to a writable folder so it can create data beside Melora.exe. On Linux or macOS, check access to the application data folder.".into()
    }

    pub fn save(&mut self, value: Preferences) -> Result<Preferences, String> {
        value.validate()?;
        fs::create_dir_all(&self.directory).map_err(|_| Self::write_error())?;
        let bytes =
            serde_json::to_vec_pretty(&value).map_err(|_| "Preferences could not be encoded.")?;
        let temporary = self.directory.join("preferences.tmp");
        fs::write(&temporary, bytes).map_err(|_| Self::write_error())?;
        fs::rename(&temporary, self.directory.join("preferences.json"))
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
}
