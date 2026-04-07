use serde::{Deserialize, Serialize};

/// Operating system information
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OsInfo {
    pub name: String,
    pub version: String,
}

/// System information from WebSocket
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub os: OsInfo,
    pub azure_sdk_version: String,
    pub omnect_device_service_version: String,
    pub boot_time: Option<String>,
    pub hostname: String,
}

/// Online status from WebSocket
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OnlineStatus {
    pub iothub: bool,
}

/// `TimeoutDuration` type for timeouts
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimeoutDuration {
    pub nanos: u32,
    pub secs: u64,
}

/// Timeout configurations from WebSocket
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Timeouts {
    pub wait_online_timeout: TimeoutDuration,
}

/// Overlay spinner state (UI state)
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySpinnerState {
    overlay: bool,
    title: String,
    text: Option<String>,
    timed_out: bool,
    progress: Option<u8>,
    countdown_seconds: Option<u32>,
}

impl OverlaySpinnerState {
    /// Create a new overlay spinner with the given title (shown by default)
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            overlay: true,
            title: title.into(),
            text: None,
            timed_out: false,
            progress: None,
            countdown_seconds: None,
        }
    }

    /// Builder pattern: add optional text to the spinner
    #[must_use]
    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Builder pattern: add optional progress to the spinner
    #[must_use]
    pub const fn with_progress(mut self, progress: u8) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Builder pattern: add optional countdown to the spinner
    #[must_use]
    pub const fn with_countdown(mut self, seconds: u32) -> Self {
        self.countdown_seconds = Some(seconds);
        self
    }

    /// Update the optional text message
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = Some(text.into());
    }

    /// Update the optional progress
    pub const fn set_progress(&mut self, progress: u8) {
        self.progress = Some(progress);
    }

    /// Update the optional countdown
    pub const fn set_countdown(&mut self, seconds: u32) {
        self.countdown_seconds = Some(seconds);
    }

    /// Mark the spinner as timed out
    pub const fn set_timed_out(&mut self) {
        self.timed_out = true;
    }

    /// Reset timed out state (back to loading/spinning)
    pub const fn set_loading(&mut self) {
        self.timed_out = false;
        self.countdown_seconds = None;
    }

    /// Show the overlay spinner
    pub const fn show(&mut self) {
        self.overlay = true;
    }

    /// Hide the overlay spinner
    pub const fn hide(&mut self) {
        self.overlay = false;
    }

    /// Check if the overlay is currently visible
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.overlay
    }

    /// Reset to default (hidden) state
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Get the overlay visibility state
    #[must_use]
    pub const fn overlay(&self) -> bool {
        self.overlay
    }

    /// Get the title
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Get the optional text
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Get the optional progress
    #[must_use]
    pub const fn progress(&self) -> Option<u8> {
        self.progress
    }

    /// Check if the spinner has timed out
    #[must_use]
    pub const fn timed_out(&self) -> bool {
        self.timed_out
    }

    /// Get the optional countdown seconds
    #[must_use]
    pub const fn countdown_seconds(&self) -> Option<u32> {
        self.countdown_seconds
    }

    /// Decrement countdown by one second (saturating at zero)
    pub const fn decrement_countdown(&mut self) {
        if let Some(s) = self.countdown_seconds {
            self.countdown_seconds = Some(s.saturating_sub(1));
        }
    }
}
