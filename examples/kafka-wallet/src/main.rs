//! Setup Lambda do exemplo kafka-wallet.
//!
//! Constrói o `DynamoRepo<Wallet>` reaproveitando o `aws_sdk_dynamodb::Client`
//! global (singleton de cold start), registra o handler `handle_wallet` no
//! `App`, e sobe o dispatcher Lambda em modo `KafkaEvent`.

use std::sync::Arc;

use aws_lambda_events::event::kafka::KafkaEvent;
use kafka_wallet::{Wallet, handle_wallet};
use serverust_lambda::run_event_lambda;
use serverust_telemetry::dynamo::DynamoRepo;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_dynamodb::Client::new(&config);
    let repo: Arc<DynamoRepo<Wallet>> = Arc::new(DynamoRepo::new(client));

    let app = serverust_core::App::new()
        .provide::<DynamoRepo<Wallet>>(repo)
        .event::<KafkaEvent, _>(handle_wallet);

    run_event_lambda::<KafkaEvent>(app).await?;
    Ok(())
}
