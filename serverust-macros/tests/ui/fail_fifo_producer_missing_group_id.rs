//! SqsFifoProducer::send_builder().send() sem .message_group_id() — compile error
//! porque o builder no estado NoGroupId nao expoe o metodo `send`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serverust_events::sqs::fifo_producer::SqsFifoProducer;
use serverust_events::sqs::producer::{ProducerConfig, SendClient, SendEntry, SendResult};

struct DummyClient;

#[async_trait]
impl SendClient for DummyClient {
    async fn send_batch(
        &self,
        _queue_url: &str,
        _entries: Vec<SendEntry>,
    ) -> Result<SendResult, String> {
        Ok(SendResult {
            successful: vec![],
            failed: vec![],
        })
    }
}

#[tokio::main]
async fn main() {
    let (producer, _task) = SqsFifoProducer::new(
        Arc::new(DummyClient) as Arc<dyn SendClient>,
        "https://sqs/q.fifo",
        ProducerConfig {
            max_batch_size: 1,
            max_linger: Duration::from_millis(10),
            base_backoff: Duration::ZERO,
            max_retries: 1,
        },
    );

    // ERRO: o builder no estado NoGroupId nao implementa send().
    let _ = producer.send_builder("body").send().await;
}
