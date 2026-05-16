//! Handler Kafka vanilla — medido para comparação de LOC vs serverust kafka-wallet.
//! Demonstra o custo de boilerplate sem framework: Base64 manual, DynamoDB verboso,
//! rdkafka FutureRecord explícito.

use aws_lambda_events::event::kafka::KafkaEvent;
use aws_sdk_dynamodb::{types::AttributeValue, Client as DynamoClient};
use base64::Engine;
use lambda_runtime::{Error, LambdaEvent};
use rdkafka::producer::{FutureProducer, FutureRecord};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct WalletEvent {
    pub user_id: String,
    pub amount: i64,
    pub operation: String,
}

#[derive(Serialize)]
pub struct WalletResult {
    pub user_id: String,
    pub new_balance: i64,
    pub status: String,
}

pub async fn handle_wallet_events(
    event: LambdaEvent<KafkaEvent>,
    dynamo: &DynamoClient,
    producer: &FutureProducer,
) -> Result<(), Error> {
    for (_partition_key, records) in &event.payload.records {
        for record in records {
            let topic = record.topic.clone().unwrap_or_default();
            let partition = record.partition;
            let offset = record.offset;

            // Base64 decode manual — sem helper do framework
            let raw_value = record.value.as_deref().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("missing value at {topic}:{partition}+{offset}"),
                )
            })?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(raw_value)?;
            let wallet_event: WalletEvent = serde_json::from_slice(&bytes)?;

            // DynamoDB put_item verboso — sem DynamoRepo<T>
            dynamo
                .put_item()
                .table_name("Wallets")
                .item("user_id", AttributeValue::S(wallet_event.user_id.clone()))
                .item("amount", AttributeValue::N(wallet_event.amount.to_string()))
                .item("operation", AttributeValue::S(wallet_event.operation.clone()))
                .send()
                .await?;

            // Kafka publish verboso — sem KafkaProducer wrapper
            let result = WalletResult {
                user_id: wallet_event.user_id,
                new_balance: wallet_event.amount,
                status: "processed".to_string(),
            };
            let payload_bytes = serde_json::to_vec(&result)?;
            producer
                .send(
                    FutureRecord::to("wallet.results")
                        .key("wallet-result")
                        .payload(&payload_bytes),
                    Duration::from_secs(5),
                )
                .await
                .map_err(|(e, _)| -> Error { Box::new(e) })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wallet_event_deserializes_from_base64() {
        let json = r#"{"user_id":"u1","amount":100,"operation":"credit"}"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(json);
        let bytes = base64::engine::general_purpose::STANDARD.decode(&encoded).unwrap();
        let event: WalletEvent = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(event.user_id, "u1");
        assert_eq!(event.amount, 100);
    }

    #[test]
    fn wallet_result_serializes() {
        let result = WalletResult {
            user_id: "u1".to_string(),
            new_balance: 100,
            status: "processed".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("processed"));
    }
}
