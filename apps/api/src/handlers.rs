use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    auth_extractor::AuthUser,
    error::AppError,
    jwt,
    models::{User, UserResponse},
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
}

pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

fn validate_credentials(email: &str, password: &str) -> Result<(), AppError> {
    if !email.contains('@') || email.len() < 3 {
        return Err(AppError::Validation("invalid email".into()));
    }
    if password.len() < 8 {
        return Err(AppError::Validation(
            "password must be at least 8 characters".into(),
        ));
    }
    Ok(())
}

pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> Result<Json<AuthResponse>, AppError> {
    validate_credentials(&body.email, &body.password)?;

    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Argon2::default()
        .hash_password(body.password.as_bytes(), &salt)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("password hashing failed: {e}")))?
        .to_string();

    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (email, password_hash) VALUES ($1, $2)
         RETURNING id, email, password_hash, created_at",
    )
    .bind(&body.email)
    .bind(&password_hash)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err) if db_err.is_unique_violation() => AppError::EmailTaken,
        _ => AppError::Internal(e.into()),
    })?;

    let token = jwt::issue(user.id, &state.jwt_secret).map_err(AppError::Internal)?;

    Ok(Json(AuthResponse { token }))
}

pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> Result<Json<AuthResponse>, AppError> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, created_at FROM users WHERE email = $1",
    )
    .bind(&body.email)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::InvalidCredentials)?;

    let parsed_hash = PasswordHash::new(&user.password_hash)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("stored hash unparsable: {e}")))?;

    Argon2::default()
        .verify_password(body.password.as_bytes(), &parsed_hash)
        .map_err(|_| AppError::InvalidCredentials)?;

    let token = jwt::issue(user.id, &state.jwt_secret).map_err(AppError::Internal)?;

    Ok(Json(AuthResponse { token }))
}

pub async fn me(
    State(state): State<AppState>,
    AuthUser(user_id): AuthUser,
) -> Result<Json<UserResponse>, AppError> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, created_at FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .ok_or(AppError::Unauthorized)?;

    Ok(Json(user.into()))
}
