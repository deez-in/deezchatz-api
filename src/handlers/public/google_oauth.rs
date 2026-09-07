use axum::{extract::State, Json};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::{
    auth::oauth::{resolve_user_id, verify_google_id_token},
    db::{keys::pending_reg_key, temp::set_temp_json},
    error::AppError,
    models::api::auth::{GoogleIdTokenReq, GoogleIdTokenResp},
    models::db::temp_registration::TempRegistration,
    state::AppState,
};

const OAUTH_TTL_SECS: u64 = 600; // 10 minutes

pub async fn verify_id_token(
    State(state): State<AppState>,
    Json(req): Json<GoogleIdTokenReq>,
) -> Result<Json<GoogleIdTokenResp>, AppError> {
    let identity = verify_google_id_token(&state, &req.id_token).await?;
    let user_id = resolve_user_id(&state, &identity.email)
        .await?
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let state_token =
        cache_pending_registration(&state, user_id.clone(), &identity, &req.i_key).await?;

    tracing::info!(
        user_id = %user_id,
        email = %identity.email,
        "Google ID token verified and pending registration cached"
    );

    Ok(Json(GoogleIdTokenResp {
        status: "success".to_string(),
        user_id,
        state: state_token,
        email: identity.email,
        name: identity.name,
        picture: identity.picture,
    }))
}

async fn cache_pending_registration(
    state: &AppState,
    user_id: String,
    identity: &crate::auth::oauth::OAuthIdentity,
    i_key: &str,
) -> Result<String, AppError> {
    let now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let pending_data = TempRegistration {
        user_id,
        i_key: i_key.to_string(),
        email: identity.email.clone(),
        name: identity.name.clone().unwrap_or_default(),
        picture: identity.picture.clone(),
        created_at: now_millis,
    };

    let json_val = serde_json::to_string(&pending_data).map_err(|e| {
        tracing::error!("Failed to serialize pending data: {}", e);
        AppError::Internal("Serialization error".to_string())
    })?;

    let state_token = Uuid::new_v4().to_string();
    let redis_key = pending_reg_key(&state_token);
    set_temp_json(state, &redis_key, &json_val, OAUTH_TTL_SECS).await?;

    Ok(state_token)
}
