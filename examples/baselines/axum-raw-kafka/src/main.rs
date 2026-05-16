//! Setup da Lambda — não contabilizado no LOC do handler.
//! Compare com examples/kafka-wallet/src/main.rs para ver a diferença de boilerplate.

mod handler;

use aws_config::BehaviorVersion;
use lambda_runtime::{Error, run, service_fn};
use rdkafka::config::ClientConfig;
use rdkafka::producer::FutureProducer;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let dynamo = aws_sdk_dynamodb::Client::new(&config);

    let brokers = std::env::var("KAFKA_BROKERS").unwrap_or_else(|_| "localhost:9092".to_string());
    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &brokers)
        .set("message.timeout.ms", "5000")
        .create()?;

    run(service_fn(|event| {
        handler::handle_wallet_events(event, &dynamo, &producer)
    }))
    .await
}
