//! Trybuild: a macro #[kafka_consumer] expande para impl EventHandler<KafkaEvent>
//! sobre uma struct unit homônima e os metadados `topic`/`group` ficam acessíveis
//! como constantes associadas.

use serde::Deserialize;
use serverust_core::events::{EventError, EventHandler};
use serverust_events::kafka::KafkaRecord;
use serverust_macros::kafka_consumer;

#[derive(Deserialize)]
struct Payment {
    amount: u64,
}

#[kafka_consumer(topic = "wallet-events", group = "wallet-processor")]
async fn process_payment(record: KafkaRecord<Payment>) -> Result<(), EventError> {
    let _ = record.payload.amount;
    Ok(())
}

fn main() {
    // Garante que o nome da fn virou uma unit struct e que implementa EventHandler<KafkaEvent>.
    fn assert_impl<H: EventHandler<aws_lambda_events::event::kafka::KafkaEvent>>(_h: H) {}
    assert_impl(process_payment);
    assert_eq!(process_payment::TOPIC, "wallet-events");
    assert_eq!(process_payment::GROUP, "wallet-processor");
}
