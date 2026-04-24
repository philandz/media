use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tonic::{metadata::MetadataValue, Request as GrpcRequest, Status};

use crate::handler::MediaHandler;
use crate::pb::service::media::media_service_server::MediaService;
use crate::pb::service::media::{
    CompleteUploadRequest as PbCompleteUploadRequest, DeleteFileRequest as PbDeleteFileRequest,
    GetFileDownloadUrlRequest as PbGetFileDownloadUrlRequest, GetFileRequest as PbGetFileRequest,
    InitUploadRequest as PbInitUploadRequest, ListFilesRequest as PbListFilesRequest,
};
use crate::pb::shared::media::{MediaFileStatus, MediaUploadStatus};
use philand_error::ErrorEnvelope;

type ApiResult<T> = Result<T, (StatusCode, Json<ErrorEnvelope>)>;
type HttpState = Arc<MediaHandler>;

pub fn router() -> Router<HttpState> {
    Router::new()
        .route("/uploads/init", post(init_upload))
        .route("/uploads/complete", post(complete_upload))
        .route("/files", get(list_files))
        .route("/files/{id}", get(get_file))
        .route("/files/{id}", delete(delete_file))
        .route("/files/{id}/download-url", get(get_file_download_url))
        .route("/public/{*object_key}", get(get_public_object))
}

pub async fn request_logging_middleware(request: axum::extract::Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let response = next.run(request).await;
    tracing::info!(method = %method, path = %path, status = %response.status(), "media request");
    response
}

// ---------------------------------------------------------------------------
// Request / Response DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct InitUploadRequest {
    pub file_name: String,
    pub content_type: String,
    pub size: u64,
    pub org_id: Option<String>,
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
    pub etag: String,
    pub confirmed_size: i64,
    pub public_url: String,
}

#[derive(Debug, Serialize)]
pub struct MediaFileResponse {
    pub file_id: String,
    pub bucket: String,
    pub object_key: String,
    pub content_type: String,
    pub original_name: String,
    pub size: u64,
    pub status: String,
    pub org_id: String,
    pub public_url: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct ListFilesResponse {
    pub files: Vec<MediaFileResponse>,
    pub total: i32,
}

#[derive(Debug, Serialize)]
pub struct FileDownloadUrlResponse {
    pub file_id: String,
    pub download_url: String,
    pub expires_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct ListFilesQuery {
    pub org_id: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

fn map_file_proto(file: crate::pb::shared::media::MediaFile) -> MediaFileResponse {
    let created_at = file.base.as_ref().map(|b| b.created_at).unwrap_or_default();
    let file_id = file.base.map(|b| b.id).unwrap_or_default();
    MediaFileResponse {
        file_id,
        bucket: file.bucket,
        object_key: file.object_key,
        content_type: file.content_type,
        original_name: file.original_name,
        size: file.size.max(0) as u64,
        status: file_status_string(file.file_status),
        org_id: file.org_id,
        public_url: file.public_url,
        created_at,
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn init_upload(
    State(handler): State<HttpState>,
    headers: HeaderMap,
    Json(body): Json<InitUploadRequest>,
) -> ApiResult<(StatusCode, Json<InitUploadResponse>)> {
    let size = i64::try_from(body.size)
        .map_err(|_| map_status(Status::invalid_argument("size is too large")))?;

    let grpc_req = with_auth(
        &headers,
        PbInitUploadRequest {
            file_name: body.file_name,
            content_type: body.content_type,
            size,
            org_id: body.org_id.unwrap_or_default(),
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
        etag: output.etag,
        confirmed_size: output.confirmed_size,
        public_url: output.public_url,
    }))
}

async fn list_files(
    State(handler): State<HttpState>,
    headers: HeaderMap,
    Query(params): Query<ListFilesQuery>,
) -> ApiResult<Json<ListFilesResponse>> {
    let grpc_req = with_auth(
        &headers,
        PbListFilesRequest {
            org_id: params.org_id.unwrap_or_default(),
            limit: params.limit.unwrap_or(20),
            offset: params.offset.unwrap_or(0),
        },
    )?;

    let output = handler
        .list_files(grpc_req)
        .await
        .map_err(map_status)?
        .into_inner();

    Ok(Json(ListFilesResponse {
        files: output.files.into_iter().map(map_file_proto).collect(),
        total: output.total,
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

    Ok(Json(map_file_proto(file)))
}

async fn delete_file(
    State(handler): State<HttpState>,
    headers: HeaderMap,
    Path(file_id): Path<String>,
) -> ApiResult<StatusCode> {
    let grpc_req = with_auth(&headers, PbDeleteFileRequest { file_id })?;

    handler.delete_file(grpc_req).await.map_err(map_status)?;

    Ok(StatusCode::NO_CONTENT)
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

async fn get_public_object(
    State(handler): State<HttpState>,
    Path(object_key): Path<String>,
) -> ApiResult<Response> {
    let output = handler
        .get_public_object_by_key(&object_key)
        .await
        .map_err(map_status)?;

    let mut response = Response::new(Body::from(output.bytes));
    *response.status_mut() = StatusCode::OK;
    let content_type = HeaderValue::from_str(&output.content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    response
        .headers_mut()
        .insert(axum::http::header::CONTENT_TYPE, content_type);
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    Ok(response)
}
