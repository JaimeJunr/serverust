use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::broker::{BoxedHandler, Broker, BrokerError, BrokerMessage};

/// Broker em memória para uso em testes sem infraestrutura Kafka.
///
/// Mensagens publicadas ficam armazenadas por tópico e podem ser inspecionadas
/// via [`InMemoryBroker::messages`]. Subscribers registrados via
/// [`Broker::subscribe`] são invocados sincronamente dentro de cada
/// [`Broker::publish`].
pub struct InMemoryBroker {
    messages: Mutex<HashMap<String, Vec<BrokerMessage>>>,
    subscribers: Mutex<HashMap<String, Vec<BoxedHandler>>>,
}

impl InMemoryBroker {
    pub fn new() -> Self {
        Self {
            messages: Mutex::new(HashMap::new()),
            subscribers: Mutex::new(HashMap::new()),
        }
    }

    /// Retorna cópia das mensagens publicadas em `topic`.
    pub fn messages(&self, topic: &str) -> Vec<BrokerMessage> {
        self.messages
            .lock()
            .unwrap()
            .get(topic)
            .cloned()
            .unwrap_or_default()
    }
}

impl Default for InMemoryBroker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Broker for InMemoryBroker {
    async fn subscribe(&self, topic: &str, handler: BoxedHandler) -> Result<(), BrokerError> {
        self.subscribers
            .lock()
            .unwrap()
            .entry(topic.to_string())
            .or_default()
            .push(handler);
        Ok(())
    }

    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), BrokerError> {
        let msg = BrokerMessage {
            topic: topic.to_string(),
            partition: None,
            offset: None,
            key: None,
            payload: payload.to_vec(),
            headers: HashMap::new(),
        };

        self.messages
            .lock()
            .unwrap()
            .entry(topic.to_string())
            .or_default()
            .push(msg.clone());

        // Clone handlers antes de await para liberar o Mutex.
        let handlers = self
            .subscribers
            .lock()
            .unwrap()
            .get(topic)
            .cloned()
            .unwrap_or_default();

        for handler in handlers {
            handler(msg.clone()).await?;
        }

        Ok(())
    }
}
