pub mod token;

use chrono::{Datelike, Utc};
use tonic::Status;
use uuid::Uuid;

use crate::manager::repository::{
    CreateFileParams, CreateUploadParams, MediaFileRow, MediaRepository,
};
use crate::manager::storage::MediaStorage;
use crate::manager::validate::{validate_content_type, validate_file_name, validate_size};
use crate::pb::common::base::{Base, BaseStatus};
use crate::pb::shared::media::{MediaFile, MediaFileStatus, MediaUploadStatus};

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

pub struct InitUploadOutput {
    pub upload_id: String,
    pub file_id: String,
    pub bucket: String,
    pub object_key: String,
    pub presigned_url: String,
    pub expires_at: i64,
    pub content_type: String,
}

pub struct CompleteUploadOutput {
    pub file_id: String,
    pub status: String,
    pub object_key: String,
    pub bucket: String,
    pub etag: String,
    pub confirmed_size: u64,
    pub public_url: String,
}

pub struct FileOutput {
    pub file_id: String,
    pub bucket: String,
    pub object_key: String,
    pub content_type: String,
    pub original_name: String,
    pub size: u64,
    pub status: String,
    pub org_id: String,
    pub created_at: i64,
    pub public_url: String,
}

pub struct ListFilesOutput {
    pub files: Vec<FileOutput>,
    pub total: u32,
}

pub struct FileDownloadUrlOutput {
    pub file_id: String,
    pub download_url: String,
    pub expires_at: i64,
}

// ---------------------------------------------------------------------------
// Output helpers
// ---------------------------------------------------------------------------

impl CompleteUploadOutput {
    pub fn upload_status_enum(&self) -> i32 {
        match self.status.as_str() {
            "ready" => MediaUploadStatus::MusReady as i32,
            "uploading" => MediaUploadStatus::MusUploading as i32,
            "failed" => MediaUploadStatus::MusFailed as i32,
            "expired" => MediaUploadStatus::MusExpired as i32,
            "init" => MediaUploadStatus::MusInit as i32,
            _ => MediaUploadStatus::MusNone as i32,
        }
    }
}

impl FileOutput {
    pub fn to_proto(&self, created_by: &str) -> MediaFile {
        let status = match self.status.as_str() {
            "ready" => MediaFileStatus::MfsReady as i32,
            "deleted" => MediaFileStatus::MfsDeleted as i32,
            _ => MediaFileStatus::MfsNone as i32,
        };

        MediaFile {
            base: Some(Base {
                id: self.file_id.clone(),
                created_at: self.created_at,
                updated_at: self.created_at,
                deleted_at: 0,
                created_by: created_by.to_string(),
                updated_by: created_by.to_string(),
                owner_id: created_by.to_string(),
                status: BaseStatus::BsActive as i32,
            }),
            bucket: self.bucket.clone(),
            object_key: self.object_key.clone(),
            content_type: self.content_type.clone(),
            size: self.size as i64,
            etag: String::new(),
            file_status: status,
            org_id: self.org_id.clone(),
            original_name: self.original_name.clone(),
            public_url: self.public_url.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// MediaBiz
// ---------------------------------------------------------------------------

pub struct MediaBiz {
    repo: MediaRepository,
    config: philand_configs::MediaServiceConfig,
    storage: MediaStorage,
    /// Internal storage client pointing directly at Garage (bypasses CDN/tunnel).
    internal_storage: MediaStorage,
}

impl MediaBiz {
    pub async fn new(
        repo: MediaRepository,
        config: philand_configs::MediaServiceConfig,
    ) -> Result<Self, Status> {
        let s3_cfg = philand_storage::S3Config {
            endpoint: config.s3_endpoint.clone(),
            region: config.s3_region.clone(),
            access_key: config.s3_access_key.clone(),
            secret_key: config.s3_secret_key.clone(),
            bucket: config.s3_bucket.clone(),
            force_path_style: config.s3_force_path_style,
        };

        let storage = MediaStorage::new(s3_cfg)
            .await
            .map_err(Self::map_internal_error)?;

        // Internal storage uses the direct endpoint (bypasses CDN/tunnel) for HEAD checks.
        let internal_s3_cfg = philand_storage::S3Config {
            endpoint: config.s3_internal_endpoint.clone(),
            region: config.s3_region.clone(),
            access_key: config.s3_access_key.clone(),
            secret_key: config.s3_secret_key.clone(),
            bucket: config.s3_bucket.clone(),
            force_path_style: config.s3_force_path_style,
        };
        let internal_storage = MediaStorage::new(internal_s3_cfg)
            .await
            .map_err(Self::map_internal_error)?;

        Ok(Self { repo, config, storage, internal_storage })
    }

    pub fn verify_token_subject(&self, token: &str) -> Result<String, Status> {
        let claims = token::decode_claims(&self.config.jwt_secret, token)?;
        Ok(claims.sub)
    }

    pub async fn init_upload(
        &self,
        user_id: &str,
        file_name: String,
        content_type: String,
        size: u64,
        org_id: Option<String>,
    ) -> Result<InitUploadOutput, Status> {
        validate_file_name(&file_name)?;
        validate_content_type(&self.config.allowed_content_type_prefixes, &content_type)?;
        validate_size(self.config.max_file_size_bytes, size)?;

        let upload_id = Uuid::new_v4().to_string();
        let file_id = Uuid::new_v4().to_string();
        let object_key = self.build_object_key(user_id, &file_id, &file_name);
        let now = Utc::now();

        let signed = self
            .storage
            .presign_put(&object_key, self.config.upload_url_ttl_seconds, now.timestamp())
            .map_err(Self::map_internal_error)?;

        self.repo
            .insert_upload(CreateUploadParams {
                id: &upload_id,
                file_id: &file_id,
                bucket: self.storage.bucket(),
                object_key: &object_key,
                original_name: &file_name,
                content_type: &content_type,
                declared_size: size as i64,
                status: "init",
                created_by: user_id,
                org_id: org_id.as_deref(),
                expires_at: now
                    + chrono::Duration::seconds(self.config.upload_url_ttl_seconds as i64),
            })
            .await
            .map_err(Self::map_internal_error)?;

        Ok(InitUploadOutput {
            upload_id,
            file_id,
            bucket: self.storage.bucket().to_string(),
            object_key,
            presigned_url: signed.url,
            expires_at: signed.expires_at,
            content_type,
        })
    }

    pub async fn complete_upload(
        &self,
        user_id: &str,
        upload_id: String,
    ) -> Result<CompleteUploadOutput, Status> {
        let upload = self
            .repo
            .get_upload_by_id(&upload_id)
            .await
            .map_err(Self::map_internal_error)?
            .ok_or_else(|| Status::not_found("upload not found"))?;

        if upload.created_by != user_id {
            return Err(Status::permission_denied("upload does not belong to caller"));
        }

        if upload.status == "ready" {
            let public_url = self.build_public_url(&upload.bucket, &upload.object_key);
            return Ok(CompleteUploadOutput {
                file_id: upload.file_id,
                status: "ready".to_string(),
                object_key: upload.object_key,
                bucket: upload.bucket,
                etag: String::new(),
                confirmed_size: upload.uploaded_size.unwrap_or(0).max(0) as u64,
                public_url,
            });
        }

        if upload.expires_at < Utc::now() {
            return Err(Status::failed_precondition("upload has expired"));
        }

        let (object_size, etag) = self.verify_object_exists(&upload.object_key).await?;

        self.repo
            .insert_file(CreateFileParams {
                id: &upload.file_id,
                bucket: &upload.bucket,
                object_key: &upload.object_key,
                original_name: &upload.original_name,
                content_type: &upload.content_type,
                size: object_size as i64,
                etag: &etag,
                status: "ready",
                created_by: user_id,
                org_id: upload.org_id.as_deref(),
            })
            .await
            .map_err(Self::map_internal_error)?;

        self.repo
            .mark_upload_ready(&upload.id, object_size as i64)
            .await
            .map_err(Self::map_internal_error)?;

        let public_url = self.build_public_url(&upload.bucket, &upload.object_key);
        Ok(CompleteUploadOutput {
            file_id: upload.file_id,
            status: "ready".to_string(),
            object_key: upload.object_key,
            bucket: upload.bucket,
            etag,
            confirmed_size: object_size,
            public_url,
        })
    }

    pub async fn get_file(&self, user_id: &str, file_id: String) -> Result<FileOutput, Status> {
        let row = self
            .repo
            .get_file_by_id(&file_id)
            .await
            .map_err(Self::map_internal_error)?
            .ok_or_else(|| Status::not_found("file not found"))?;

        if row.created_by != user_id {
            return Err(Status::permission_denied("file does not belong to caller"));
        }

        Ok(map_file_row(row, &self.config.s3_public_base_url))
    }

    pub async fn list_files(
        &self,
        user_id: &str,
        org_id: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<ListFilesOutput, Status> {
        let (rows, total) = self
            .repo
            .list_files_by_user(user_id, org_id, limit, offset)
            .await
            .map_err(Self::map_internal_error)?;

        Ok(ListFilesOutput {
            files: rows.into_iter().map(|r| map_file_row(r, &self.config.s3_public_base_url)).collect(),
            total,
        })
    }

    pub async fn delete_file(&self, user_id: &str, file_id: String) -> Result<(), Status> {
        let row = self
            .repo
            .get_file_by_id(&file_id)
            .await
            .map_err(Self::map_internal_error)?
            .ok_or_else(|| Status::not_found("file not found"))?;

        if row.created_by != user_id {
            return Err(Status::permission_denied("file does not belong to caller"));
        }

        self.repo
            .soft_delete_file(&file_id)
            .await
            .map_err(Self::map_internal_error)?;

        Ok(())
    }

    pub async fn get_file_download_url(
        &self,
        user_id: &str,
        file_id: String,
    ) -> Result<FileDownloadUrlOutput, Status> {
        let row = self
            .repo
            .get_file_by_id(&file_id)
            .await
            .map_err(Self::map_internal_error)?
            .ok_or_else(|| Status::not_found("file not found"))?;

        if row.created_by != user_id {
            return Err(Status::permission_denied("file does not belong to caller"));
        }

        let now = Utc::now().timestamp();
        let signed = self
            .storage
            .presign_get(&row.object_key, self.config.download_url_ttl_seconds, now)
            .map_err(Self::map_internal_error)?;

        Ok(FileDownloadUrlOutput {
            file_id: row.id,
            download_url: signed.url,
            expires_at: signed.expires_at,
        })
    }

    // ---------------------------------------------------------------------------
    // Private helpers
    // ---------------------------------------------------------------------------

    fn build_public_url(&self, bucket: &str, object_key: &str) -> String {
        let base = self.config.s3_public_base_url.trim_end_matches('/');
        let key = object_key.trim_start_matches('/');
        format!("{}/{}/{}", base, bucket, key)
    }

    fn build_object_key(&self, user_id: &str, file_id: &str, file_name: &str) -> String {
        let now = Utc::now();
        let safe_name: String = file_name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!(
            "media/{}/{:04}/{:02}/{:02}/{}/{}",
            user_id,
            now.year(),
            now.month(),
            now.day(),
            file_id,
            safe_name
        )
    }

    async fn verify_object_exists(&self, object_key: &str) -> Result<(u64, String), Status> {
        self.internal_storage
            .head_object(object_key)
            .await
            .map_err(|_| Status::failed_precondition("uploaded object not found in S3"))
    }

    fn map_internal_error(error: impl ToString) -> Status {
        Status::internal(error.to_string())
    }
}

fn map_file_row(row: MediaFileRow, public_base_url: &str) -> FileOutput {
    let key = row.object_key.trim_start_matches('/');
    let public_url = format!(
        "{}/{}/{}",
        public_base_url.trim_end_matches('/'),
        row.bucket,
        key
    );
    FileOutput {
        file_id: row.id,
        bucket: row.bucket,
        object_key: row.object_key,
        content_type: row.content_type,
        original_name: row.original_name,
        size: row.size.max(0) as u64,
        status: row.status,
        org_id: row.org_id.unwrap_or_default(),
        created_at: row.created_at.timestamp(),
        public_url,
    }
}
