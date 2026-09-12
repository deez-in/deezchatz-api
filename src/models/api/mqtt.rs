use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct VerifyMqttClientReq {
    pub username: String,
    pub clientid: String,
    pub password: String,
    pub ip: Option<String>,
}
