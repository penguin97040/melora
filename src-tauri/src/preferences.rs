use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_PREFERENCES_BYTES: u64 = 4096;
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

// Exclusive creation never follows or truncates a pre-existing file or symlink.
// Keep temporary files in the destination directory for an atomic rename.
struct TemporaryPreferences(PathBuf);

impl TemporaryPreferences {
    fn create(directory: &Path) -> std::io::Result<(Self, File)> {
        for _ in 0..128 {
            let path = directory.join(format!(
                "preferences-{}-{}.tmp",
                std::process::id(),
                NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok((Self(path), file)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Temporary preferences unavailable",
        ))
    }
}

impl Drop for TemporaryPreferences {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn read_preferences(path: &Path) -> std::io::Result<Vec<u8>> {
    // Reject links and special files before opening, including FIFOs that could
    // otherwise block startup. Recheck the opened handle before reading.
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid preferences file",
        ));
    }
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid preferences file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_PREFERENCES_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PREFERENCES_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Preferences file too large",
        ));
    }
    Ok(bytes)
}

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
        match read_preferences(&store.directory.join("preferences.json")) {
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
        let (_temporary, mut file) = TemporaryPreferences::create(&self.directory)?;
        file.write_all(b"{}")?;
        file.sync_all()
    }

    pub fn save(&mut self, value: Preferences) -> Result<Preferences, String> {
        value.validate()?;
        fs::create_dir_all(&self.directory).map_err(|_| Self::write_error())?;
        let bytes =
            serde_json::to_vec_pretty(&value).map_err(|_| "Preferences could not be encoded.")?;
        let (temporary, mut file) =
            TemporaryPreferences::create(&self.directory).map_err(|_| Self::write_error())?;
        file.write_all(&bytes).map_err(|_| Self::write_error())?;
        file.sync_all().map_err(|_| Self::write_error())?;
        // Close before replacing the destination, including on Windows.
        drop(file);
        fs::rename(&temporary.0, self.directory.join("preferences.json"))
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
    fn planted_temporary_symlinks_never_overwrite_their_target() {
        let path = temp_path("symlink");
        fs::create_dir_all(&path).unwrap();
        let target = path.join("untouched.txt");
        fs::write(&target, b"synthetic sentinel").unwrap();
        std::os::unix::fs::symlink(&target, path.join("preferences.tmp")).unwrap();
        let first = NEXT_TEMPORARY.load(Ordering::Relaxed);
        for sequence in first..first + 256 {
            std::os::unix::fs::symlink(
                &target,
                path.join(format!(
                    "preferences-{}-{}.tmp",
                    std::process::id(),
                    sequence
                )),
            )
            .unwrap();
        }
        let mut store = Store::open(path.clone());
        let _ = store.save(Preferences::default());
        assert_eq!(fs::read(&target).unwrap(), b"synthetic sentinel");
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
