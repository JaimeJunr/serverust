//! Subscriber com `fifo` mas sem `SqsFifoMetadata` na assinatura — compile error.

use serverust_events::broker::BrokerError;
use serverust_macros::subscriber;

#[subscriber(driver = "sqs", queue = "orders.fifo", fifo)]
async fn handle_fifo_without_metadata(_event: serde_json::Value) -> Result<(), BrokerError> {
    Ok(())
}

fn main() {}
