use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleIdTokenReq {
    pub id_token: String,
    pub i_key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleIdTokenResp {
    pub status: String,
    pub user_id: String,
    pub state: String,
    pub email: String,
    pub name: Option<String>,
    pub picture: Option<String>,
}

#[derive(Deserialize)]
pub struct GoogleIdTokenClaims {
    pub email: String,
    pub email_verified: bool,
    pub picture: Option<String>,
    pub name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthDeleteReq {
    pub code: String,
    pub code_verifier: Option<String>,
    pub redirect_uri: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthRegisterReq {
    pub code: String,
    pub code_verifier: Option<String>,
    pub redirect_uri: String,
    pub i_key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_register_req_deserialization() {
        let json = r#"{
            "code": "4/0AeanS0b...",
            "codeVerifier": "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            "redirectUri": "https://deezchatz.app/auth/callback",
            "iKey": "base64encodedidentitykey..."
        }"#;

        let req: OAuthRegisterReq = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(req.code, "4/0AeanS0b...");
        assert_eq!(
            req.code_verifier.as_deref(),
            Some("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
        );
        assert_eq!(req.redirect_uri, "https://deezchatz.app/auth/callback");
        assert_eq!(req.i_key, "base64encodedidentitykey...");
    }

    #[test]
    fn test_oauth_delete_req_deserialization() {
        let json = r#"{
            "code": "4/0AeanS0b...",
            "redirectUri": "https://deezchatz.app/auth/callback"
        }"#;

        let req: OAuthDeleteReq = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(req.code, "4/0AeanS0b...");
        assert!(req.code_verifier.is_none());
        assert_eq!(req.redirect_uri, "https://deezchatz.app/auth/callback");
    }
}
