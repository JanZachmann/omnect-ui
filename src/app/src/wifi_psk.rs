use pbkdf2::pbkdf2_hmac;
use sha1::Sha1;

const WPA_PSK_ITERATIONS: u32 = 4096;
const WPA_PSK_KEY_LENGTH: usize = 32;

/// Compute WPA PSK from password and SSID per IEEE 802.11i.
///
/// Uses PBKDF2(HMAC-SHA1, password, SSID, 4096, 256 bits) → 64-char hex.
#[must_use]
pub fn compute_wpa_psk(password: &str, ssid: &str) -> String {
    let mut key = [0u8; WPA_PSK_KEY_LENGTH];
    pbkdf2_hmac::<Sha1>(
        password.as_bytes(),
        ssid.as_bytes(),
        WPA_PSK_ITERATIONS,
        &mut key,
    );
    hex::encode(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IEEE 802.11i test vector
    #[test]
    fn ieee_test_vector() {
        let psk = compute_wpa_psk("password", "IEEE");
        assert_eq!(
            psk,
            "f42c6fc52df0ebef9ebb4b90b38a5f902e83fe1b135a70e23aed762e9710a12e"
        );
    }

    #[test]
    fn different_ssid_produces_different_psk() {
        let psk1 = compute_wpa_psk("password", "NetworkA");
        let psk2 = compute_wpa_psk("password", "NetworkB");
        assert_ne!(psk1, psk2);
    }

    #[test]
    fn output_is_64_hex_chars() {
        let psk = compute_wpa_psk("testpass", "testssid");
        assert_eq!(psk.len(), 64);
        assert!(psk.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
