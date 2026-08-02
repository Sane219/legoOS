use axum::{
    extract::{FromRef, FromRequestParts},
    http::request::Parts,
};
use uuid::Uuid;

use crate::{error::AppError, jwt, state::AppState};

fn user_id_from_token(token: &str, jwt_secret: &str) -> Result<Uuid, AppError> {
    let claims = jwt::verify(token, jwt_secret).map_err(|_| AppError::Unauthorized)?;
    Uuid::parse_str(&claims.sub).map_err(|_| AppError::Unauthorized)
}

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

        Ok(AuthUser(user_id_from_token(token, &app_state.jwt_secret)?))
    }
}

/// Same as `AuthUser`, but also accepts the token as a `?token=` query parameter, falling
/// back to it only when there's no `Authorization` header. Browsers can't set custom
/// headers on a WebSocket handshake, so this is the only way for a WS route to authenticate
/// a token issued to client-side JS. Use `AuthUser` (header-only) everywhere else — a token
/// in a URL can end up in server access logs, so this trades that off only where required.
pub struct WsAuthUser(pub Uuid);

impl<S> FromRequestParts<S> for WsAuthUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        let header_token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|header| header.strip_prefix("Bearer "))
            .map(str::to_string);

        let token = match header_token {
            Some(token) => token,
            None => parts
                .uri
                .query()
                .and_then(|query| {
                    url::form_urlencoded::parse(query.as_bytes())
                        .find(|(key, _)| key == "token")
                        .map(|(_, value)| value.into_owned())
                })
                .ok_or(AppError::Unauthorized)?,
        };

        Ok(WsAuthUser(user_id_from_token(
            &token,
            &app_state.jwt_secret,
        )?))
    }
}
