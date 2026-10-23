//! Hardened reads and writes for the small settings files in the app's settings folder
//! (`spotify.json`, Melora's playlist order and the app's own preferences). One copy, so a fix
//! reaches every file.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Reads at most `max_bytes`. Rejects links and special files (a FIFO could block startup),
/// then re-checks the open handle.
pub fn read_bounded(path: &Path, max_bytes: u64) -> std::io::Result<Vec<u8>> {
    let invalid = || std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid settings file");
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(invalid());
    }
    let file = File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(max_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(invalid());
    }
    Ok(bytes)
}

/// A temporary file beside the destination, removed on drop unless it was renamed into place.
/// Exclusive creation never follows or truncates an existing file or link.
pub struct Temporary {
    path: PathBuf,
    file: Option<File>,
}

impl Temporary {
    pub fn create(directory: &Path, name: &str) -> std::io::Result<Self> {
        for _ in 0..128 {
            let path = directory.join(format!(
                "{name}-{}-{}.tmp",
                std::process::id(),
                NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                    })
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Temporary settings file unavailable",
        ))
    }

    /// Writes and syncs the whole content.
    pub fn write_synced(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| std::io::Error::other("Temporary settings file closed"))?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    /// Closes the file (needed on Windows) and atomically replaces `destination` with it.
    pub fn replace(mut self, destination: &Path) -> std::io::Result<()> {
        drop(self.file.take());
        fs::rename(&self.path, destination)
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        drop(self.file.take());
        // After a successful rename the path no longer exists, so this is a no-op.
        let _ = fs::remove_file(&self.path);
    }
}

/// Creates `directory` if needed and atomically replaces `directory/name` with `bytes`.
pub fn write_atomic(directory: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    fs::create_dir_all(directory)?;
    let mut temporary = Temporary::create(directory, name)?;
    temporary.write_synced(bytes)?;
    temporary.replace(&directory.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ora-fsio-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn leftovers(directory: &Path) -> Vec<String> {
        fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect()
    }

    #[test]
    fn writes_replace_atomically_and_leave_no_temporary_files() {
        let directory = temp_path("write");
        write_atomic(&directory, "settings.json", b"one").unwrap();
        write_atomic(&directory, "settings.json", b"two").unwrap();
        assert_eq!(
            read_bounded(&directory.join("settings.json"), 16).unwrap(),
            b"two"
        );
        assert!(leftovers(&directory).is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_dropped_temporary_file_is_removed() {
        let directory = temp_path("drop");
        fs::create_dir_all(&directory).unwrap();
        let mut temporary = Temporary::create(&directory, "probe").unwrap();
        temporary.write_synced(b"{}").unwrap();
        assert_eq!(leftovers(&directory).len(), 1);
        drop(temporary);
        assert!(leftovers(&directory).is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn oversized_and_special_files_are_rejected() {
        let directory = temp_path("read");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("settings.json");
        fs::write(&path, vec![b'x'; 17]).unwrap();
        assert_eq!(
            read_bounded(&path, 16).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(
            read_bounded(&directory, 16).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        #[cfg(unix)]
        {
            let link = directory.join("link.json");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            assert_eq!(
                read_bounded(&link, 64).unwrap_err().kind(),
                std::io::ErrorKind::InvalidData
            );
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn planted_temporary_symlinks_never_overwrite_their_target() {
        let directory = temp_path("planted");
        fs::create_dir_all(&directory).unwrap();
        let target = directory.join("untouched.txt");
        fs::write(&target, b"synthetic sentinel").unwrap();
        let first = NEXT_TEMPORARY.load(Ordering::Relaxed);
        for sequence in first..first + 256 {
            std::os::unix::fs::symlink(
                &target,
                directory.join(format!(
                    "settings.json-{}-{sequence}.tmp",
                    std::process::id()
                )),
            )
            .unwrap();
        }
        let _ = write_atomic(&directory, "settings.json", b"replacement");
        assert_eq!(fs::read(&target).unwrap(), b"synthetic sentinel");
        fs::remove_dir_all(directory).unwrap();
    }
}
