use axum::{extract::FromRequestParts, http::request::Parts};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    crypto::verify_signature,
    db::{keys::user_pk, lib::get_item},
    error::AppError,
    models::db::profile::Profile,
    state::AppState,
};

pub struct AuthenticatedUser {
    pub user_id: String,
}

pub fn validate_timestamp(timestamp_str: &str, now_secs: u64) -> Result<u64, AppError> {
    let timestamp: u64 = timestamp_str
        .parse()
        .map_err(|_| AppError::Unauthorized("Invalid timestamp format".to_string()))?;

    // Allow +/- 10 seconds drift
    let drift = now_secs.abs_diff(timestamp);

    if drift > 10 {
        return Err(AppError::Unauthorized(
            "Timestamp expired or too far in the future".to_string(),
        ));
    }

    Ok(timestamp)
}

impl FromRequestParts<AppState> for AuthenticatedUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let user_id = parts
            .headers
            .get("X-User-Id")
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing X-User-Id header".to_string()))?;

        // Validate UUID v4 format
        let parsed_uuid = uuid::Uuid::parse_str(user_id)
            .map_err(|_| AppError::Unauthorized("Invalid user ID format".to_string()))?;
        if parsed_uuid.get_version_num() != 4 {
            return Err(AppError::Unauthorized(
                "Invalid user ID format: must be UUID v4".to_string(),
            ));
        }

        let timestamp_str = parts
            .headers
            .get("X-Timestamp")
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing X-Timestamp header".to_string()))?;

        let signature_b64 = parts
            .headers
            .get("X-Signature")
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing X-Signature header".to_string()))?;

        let vrf_b64 = parts
            .headers
            .get("X-Vrf")
            .and_then(|h| h.to_str().ok())
            .ok_or_else(|| AppError::Unauthorized("Missing X-Vrf header".to_string()))?;

        // 1. Timestamp validation (prevent replay attacks)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        validate_timestamp(timestamp_str, now)?;

        // 2. Fetch User Profile
        let pk = user_pk(user_id);
        let sk = crate::db::keys::profile_sk();
        let item_opt = get_item(state, &pk, sk).await?;

        let item = item_opt.ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

        let profile = Profile::from(item);

        let signed_prekey_b64 = profile
            .signed_prekey
            .ok_or_else(|| AppError::Unauthorized("Missing signed_prekey for user".to_string()))?;

        // 3. Verify Signature
        // The signed payload is: userId + timestamp
        let payload = format!("{}{}", user_id, timestamp_str);

        let signed_prekey_bytes = crate::crypto::decode_b64_key(
            &signed_prekey_b64,
            crate::crypto::PUBLIC_KEY_LENGTH,
            "signedPreKey",
        )?;
        let signature_bytes = crate::crypto::decode_b64_key(
            signature_b64,
            crate::crypto::SIGNATURE_LENGTH,
            "signature",
        )?;
        let expected_vrf_bytes =
            crate::crypto::decode_b64_key(vrf_b64, crate::crypto::VRF_LENGTH, "vrf")?;

        let public_key: [u8; 33] = signed_prekey_bytes
            .try_into()
            .map_err(|_| AppError::Internal("Key length invariant violated".into()))?;
        let sig: [u8; 96] = signature_bytes
            .try_into()
            .map_err(|_| AppError::Internal("Signature length invariant violated".into()))?;

        match verify_signature(&public_key, payload.as_bytes(), &sig) {
            Ok(output_vrf) => {
                if output_vrf != expected_vrf_bytes.as_slice() {
                    return Err(AppError::Unauthorized("VRF mismatch".to_string()));
                }

                // Redis replay protection: TTL must outlast the timestamp drift window (+10s and -10s).
                // A fixed TTL of 20 seconds ensures a replay signature remains cached until
                // its underlying timestamp expires completely.
                let replay_ttl = 20;
                let replay_key = format!("replay:sig:{}", signature_b64);
                let was_set =
                    crate::db::temp::set_temp_json_nx(state, &replay_key, "1", replay_ttl).await?;
                if !was_set {
                    return Err(AppError::Unauthorized("Replay attack detected".to_string()));
                }

                Ok(AuthenticatedUser {
                    user_id: user_id.to_string(),
                })
            }
            Err(_) => Err(AppError::Unauthorized("Invalid signature".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_timestamp_success() {
        let now = 1_700_000_000u64;

        // Exact match
        assert!(validate_timestamp("1700000000", now).is_ok());

        // 10 seconds in past (boundary)
        assert!(validate_timestamp("1699999990", now).is_ok());

        // 10 seconds in future (boundary)
        assert!(validate_timestamp("1700000010", now).is_ok());

        // 5 seconds in past
        assert!(validate_timestamp("1699999995", now).is_ok());
    }

    #[test]
    fn test_validate_timestamp_expired() {
        let now = 1_700_000_000u64;

        // 11 seconds in past -> expired
        let res = validate_timestamp("1699999989", now);
        assert!(res.is_err());
        match res.unwrap_err() {
            AppError::Unauthorized(msg) => {
                assert_eq!(msg, "Timestamp expired or too far in the future");
            }
            _ => panic!("Expected Unauthorized error"),
        }
    }

    #[test]
    fn test_validate_timestamp_future() {
        let now = 1_700_000_000u64;

        // 11 seconds in future -> too far in future
        let res = validate_timestamp("1700000011", now);
        assert!(res.is_err());
        match res.unwrap_err() {
            AppError::Unauthorized(msg) => {
                assert_eq!(msg, "Timestamp expired or too far in the future");
            }
            _ => panic!("Expected Unauthorized error"),
        }
    }

    #[test]
    fn test_validate_timestamp_invalid_format() {
        let now = 1_700_000_000u64;

        let res = validate_timestamp("not_a_number", now);
        assert!(res.is_err());
        match res.unwrap_err() {
            AppError::Unauthorized(msg) => {
                assert_eq!(msg, "Invalid timestamp format");
            }
            _ => panic!("Expected Unauthorized error"),
        }
    }
}
