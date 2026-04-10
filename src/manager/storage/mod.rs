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

    pub async fn head_object_size(&self, object_key: &str) -> Result<u64, String> {
        let signed = presign_get_object(&self.cfg, object_key, Duration::from_secs(300))
            .map_err(|e| e.to_string())?;

        let response = reqwest::Client::new()
            .get(signed)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !(response.status().is_success()
            || response.status() == reqwest::StatusCode::PARTIAL_CONTENT)
        {
            return Err(format!(
                "signed get failed with status {}",
                response.status()
            ));
        }

        if let Some(total) = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_total_from_content_range)
        {
            return Ok(total);
        }

        response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| "cannot detect object size from response headers".to_string())
    }
}

fn parse_total_from_content_range(header_value: &str) -> Option<u64> {
    // bytes 0-0/12345
    let (_, total) = header_value.split_once('/')?;
    if total == "*" {
        return None;
    }
    total.parse::<u64>().ok()
}
