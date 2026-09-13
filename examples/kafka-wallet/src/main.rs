//! Entry point — detecta runtime (Lambda vs long-running), registra o handler
//! via EventRouter e sobe o loop de eventos.

use std::sync::Arc;

use aws_lambda_events::event::kafka::KafkaEvent;
use kafka_wallet::handle_wallet;
use lambda_runtime::{LambdaEvent, service_fn};
use serverust_events::broker::lambda::LambdaBroker;
use serverust_events::router::EventRouter;
use serverust_events::runtime::Runtime;
use serverust_telemetry::dynamo::DynamoRepo;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // `load_defaults` com BehaviorVersion explícita: `load_from_env` está
    // deprecada e fixa a versão de comportamento do SDK de forma implícita.
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
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
            // Modo long-running: habilite a feature `kafka` e use KafkaBroker.
            // let broker = Arc::new(KafkaBroker::from_env()?);
            // router.attach(broker.clone()).await?;
            // loop { let msg = poll_rdkafka(&broker); broker.dispatch(msg).await?; }
            //
            // Sem a feature `kafka`, este exemplo é Lambda-only.
            eprintln!(
                "Long-running mode não configurado: habilite a feature `kafka` e implemente o poll loop."
            );
            std::process::exit(1);
        }
    }
    Ok(())
}
