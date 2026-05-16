//! Teste cold/warm para o `KafkaProducer`.
//!
//! O contrato é: a primeira chamada a `KafkaProducer::from_env()` constrói
//! o `FutureProducer` interno e o armazena em um `OnceLock` estático; as
//! chamadas seguintes retornam *o mesmo* `&'static KafkaProducer`, sem
//! recriar o cliente. Isto simula o reuso de conexão entre invocations
//! da mesma instância Lambda (warm start).
//!
//! Notamos que `ClientConfig::create()` da rdkafka não abre conexão TCP
//! — o socket é criado lazy no primeiro send. Por isso o teste pode
//! validar o singleton mesmo sem broker real.

#![cfg(feature = "kafka-producer")]

use serverust_events::producer::KafkaProducer;

#[tokio::test(flavor = "multi_thread")]
async fn from_env_returns_same_instance_on_warm_invocation() {
    // SAFETY: tests run single-threaded for env mutation; flavor multi_thread
    // is only required because rdkafka spawns background tasks.
    unsafe {
        std::env::set_var("KAFKA_BROKERS", "127.0.0.1:9092");
        std::env::remove_var("MSK_BOOTSTRAP_SERVERS");
        std::env::remove_var("MSK_IAM_ROLE");
    }

    let cold = KafkaProducer::from_env().expect("cold start should build producer");
    let warm = KafkaProducer::from_env().expect("warm start should reuse producer");

    assert!(
        std::ptr::eq(cold, warm),
        "OnceLock deve devolver a mesma instância entre invocations"
    );
}
