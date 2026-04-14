CREATE TABLE IF NOT EXISTS media_files (
    id           VARCHAR(36) PRIMARY KEY,
    bucket       VARCHAR(255) NOT NULL,
    object_key   VARCHAR(1024) NOT NULL,
    content_type VARCHAR(255) NOT NULL,
    size         BIGINT NOT NULL,
    status       VARCHAR(20) NOT NULL COMMENT 'ready | deleted',
    created_by   VARCHAR(36) NOT NULL,
    org_id       VARCHAR(36) NULL,
    created_at   TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    UNIQUE KEY uk_media_files_bucket_object (bucket(191), object_key(512)),
    KEY idx_media_files_created_by_created_at (created_by, created_at),
    KEY idx_media_files_org_id_created_at (org_id, created_at)
);

CREATE TABLE IF NOT EXISTS media_uploads (
    id            VARCHAR(36) PRIMARY KEY,
    file_id       VARCHAR(36) NOT NULL,
    bucket        VARCHAR(255) NOT NULL,
    object_key    VARCHAR(1024) NOT NULL,
    original_name VARCHAR(255) NOT NULL,
    content_type  VARCHAR(255) NOT NULL,
    declared_size BIGINT NOT NULL,
    uploaded_size BIGINT NULL,
    status        VARCHAR(20) NOT NULL COMMENT 'init | uploading | ready | failed | expired',
    created_by    VARCHAR(36) NOT NULL,
    org_id        VARCHAR(36) NULL,
    expires_at    TIMESTAMP NOT NULL,
    completed_at  TIMESTAMP NULL,
    created_at    TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at    TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    KEY idx_media_uploads_status_expires (status, expires_at),
    KEY idx_media_uploads_file_id (file_id),
    KEY idx_media_uploads_created_by_created_at (created_by, created_at)
);
