use std::sync::Arc;

use tonic::{Request, Response, Status};

use crate::manager::biz::MediaBiz;
use crate::pb::service::media::media_service_server::MediaService;
use crate::pb::service::media::{
    CompleteUploadRequest, CompleteUploadResponse, GetFileDownloadUrlRequest,
    GetFileDownloadUrlResponse, GetFileRequest, GetFileResponse, InitUploadRequest,
    InitUploadResponse,
};

use super::metadata::extract_bearer_token;

pub struct MediaHandler {
    biz: Arc<MediaBiz>,
}

impl MediaHandler {
    pub fn new(biz: Arc<MediaBiz>) -> Self {
        Self { biz }
    }
}

#[tonic::async_trait]
impl MediaService for MediaHandler {
    async fn init_upload(
        &self,
        request: Request<InitUploadRequest>,
    ) -> Result<Response<InitUploadResponse>, Status> {
        let token = extract_bearer_token(&request)?;
        let user_id = self.biz.verify_token_subject(&token)?;
        let req = request.into_inner();

        if req.size < 0 {
            return Err(Status::invalid_argument("size must be non-negative"));
        }

        let output = self
            .biz
            .init_upload(&user_id, req.file_name, req.content_type, req.size as u64)
            .await?;

        let required_content_type = output.required_content_type();

        Ok(Response::new(InitUploadResponse {
            upload_id: output.upload_id,
            file_id: output.file_id,
            bucket: output.bucket,
            object_key: output.object_key,
            presigned_url: output.presigned_url,
            expires_at: output.expires_at,
            required_content_type,
        }))
    }

    async fn complete_upload(
        &self,
        request: Request<CompleteUploadRequest>,
    ) -> Result<Response<CompleteUploadResponse>, Status> {
        let token = extract_bearer_token(&request)?;
        let user_id = self.biz.verify_token_subject(&token)?;
        let req = request.into_inner();

        let output = self.biz.complete_upload(&user_id, req.upload_id).await?;
        let upload_status = output.upload_status_enum();

        Ok(Response::new(CompleteUploadResponse {
            file_id: output.file_id,
            bucket: output.bucket,
            object_key: output.object_key,
            upload_status,
        }))
    }

    async fn get_file(
        &self,
        request: Request<GetFileRequest>,
    ) -> Result<Response<GetFileResponse>, Status> {
        let token = extract_bearer_token(&request)?;
        let user_id = self.biz.verify_token_subject(&token)?;
        let req = request.into_inner();

        let output = self.biz.get_file(&user_id, req.file_id).await?;

        Ok(Response::new(GetFileResponse {
            file: Some(output.to_proto(&user_id)),
        }))
    }

    async fn get_file_download_url(
        &self,
        request: Request<GetFileDownloadUrlRequest>,
    ) -> Result<Response<GetFileDownloadUrlResponse>, Status> {
        let token = extract_bearer_token(&request)?;
        let user_id = self.biz.verify_token_subject(&token)?;
        let req = request.into_inner();

        let output = self.biz.get_file_download_url(&user_id, req.file_id).await?;

        Ok(Response::new(GetFileDownloadUrlResponse {
            file_id: output.file_id,
            download_url: output.download_url,
            expires_at: output.expires_at,
        }))
    }
}
