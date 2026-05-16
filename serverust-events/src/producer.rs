//! Producer Kafka injetável, opt-in via feature `kafka-producer`.
//!
//! `KafkaProducer` encapsula um `rdkafka::producer::FutureProducer` e oferece:
//!
//! - [`KafkaProducer::from_env`] — constrói (uma única vez) a partir de
//!   `MSK_BOOTSTRAP_SERVERS` ou `KAFKA_BROKERS`, com SASL/IAM opcional via
//!   `MSK_IAM_ROLE`.
//! - [`KafkaProducer::publish`] — serializa `&T: Serialize` via
//!   `serde_json::to_vec` e produz o registro.
//!
//! Reuso de conexão: o produtor vive em um `OnceLock` estático, então
//! invocations Lambda seguintes na mesma instância reaproveitam o socket TCP.

use std::sync::OnceLock;
use std::time::Duration;

use rdkafka::ClientContext;
use rdkafka::client::OAuthToken;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::ConsumerContext;
use rdkafka::producer::{FutureProducer, FutureRecord, ProducerContext};
use serde::Serialize;
use thiserror::Error;

/// Erros do producer.
#[derive(Debug, Error)]
pub enum KafkaError {
    /// Nenhuma das envs `MSK_BOOTSTRAP_SERVERS` / `KAFKA_BROKERS` definida.
    #[error("missing required env: {0}")]
    MissingEnv(&'static str),

    /// Erro nativo da librdkafka (criação ou send).
    #[error("rdkafka error: {0}")]
    Rdkafka(#[from] rdkafka::error::KafkaError),

    /// Falha ao serializar o payload para JSON.
    #[error("serialize error: {0}")]
    Serialize(#[from] serde_json::Error),

    /// Falha ao gerar token SASL IAM.
    #[error("sasl signer error: {0}")]
    SaslSigner(String),
}

/// Contexto rdkafka que injeta o token IAM MSK via OAUTHBEARER quando o
/// callback é chamado pela librdkafka.
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
        // O callback roda em thread da librdkafka. Cria runtime efêmero
        // só para chamar o signer assíncrono.
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

/// Producer Kafka encapsulado para uso injetável via DI ou direto.
pub struct KafkaProducer {
    inner: FutureProducer<MskIamContext>,
}

static INSTANCE: OnceLock<KafkaProducer> = OnceLock::new();

impl KafkaProducer {
    /// Devolve o producer singleton, construindo-o na primeira chamada.
    ///
    /// Envs consultadas (na ordem):
    /// - `MSK_BOOTSTRAP_SERVERS` ou `KAFKA_BROKERS` (obrigatória, uma das duas).
    /// - `AWS_REGION` (usada quando `MSK_IAM_ROLE` está presente).
    /// - `MSK_IAM_ROLE` (opcional, ativa SASL_SSL + OAUTHBEARER).
    ///
    /// Retorna `&'static KafkaProducer`: o ponteiro é estável e idêntico
    /// entre invocations da mesma instância Lambda (warm start).
    pub fn from_env() -> Result<&'static KafkaProducer, KafkaError> {
        if let Some(p) = INSTANCE.get() {
            return Ok(p);
        }
        let built = Self::build_from_env()?;
        Ok(INSTANCE.get_or_init(|| built))
    }

    fn build_from_env() -> Result<Self, KafkaError> {
        let brokers = std::env::var("MSK_BOOTSTRAP_SERVERS")
            .or_else(|_| std::env::var("KAFKA_BROKERS"))
            .map_err(|_| KafkaError::MissingEnv("MSK_BOOTSTRAP_SERVERS or KAFKA_BROKERS"))?;

        let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());

        let mut cfg = ClientConfig::new();
        cfg.set("bootstrap.servers", &brokers);
        cfg.set("message.timeout.ms", "5000");

        let context = MskIamContext {
            region: region.clone(),
        };

        if std::env::var("MSK_IAM_ROLE").is_ok() {
            cfg.set("security.protocol", "SASL_SSL");
            cfg.set("sasl.mechanisms", "OAUTHBEARER");
        }

        let inner: FutureProducer<MskIamContext> = cfg.create_with_context(context)?;
        Ok(KafkaProducer { inner })
    }

    /// Publica `payload` no `topic` com `key`, serializando via
    /// `serde_json::to_vec`. Espera o ack do broker (timeout 5s).
    pub async fn publish<T: Serialize + ?Sized>(
        &self,
        topic: &str,
        key: &str,
        payload: &T,
    ) -> Result<(), KafkaError> {
        let bytes = serde_json::to_vec(payload)?;
        let record: FutureRecord<'_, str, [u8]> =
            FutureRecord::to(topic).key(key).payload(bytes.as_slice());
        self.inner
            .send(record, Duration::from_secs(5))
            .await
            .map_err(|(e, _)| KafkaError::Rdkafka(e))?;
        Ok(())
    }
}
