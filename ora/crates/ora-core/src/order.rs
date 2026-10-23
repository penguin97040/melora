//! Melora's own order for the sidebar's playlists, kept beside `spotify.json` and separate from
//! Spotify's order (which apps can't change).
//!
//! Rules:
//! - One list of playlist IDs per account (its Spotify ID); playlists not in it keep Spotify's
//!   order after the ones that are.
//! - A missing or unreadable file is an empty order; saving replaces it whole, atomically.
//! - The file is bounded: at most `MAX_ACCOUNTS` accounts of `MAX_PLAYLISTS` IDs each.

use crate::{
    browser::Browser,
    fsio::{read_bounded, write_atomic},
    http::HttpClient,
    link::is_base62_id,
    store::CredentialStore,
    Ora,
};
use ora_types::{Feature, OraError};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

const FILE_NAME: &str = "playlist-order.json";
/// More than Spotify lists for most accounts; IDs past this aren't kept.
pub const MAX_PLAYLISTS: usize = 5_000;
/// Development Mode allows five accounts; a few more leaves room for changes.
const MAX_ACCOUNTS: usize = 8;
const MAX_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderFile {
    accounts: BTreeMap<String, Vec<String>>,
}

pub struct OrderStore {
    directory: PathBuf,
}

impl OrderStore {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn read(&self) -> Result<OrderFile, OraError> {
        match read_bounded(&self.directory.join(FILE_NAME), MAX_BYTES) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(OrderFile::default()),
            // Too large counts as unreadable, so the next save replaces it.
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                Ok(OrderFile::default())
            }
            Err(_) => Err(OraError::SettingsUnwritable),
        }
    }

    /// The account's saved order, valid IDs only.
    pub fn load(&self, account: &str) -> Result<Vec<String>, OraError> {
        let mut file = self.read()?;
        Ok(clean(file.accounts.remove(account).unwrap_or_default()))
    }

    /// Replaces the account's order; an empty list forgets it (back to Spotify's order).
    pub fn save(&self, account: &str, ids: &[String]) -> Result<(), OraError> {
        let mut file = self.read()?;
        let ids = clean(ids.to_vec());
        if ids.is_empty() {
            if file.accounts.remove(account).is_none() {
                return Ok(());
            }
        } else {
            if !file.accounts.contains_key(account) {
                while file.accounts.len() >= MAX_ACCOUNTS {
                    file.accounts.pop_first();
                }
            }
            file.accounts.insert(account.to_owned(), ids);
        }
        let bytes = serde_json::to_vec_pretty(&file).map_err(|_| OraError::SettingsUnwritable)?;
        write_atomic(&self.directory, FILE_NAME, &bytes).map_err(|_| OraError::SettingsUnwritable)
    }
}

/// Valid playlist IDs, each once, at most `MAX_PLAYLISTS`.
fn clean(ids: Vec<String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::with_capacity(ids.len().min(MAX_PLAYLISTS));
    for id in ids {
        if kept.len() == MAX_PLAYLISTS {
            break;
        }
        if is_base62_id(&id) && !kept.contains(&id) {
            kept.push(id);
        }
    }
    kept
}

impl<H: HttpClient, S: CredentialStore, B: Browser> Ora<H, S, B> {
    /// Melora's order for the account's playlists (IDs first to last); empty means Spotify's.
    pub fn playlist_order(&self) -> Result<Vec<String>, OraError> {
        self.require(Feature::Playlists)?;
        match self.account() {
            Some(account) => self.order_store().load(&account),
            None => Ok(Vec::new()),
        }
    }

    /// Saves Melora's order for the account's playlists; an empty list goes back to Spotify's.
    pub fn set_playlist_order(&self, ids: &[String]) -> Result<(), OraError> {
        self.require(Feature::Playlists)?;
        if ids.len() > MAX_PLAYLISTS {
            return Err(OraError::Unsupported);
        }
        let account = self.account().ok_or(OraError::Unsupported)?;
        self.order_store().save(&account, ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    const A: &str = "PlAyLiSt0123456789abcd";
    const B: &str = "OtHeRs0123456789abcdef";

    fn temp_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ora-order-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn each_account_keeps_its_own_order() {
        let path = temp_path("accounts");
        let store = OrderStore::new(path.clone());
        assert_eq!(store.load("user1"), Ok(Vec::new()));
        store
            .save("user1", &[B.into(), A.into(), "bad".into(), B.into()])
            .unwrap();
        store.save("user2", &[A.into()]).unwrap();
        assert_eq!(store.load("user1"), Ok(vec![B.to_owned(), A.to_owned()]));
        assert_eq!(store.load("user2"), Ok(vec![A.to_owned()]));
        // An empty order forgets the account.
        store.save("user1", &[]).unwrap();
        assert_eq!(store.load("user1"), Ok(Vec::new()));
        assert_eq!(store.load("user2"), Ok(vec![A.to_owned()]));
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn unreadable_files_are_an_empty_order_and_get_replaced() {
        let path = temp_path("invalid");
        fs::create_dir_all(&path).unwrap();
        let store = OrderStore::new(path.clone());
        fs::write(path.join(FILE_NAME), r#"{"accounts":{},"extra":1}"#).unwrap();
        assert_eq!(store.load("user1"), Ok(Vec::new()));
        store.save("user1", &[A.into()]).unwrap();
        assert_eq!(store.load("user1"), Ok(vec![A.to_owned()]));
        fs::write(path.join(FILE_NAME), vec![b' '; MAX_BYTES as usize + 1]).unwrap();
        assert_eq!(store.load("user1"), Ok(Vec::new()));
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn the_file_stays_bounded() {
        let path = temp_path("bounded");
        let store = OrderStore::new(path.clone());
        for n in 0..MAX_ACCOUNTS + 3 {
            store.save(&format!("user{n:02}"), &[A.into()]).unwrap();
        }
        let saved: OrderFile =
            serde_json::from_slice(&fs::read(path.join(FILE_NAME)).unwrap()).unwrap();
        assert_eq!(saved.accounts.len(), MAX_ACCOUNTS);
        let many: Vec<String> = (0..MAX_PLAYLISTS + 10)
            .map(|n| format!("{n:0>22}"))
            .collect();
        store.save("user99", &many).unwrap();
        assert_eq!(store.load("user99").unwrap().len(), MAX_PLAYLISTS);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn an_unwritable_folder_is_an_error() {
        let path = temp_path("blocked");
        fs::write(&path, "blocking file").unwrap();
        let store = OrderStore::new(path.join("inner"));
        assert_eq!(
            store.save("user1", &[A.into()]),
            Err(OraError::SettingsUnwritable)
        );
        fs::remove_file(path).unwrap();
    }
}
