use crate::{
    db::keys::{offline_message_pk, offline_message_sk},
    error::AppError,
    state::AppState,
};
use aws_sdk_dynamodb::types::AttributeValue;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

const OFFLINE_MESSAGE_TTL_SECS: u64 = 7 * 24 * 60 * 60; // 7 days (1 week)

pub async fn put_offline_message(
    state: &AppState,
    recipient_id: &str,
    sender_id: &str,
    sender_device_id: &str,
    topic: &str,
    payload: &str,
) -> Result<(), AppError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let now_ms = now.as_millis() as u64;
    let now_secs = now.as_secs();
    let ttl_secs = now_secs + OFFLINE_MESSAGE_TTL_SECS;

    let pk = offline_message_pk(recipient_id);
    let sk = offline_message_sk(sender_id, now_secs);

    let mut item = HashMap::new();
    item.insert("pk".to_string(), AttributeValue::S(pk));
    item.insert("sk".to_string(), AttributeValue::S(sk));
    item.insert("topic".to_string(), AttributeValue::S(topic.to_string()));
    item.insert(
        "payload".to_string(),
        AttributeValue::S(payload.to_string()),
    );
    item.insert(
        "createdAt".to_string(),
        AttributeValue::N(now_ms.to_string()),
    );
    item.insert("ttl".to_string(), AttributeValue::N(ttl_secs.to_string()));

    state
        .dynamo
        .put_item()
        .table_name(&state.primary_table)
        .set_item(Some(item))
        .send()
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to persist offline message: {}",
                aws_sdk_dynamodb::error::DisplayErrorContext(&e)
            );
            AppError::Internal("Failed to store offline message".into())
        })?;

    Ok(())
}
