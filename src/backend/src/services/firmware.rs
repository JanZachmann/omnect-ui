//! Firmware update service
//!
//! Handles firmware file management and operations independent of HTTP concerns.

#![allow(unused_imports)] // OpenOptionsExt needed for .mode() method

use crate::{config::AppConfig, omnect_device_service_client::DeviceServiceClient};
use actix_multipart::Field;
use anyhow::{Context, Result};
use futures_util::StreamExt;
use log::{debug, error, info};
use std::{
    os::unix::fs::OpenOptionsExt, // Required for .mode() on OpenOptions
    time::Instant,
};
use tokio::{fs, io::AsyncWriteExt};

#[cfg(any(test, feature = "mock"))]
use std::sync::{LazyLock, Mutex, MutexGuard};

#[cfg(any(test, feature = "mock"))]
#[allow(dead_code)]
static DATA_FOLDER_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Service for firmware update file operations
pub struct FirmwareService;

impl FirmwareService {
    /// Acquire a lock for data folder operations (test-only)
    ///
    /// This ensures that tests modifying the data folder don't interfere with each other
    #[cfg(any(test, feature = "mock"))]
    #[allow(dead_code)]
    pub fn lock_for_test() -> MutexGuard<'static, ()> {
        DATA_FOLDER_LOCK.lock().unwrap()
    }

    /// Handle uploaded firmware file via streaming - clears data folder and writes stream to file
    ///
    /// # Arguments
    /// * `field` - The multipart field containing the file stream
    ///
    /// # Returns
    /// Result indicating success or failure
    #[allow(clippy::future_not_send)]
    pub async fn receive_firmware(mut field: Field) -> Result<()> {
        const WRITE_BUFFER_SIZE: usize = 512 * 1024;
        const FLUSH_INTERVAL_BYTES: usize = 5 * 1024 * 1024;
        const FLUSH_INTERVAL_SECS: u64 = 10;
        const CHUNK_TIMEOUT_SECS: u64 = 30;
        const TOTAL_TIMEOUT_SECS: u64 = 900;

        info!("firmware upload started");
        let start = Instant::now();
        let mut last_flush = Instant::now();
        let mut total_bytes = 0;
        let mut bytes_since_flush = 0;

        // Clear data folder before writing new firmware
        if let Err(e) = Self::clear_data_folder().await {
            error!("failed to clear data folder: {e:#}");
        }

        let local_update_file = &AppConfig::get().paths.local_update_file;

        // 1. Create the destination file with permissions set atomically
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o750)
            .open(local_update_file)
            .await
            .context("failed to create update file")?;
        let mut file = tokio::io::BufWriter::with_capacity(WRITE_BUFFER_SIZE, file);

        // 2. Stream chunks to the file with timeout protection
        loop {
            // Check total timeout
            if start.elapsed().as_secs() > TOTAL_TIMEOUT_SECS {
                anyhow::bail!("upload exceeded maximum duration of {TOTAL_TIMEOUT_SECS} seconds");
            }

            // Wait for next chunk with timeout
            let chunk_result = tokio::time::timeout(
                std::time::Duration::from_secs(CHUNK_TIMEOUT_SECS),
                field.next(),
            )
            .await
            .context(format!(
                "chunk timeout: no data received for {CHUNK_TIMEOUT_SECS} seconds"
            ))?;

            // Check if stream is complete
            let Some(chunk) = chunk_result else {
                break;
            };

            let data = chunk
                .map_err(|e| anyhow::anyhow!(e.to_string()))
                .context("failed to read chunk from stream")?;

            let chunk_len = data.len();
            total_bytes += chunk_len;
            bytes_since_flush += chunk_len;

            file.write_all(&data)
                .await
                .context("failed to write chunk to file")?;

            // Periodic flush for durability and accurate metrics
            let should_flush = bytes_since_flush >= FLUSH_INTERVAL_BYTES
                || last_flush.elapsed().as_secs() >= FLUSH_INTERVAL_SECS;

            if should_flush {
                file.flush()
                    .await
                    .context("failed to flush intermediate data")?;
                bytes_since_flush = 0;
                last_flush = Instant::now();
            }
        }

        // Final flush
        file.flush().await.context("failed to flush update file")?;

        #[allow(clippy::cast_precision_loss)]
        let size_mb = total_bytes as f64 / 1024.0 / 1024.0;
        info!("firmware upload completed: {size_mb:.2} MB");

        Ok(())
    }

    /// Load the firmware update file via the device service client
    ///
    /// # Arguments
    /// * `service_client` - Device service client for loading the update
    ///
    /// # Returns
    /// Result with the response data from the device service
    #[allow(clippy::future_not_send)]
    pub async fn load_update<SC: DeviceServiceClient>(service_client: &SC) -> Result<String> {
        use crate::omnect_device_service_client::LoadUpdate;

        service_client
            .load_update(LoadUpdate {
                update_file_path: AppConfig::get().paths.host_update_file.clone(),
            })
            .await
    }

    /// Run the firmware update via the device service client
    ///
    /// # Arguments
    /// * `service_client` - Device service client for running the update
    /// * `run_update` - The update configuration
    ///
    /// # Returns
    /// Result indicating success or failure
    #[allow(clippy::future_not_send)]
    pub async fn run_update<ServiceClient: DeviceServiceClient>(
        service_client: &ServiceClient,
        run_update: crate::omnect_device_service_client::RunUpdate,
    ) -> Result<()> {
        service_client.run_update(run_update).await
    }

    /// Clear all files in the data folder
    async fn clear_data_folder() -> Result<()> {
        debug!("clear_data_folder() called");
        let mut entries = fs::read_dir(&AppConfig::get().paths.data_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file() {
                fs::remove_file(entry.path()).await?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[cfg(feature = "mock")]
    use mockall_double::double;

    #[cfg(feature = "mock")]
    #[double]
    use crate::omnect_device_service_client::DeviceServiceClient;

    // Note: Streaming tests would require mocking actix_multipart::Field which is complex.
    // Focusing on file system operations for now.

    mod clear_data_folder {
        use super::*;

        #[tokio::test]
        async fn removes_all_files() {
            let _lock = FirmwareService::lock_for_test();
            let data_path = &AppConfig::get().paths.data_dir;

            // Ensure directory exists
            fs::create_dir_all(data_path)
                .await
                .expect("should create data dir");

            // Create some test files
            let mut file1 = fs::File::create(data_path.join("file1.txt"))
                .await
                .expect("should create file1");
            file1.write_all(b"test").await.expect("should write");

            let mut file2 = fs::File::create(data_path.join("file2.txt"))
                .await
                .expect("should create file2");
            file2.write_all(b"test").await.expect("should write");

            // Verify files exist
            assert!(data_path.join("file1.txt").exists());
            assert!(data_path.join("file2.txt").exists());

            // Clear folder
            FirmwareService::clear_data_folder()
                .await
                .expect("should clear folder");

            // Verify files are deleted
            assert!(!data_path.join("file1.txt").exists());
            assert!(!data_path.join("file2.txt").exists());
        }

        #[tokio::test]
        async fn succeeds_with_empty_directory() {
            let _lock = FirmwareService::lock_for_test();
            let data_path = &AppConfig::get().paths.data_dir;

            // Ensure directory exists and is empty
            fs::create_dir_all(data_path)
                .await
                .expect("should create data dir");

            // Clear folder when already empty
            let result = FirmwareService::clear_data_folder().await;
            assert!(result.is_ok());
        }

        #[tokio::test]
        async fn preserves_subdirectories() {
            let _lock = FirmwareService::lock_for_test();
            let data_path = &AppConfig::get().paths.data_dir;

            // Ensure directory exists
            fs::create_dir_all(data_path)
                .await
                .expect("should create data dir");

            // Create a subdirectory
            let subdir = data_path.join("subdir");
            fs::create_dir_all(&subdir)
                .await
                .expect("should create subdir");

            // Create a file in root
            let mut file = fs::File::create(data_path.join("file.txt"))
                .await
                .expect("should create file");
            file.write_all(b"test").await.expect("should write");

            // Clear folder
            FirmwareService::clear_data_folder()
                .await
                .expect("should clear folder");

            // File should be deleted
            assert!(!data_path.join("file.txt").exists());

            // Subdirectory should still exist (only files are removed)
            assert!(subdir.exists());
            assert!(subdir.is_dir());

            // Cleanup
            let _ = fs::remove_dir(&subdir).await;
        }
    }

    mod load_update {
        use super::*;
        use crate::omnect_device_service_client::LoadUpdate;

        #[tokio::test]
        async fn forwards_request_to_device_service() {
            let mut device_mock = DeviceServiceClient::default();

            device_mock
                .expect_load_update()
                .withf(|req: &LoadUpdate| {
                    req.update_file_path == AppConfig::get().paths.host_update_file
                })
                .times(1)
                .returning(|_| Box::pin(async { Ok("update loaded successfully".to_string()) }));

            let result = FirmwareService::load_update(&device_mock).await;

            assert!(result.is_ok());
            assert_eq!(result.unwrap(), "update loaded successfully");
        }

        #[tokio::test]
        async fn returns_error_on_device_service_failure() {
            let mut device_mock = DeviceServiceClient::default();

            device_mock
                .expect_load_update()
                .returning(|_| Box::pin(async { Err(anyhow::anyhow!("device service error")) }));

            let result = FirmwareService::load_update(&device_mock).await;

            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("device service error")
            );
        }
    }

    mod run_update {
        use super::*;

        #[tokio::test]
        async fn forwards_request_to_device_service() {
            let mut device_mock = DeviceServiceClient::default();

            device_mock
                .expect_run_update()
                .times(1)
                .returning(|_| Box::pin(async { Ok(()) }));

            let run_update: crate::omnect_device_service_client::RunUpdate =
                serde_json::from_str(r#"{"validate_iothub_connection": true}"#)
                    .expect("should deserialize");

            let result = FirmwareService::run_update(&device_mock, run_update).await;

            assert!(result.is_ok());
        }

        #[tokio::test]
        async fn returns_error_on_device_service_failure() {
            let mut device_mock = DeviceServiceClient::default();

            device_mock
                .expect_run_update()
                .returning(|_| Box::pin(async { Err(anyhow::anyhow!("update execution failed")) }));

            let run_update: crate::omnect_device_service_client::RunUpdate =
                serde_json::from_str(r#"{"validate_iothub_connection": false}"#)
                    .expect("should deserialize");

            let result = FirmwareService::run_update(&device_mock, run_update).await;

            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("update execution failed")
            );
        }
    }
}
