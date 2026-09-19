//! Subscriber sem `fifo` mas com `SqsFifoMetadata` na assinatura — compile error.

use serverust_events::broker::BrokerError;
use serverust_events::sqs::extract::SqsFifoMetadata;
use serverust_macros::subscriber;

#[subscriber(driver = "sqs", queue = "orders")]
async fn handle_standard_with_fifo_metadata(
    _event: serde_json::Value,
    _meta: SqsFifoMetadata,
) -> Result<(), BrokerError> {
    Ok(())
}

fn main() {}
