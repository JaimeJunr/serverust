//! Handler kafka-wallet: consome WalletEvent e persiste saldo em DynamoDB.

use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use serverust_events::broker::BrokerError;
use serverust_macros::{dynamo_table, subscriber};
use serverust_telemetry::dynamo::DynamoRepo;

#[derive(Debug, Clone, Deserialize)]
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

// Singleton do repositório — warm-start seguro para Lambda e long-running.
static REPO: OnceLock<Arc<DynamoRepo<Wallet>>> = OnceLock::new();

/// Inicializa o repositório DynamoDB antes de `attach` ser chamado.
pub fn init_repo(repo: Arc<DynamoRepo<Wallet>>) {
    if REPO.set(repo).is_err() {
        panic!("REPO already initialized");
    }
}

/// Processa WalletEvent e persiste o saldo atualizado em DynamoDB.
///
/// Para publicar resultados em `wallet.results` a partir do modo Lambda, use
/// um `KafkaBroker` (feature `kafka`) ou `KafkaProducer` explicitamente —
/// `LambdaBroker` é sink-only e não pode publicar de volta ao Kafka.
#[subscriber(topic = "wallet.events")]
pub async fn handle_wallet(event: WalletEvent) -> Result<(), BrokerError> {
    let repo = REPO
        .get()
        .ok_or_else(|| BrokerError::Configuration("call init_repo before attach".into()))?;

    let current = repo
        .get(event.user_id.clone())
        .await
        .map_err(|e| BrokerError::Transport(e.to_string()))?
        .unwrap_or(Wallet {
            user_id: event.user_id.clone(),
            balance: 0,
        });

    let new_balance = match event.operation.as_str() {
        "credit" => current.balance + event.amount,
        "debit" => current.balance - event.amount,
        op => return Err(BrokerError::Subscribe(format!("unknown operation: {op}"))),
    };

    repo.put(&Wallet {
        user_id: event.user_id.clone(),
        balance: new_balance,
    })
    .await
    .map_err(|e| BrokerError::Transport(e.to_string()))?;

    Ok(())
}
