use tonic::{Request, Status};

#[allow(clippy::result_large_err)]
pub fn extract_bearer_token<T>(request: &Request<T>) -> Result<String, Status> {
    let auth = request
        .metadata()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Status::unauthenticated("Missing authorization metadata"))?;

    auth.strip_prefix("Bearer ")
        .map(|t| t.to_string())
        .ok_or_else(|| Status::unauthenticated("Authorization metadata must start with 'Bearer '"))
}
