use axum::{
    extract::{rejection::JsonRejection, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    crypto::{decode_b64_key, verify_signature, PUBLIC_KEY_LENGTH, SIGNATURE_LENGTH, VRF_LENGTH},
    db::{
        keys::{profile_sk, user_pk},
        lib::get_item,
    },
    models::{api::mqtt::VerifyMqttClientReq, db::profile::Profile},
    state::AppState,
};

pub const SIGNATURE_CHAR_LEN: usize = 128;
pub const VRF_CHAR_LEN: usize = 44;
pub const TIMESTAMP_CHAR_LEN: usize = 10;
pub const EXPECTED_PASSWORD_LEN: usize = SIGNATURE_CHAR_LEN + VRF_CHAR_LEN + TIMESTAMP_CHAR_LEN; // 182

pub struct UnpackedPassword<'a> {
    pub signature_b64: &'a str,
    pub vrf_b64: &'a str,
    pub timestamp_str: &'a str,
}

pub fn split_password(password: &str) -> Result<UnpackedPassword<'_>, &'static str> {
    if !password.is_ascii() || password.len() != EXPECTED_PASSWORD_LEN {
        return Err("Password must be exactly 182 ASCII characters");
    }

    let signature_b64 = &password[..SIGNATURE_CHAR_LEN];
    let vrf_b64 = &password[SIGNATURE_CHAR_LEN..SIGNATURE_CHAR_LEN + VRF_CHAR_LEN];
    let timestamp_str = &password[SIGNATURE_CHAR_LEN + VRF_CHAR_LEN..];

    Ok(UnpackedPassword {
        signature_b64,
        vrf_b64,
        timestamp_str,
    })
}

async fn do_verify(state: &AppState, req: &VerifyMqttClientReq) -> Result<(), &'static str> {
    // 1. Unpack fixed-length password
    let unpacked = split_password(&req.password)?;

    // 2. Validate UUID formats
    let parsed_user_id =
        uuid::Uuid::parse_str(&req.username).map_err(|_| "Invalid username: must be UUID v4")?;
    if parsed_user_id.get_version_num() != 4 {
        return Err("Invalid username: must be UUID v4");
    }

    let parsed_client_id =
        uuid::Uuid::parse_str(&req.clientid).map_err(|_| "Invalid clientid: must be UUID v4")?;
    if parsed_client_id.get_version_num() != 4 {
        return Err("Invalid clientid: must be UUID v4");
    }

    // 3. Validate timestamp drift (epoch seconds, +/- 10s)
    let timestamp: u64 = unpacked
        .timestamp_str
        .parse()
        .map_err(|_| "Invalid timestamp: not an integer")?;

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let drift = now_secs.abs_diff(timestamp);
    if drift > 10 {
        return Err("Timestamp expired or too far in the future");
    }

    // 4. Fetch user profile from DynamoDB
    let pk = user_pk(&req.username);
    let sk = profile_sk();
    let item_opt = get_item(state, &pk, sk)
        .await
        .map_err(|_| "Database lookup failed")?;

    let item = item_opt.ok_or("User profile not found")?;
    let profile = Profile::from(item);

    // 5. Verify clientId matches registered deviceId
    match &profile.device_id {
        Some(dev_id) if dev_id == &req.clientid => {}
        _ => return Err("Client ID does not match registered device ID"),
    }

    // 6. Verify VXEdDSA signature and VRF
    let signed_prekey_b64 = profile
        .signed_prekey
        .as_deref()
        .ok_or("Missing signedPreKey for user")?;

    let signed_prekey_bytes = decode_b64_key(signed_prekey_b64, PUBLIC_KEY_LENGTH, "signedPreKey")
        .map_err(|_| "Invalid signedPreKey base64")?;
    let signature_bytes = decode_b64_key(unpacked.signature_b64, SIGNATURE_LENGTH, "signature")
        .map_err(|_| "Invalid signature base64")?;
    let expected_vrf_bytes =
        decode_b64_key(unpacked.vrf_b64, VRF_LENGTH, "vrf").map_err(|_| "Invalid vrf base64")?;

    let public_key: [u8; 33] = signed_prekey_bytes
        .try_into()
        .map_err(|_| "Public key length invariant violated")?;
    let sig: [u8; 96] = signature_bytes
        .try_into()
        .map_err(|_| "Signature length invariant violated")?;

    let payload = format!("{}{}", req.username, unpacked.timestamp_str);

    match verify_signature(&public_key, payload.as_bytes(), &sig) {
        Ok(output_vrf) => {
            if output_vrf != expected_vrf_bytes.as_slice() {
                return Err("VRF mismatch");
            }
            Ok(())
        }
        Err(_) => Err("Invalid signature"),
    }
}

pub async fn verify_mqtt_client(
    State(state): State<AppState>,
    req: Result<Json<VerifyMqttClientReq>, JsonRejection>,
) -> impl IntoResponse {
    let Json(req) = match req {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Failed to parse MQTT client verification request: {}", e);
            return (StatusCode::OK, "deny");
        }
    };

    match do_verify(&state, &req).await {
        Ok(()) => {
            tracing::info!(
                user_id = %req.username,
                device_id = %req.clientid,
                "MQTT client verification allowed"
            );
            (StatusCode::OK, "allow")
        }
        Err(reason) => {
            tracing::warn!(
                user_id = %req.username,
                device_id = %req.clientid,
                reason = %reason,
                "MQTT client verification denied"
            );
            (StatusCode::OK, "deny")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_password_valid() {
        let sig = "a".repeat(128);
        let vrf = "b".repeat(44);
        let ts = "1726110000"; // 10 digits
        let password = format!("{}{}{}", sig, vrf, ts);
        assert_eq!(password.len(), EXPECTED_PASSWORD_LEN);

        let unpacked = split_password(&password).expect("should unpack successfully");
        assert_eq!(unpacked.signature_b64, sig);
        assert_eq!(unpacked.vrf_b64, vrf);
        assert_eq!(unpacked.timestamp_str, ts);
    }

    #[test]
    fn test_split_password_invalid_length() {
        let short_password = "a".repeat(181);
        assert!(split_password(&short_password).is_err());

        let long_password = "a".repeat(183);
        assert!(split_password(&long_password).is_err());
    }

    #[test]
    fn test_split_password_non_ascii() {
        // 181 valid chars + 1 multi-byte char
        let mut password = "a".repeat(181);
        password.push('🦀');
        assert!(split_password(&password).is_err());
    }

    #[test]
    fn test_timestamp_drift_check() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Exact now: drift 0 -> allowed
        assert!(now.abs_diff(now) <= 10);

        // 9 seconds ago -> allowed
        let recent = now - 9;
        assert!(now.abs_diff(recent) <= 10);

        // 11 seconds ago -> denied
        let expired = now - 11;
        assert!(now.abs_diff(expired) > 10);

        // 11 seconds in future -> denied
        let future = now + 11;
        assert!(now.abs_diff(future) > 10);
    }
}
