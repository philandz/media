# media

Media service for Philand v2.

## Responsibilities

- Initialize upload sessions and return presigned S3 PUT URLs
- Complete upload sessions after object verification
- Store and serve media file metadata

## Runtime Endpoints

- gRPC: `GRPC_HOST:GRPC_PORT` (default `127.0.0.1:50052`)
- HTTP: `HTTP_HOST:HTTP_PORT` (default `127.0.0.1:3002`)
- Health: `GET /health`
- Upload init: `POST /uploads/init`
- Upload complete: `POST /uploads/complete`
- Get file metadata: `GET /files/{id}`
- Get file download URL: `GET /files/{id}/download-url`

The service is gRPC-first (`service.media.MediaService`). HTTP endpoints are thin adapters over the same gRPC handler implementation.

## Local Run

1. Copy `.env.example` to `.env` and adjust values.
2. Ensure MySQL and an S3-compatible endpoint are reachable.
3. Start service:

```bash
cargo run
```

## Required env

- `DATABASE_URL`
- `JWT_SECRET`
- `S3_ENDPOINT`
- `S3_ACCESS_KEY`
- `S3_SECRET_KEY`
- `S3_BUCKET`

See `../libs/configs/README.md` for the full config contract.

## Phase B Local (S3-Compatible + Upload Flow)

### 1) Ensure an S3-compatible endpoint is available

Examples: Garage on homelab, MinIO, AWS S3. The media service only depends on S3 API compatibility.

### 2) Validate S3 compatibility

```bash
export AWS_ENDPOINT_URL=<your-s3-endpoint>
export AWS_DEFAULT_REGION=<your-s3-region>
export AWS_ACCESS_KEY_ID=<your-access-key>
export AWS_SECRET_ACCESS_KEY=<your-secret-key>
export S3_BUCKET=<your-bucket>
bash scripts/media_local_s3_check.sh
```

### 3) Run media + gateway locally

- Set media `.env` with S3 endpoint and credentials.
- Set gateway env with `MEDIA_URL=http://127.0.0.1:3002`.
- Run both services.

### 4) End-to-end upload test

```bash
bash scripts/media_local_e2e.sh
```

This validates init -> presigned PUT -> complete -> metadata lookup through gateway.

### 5) Private bucket file display

For private buckets, request a short-lived download URL from media API:

```bash
curl -X GET "http://127.0.0.1:3000/api/media/files/<file_id>/download-url" \
  -H "authorization: Bearer <access_token>"
```

Response includes a temporary `download_url` and `expires_at`.

## Phase C Homelab (GitOps + Gateway E2E)

### 1) Confirm infra manifests and secrets

- Ensure `infra/deploy/media/deployment.yaml` points to a pushed media image tag.
- Ensure `infra/deploy/media/sealed-secret.yaml` contains valid sealed keys for:
  - `database_url`
  - `jwt_secret`
  - `s3_endpoint`
  - `s3_region`
  - `s3_access_key`
  - `s3_secret_key`
  - `s3_bucket`

### 2) Apply/sync ArgoCD media app

Apply once if the app is not present yet:

```bash
kubectl apply -n argocd -f infra/deploy/argocd/media.yaml
```

Sync and wait for Healthy/Synced:

```bash
argocd app sync media
argocd app wait media --health --sync
```

### 3) Verify runtime in cluster

```bash
kubectl -n philandz get deploy,svc,pods | grep media
kubectl -n philandz logs deploy/media --tail=100
```

### 4) Verify gateway routing to media

Gateway must include `MEDIA_URL=http://media.philandz.svc.cluster.local:3002` and proxy `/api/media/*`.

### 5) Run E2E against public v2 domain

```bash
GATEWAY_URL="https://v2.philand.io.vn" \
MEDIA_E2E_CURL_INSECURE=false \
bash scripts/media_local_e2e.sh
```

If your TLS chain is not trusted in the current shell environment, set:

```bash
MEDIA_E2E_CURL_INSECURE=true
```

### 6) Optional direct S3 compatibility check

```bash
AWS_ENDPOINT_URL="https://s3.philand.io.vn" \
AWS_DEFAULT_REGION="garage" \
AWS_ACCESS_KEY_ID="<key>" \
AWS_SECRET_ACCESS_KEY="<secret>" \
S3_BUCKET="philand-v2-media" \
bash scripts/media_local_s3_check.sh
```
