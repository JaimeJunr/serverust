//! Implementação de [`Broker`] para Apache Kafka via `rust-rdkafka`.
//!
//! `KafkaBroker` encapsula um `FutureProducer` (publish) e um registro
//! interno de inscrições (subscribers ativados em runtime — ver US-7).
//!
//! O ciclo de consumo (poll loop / Lambda trigger) é responsabilidade de
//! camadas superiores (`EventRouter`, `serverust-lambda`). Esta struct
//! foca em prover a interface do trait `Broker` e a infraestrutura de
//! conexão (config + IAM SASL opcional).

use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use rdkafka::ClientContext;
use rdkafka::client::OAuthToken;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::ConsumerContext;
use rdkafka::producer::{FutureProducer, FutureRecord, ProducerContext};

use super::{BoxedHandler, Broker, BrokerError, BrokerMessage};

/// Contexto rdkafka que fornece o token IAM MSK via OAUTHBEARER.
#[derive(Clone)]
struct MskIamContext {
    region: String,
}

impl ClientContext for MskIamContext {
    const ENABLE_REFRESH_OAUTH_TOKEN: bool = true;

    fn generate_oauth_token(
        &self,
        _oauthbearer_config: Option<&str>,
    ) -> Result<OAuthToken, Box<dyn std::error::Error>> {
        let region = self.region.clone();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        let (token, expiry_ms) = rt
            .block_on(async move {
                aws_msk_iam_sasl_signer::generate_auth_token(aws_types::region::Region::new(region))
                    .await
            })
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        Ok(OAuthToken {
            token,
            lifetime_ms: expiry_ms,
            principal_name: String::new(),
        })
    }
}

impl ConsumerContext for MskIamContext {}
impl ProducerContext for MskIamContext {
    type DeliveryOpaque = ();
    fn delivery(
        &self,
        _delivery_result: &rdkafka::producer::DeliveryResult<'_>,
        _delivery_opaque: Self::DeliveryOpaque,
    ) {
    }
}

/// Configuração do `KafkaBroker`.
#[derive(Debug, Clone)]
pub struct KafkaBrokerConfig {
    /// Lista `host:port,host:port` de bootstrap servers.
    pub brokers: String,
    /// Região AWS — relevante apenas quando `iam_auth = true`.
    pub region: String,
    /// Quando `true`, habilita SASL_SSL + OAUTHBEARER usando IAM MSK.
    pub iam_auth: bool,
}

impl KafkaBrokerConfig {
    /// Lê config das envs:
    /// - `MSK_BOOTSTRAP_SERVERS` ou `KAFKA_BROKERS` (obrigatória).
    /// - `AWS_REGION` (default `us-east-1`).
    /// - `MSK_IAM_ROLE` (presença habilita IAM SASL).
    pub fn from_env() -> Result<Self, BrokerError> {
        let brokers = std::env::var("MSK_BOOTSTRAP_SERVERS")
            .or_else(|_| std::env::var("KAFKA_BROKERS"))
            .map_err(|_| {
                BrokerError::Configuration(
                    "missing MSK_BOOTSTRAP_SERVERS or KAFKA_BROKERS".to_string(),
                )
            })?;
        let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        let iam_auth = std::env::var("MSK_IAM_ROLE").is_ok();
        Ok(Self {
            brokers,
            region,
            iam_auth,
        })
    }
}

/// Broker Kafka real (rust-rdkafka).
pub struct KafkaBroker {
    producer: FutureProducer<MskIamContext>,
    subscriptions: Mutex<Vec<Subscription>>,
}

/// Registro interno de uma inscrição (handler + tópico).
///
/// O handler é invocado por [`KafkaBroker::dispatch`] quando o consumer
/// loop entrega uma mensagem para o tópico correspondente.
pub(crate) struct Subscription {
    pub(crate) topic: String,
    pub(crate) handler: BoxedHandler,
}

impl KafkaBroker {
    /// Constrói um `KafkaBroker` a partir das envs padrão.
    ///
    /// Equivalente a `KafkaBroker::with_config(KafkaBrokerConfig::from_env()?)`.
    pub fn from_env() -> Result<Self, BrokerError> {
        let cfg = KafkaBrokerConfig::from_env()?;
        Self::with_config(cfg)
    }

    /// Constrói um `KafkaBroker` a partir de uma configuração explícita.
    pub fn with_config(cfg: KafkaBrokerConfig) -> Result<Self, BrokerError> {
        let mut client_cfg = ClientConfig::new();
        client_cfg.set("bootstrap.servers", &cfg.brokers);
        client_cfg.set("message.timeout.ms", "5000");

        if cfg.iam_auth {
            client_cfg.set("security.protocol", "SASL_SSL");
            client_cfg.set("sasl.mechanisms", "OAUTHBEARER");
        }

        let context = MskIamContext { region: cfg.region };

        let producer: FutureProducer<MskIamContext> = client_cfg
            .create_with_context(context)
            .map_err(|e| BrokerError::Configuration(format!("rdkafka init failed: {e}")))?;

        Ok(Self {
            producer,
            subscriptions: Mutex::new(Vec::new()),
        })
    }

    /// Lista os tópicos atualmente inscritos (somente leitura, ordem de inscrição).
    pub fn subscribed_topics(&self) -> Vec<String> {
        self.subscriptions
            .lock()
            .expect("subscriptions mutex poisoned")
            .iter()
            .map(|s| s.topic.clone())
            .collect()
    }

    /// Despacha `msg` para todos os handlers inscritos em `msg.topic`.
    ///
    /// Esta é a primitiva consumida pelo consumer loop long-running:
    /// a tarefa de polling do rdkafka traduz cada `BorrowedMessage` em
    /// [`BrokerMessage`] e chama `dispatch` para entregar aos handlers.
    /// Testar o loop real exige broker físico; `dispatch` é testado de
    /// forma isolada.
    pub async fn dispatch(&self, msg: BrokerMessage) -> Result<(), BrokerError> {
        let handlers: Vec<BoxedHandler> = self
            .subscriptions
            .lock()
            .map_err(|_| BrokerError::Subscribe("subscriptions mutex poisoned".into()))?
            .iter()
            .filter(|s| s.topic == msg.topic)
            .map(|s| s.handler.clone())
            .collect();

        for handler in handlers {
            handler(msg.clone()).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl Broker for KafkaBroker {
    async fn subscribe(&self, topic: &str, handler: BoxedHandler) -> Result<(), BrokerError> {
        self.subscriptions
            .lock()
            .map_err(|_| BrokerError::Subscribe("subscriptions mutex poisoned".into()))?
            .push(Subscription {
                topic: topic.to_string(),
                handler,
            });
        Ok(())
    }

    async fn publish(&self, topic: &str, payload: &[u8]) -> Result<(), BrokerError> {
        let record: FutureRecord<'_, [u8], [u8]> = FutureRecord::to(topic).payload(payload);
        self.producer
            .send(record, Duration::from_secs(5))
            .await
            .map_err(|(e, _)| BrokerError::Publish(e.to_string()))?;
        Ok(())
    }
}
