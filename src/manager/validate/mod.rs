#![allow(clippy::result_large_err)]

use tonic::Status;

pub fn validate_file_name(file_name: &str) -> Result<(), Status> {
    let trimmed = file_name.trim();
    if trimmed.is_empty() {
        return Err(Status::invalid_argument("file_name must not be empty"));
    }
    if trimmed.len() > 255 {
        return Err(Status::invalid_argument("file_name exceeds max length 255"));
    }
    Ok(())
}

pub fn validate_content_type(
    allowed_prefixes: &[String],
    content_type: &str,
) -> Result<(), Status> {
    let normalized = content_type.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(Status::invalid_argument("content_type must not be empty"));
    }

    let allowed = allowed_prefixes
        .iter()
        .map(|v| v.trim().to_ascii_lowercase())
        .any(|prefix| normalized.starts_with(&prefix));

    if !allowed {
        return Err(Status::invalid_argument(
            "content_type is not allowed for uploads",
        ));
    }

    Ok(())
}

pub fn validate_size(max_size: u64, size: u64) -> Result<(), Status> {
    if size == 0 {
        return Err(Status::invalid_argument("size must be greater than 0"));
    }
    if size > max_size {
        return Err(Status::invalid_argument(format!(
            "size exceeds max allowed bytes ({max_size})"
        )));
    }
    Ok(())
}
