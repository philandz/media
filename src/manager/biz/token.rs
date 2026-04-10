use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tonic::Status;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    pub org_id: String,
    pub exp: usize,
}

pub fn decode_claims(jwt_secret: &str, token: &str) -> Result<Claims, Status> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map(|v| v.claims)
    .map_err(|_| Status::unauthenticated("Invalid token"))
}
