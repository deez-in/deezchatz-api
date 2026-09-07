use axum::{
    extract::{Path, State},
    Json,
};

use crate::{
    auth::signature::AuthenticatedUser,
    db::{
        device::pop_opk,
        keys::{profile_sk, user_pk},
        lib::get_item,
        user::resolve_user_by_identifier,
    },
    error::AppError,
    models::db::profile::Profile,
    state::AppState,
};

use crate::models::api::bundle::{Opk, PreKeyBundleResp, SyncBundleResp};

pub async fn get_bundle(
    State(state): State<AppState>,
    Path(identifier): Path<String>,
    _auth_user: AuthenticatedUser, // Requires valid signature
) -> Result<Json<PreKeyBundleResp>, AppError> {
    if identifier.trim().is_empty() {
        return Err(AppError::BadRequest("Missing identifier".to_string()));
    }

    // 1. Dual-path lookup strategy
    let is_email = identifier.contains('@');
    let is_phone = identifier.starts_with('+')
        || (!identifier.is_empty() && identifier.chars().all(|c| c.is_ascii_digit()));

    if !is_email && !is_phone {
        // Must be a valid UUID
        uuid::Uuid::parse_str(&identifier).map_err(|_| {
            AppError::BadRequest(
                "Invalid identifier format: must be email, phone number, or UUID".to_string(),
            )
        })?;
    }

    let mut retries = 5;
    loop {
        let profile_item_opt = if is_email || is_phone {
            // Pointer lookup -> profile
            resolve_user_by_identifier(&state, &identifier).await?
        } else {
            // Assume userId -> query base table directly
            let pk = user_pk(&identifier);
            get_item(&state, &pk, profile_sk()).await?
        };

        let item = profile_item_opt
            .ok_or_else(|| AppError::NotFound("Requested user not found".to_string()))?;

        // We got the profile. Ensure we know the pk to pop OPK later.
        let pk = item
            .get("pk")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| {
                AppError::Internal("Database error: missing pk on user profile".to_string())
            })?
            .clone();

        let user_id = pk.strip_prefix("USER#").unwrap_or(&pk).to_string();

        let profile = Profile::from(item);

        let device_id = profile.device_id.unwrap_or_default();
        let identity_key = profile.identity_key.unwrap_or_default();
        let signed_pre_key = profile.signed_prekey.unwrap_or_default();
        let signature = profile.signature.unwrap_or_default();
        let phone = profile.phone;
        let picture = profile.picture;

        // Get last OPK with its index
        let mut opk = None;
        if !profile.opks.is_empty() {
            let last_index = profile.opks.len() - 1;
            let last_opk = profile
                .opks
                .last()
                .ok_or_else(|| AppError::Internal("OPKs unexpectedly empty".to_string()))?
                .clone();

            opk = Some(Opk {
                id: last_index,
                key: last_opk.clone(),
            });

            match pop_opk(&state, &pk, profile_sk(), last_index, &last_opk).await {
                Ok(_) => {}
                Err(AppError::Conflict(_)) => {
                    if retries > 0 {
                        retries -= 1;
                        let delay_ms = 50u64 * 2u64.pow((4 - retries) as u32);
                        tracing::warn!(
                            retries_left = retries,
                            delay_ms = delay_ms,
                            "OPK conflict detected. Retrying get_bundle with backoff..."
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        continue;
                    } else {
                        return Err(AppError::Conflict(
                            "OPK conflict: too many retries".to_string(),
                        ));
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        return Ok(Json(PreKeyBundleResp {
            user_id,
            device_id,
            identity_key,
            spk_id: 1,
            signed_pre_key,
            signature,
            phone,
            picture,
            opk,
        }));
    }
}

pub async fn get_sync_bundle(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    _auth_user: AuthenticatedUser,
) -> Result<Json<SyncBundleResp>, AppError> {
    if user_id.trim().is_empty() {
        return Err(AppError::BadRequest("Missing userId".to_string()));
    }

    let parsed_uuid = uuid::Uuid::parse_str(&user_id)
        .map_err(|_| AppError::BadRequest("Invalid userId format: must be UUID".to_string()))?;
    if parsed_uuid.get_version_num() != 4 {
        return Err(AppError::BadRequest(
            "Invalid userId format: must be UUID v4".to_string(),
        ));
    }

    let pk = user_pk(&user_id);
    let item = get_item(&state, &pk, profile_sk())
        .await?
        .ok_or_else(|| AppError::NotFound("Requested user not found".to_string()))?;

    let resolved_user_id = item
        .get("pk")
        .and_then(|v| v.as_s().ok())
        .and_then(|pk| pk.strip_prefix("USER#"))
        .unwrap_or(&user_id)
        .to_string();

    let profile = Profile::from(item);
    let identity_key = profile
        .identity_key
        .ok_or_else(|| AppError::Internal("Database error: missing identity key".to_string()))?;

    Ok(Json(SyncBundleResp {
        user_id: resolved_user_id,
        identity_key,
        picture: profile.picture,
        display_name: profile.name,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prekey_bundle_serialization() {
        let bundle = PreKeyBundleResp {
            user_id: "test-user-id".to_string(),
            device_id: "test-device-id".to_string(),
            identity_key: "ik-base64".to_string(),
            spk_id: 1,
            signed_pre_key: "spk-base64".to_string(),
            signature: "sig-base64".to_string(),
            phone: Some("+1234567890".to_string()),
            picture: Some("https://example.com/pic.jpg".to_string()),
            opk: Some(Opk {
                id: 42,
                key: "opk-key-base64".to_string(),
            }),
        };

        let json = serde_json::to_string(&bundle).expect("should serialize PreKeyBundleResp");
        let v: serde_json::Value = serde_json::from_str(&json).expect("should parse JSON");

        assert_eq!(v["spkId"], 1);
        assert_eq!(v["userId"], "test-user-id");
        assert_eq!(v["deviceId"], "test-device-id");
        assert_eq!(v["identityKey"], "ik-base64");
        assert_eq!(v["signedPreKey"], "spk-base64");
        assert_eq!(v["signature"], "sig-base64");
        assert_eq!(v["phone"], "+1234567890");
        assert_eq!(v["picture"], "https://example.com/pic.jpg");
        assert_eq!(v["opk"]["id"], 42);
        assert_eq!(v["opk"]["key"], "opk-key-base64");
    }
}
