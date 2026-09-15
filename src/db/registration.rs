use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::{
    auth::google_oauth::OAuthIdentity,
    db::{keys::pending_reg_key, temp::set_temp_json, user::resolve_user_id_by_email},
    error::AppError,
    models::api::auth::GoogleIdTokenResp,
    models::db::temp_registration::TempRegistration,
    state::AppState,
};

pub const OAUTH_TTL_SECS: u64 = 600; // 10 minutes

pub async fn initiate_registration(
    state: &AppState,
    identity: &OAuthIdentity,
    i_key: &str,
) -> Result<GoogleIdTokenResp, AppError> {
    let user_id = resolve_user_id_by_email(state, &identity.email)
        .await?
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let state_token = cache_pending_registration(state, user_id.clone(), identity, i_key).await?;

    tracing::info!(
        user_id = %user_id,
        email = %identity.email,
        "Pending registration cached for OAuth identity"
    );

    Ok(GoogleIdTokenResp {
        status: "success".to_string(),
        user_id,
        state: state_token,
        email: identity.email.clone(),
        name: identity.name.clone(),
        picture: identity.picture.clone(),
    })
}

async fn cache_pending_registration(
    state: &AppState,
    user_id: String,
    identity: &OAuthIdentity,
    i_key: &str,
) -> Result<String, AppError> {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let pending_data = TempRegistration {
        user_id,
        i_key: i_key.to_string(),
        email: identity.email.clone(),
        name: identity.name.clone().unwrap_or_default(),
        picture: identity.picture.clone(),
        created_at: now_secs,
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
