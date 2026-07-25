use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use uuid::Uuid;

use crate::{error::AppError, jwt, state::AppState};

/// Extracts and validates the JWT from the `Authorization: Bearer <token>` header,
/// injecting the authenticated user's id into the handler.
pub struct AuthUser(pub Uuid);

impl<S> FromRequestParts<S> for AuthUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(AppError::Unauthorized)?;

        let token = header
            .strip_prefix("Bearer ")
            .ok_or(AppError::Unauthorized)?;

        let claims =
            jwt::verify(token, &app_state.jwt_secret).map_err(|_| AppError::Unauthorized)?;

        let user_id = Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized)?;

        Ok(AuthUser(user_id))
    }
}
