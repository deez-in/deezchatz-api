use crate::{error::AppError, state::AppState};
use aws_sdk_dynamodb::types::{AttributeValue, TransactWriteItem, WriteRequest};
use std::collections::HashMap;

pub async fn get_item(
    state: &AppState,
    pk: &str,
    sk: &str,
) -> Result<Option<HashMap<String, AttributeValue>>, AppError> {
    let res = state
        .dynamo
        .get_item()
        .table_name(&state.primary_table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S(sk.to_string()))
        .send()
        .await
        .map_err(|e| {
            tracing::error!(
                "DynamoDB get_item error: {}",
                aws_sdk_dynamodb::error::DisplayErrorContext(&e)
            );
            AppError::Internal("Database error".into())
        })?;

    Ok(res.item)
}

pub async fn transact_write_items(
    state: &AppState,
    items: Vec<TransactWriteItem>,
) -> Result<(), AppError> {
    state
        .dynamo
        .transact_write_items()
        .set_transact_items(Some(items))
        .send()
        .await
        .map_err(|e| {
            if let aws_sdk_dynamodb::error::SdkError::ServiceError(ref se) = e {
                if se.err().is_transaction_canceled_exception() {
                    return AppError::Conflict(
                        "Conflict: An item condition check failed or already exists".to_string(),
                    );
                }
            }
            tracing::error!(
                "DynamoDB transact_write_items error: {}",
                aws_sdk_dynamodb::error::DisplayErrorContext(&e)
            );
            AppError::Internal("Database error".into())
        })?;

    Ok(())
}

pub(crate) async fn execute_batch_delete(
    state: &AppState,
    requests: Vec<WriteRequest>,
) -> Result<(), AppError> {
    if requests.is_empty() {
        return Ok(());
    }

    state
        .dynamo
        .batch_write_item()
        .request_items(&state.primary_table, requests)
        .send()
        .await
        .map_err(|e| {
            tracing::error!(
                "Failed to execute batch_write_item delete: {}",
                aws_sdk_dynamodb::error::DisplayErrorContext(&e)
            );
            AppError::Internal("Database error".into())
        })?;

    Ok(())
}
