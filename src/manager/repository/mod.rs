use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, MySqlPool};

#[derive(Clone)]
pub struct MediaRepository {
    pool: std::sync::Arc<MySqlPool>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct MediaFileRow {
    pub id: String,
    pub bucket: String,
    pub object_key: String,
    pub content_type: String,
    pub size: i64,
    pub status: String,
    pub created_by: String,
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
    pub content_type: &'a str,
    pub size: i64,
    pub status: &'a str,
    pub created_by: &'a str,
    pub org_id: Option<&'a str>,
}

impl MediaRepository {
    pub async fn new(
        config: &philand_configs::MediaServiceConfig,
    ) -> Result<Self, philand_storage::StorageError> {
        let pool = std::sync::Arc::new(
            sqlx::mysql::MySqlPoolOptions::new()
                .connect(&config.database_url)
                .await
                .map_err(philand_storage::StorageError::Sqlx)?,
        );

        let repo = Self { pool };
        repo.ensure_tables().await?;
        Ok(repo)
    }

    async fn ensure_tables(&self) -> Result<(), philand_storage::StorageError> {
        let mut migrator = sqlx::migrate::Migrator::new(std::path::Path::new("./migrations"))
            .await
            .map_err(philand_storage::StorageError::Migrate)?;
        migrator.set_ignore_missing(true);
        migrator
            .run(&*self.pool)
            .await
            .map_err(philand_storage::StorageError::Migrate)
    }

    pub async fn insert_upload(&self, params: CreateUploadParams<'_>) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO media_uploads
                (id, file_id, bucket, object_key, original_name, content_type, declared_size, status, created_by, org_id, expires_at)
            VALUES
                (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
            SELECT id, file_id, bucket, object_key, original_name, content_type, declared_size,
                   uploaded_size, status, created_by, org_id, expires_at, completed_at, created_at, updated_at
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
            SET status = 'ready', uploaded_size = ?, completed_at = UTC_TIMESTAMP(), updated_at = UTC_TIMESTAMP()
            WHERE id = ?
            "#,
        )
        .bind(uploaded_size)
        .bind(upload_id)
        .execute(&*self.pool)
        .await
        .map(|_| ())
    }

    pub async fn insert_file(&self, params: CreateFileParams<'_>) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO media_files
                (id, bucket, object_key, content_type, size, status, created_by, org_id)
            VALUES
                (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(params.id)
        .bind(params.bucket)
        .bind(params.object_key)
        .bind(params.content_type)
        .bind(params.size)
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
            SELECT id, bucket, object_key, content_type, size, status, created_by, created_at
            FROM media_files
            WHERE id = ?
            LIMIT 1
            "#,
        )
        .bind(id)
        .fetch_optional(&*self.pool)
        .await
    }
}
