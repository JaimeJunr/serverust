//! Verifica a expansão runtime de `#[kafka_consumer]`: registro via `App::event`,
//! dispatch de `KafkaEvent` fake, filtro por tópico e injeção via `State<Arc<T>>`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aws_lambda_events::event::kafka::{KafkaEvent, KafkaRecord as RawKafkaRecord};
use base64::Engine;
use serde::Deserialize;
use serverust_core::App;
use serverust_core::events::EventError;
use serverust_core::extract::State;
use serverust_events::kafka::KafkaRecord;
use serverust_macros::kafka_consumer;

#[derive(Deserialize)]
struct Payment {
    amount: u64,
}

trait WalletRepository: Send + Sync {
    fn credit(&self, amount: u64);
}

struct InMemRepo {
    log: Mutex<Vec<u64>>,
}

impl WalletRepository for InMemRepo {
    fn credit(&self, amount: u64) {
        self.log.lock().unwrap().push(amount);
    }
}

#[kafka_consumer(topic = "wallet-events", group = "wallet-processor")]
async fn process_payment(
    record: KafkaRecord<Payment>,
    State(repo): State<Arc<dyn WalletRepository>>,
) -> Result<(), EventError> {
    repo.credit(record.payload.amount);
    Ok(())
}

fn b64_json(v: &serde_json::Value) -> String {
    base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(v).unwrap())
}

fn make_event(records: &[(&str, u64)]) -> KafkaEvent {
    let mut by_partition: HashMap<String, Vec<RawKafkaRecord>> = HashMap::new();
    for (i, (topic, amount)) in records.iter().enumerate() {
        let raw = serde_json::from_value::<RawKafkaRecord>(serde_json::json!({
            "topic": topic,
            "partition": 0,
            "offset": i,
            "timestamp": 0,
            "timestampType": "CREATE_TIME",
            "key": null,
            "value": b64_json(&serde_json::json!({"amount": amount})),
            "headers": [],
        }))
        .unwrap();
        by_partition
            .entry(format!("{topic}-0"))
            .or_default()
            .push(raw);
    }
    let mut ev = KafkaEvent::default();
    ev.records = by_partition;
    ev
}

#[tokio::test]
async fn macro_registra_handler_e_dispara_para_topico_correto() {
    let repo = Arc::new(InMemRepo {
        log: Mutex::new(Vec::new()),
    });
    let app = App::new()
        .provide::<dyn WalletRepository>(repo.clone() as Arc<dyn WalletRepository>)
        .event::<KafkaEvent, _>(process_payment);

    let dispatcher = app.into_event_dispatcher::<KafkaEvent>();
    let event = make_event(&[("wallet-events", 100), ("wallet-events", 250)]);

    dispatcher.dispatch_event(event).await.unwrap();

    let logged = repo.log.lock().unwrap().clone();
    assert_eq!(logged, vec![100, 250]);
}

#[tokio::test]
async fn macro_filtra_registros_de_outros_topicos() {
    let repo = Arc::new(InMemRepo {
        log: Mutex::new(Vec::new()),
    });
    let app = App::new()
        .provide::<dyn WalletRepository>(repo.clone() as Arc<dyn WalletRepository>)
        .event::<KafkaEvent, _>(process_payment);

    let dispatcher = app.into_event_dispatcher::<KafkaEvent>();
    // Mistura: 1 do tópico esperado + 1 de outro tópico que deve ser descartado.
    let event = make_event(&[("wallet-events", 7), ("other-topic", 999)]);

    dispatcher.dispatch_event(event).await.unwrap();

    let logged = repo.log.lock().unwrap().clone();
    assert_eq!(
        logged,
        vec![7],
        "registro de tópico estranho deve ser ignorado"
    );
}

#[tokio::test]
async fn constantes_associadas_expoem_metadados() {
    assert_eq!(process_payment::TOPIC, "wallet-events");
    assert_eq!(process_payment::GROUP, "wallet-processor");
    assert_eq!(process_payment::BATCH_SIZE, 0usize);
}
