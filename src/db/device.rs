use crate::{error::AppError, state::AppState};
use aws_sdk_dynamodb::types::AttributeValue;

pub async fn update_item_fcm(
    state: &AppState,
    pk: &str,
    sk: &str,
    fcm_token: &str,
) -> Result<(), AppError> {
    // Secondary update: sync the denormalized fcmToken on the PROFILE item.
    // This is best-effort — a failure is logged but does not fail the request,
    // since the DEVICE# item is the authoritative source for push delivery.
    let profile_future = state
        .dynamo
        .update_item()
        .table_name(&state.primary_table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S("PROFILE".to_string()))
        .update_expression("SET fcmToken = :token")
        .expression_attribute_values(":token", AttributeValue::S(fcm_token.to_string()))
        .send();

    // Primary update: write to the DEVICE# item (authoritative source for notifications).
    // The condition ensures the device exists before updating.
    let device_future = state
        .dynamo
        .update_item()
        .table_name(&state.primary_table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S(sk.to_string()))
        .update_expression("SET fcmToken = :token")
        .expression_attribute_values(":token", AttributeValue::S(fcm_token.to_string()))
        .condition_expression("attribute_exists(signedDeviceKey)")
        .send();

    let (device_result, profile_result) = tokio::join!(device_future, profile_future);

    device_result.map_err(|e| {
        if let aws_sdk_dynamodb::error::SdkError::ServiceError(ref se) = e {
            if se.err().is_conditional_check_failed_exception() {
                return AppError::NotFound("Device not found or not registered".to_string());
            }
        }
        tracing::error!(
            "Failed to update fcmToken in DynamoDB: {}",
            aws_sdk_dynamodb::error::DisplayErrorContext(&e)
        );
        AppError::Internal("Database error".into())
    })?;

    if let Err(ref e) = profile_result {
        tracing::warn!(
            "Failed to sync fcmToken to PROFILE item (non-fatal): {}",
            aws_sdk_dynamodb::error::DisplayErrorContext(e)
        );
    }

    Ok(())
}

pub async fn pop_opk(
    state: &AppState,
    pk: &str,
    sk: &str,
    opk_index: usize,
    expected_opk: &str,
) -> Result<(), AppError> {
    state
        .dynamo
        .update_item()
        .table_name(&state.primary_table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S(sk.to_string()))
        .update_expression(format!("REMOVE opks[{}]", opk_index))
        .condition_expression(format!("opks[{}] = :expected", opk_index))
        .expression_attribute_values(":expected", AttributeValue::S(expected_opk.to_string()))
        .send()
        .await
        .map_err(|e| {
            if let aws_sdk_dynamodb::error::SdkError::ServiceError(ref se) = e {
                if se.err().is_conditional_check_failed_exception() {
                    return AppError::Conflict("OPK conflict detected".to_string());
                }
            }
            tracing::error!(
                "Failed to pop OPK in DynamoDB: {}",
                aws_sdk_dynamodb::error::DisplayErrorContext(&e)
            );
            AppError::Internal("Database error".into())
        })?;

    Ok(())
}

pub async fn clear_device_fcm_token(
    state: &AppState,
    pk: &str,
    device_sk: &str,
) -> Result<(), AppError> {
    // Remove from DEVICE# item (primary)
    let device_result = state
        .dynamo
        .update_item()
        .table_name(&state.primary_table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S(device_sk.to_string()))
        .update_expression("REMOVE fcmToken")
        .send()
        .await;

    if let Err(ref e) = device_result {
        tracing::warn!(
            "Failed to clear fcmToken from DEVICE item (non-fatal): {}",
            aws_sdk_dynamodb::error::DisplayErrorContext(e)
        );
    }

    // Remove from PROFILE item (denormalized copy, best-effort)
    let profile_result = state
        .dynamo
        .update_item()
        .table_name(&state.primary_table)
        .key("pk", AttributeValue::S(pk.to_string()))
        .key("sk", AttributeValue::S("PROFILE".to_string()))
        .update_expression("REMOVE fcmToken")
        .send()
        .await;

    if let Err(ref e) = profile_result {
        tracing::warn!(
            "Failed to clear fcmToken from PROFILE item (non-fatal): {}",
            aws_sdk_dynamodb::error::DisplayErrorContext(e)
        );
    }

    Ok(())
}
