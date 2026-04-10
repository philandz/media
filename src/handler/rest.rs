use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tonic::{metadata::MetadataValue, Request as GrpcRequest, Status};

use crate::handler::MediaHandler;
use crate::pb::service::media::media_service_server::MediaService;
use crate::pb::service::media::{
    CompleteUploadRequest as PbCompleteUploadRequest,
    GetFileDownloadUrlRequest as PbGetFileDownloadUrlRequest,
    GetFileRequest as PbGetFileRequest,
    InitUploadRequest as PbInitUploadRequest,
};
use crate::pb::shared::media::{MediaFileStatus, MediaUploadStatus};
use philand_error::ErrorEnvelope;

type ApiResult<T> = Result<T, (StatusCode, Json<ErrorEnvelope>)>;
type HttpState = Arc<MediaHandler>;

pub fn router() -> Router<HttpState> {
    Router::new()
        .route("/uploads/init", post(init_upload))
        .route("/uploads/complete", post(complete_upload))
        .route("/files/{id}", get(get_file))
        .route("/files/{id}/download-url", get(get_file_download_url))
}

pub async fn request_logging_middleware(request: axum::extract::Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let response = next.run(request).await;
    tracing::info!(method = %method, path = %path, status = %response.status(), "media request");
    response
}

#[derive(Debug, Deserialize)]
pub struct InitUploadRequest {
    pub file_name: String,
    pub content_type: String,
    pub size: u64,
}

#[derive(Debug, Serialize)]
pub struct InitUploadResponse {
    pub upload_id: String,
    pub file_id: String,
    pub bucket: String,
    pub object_key: String,
    pub presigned_url: String,
    pub expires_at: i64,
    pub required_headers: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct CompleteUploadRequest {
    pub upload_id: String,
}

#[derive(Debug, Serialize)]
pub struct CompleteUploadResponse {
    pub file_id: String,
    pub status: String,
    pub object_key: String,
    pub bucket: String,
}

#[derive(Debug, Serialize)]
pub struct MediaFileResponse {
    pub file_id: String,
    pub bucket: String,
    pub object_key: String,
    pub content_type: String,
    pub size: u64,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct FileDownloadUrlResponse {
    pub file_id: String,
    pub download_url: String,
    pub expires_at: i64,
}

fn map_status(status: Status) -> (StatusCode, Json<ErrorEnvelope>) {
    let (http, envelope) = philand_error::http_error_from_tonic_status(&status);
    (http, Json(envelope))
}

fn with_auth<T>(headers: &HeaderMap, req: T) -> ApiResult<GrpcRequest<T>> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| map_status(Status::unauthenticated("Missing Authorization header")))?;

    if !auth.starts_with("Bearer ") {
        return Err(map_status(Status::unauthenticated(
            "Authorization header must start with 'Bearer '",
        )));
    }

    let mut grpc_req = GrpcRequest::new(req);
    let value = MetadataValue::try_from(auth)
        .map_err(|_| map_status(Status::unauthenticated("Invalid Authorization header")))?;
    grpc_req.metadata_mut().insert("authorization", value);
    Ok(grpc_req)
}

fn upload_status_string(v: i32) -> String {
    match MediaUploadStatus::try_from(v).unwrap_or(MediaUploadStatus::MusNone) {
        MediaUploadStatus::MusInit => "init",
        MediaUploadStatus::MusUploading => "uploading",
        MediaUploadStatus::MusReady => "ready",
        MediaUploadStatus::MusFailed => "failed",
        MediaUploadStatus::MusExpired => "expired",
        MediaUploadStatus::MusNone => "none",
    }
    .to_string()
}

fn file_status_string(v: i32) -> String {
    match MediaFileStatus::try_from(v).unwrap_or(MediaFileStatus::MfsNone) {
        MediaFileStatus::MfsReady => "ready",
        MediaFileStatus::MfsDeleted => "deleted",
        MediaFileStatus::MfsNone => "none",
    }
    .to_string()
}

fn upload_size_to_i64(size: u64) -> ApiResult<i64> {
    i64::try_from(size)
        .map_err(|_| map_status(Status::invalid_argument("size is too large to process")))
}

async fn init_upload(
    State(handler): State<HttpState>,
    headers: HeaderMap,
    Json(body): Json<InitUploadRequest>,
) -> ApiResult<(StatusCode, Json<InitUploadResponse>)> {
    let size = upload_size_to_i64(body.size)?;
    let grpc_req = with_auth(
        &headers,
        PbInitUploadRequest {
            file_name: body.file_name,
            content_type: body.content_type,
            size,
        },
    )?;

    let output = handler
        .init_upload(grpc_req)
        .await
        .map_err(map_status)?
        .into_inner();

    Ok((
        StatusCode::CREATED,
        Json(InitUploadResponse {
            upload_id: output.upload_id,
            file_id: output.file_id,
            bucket: output.bucket,
            object_key: output.object_key,
            presigned_url: output.presigned_url,
            expires_at: output.expires_at,
            required_headers: serde_json::json!({
                "content-type": output.required_content_type,
            }),
        }),
    ))
}

async fn complete_upload(
    State(handler): State<HttpState>,
    headers: HeaderMap,
    Json(body): Json<CompleteUploadRequest>,
) -> ApiResult<Json<CompleteUploadResponse>> {
    let grpc_req = with_auth(
        &headers,
        PbCompleteUploadRequest {
            upload_id: body.upload_id,
        },
    )?;

    let output = handler
        .complete_upload(grpc_req)
        .await
        .map_err(map_status)?
        .into_inner();

    Ok(Json(CompleteUploadResponse {
        file_id: output.file_id,
        status: upload_status_string(output.upload_status),
        object_key: output.object_key,
        bucket: output.bucket,
    }))
}

async fn get_file(
    State(handler): State<HttpState>,
    headers: HeaderMap,
    Path(file_id): Path<String>,
) -> ApiResult<Json<MediaFileResponse>> {
    let grpc_req = with_auth(&headers, PbGetFileRequest { file_id })?;

    let output = handler
        .get_file(grpc_req)
        .await
        .map_err(map_status)?
        .into_inner();

    let file = output
        .file
        .ok_or_else(|| map_status(Status::not_found("file not found")))?;

    let created_at = file.base.as_ref().map(|v| v.created_at).unwrap_or_default();
    let file_id = file.base.map(|v| v.id).unwrap_or_default();

    Ok(Json(MediaFileResponse {
        file_id,
        bucket: file.bucket,
        object_key: file.object_key,
        content_type: file.content_type,
        size: file.size.max(0) as u64,
        status: file_status_string(file.file_status),
        created_at,
    }))
}

async fn get_file_download_url(
    State(handler): State<HttpState>,
    headers: HeaderMap,
    Path(file_id): Path<String>,
) -> ApiResult<Json<FileDownloadUrlResponse>> {
    let grpc_req = with_auth(&headers, PbGetFileDownloadUrlRequest { file_id })?;

    let output = handler
        .get_file_download_url(grpc_req)
        .await
        .map_err(map_status)?
        .into_inner();

    Ok(Json(FileDownloadUrlResponse {
        file_id: output.file_id,
        download_url: output.download_url,
        expires_at: output.expires_at,
    }))
}
