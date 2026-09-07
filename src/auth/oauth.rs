use crate::{
    db::{keys::email_lookup_pk, keys::lookup_sk, lib::get_item},
    error::AppError,
    state::AppState,
};
use serde::Deserialize;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct OAuthIdentity {
    pub email: String,
    pub name: Option<String>,
    pub picture: Option<String>,
}

#[derive(Deserialize)]
struct GoogleTokenResponse {
    id_token: Option<String>,
    #[allow(dead_code)]
    access_token: Option<String>,
}

pub async fn exchange_google_auth_code(
    state: &AppState,
    code: &str,
    code_verifier: Option<&str>,
    redirect_uri: &str,
) -> Result<OAuthIdentity, AppError> {
    let mut form_data = vec![
        ("code", code),
        ("client_id", &state.google_client_id_web),
        ("client_secret", &state.google_client_secret),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ];

    if let Some(cv) = code_verifier {
        form_data.push(("code_verifier", cv));
    }

    let resp = state
        .http_client
        .post("https://oauth2.googleapis.com/token")
        .form(&form_data)
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Failed to exchange Google code: {}", e);
            AppError::BadGateway("Failed to exchange Google authorization code".into())
        })?;

    if !resp.status().is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        tracing::error!("Google token exchange failed: {}", err_text);
        return Err(AppError::Unauthorized(
            "Invalid authorization code".to_string(),
        ));
    }

    let token_resp: GoogleTokenResponse = resp.json().await.map_err(|e| {
        tracing::error!("Failed to parse Google token response: {}", e);
        AppError::BadGateway("Invalid response from Google".into())
    })?;

    let id_token = token_resp.id_token.ok_or_else(|| {
        tracing::error!("Google response missing id_token");
        AppError::Unauthorized("Missing id_token from Google".into())
    })?;

    verify_google_id_token(state, &id_token).await
}

pub async fn verify_google_id_token(
    state: &AppState,
    id_token: &str,
) -> Result<OAuthIdentity, AppError> {
    let mut jwks = None;
    {
        let cache = state.google_jwks.read().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if cache.1.is_some() && now < cache.0 {
            jwks = cache.1.clone();
        }
    }

    if jwks.is_none() {
        let mut cache = state.google_jwks.write().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if cache.1.is_some() && now < cache.0 {
            jwks = cache.1.clone();
        } else {
            let resp = state
                .http_client
                .get("https://www.googleapis.com/oauth2/v3/certs")
                .send()
                .await
                .map_err(|e| {
                    tracing::error!("Failed to fetch JWKS: {}", e);
                    AppError::BadGateway("Failed to fetch JWKS".into())
                })?;

            let max_age_secs = resp
                .headers()
                .get(reqwest::header::CACHE_CONTROL)
                .and_then(|h| h.to_str().ok())
                .and_then(|s| {
                    s.split(',').find_map(|part| {
                        let part = part.trim();
                        part.strip_prefix("max-age=")
                            .and_then(|val| val.trim().parse::<u64>().ok())
                    })
                })
                .unwrap_or(3600)
                .clamp(300, 86400);

            let fetched_jwks: jsonwebtoken::jwk::JwkSet = resp.json().await.map_err(|e| {
                tracing::error!("Failed to parse JWKS: {}", e);
                AppError::BadGateway("Failed to parse JWKS".into())
            })?;
            jwks = Some(fetched_jwks.clone());
            cache.0 = now + max_age_secs;
            cache.1 = Some(fetched_jwks);
            tracing::info!(ttl_secs = max_age_secs, "Refreshed Google JWKS cache");
        }
    }

    let jwks = jwks.ok_or_else(|| AppError::Internal("JWKS unavailable".to_string()))?;
    let header = jsonwebtoken::decode_header(id_token).map_err(|e| {
        tracing::error!("Invalid ID token header: {}", e);
        AppError::BadRequest("Invalid ID token header".to_string())
    })?;
    let kid = header.kid.ok_or_else(|| {
        tracing::error!("Missing kid in ID token");
        AppError::BadRequest("Missing kid in ID token".to_string())
    })?;

    let jwk = jwks.find(&kid).ok_or_else(|| {
        tracing::error!("Unknown kid in ID token: {}", kid);
        AppError::BadRequest("Unknown kid in ID token".to_string())
    })?;
    let decoding_key = jsonwebtoken::DecodingKey::from_jwk(jwk).map_err(|e| {
        tracing::error!("Invalid JWK: {}", e);
        AppError::BadRequest("Invalid JWK".to_string())
    })?;

    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    
    let mut audiences = vec![];
    if !state.google_client_id_web.is_empty() {
        audiences.push(state.google_client_id_web.as_str());
    }
    if !state.google_client_id_android.is_empty() {
        audiences.push(state.google_client_id_android.as_str());
    }
    
    validation.set_audience(&audiences);
    validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);

    let token_data = jsonwebtoken::decode::<crate::models::api::auth::GoogleIdTokenClaims>(
        id_token,
        &decoding_key,
        &validation,
    )
    .map_err(|e| {
        tracing::error!("Invalid ID token: {}", e);
        AppError::Unauthorized("Invalid ID token".to_string())
    })?;

    let claims = token_data.claims;
    if !claims.email_verified {
        tracing::error!("Google email not verified for {:?}", claims.email);
        return Err(AppError::Unauthorized("Google email not verified".into()));
    }

    Ok(OAuthIdentity {
        email: claims.email,
        name: claims.name,
        picture: claims.picture,
    })
}

pub async fn resolve_user_id(
    state: &AppState,
    email: &str,
) -> Result<Option<String>, AppError> {
    let email_pk = email_lookup_pk(email);
    let existing_pointer = get_item(state, &email_pk, lookup_sk()).await?;
    
    if let Some(ref item) = existing_pointer {
        let user_id = item.get("userId")
            .and_then(|v| v.as_s().ok())
            .map(|id| id.to_string());
        Ok(user_id)
    } else {
        Ok(None)
    }
}
