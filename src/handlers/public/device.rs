use axum::{extract::State, Json};

use crate::{
    auth::signature::AuthenticatedUser,
    db::keys::{device_sk, user_pk},
    error::AppError,
    models::api::device::{UpdateFcmTokenReq, UpdateFcmTokenResp},
    state::AppState,
};

pub async fn update_fcm_token(
    State(state): State<AppState>,
    auth_user: AuthenticatedUser,
    Json(req): Json<UpdateFcmTokenReq>,
) -> Result<Json<UpdateFcmTokenResp>, AppError> {
    uuid::Uuid::parse_str(&req.device_id)
        .map_err(|_| AppError::BadRequest("Invalid deviceId format: must be UUID".to_string()))?;

    let pk = user_pk(&auth_user.user_id);
    let sk = device_sk(&req.device_id);

    // Update FCM token in DynamoDB
    crate::db::device::update_device_fcm_token(&state, &pk, &sk, &req.fcm_token).await?;

    tracing::info!(user_id = %auth_user.user_id, device_id = %req.device_id, "FCM token updated successfully");

    Ok(Json(UpdateFcmTokenResp {
        status: "success".to_string(),
        message: "FCM token updated".to_string(),
    }))
}
