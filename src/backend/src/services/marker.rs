//! Volatile marker files for cross-restart state tracking.
//!
//! Marker files record whether certain events have occurred or been acknowledged.
//! Stored in `/tmp/` (volatile), they are lost on reboot — this is intentional,
//! as it guarantees the user sees important notifications at least once per
//! boot cycle.

use log::{error, info};
use std::{fs, io, io::ErrorKind, path::Path};

/// A volatile marker file in `/tmp/`.
pub struct MarkerFile {
    path: &'static str,
    label: &'static str,
}

impl MarkerFile {
    #[must_use]
    pub const fn new(path: &'static str, label: &'static str) -> Self {
        Self { path, label }
    }

    fn path(&self) -> &Path {
        Path::new(self.path)
    }

    #[must_use]
    pub fn exists(&self) -> bool {
        self.path().exists()
    }

    /// Set the marker file.
    pub fn set(&self) -> io::Result<()> {
        info!("Setting {} marker at: {}", self.label, self.path);
        fs::write(self.path(), "")
    }

    /// Set the marker file, logging errors instead of propagating them.
    pub fn set_or_log(&self) {
        if let Err(e) = self.set() {
            error!("Failed to set {} marker: {e}", self.label);
        }
    }

    /// Clear the marker file. `NotFound` is silently ignored.
    pub fn clear(&self) {
        match fs::remove_file(self.path()) {
            Ok(()) => info!("Cleared {} marker", self.label),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => error!("Failed to clear {} marker: {e}", self.label),
        }
    }
}

/// Set when automatic network rollback happens, cleared when user acknowledges.
pub static NETWORK_ROLLBACK_OCCURRED: MarkerFile = MarkerFile::new(
    "/tmp/network_rollback_occurred",
    "network rollback occurred",
);

/// Set when user dismisses the factory reset result modal.
pub static FACTORY_RESET_RESULT_ACKED: MarkerFile = MarkerFile::new(
    "/tmp/factory_reset_result_acked",
    "factory reset result acked",
);

/// Set when user dismisses the update validation modal.
pub static UPDATE_VALIDATION_ACKED: MarkerFile =
    MarkerFile::new("/tmp/update_validation_acked", "update validation acked");
