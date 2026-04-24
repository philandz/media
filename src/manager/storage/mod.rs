use std::time::Duration;

use philand_storage::{presign_get_object, presign_put_object, S3Config, StorageError};

#[derive(Clone)]
pub struct MediaStorage {
    cfg: S3Config,
}

#[derive(Debug, Clone)]
pub struct PresignedPut {
    pub url: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone)]
pub struct PresignedGet {
    pub url: String,
    pub expires_at: i64,
}

impl MediaStorage {
    pub async fn new(cfg: S3Config) -> Result<Self, StorageError> {
        cfg.validate()?;
        Ok(Self { cfg })
    }

    pub fn bucket(&self) -> &str {
        &self.cfg.bucket
    }

    pub fn presign_put(
        &self,
        object_key: &str,
        ttl_seconds: u64,
        now_unix: i64,
    ) -> Result<PresignedPut, StorageError> {
        let signed = presign_put_object(&self.cfg, object_key, Duration::from_secs(ttl_seconds))?;
        Ok(PresignedPut {
            url: signed.to_string(),
            expires_at: now_unix + ttl_seconds as i64,
        })
    }

    pub fn presign_get(
        &self,
        object_key: &str,
        ttl_seconds: u64,
        now_unix: i64,
    ) -> Result<PresignedGet, StorageError> {
        let signed = presign_get_object(&self.cfg, object_key, Duration::from_secs(ttl_seconds))?;
        Ok(PresignedGet {
            url: signed.to_string(),
            expires_at: now_unix + ttl_seconds as i64,
        })
    }

    /// Verifies the object exists in S3 and returns (size_bytes, etag).
    /// Uses a presigned GET with a short TTL — internal endpoint bypasses CDN.
    pub async fn head_object(&self, object_key: &str) -> Result<(u64, String), String> {
        let signed = presign_get_object(&self.cfg, object_key, Duration::from_secs(30))
            .map_err(|e| e.to_string())?;

        let response = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .http1_only()
            .build()
            .map_err(|e| e.to_string())?
            .get(signed)
            .header("Range", "bytes=0-0")
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !response.status().is_success()
            && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
        {
            return Err(format!(
                "S3 object check failed with status {}",
                response.status()
            ));
        }

        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .trim_matches('"')
            .to_string();

        // Content-Range: bytes 0-0/TOTAL
        let size = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split('/').next_back())
            .and_then(|v| v.parse::<u64>().ok())
            .or_else(|| {
                response
                    .headers()
                    .get(reqwest::header::CONTENT_LENGTH)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
            })
            .ok_or_else(|| "cannot detect object size from response".to_string())?;

        Ok((size, etag))
    }

    /// Downloads the full object content via an internal presigned GET.
    pub async fn get_object_bytes(&self, object_key: &str) -> Result<(Vec<u8>, String), String> {
        let signed = presign_get_object(&self.cfg, object_key, Duration::from_secs(30))
            .map_err(|e| e.to_string())?;

        let response = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .http1_only()
            .build()
            .map_err(|e| e.to_string())?
            .get(signed)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !response.status().is_success() {
            return Err(format!(
                "S3 object download failed with status {}",
                response.status()
            ));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();

        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        Ok((bytes.to_vec(), content_type))
    }
}
