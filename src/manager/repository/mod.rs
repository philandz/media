use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, MySqlPool};
use std::sync::Arc;

#[derive(Clone)]
pub struct MediaRepository {
    pool: Arc<MySqlPool>,
}

// ---------------------------------------------------------------------------
// Row types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct MediaFileRow {
    pub id: String,
    pub bucket: String,
    pub object_key: String,
    pub original_name: String,
    pub content_type: String,
    pub size: i64,
    pub etag: String,
    pub status: String,
    pub created_by: String,
    pub org_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct MediaUploadRow {
    pub id: String,
    pub file_id: String,
    pub bucket: String,
    pub object_key: String,
    pub original_name: String,
    pub content_type: String,
    pub declared_size: i64,
    pub uploaded_size: Option<i64>,
    pub status: String,
    pub created_by: String,
    pub org_id: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Param types
// ---------------------------------------------------------------------------

pub struct CreateUploadParams<'a> {
    pub id: &'a str,
    pub file_id: &'a str,
    pub bucket: &'a str,
    pub object_key: &'a str,
    pub original_name: &'a str,
    pub content_type: &'a str,
    pub declared_size: i64,
    pub status: &'a str,
    pub created_by: &'a str,
    pub org_id: Option<&'a str>,
    pub expires_at: DateTime<Utc>,
}

pub struct CreateFileParams<'a> {
    pub id: &'a str,
    pub bucket: &'a str,
    pub object_key: &'a str,
    pub original_name: &'a str,
    pub content_type: &'a str,
    pub size: i64,
    pub etag: &'a str,
    pub status: &'a str,
    pub created_by: &'a str,
    pub org_id: Option<&'a str>,
}

// ---------------------------------------------------------------------------
// Repository impl
// ---------------------------------------------------------------------------

impl MediaRepository {
    pub async fn new(
        config: &philand_configs::MediaServiceConfig,
    ) -> Result<Self, philand_storage::StorageError> {
        let pool = Arc::new(
            sqlx::mysql::MySqlPoolOptions::new()
                .connect(&config.database_url)
                .await
                .map_err(philand_storage::StorageError::Sqlx)?,
        );

        let repo = Self { pool };
        repo.run_migrations().await?;
        Ok(repo)
    }

    async fn run_migrations(&self) -> Result<(), philand_storage::StorageError> {
        let mut migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
            .await
            .map_err(philand_storage::StorageError::Migrate)?;
        migrator.set_ignore_missing(true);
        migrator
            .run(&*self.pool)
            .await
            .map_err(philand_storage::StorageError::Migrate)
    }

    // ---------------------------------------------------------------------------
    // Uploads
    // ---------------------------------------------------------------------------

    pub async fn insert_upload(&self, params: CreateUploadParams<'_>) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO media_uploads
                (id, file_id, bucket, object_key, original_name, content_type,
                 declared_size, status, created_by, org_id, expires_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(params.id)
        .bind(params.file_id)
        .bind(params.bucket)
        .bind(params.object_key)
        .bind(params.original_name)
        .bind(params.content_type)
        .bind(params.declared_size)
        .bind(params.status)
        .bind(params.created_by)
        .bind(params.org_id)
        .bind(params.expires_at)
        .execute(&*self.pool)
        .await
        .map(|_| ())
    }

    pub async fn get_upload_by_id(&self, id: &str) -> Result<Option<MediaUploadRow>, sqlx::Error> {
        sqlx::query_as::<_, MediaUploadRow>(
            r#"
            SELECT id, file_id, bucket, object_key, original_name, content_type,
                   declared_size, uploaded_size, status, created_by, org_id,
                   expires_at, completed_at, created_at, updated_at
            FROM media_uploads
            WHERE id = ?
            LIMIT 1
            "#,
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await
    }

    pub async fn mark_upload_ready(
        &self,
        upload_id: &str,
        uploaded_size: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE media_uploads
            SET status = 'ready',
                uploaded_size = ?,
                completed_at = UTC_TIMESTAMP(),
                updated_at = UTC_TIMESTAMP()
            WHERE id = ?
            "#,
        )
        .bind(uploaded_size)
        .bind(upload_id)
        .execute(&*self.pool)
        .await
        .map(|_| ())
    }

    // ---------------------------------------------------------------------------
    // Files
    // ---------------------------------------------------------------------------

    pub async fn insert_file(&self, params: CreateFileParams<'_>) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO media_files
                (id, bucket, object_key, original_name, content_type, size, etag, status, created_by, org_id)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON DUPLICATE KEY UPDATE
                size = VALUES(size),
                etag = VALUES(etag),
                status = VALUES(status),
                updated_at = UTC_TIMESTAMP()
            "#,
        )
        .bind(params.id)
        .bind(params.bucket)
        .bind(params.object_key)
        .bind(params.original_name)
        .bind(params.content_type)
        .bind(params.size)
        .bind(params.etag)
        .bind(params.status)
        .bind(params.created_by)
        .bind(params.org_id)
        .execute(&*self.pool)
        .await
        .map(|_| ())
    }

    pub async fn get_file_by_id(&self, id: &str) -> Result<Option<MediaFileRow>, sqlx::Error> {
        sqlx::query_as::<_, MediaFileRow>(
            r#"
            SELECT id, bucket, object_key, original_name, content_type,
                   size, etag, status, created_by, org_id, created_at
            FROM media_files
            WHERE id = ? AND status != 'deleted'
            LIMIT 1
            "#,
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await
    }

    pub async fn get_file_by_object_key(
        &self,
        object_key: &str,
    ) -> Result<Option<MediaFileRow>, sqlx::Error> {
        sqlx::query_as::<_, MediaFileRow>(
            r#"
            SELECT id, bucket, object_key, original_name, content_type,
                   size, etag, status, created_by, org_id, created_at
            FROM media_files
            WHERE object_key = ? AND status != 'deleted'
            LIMIT 1
            "#,
        )
        .bind(object_key)
        .fetch_optional(&*self.pool)
        .await
    }

    pub async fn list_files_by_user(
        &self,
        user_id: &str,
        org_id: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<(Vec<MediaFileRow>, u32), sqlx::Error> {
        let rows = if let Some(oid) = org_id {
            sqlx::query_as::<_, MediaFileRow>(
                r#"
                SELECT id, bucket, object_key, original_name, content_type,
                       size, etag, status, created_by, org_id, created_at
                FROM media_files
                WHERE created_by = ? AND org_id = ? AND status != 'deleted'
                ORDER BY created_at DESC
                LIMIT ? OFFSET ?
                "#,
            )
            .bind(user_id)
            .bind(oid)
            .bind(limit)
            .bind(offset)
            .fetch_all(&*self.pool)
            .await?
        } else {
            sqlx::query_as::<_, MediaFileRow>(
                r#"
                SELECT id, bucket, object_key, original_name, content_type,
                       size, etag, status, created_by, org_id, created_at
                FROM media_files
                WHERE created_by = ? AND status != 'deleted'
                ORDER BY created_at DESC
                LIMIT ? OFFSET ?
                "#,
            )
            .bind(user_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&*self.pool)
            .await?
        };

        // Simple total count (same filter, no pagination)
        let total: u32 = if let Some(oid) = org_id {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM media_files WHERE created_by = ? AND org_id = ? AND status != 'deleted'",
            )
            .bind(user_id)
            .bind(oid)
            .fetch_one(&*self.pool)
            .await? as u32
        } else {
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM media_files WHERE created_by = ? AND status != 'deleted'",
            )
            .bind(user_id)
            .fetch_one(&*self.pool)
            .await? as u32
        };

        Ok((rows, total))
    }

    pub async fn soft_delete_file(&self, file_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE media_files
            SET status = 'deleted', updated_at = UTC_TIMESTAMP()
            WHERE id = ?
            "#,
        )
        .bind(file_id)
        .execute(&*self.pool)
        .await
        .map(|_| ())
    }
}
