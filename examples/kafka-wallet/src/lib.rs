//! Handler kafka-wallet: consome WalletEvent, persiste em DynamoDB e publica
//! resultado em outra fila Kafka. Tudo via APIs do serverust — sem boilerplate.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serverust_core::events::EventError;
use serverust_core::extract::State;
use serverust_events::kafka::KafkaRecord;
use serverust_events::producer::KafkaProducer;
use serverust_macros::{dynamo_table, kafka_consumer};
use serverust_telemetry::dynamo::DynamoRepo;

#[derive(Debug, Deserialize)]
pub struct WalletEvent {
    pub user_id: String,
    pub amount: i64,
    pub operation: String,
}

#[dynamo_table("Wallets", pk = "user_id")]
#[derive(Debug, Serialize, Deserialize)]
pub struct Wallet {
    pub user_id: String,
    pub balance: i64,
}

#[derive(Debug, Serialize)]
pub struct WalletResult {
    pub user_id: String,
    pub new_balance: i64,
    pub status: &'static str,
}

#[kafka_consumer(topic = "wallet.events", group = "wallet-processor")]
pub async fn handle_wallet(
    record: KafkaRecord<WalletEvent>,
    State(repo): State<Arc<DynamoRepo<Wallet>>>,
) -> Result<(), EventError> {
    let event = record.payload;
    let current = repo
        .get(event.user_id.clone())
        .await
        .map_err(|e| EventError(e.to_string()))?
        .unwrap_or(Wallet {
            user_id: event.user_id.clone(),
            balance: 0,
        });
    let new_balance = match event.operation.as_str() {
        "credit" => current.balance + event.amount,
        "debit" => current.balance - event.amount,
        op => return Err(EventError(format!("unknown operation: {op}"))),
    };
    let updated = Wallet {
        user_id: event.user_id.clone(),
        balance: new_balance,
    };
    repo.put(&updated)
        .await
        .map_err(|e| EventError(e.to_string()))?;
    let producer = KafkaProducer::from_env().map_err(|e| EventError(e.to_string()))?;
    producer
        .publish(
            "wallet.results",
            &updated.user_id,
            &WalletResult {
                user_id: event.user_id,
                new_balance,
                status: "processed",
            },
        )
        .await
        .map_err(|e| EventError(e.to_string()))?;
    Ok(())
}
