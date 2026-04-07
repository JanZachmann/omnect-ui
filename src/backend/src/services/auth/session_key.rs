//! Session key management service
//!
//! Loads or generates the actix-session signing/encryption key. The key is
//! persisted to disk so that server restarts do not invalidate existing browser
//! session cookies.

use actix_web::cookie::Key;
use log::{info, warn};

/// actix-web's `cookie::Key` requires at least 64 bytes (512 bits).
const SESSION_KEY_LEN: usize = 64;

/// Service for session key management
pub struct SessionKeyService;

impl SessionKeyService {
    /// Return a session key loaded from `path`, or generate and save a new one.
    ///
    /// Falls back to an ephemeral (in-memory only) key when the path is
    /// unreadable for reasons other than the file not existing yet.
    #[must_use]
    pub fn load_or_generate(path: &std::path::Path) -> Key {
        match std::fs::read(path) {
            Ok(bytes) if bytes.len() >= SESSION_KEY_LEN => {
                info!("loaded session key from {}", path.display());
                Key::from(&bytes)
            }
            Ok(_) => {
                warn!(
                    "session key at {} is too short, regenerating",
                    path.display()
                );
                Self::generate_and_save(path)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                info!(
                    "no session key found at {}, generating new key",
                    path.display()
                );
                Self::generate_and_save(path)
            }
            Err(e) => {
                warn!(
                    "failed to read session key from {}: {e:#}, using ephemeral key",
                    path.display()
                );
                Key::generate()
            }
        }
    }

    fn generate_and_save(path: &std::path::Path) -> Key {
        let key = Key::generate();
        if let Err(e) = std::fs::write(path, key.master()) {
            warn!("failed to persist session key to {}: {e:#}", path.display());
        }
        key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_and_saves_key_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.key");

        assert!(!path.exists());
        let _ = SessionKeyService::load_or_generate(&path);
        assert!(path.exists());

        let saved = std::fs::read(&path).unwrap();
        assert_eq!(saved.len(), SESSION_KEY_LEN);
    }

    #[test]
    fn loads_existing_valid_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.key");

        // Write a known 64-byte key.
        let original_bytes: Vec<u8> = (0..SESSION_KEY_LEN as u8).collect();
        std::fs::write(&path, &original_bytes).unwrap();

        let key = SessionKeyService::load_or_generate(&path);

        // The master slice must match the bytes we wrote.
        assert_eq!(key.master(), original_bytes.as_slice());
    }

    #[test]
    fn regenerates_when_file_too_short() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.key");

        // Write fewer than SESSION_KEY_LEN bytes.
        std::fs::write(&path, b"tooshort").unwrap();

        let _ = SessionKeyService::load_or_generate(&path);

        // The file should now contain a full-length key.
        let saved = std::fs::read(&path).unwrap();
        assert_eq!(saved.len(), SESSION_KEY_LEN);
    }

    #[test]
    fn returns_ephemeral_key_on_read_error() {
        // Point at a path that is unreadable for a reason other than NotFound:
        // use the directory itself as the key path.
        let dir = tempfile::tempdir().unwrap();
        let not_a_file = dir.path(); // reading a directory returns an error

        // Should not panic and should return a usable key.
        let key = SessionKeyService::load_or_generate(not_a_file);
        assert_eq!(key.master().len(), SESSION_KEY_LEN);
    }

    #[test]
    fn round_trip_key_is_stable_across_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.key");

        let first = SessionKeyService::load_or_generate(&path);
        let second = SessionKeyService::load_or_generate(&path);

        assert_eq!(first.master(), second.master());
    }
}
