//! Entry point — detecta runtime (Lambda vs long-running), registra o handler
//! via EventRouter e sobe o loop de eventos.

use std::sync::Arc;

use aws_lambda_events::event::kafka::KafkaEvent;
use kafka_wallet::handle_wallet;
use lambda_runtime::{service_fn, LambdaEvent};
use serverust_events::broker::lambda::LambdaBroker;
use serverust_events::router::EventRouter;
use serverust_events::runtime::Runtime;
use serverust_telemetry::dynamo::DynamoRepo;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_dynamodb::Client::new(&config);
    kafka_wallet::init_repo(Arc::new(DynamoRepo::new(client)));

    let router = handle_wallet::register(EventRouter::new());

    match Runtime::detect() {
        Runtime::Lambda => {
            let broker = Arc::new(LambdaBroker::new());
            router.attach(broker.clone()).await?;
            lambda_runtime::run(service_fn(move |event: LambdaEvent<KafkaEvent>| {
                let broker = broker.clone();
                async move {
                    broker
                        .handle_kafka_event(&event.payload)
                        .await
                        .map_err(|e| e.to_string())
                }
            }))
            .await?;
        }
        Runtime::LongRunning => {
            // Modo long-running usa KafkaBroker (feature `kafka`) com loop de poll.
            // Exemplo: let broker = Arc::new(KafkaBroker::from_env()?);
            //          router.attach(broker.clone()).await?;
            //          // chamar broker.dispatch(msg) para cada msg recebida via rdkafka
            eprintln!(
                "Long-running mode: configure KafkaBroker (feature `kafka`) e dispatch loop."
            );
        }
    }
    Ok(())
}
