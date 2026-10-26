use anyhow::{Result, anyhow};
pub use omnect_ui_core::types::VersionInfo;
use semver::{Version, VersionReq};

/// Compares the version a service reports with the version this client needs.
pub fn version_info(required: &str, current: &str) -> Result<VersionInfo> {
    let required_version = VersionReq::parse(required)
        .map_err(|e| anyhow!("failed to parse requirement '{required}': {e}"))?;
    let current_version =
        Version::parse(current).map_err(|e| anyhow!("failed to parse version '{current}': {e}"))?;

    Ok(VersionInfo {
        required: required.to_string(),
        current: current.to_string(),
        mismatch: !required_version.matches(&current_version),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_version_has_no_mismatch() {
        let info = version_info(">=1.2.0", "1.2.0").unwrap();
        assert_eq!(
            info,
            VersionInfo {
                required: ">=1.2.0".to_string(),
                current: "1.2.0".to_string(),
                mismatch: false,
            }
        );
    }

    #[test]
    fn older_version_is_a_mismatch() {
        assert!(version_info(">=1.2.0", "1.1.9").unwrap().mismatch);
    }

    #[test]
    fn version_that_does_not_parse_is_an_error() {
        assert!(version_info(">=1.2.0", "not-a-version").is_err());
    }

    #[test]
    fn requirement_that_does_not_parse_is_an_error() {
        assert!(version_info("not-a-requirement", "1.2.0").is_err());
    }
}
