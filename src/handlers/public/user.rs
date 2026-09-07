use axum::{extract::State, Json};

use uuid::Uuid;

use crate::{
    auth::signature::AuthenticatedUser,
    auth::oauth::{exchange_google_auth_code, resolve_user_id},
    db::user::{delete_user_account, put_user_report},
    error::AppError,
    state::AppState,
};

use crate::models::api::auth::OAuthDeleteReq;
use crate::models::api::user::{DeleteAccountResp, ReportUserReq, ReportUserResp};

pub async fn delete_account(
    State(state): State<AppState>,
    auth_user: AuthenticatedUser,
) -> Result<Json<DeleteAccountResp>, AppError> {
    delete_user_account(&state, &auth_user.user_id).await?;

    tracing::info!(user_id = %auth_user.user_id, "User account deleted successfully");

    Ok(Json(DeleteAccountResp {
        status: "success".to_string(),
        message: "Account deleted".to_string(),
    }))
}

pub async fn report_user(
    State(state): State<AppState>,
    auth_user: AuthenticatedUser,
    Json(req): Json<ReportUserReq>,
) -> Result<Json<ReportUserResp>, AppError> {
    let parsed_uuid = Uuid::parse_str(&req.reported_user_id).map_err(|_| {
        AppError::BadRequest("Invalid reportedUserId format: must be UUID".to_string())
    })?;

    if parsed_uuid.get_version_num() != 4 {
        return Err(AppError::BadRequest(
            "Invalid reportedUserId format: must be UUID v4".to_string(),
        ));
    }

    if auth_user.user_id == req.reported_user_id {
        return Err(AppError::BadRequest(
            "Cannot report your own account".to_string(),
        ));
    }

    if req.reason.trim().is_empty() {
        return Err(AppError::BadRequest(
            "Report reason cannot be empty".to_string(),
        ));
    }

    let report_id = format!("rep_{}", Uuid::new_v4());

    let messages_json = serde_json::to_string(&req.messages).map_err(|e| {
        tracing::error!("Failed to serialize report messages: {}", e);
        AppError::Internal("Internal server error".to_string())
    })?;

    put_user_report(
        &state,
        &auth_user.user_id,
        &req.reported_user_id,
        &req.reason,
        &messages_json,
    )
    .await?;

    tracing::info!(
        reporter_id = %auth_user.user_id,
        reported_id = %req.reported_user_id,
        report_id = %report_id,
        "User abuse report submitted"
    );

    Ok(Json(ReportUserResp {
        status: "success".to_string(),
        report_id,
    }))
}

pub async fn delete_account_by_oauth(
    State(state): State<AppState>,
    Json(req): Json<OAuthDeleteReq>,
) -> Result<Json<DeleteAccountResp>, AppError> {
    let identity = exchange_google_auth_code(
        &state,
        &req.code,
        req.code_verifier.as_deref(),
        &req.redirect_uri,
    )
    .await?;

    let user_id = match resolve_user_id(&state, &identity.email).await? {
        Some(id) => id,
        None => {
            return Err(AppError::NotFound(
                "Account not found for this Google email".to_string(),
            ))
        }
    };

    delete_user_account(&state, &user_id).await?;

    tracing::info!(user_id = %user_id, email = %identity.email, "User account deleted successfully via OAuth");

    Ok(Json(DeleteAccountResp {
        status: "success".to_string(),
        message: "Account deleted".to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_user_request_deserialization() {
        let json_str = r#"{
            "reportedUserId": "c4b12d59-8f0a-4a27-a57e-399a0937a012",
            "reason": "Harassment / Spam / Abuse",
            "messages": [
                {
                    "id": "msg_01HZX87",
                    "content": "Abusive text message content",
                    "sender_id": "c4b12d59-8f0a-4a27-a57e-399a0937a012",
                    "created_at": 1756548239000
                }
            ]
        }"#;

        let req: ReportUserReq =
            serde_json::from_str(json_str).expect("should deserialize ReportUserReq");
        assert_eq!(req.reported_user_id, "c4b12d59-8f0a-4a27-a57e-399a0937a012");
        assert_eq!(req.reason, "Harassment / Spam / Abuse");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].id, "msg_01HZX87");
        assert_eq!(req.messages[0].content, "Abusive text message content");
        assert_eq!(
            req.messages[0].sender_id,
            "c4b12d59-8f0a-4a27-a57e-399a0937a012"
        );
        assert_eq!(req.messages[0].created_at, 1756548239000);
    }
}
