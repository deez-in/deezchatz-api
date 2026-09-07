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
