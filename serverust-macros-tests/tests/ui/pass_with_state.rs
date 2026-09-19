//! Trybuild: a macro #[kafka_consumer] suporta parâmetros `State<Arc<T>>` para DI
//! e os resolve via o `Container` compartilhado no momento do dispatch.

use std::sync::Arc;

use serde::Deserialize;
use serverust_core::events::{EventError, EventHandler};
use serverust_core::extract::State;
use serverust_events::kafka::KafkaRecord;
use serverust_macros::kafka_consumer;

trait WalletRepository: Send + Sync {
    fn credit(&self, amount: u64);
}

#[derive(Deserialize)]
struct Payment {
    amount: u64,
}

#[kafka_consumer(
    topic = "wallet-events",
    group = "wallet-processor",
    batch_size = 100
)]
async fn process_payment(
    record: KafkaRecord<Payment>,
    State(repo): State<Arc<dyn WalletRepository>>,
) -> Result<(), EventError> {
    repo.credit(record.payload.amount);
    Ok(())
}

fn main() {
    fn assert_impl<H: EventHandler<aws_lambda_events::event::kafka::KafkaEvent>>(_h: H) {}
    assert_impl(process_payment);
    assert_eq!(process_payment::BATCH_SIZE, 100usize);
}
