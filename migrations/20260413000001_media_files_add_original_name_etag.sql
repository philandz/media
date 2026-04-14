-- Add original_name and etag columns to media_files.
-- original_name: the client-provided file name at upload time.
-- etag: the ETag returned by S3 after object verification.

ALTER TABLE media_files
    ADD COLUMN original_name VARCHAR(512) NOT NULL DEFAULT '' AFTER object_key,
    ADD COLUMN etag          VARCHAR(255) NOT NULL DEFAULT '' AFTER size;
