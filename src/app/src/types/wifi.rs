use serde::{Deserialize, Serialize};

/// `WiFi` service availability info returned by the backend
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum WifiAvailability {
    Available {
        version: String,
        interface_name: String,
    },
    Unavailable {
        socket_present: bool,
        version: Option<String>,
        min_required_version: String,
    },
}

/// A `WiFi` network discovered during scanning
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetwork {
    pub ssid: String,
    pub mac: String,
    pub ch: u16,
    pub rssi: i16,
}

/// A saved `WiFi` network from `wpa_supplicant`
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WifiSavedNetwork {
    pub ssid: String,
    pub flags: String,
}

/// `WiFi` connection status from the service
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WifiConnectionStatus {
    pub state: WifiConnectionState,
    pub ssid: Option<String>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WifiScanState {
    #[default]
    Idle,
    Scanning,
    Finished,
    Error(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WifiConnectionState {
    #[default]
    Idle,
    Connecting,
    Connected,
    Failed(String),
}

/// Top-level `WiFi` state machine exposed in the `ViewModel`
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WifiState {
    #[default]
    Unknown,
    Unavailable {
        socket_present: bool,
        version: Option<String>,
        min_required_version: String,
    },
    Ready {
        interface_name: String,
        version: Option<String>,
        status: WifiConnectionStatus,
        scan_state: WifiScanState,
        scan_results: Vec<WifiNetwork>,
        saved_networks: Vec<WifiSavedNetwork>,
        scan_poll_attempt: u32,
        connect_poll_attempt: u32,
    },
}

// --- API Response Types ---

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct WifiConnectRequest {
    pub ssid: String,
    pub psk: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct WifiForgetRequest {
    pub ssid: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct WifiScanStartedResponse {
    pub status: String,
    pub state: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct WifiScanResultsResponse {
    pub status: String,
    pub state: String,
    pub networks: Vec<WifiNetwork>,
}

// WifiNetwork is defined above in the file

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct WifiConnectResponse {
    pub status: String,
    pub state: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct WifiDisconnectResponse {
    pub status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WifiStatusResponse {
    pub status: String,
    pub state: String,
    pub ssid: Option<String>,
    pub ip_address: Option<String>,
    pub interface_name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct WifiSavedNetworksResponse {
    pub status: String,
    pub networks: Vec<WifiSavedNetwork>,
}

// WifiSavedNetwork is defined above

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct WifiForgetResponse {
    pub status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WifiVersionResponse {
    pub version: String,
}
