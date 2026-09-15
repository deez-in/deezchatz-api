use crate::{
    db::{
        keys::{email_lookup_pk, lookup_sk, phone_lookup_pk, user_pk},
        lib::{execute_batch_delete, get_item},
    },
    error::AppError,
    state::AppState,
};
use aws_sdk_dynamodb::types::{AttributeValue, DeleteRequest, WriteRequest};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn resolve_user_id_by_email(
    state: &AppState,
    email: &str,
) -> Result<Option<String>, AppError> {
    let email_pk = email_lookup_pk(email);
    let existing_pointer = get_item(state, &email_pk, lookup_sk()).await?;

    if let Some(ref item) = existing_pointer {
        let user_id = item
            .get("userId")
            .and_then(|v| v.as_s().ok())
            .map(|id| id.to_string());
        Ok(user_id)
    } else {
        Ok(None)
    }
}

pub async fn resolve_user_id_by_phone(
    state: &AppState,
    phone: &str,
) -> Result<Option<String>, AppError> {
    let pk = phone_lookup_pk(phone);
    let sk = lookup_sk();
    let pointer = get_item(state, &pk, sk).await?;

    if let Some(ref item) = pointer {
        let user_id = item
            .get("userId")
            .and_then(|v| v.as_s().ok())
            .map(|id| id.to_string());
        Ok(user_id)
    } else {
        Ok(None)
    }
}

pub async fn get_user_profile_by_id(
    state: &AppState,
    user_id: &str,
) -> Result<Option<HashMap<String, AttributeValue>>, AppError> {
    let user_pk = user_pk(user_id);
    let profile_sk = crate::db::keys::profile_sk();
    get_item(state, &user_pk, profile_sk).await
}

pub async fn get_user_profile_by_identifier(
    state: &AppState,
    identifier: &str,
) -> Result<Option<HashMap<String, AttributeValue>>, AppError> {
    let user_id = if identifier.contains('@') {
        resolve_user_id_by_email(state, identifier).await?
    } else {
        resolve_user_id_by_phone(state, identifier).await?
    };

    match user_id {
        Some(uid) => get_user_profile_by_id(state, &uid).await,
        None => Ok(None),
    }
}

pub async fn delete_user_account(state: &AppState, user_id: &str) -> Result<(), AppError> {
    let pk = user_pk(user_id);

    // 1. Query all items under USER#<user_id>
    let query_res = state
        .dynamo
        .query()
        .table_name(&state.primary_table)
        .key_condition_expression("pk = :pk")
        .expression_attribute_values(":pk", AttributeValue::S(pk.clone()))
        .send()
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to query user items for deletion: {}",
                aws_sdk_dynamodb::error::DisplayErrorContext(&e)
            );
            AppError::Internal("Database error".into())
        })?;

    let items = query_res.items.unwrap_or_default();
    if items.is_empty() {
        return Err(AppError::NotFound("User not found".to_string()));
    }

    let mut keys_to_delete: Vec<(String, String)> = Vec::new();
    let mut email_to_delete: Option<String> = None;
    let mut phone_to_delete: Option<String> = None;

    for item in items {
        if let Some(sk) = item.get("sk").and_then(|v| v.as_s().ok()) {
            keys_to_delete.push((pk.clone(), sk.clone()));

            if sk == "PROFILE" {
                if let Some(email) = item.get("email").and_then(|v| v.as_s().ok()) {
                    if !email.trim().is_empty() {
                        email_to_delete = Some(email.clone());
                    }
                }
                if let Some(phone) = item.get("phone").and_then(|v| v.as_s().ok()) {
                    if !phone.trim().is_empty() {
                        phone_to_delete = Some(phone.clone());
                    }
                }
            }
        }
    }

    // 2. Add email & phone lookup pointer keys
    if let Some(email) = email_to_delete {
        keys_to_delete.push((email_lookup_pk(&email), lookup_sk().to_string()));
    }
    if let Some(phone) = phone_to_delete {
        keys_to_delete.push((phone_lookup_pk(&phone), lookup_sk().to_string()));
    }

    // 3. Formulate WriteRequest items
    let mut write_requests = Vec::new();
    for (item_pk, item_sk) in keys_to_delete {
        let delete_req = DeleteRequest::builder()
            .key("pk", AttributeValue::S(item_pk))
            .key("sk", AttributeValue::S(item_sk))
            .build()
            .map_err(|e| {
                tracing::error!("Failed to build delete request: {}", e);
                AppError::Internal("Database error".to_string())
            })?;

        write_requests.push(WriteRequest::builder().delete_request(delete_req).build());
    }

    // 4. Chunk into batches of up to 25 items (DynamoDB BatchWriteItem maximum)
    let chunks: Vec<Vec<WriteRequest>> = write_requests
        .chunks(25)
        .map(|chunk| chunk.to_vec())
        .collect();

    if chunks.is_empty() {
        return Ok(());
    }

    // Split batches into two concurrent worker branches and join them with tokio::join!
    let mid = chunks.len().div_ceil(2);
    let (left_chunks, right_chunks) = chunks.split_at(mid);

    let state_left = state.clone();
    let state_right = state.clone();
    let left_batches = left_chunks.to_vec();
    let right_batches = right_chunks.to_vec();

    let left_future = async move {
        for batch in left_batches {
            execute_batch_delete(&state_left, batch).await?;
        }
        Ok::<(), AppError>(())
    };

    let right_future = async move {
        for batch in right_batches {
            execute_batch_delete(&state_right, batch).await?;
        }
        Ok::<(), AppError>(())
    };

    let (res_left, res_right) = tokio::join!(left_future, right_future);
    res_left?;
    res_right?;

    Ok(())
}

pub async fn put_user_report(
    state: &AppState,
    reporter_id: &str,
    reported_id: &str,
    reason: &str,
    messages_json: &str,
) -> Result<(), AppError> {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let pk = crate::db::keys::reporter_pk(reporter_id);
    let sk = reported_id;

    let mut item = HashMap::new();
    item.insert("pk".to_string(), AttributeValue::S(pk));
    item.insert("sk".to_string(), AttributeValue::S(sk.to_string()));
    item.insert("reason".to_string(), AttributeValue::S(reason.to_string()));
    item.insert(
        "messages".to_string(),
        AttributeValue::S(messages_json.to_string()),
    );
    item.insert(
        "createdAt".to_string(),
        AttributeValue::N(now_secs.to_string()),
    );

    state
        .dynamo
        .put_item()
        .table_name(&state.primary_table)
        .set_item(Some(item))
        .send()
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to put user report in DynamoDB: {}",
                aws_sdk_dynamodb::error::DisplayErrorContext(&e)
            );
            AppError::Internal("Database error".into())
        })?;

    Ok(())
}
