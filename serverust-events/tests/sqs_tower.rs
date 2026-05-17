//! Testes da pipeline Tower para subscribers SQS (US-006).
//!
//! Cobre:
//! - `SqsSubscriber` impl `tower::Service<SqsMessage>`
//! - Smoke test compondo com `TracingLayer` (de `serverust-telemetry`)
//! - Ordem da pipeline: tracing externo, retry interno, idempotency no meio

#![cfg(feature = "sqs")]

use std::sync::Arc;
use std::sync::Mutex;

use aws_lambda_events::event::sqs::SqsMessage;
use serverust_events::broker::BrokerError;
use serverust_events::sqs::layers::{IdempotencyLayer, RetryLayer};
use serverust_events::sqs::subscriber::SqsSubscriber;
use serverust_telemetry::InMemoryIdempotencyStore;
use serverust_telemetry::tower::TracingLayer;
use tower::{Service, ServiceBuilder, ServiceExt};

fn dummy_message(id: &str, body: &str) -> SqsMessage {
    let mut m = SqsMessage::default();
    m.message_id = Some(id.to_string());
    m.body = Some(body.to_string());
    m
}

fn message_without_id(body: &str) -> SqsMessage {
    let mut m = SqsMessage::default();
    m.message_id = None;
    m.body = Some(body.to_string());
    m
}

#[tokio::test]
async fn sqs_subscriber_implements_tower_service() {
    let called = Arc::new(Mutex::new(Vec::<String>::new()));
    let called_for_handler = called.clone();
    let mut svc = SqsSubscriber::new(move |msg: SqsMessage| {
        let called = called_for_handler.clone();
        async move {
            called.lock().unwrap().push(msg.body.unwrap_or_default());
            Ok::<_, BrokerError>(())
        }
    });

    svc.ready().await.unwrap();
    svc.call(dummy_message("m-1", "hello")).await.unwrap();

    assert_eq!(called.lock().unwrap().as_slice(), &["hello".to_string()]);
}

#[tokio::test]
async fn smoke_test_with_telemetry_tracing_layer() {
    // Garantia: TracingLayer (do crate serverust-telemetry) compõe com um
    // Service<SqsMessage> sem qualquer modificação no crate de telemetry.
    let called = Arc::new(Mutex::new(0_u32));
    let called_for_handler = called.clone();

    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let called = called_for_handler.clone();
        async move {
            *called.lock().unwrap() += 1;
            Ok::<_, BrokerError>(())
        }
    });

    let mut svc = ServiceBuilder::new()
        .layer(TracingLayer::new("sqs.smoke"))
        .service(subscriber);

    svc.ready().await.unwrap();
    svc.call(dummy_message("m-1", "x")).await.unwrap();
    assert_eq!(*called.lock().unwrap(), 1);
}

#[tokio::test]
async fn pipeline_order_tracing_outer_retry_inner_idempotency_between() {
    // Verifica a ordem documentada da pipeline (PRD §6.3):
    //   inbound → TracingLayer → IdempotencyLayer → RetryLayer → handler
    //
    // Como observar ordem sem inspecionar spans:
    //   - O handler falha 2x e tem sucesso na 3a (RetryLayer com max=3 cobre).
    //   - Se IdempotencyLayer estivesse DENTRO do RetryLayer, cada tentativa
    //     gravaria/consultaria o store — após o 1o sucesso, a chave estaria
    //     marcada e a próxima entrada do batch faria skip mesmo com message_id
    //     diferente (não acontece aqui). Aqui usamos duas mensagens com IDs
    //     iguais para detectar dedupe correta: a 2a mensagem é skipada PELO
    //     IdempotencyLayer ANTES do RetryLayer, então o handler NÃO é chamado
    //     uma 4a vez.
    //   - TracingLayer fora: ao envolver o ServiceBuilder na ordem
    //     `.layer(TracingLayer).layer(IdempotencyLayer).layer(RetryLayer)`,
    //     o tracing fica como camada mais externa (verificado por composição
    //     compilar — type alias garante o shape final).

    let attempts = Arc::new(Mutex::new(0_u32));
    let attempts_for_handler = attempts.clone();
    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let attempts = attempts_for_handler.clone();
        async move {
            let mut g = attempts.lock().unwrap();
            *g += 1;
            let n = *g;
            drop(g);
            if n < 3 {
                Err(BrokerError::Subscribe(format!("attempt {n} failed")))
            } else {
                Ok::<_, BrokerError>(())
            }
        }
    });

    let store: Arc<dyn serverust_telemetry::IdempotencyStore> =
        Arc::new(InMemoryIdempotencyStore::new());

    let mut svc = ServiceBuilder::new()
        .layer(TracingLayer::new("sqs.order-test"))
        .layer(IdempotencyLayer::new(store.clone()))
        .layer(RetryLayer::new(3))
        .service(subscriber);

    let msg = dummy_message("dup-key", "payload");

    // 1ª entrega: passa pelo idempotency (novo) → retry 3x → sucesso.
    svc.ready().await.unwrap();
    svc.call(msg.clone()).await.unwrap();
    assert_eq!(*attempts.lock().unwrap(), 3, "retry deve tentar 3x");

    // 2ª entrega com mesmo message_id: idempotency dedupa ANTES do retry
    // (handler não é chamado de novo).
    svc.ready().await.unwrap();
    svc.call(msg).await.unwrap();
    assert_eq!(
        *attempts.lock().unwrap(),
        3,
        "handler não pode ser chamado de novo: dedupe pelo IdempotencyLayer fora do RetryLayer"
    );
}

#[tokio::test]
async fn retry_layer_exhausts_and_propagates_last_error() {
    let subscriber = SqsSubscriber::new(|_msg: SqsMessage| async move {
        Err::<(), _>(BrokerError::Subscribe("always fails".into()))
    });

    let mut svc = ServiceBuilder::new()
        .layer(RetryLayer::new(2))
        .service(subscriber);

    svc.ready().await.unwrap();
    let err = svc.call(dummy_message("m-1", "x")).await.unwrap_err();
    assert!(matches!(err, BrokerError::Subscribe(_)));
    assert!(err.to_string().contains("always fails"));
}

#[tokio::test]
async fn idempotency_layer_skips_when_record_exists() {
    let calls = Arc::new(Mutex::new(0_u32));
    let calls_for_handler = calls.clone();
    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let calls = calls_for_handler.clone();
        async move {
            *calls.lock().unwrap() += 1;
            Ok::<_, BrokerError>(())
        }
    });

    let store: Arc<dyn serverust_telemetry::IdempotencyStore> =
        Arc::new(InMemoryIdempotencyStore::new());

    let mut svc = ServiceBuilder::new()
        .layer(IdempotencyLayer::new(store))
        .service(subscriber);

    let msg = dummy_message("idem-1", "x");
    svc.ready().await.unwrap();
    svc.call(msg.clone()).await.unwrap();
    svc.ready().await.unwrap();
    svc.call(msg).await.unwrap();

    assert_eq!(*calls.lock().unwrap(), 1, "2a chamada deve ser skipada");
}

#[tokio::test]
async fn idempotency_layer_without_message_id_falls_through() {
    let calls = Arc::new(Mutex::new(0_u32));
    let calls_for_handler = calls.clone();
    let subscriber = SqsSubscriber::new(move |_msg: SqsMessage| {
        let calls = calls_for_handler.clone();
        async move {
            *calls.lock().unwrap() += 1;
            Ok::<_, BrokerError>(())
        }
    });

    let store: Arc<dyn serverust_telemetry::IdempotencyStore> =
        Arc::new(InMemoryIdempotencyStore::new());

    let mut svc = ServiceBuilder::new()
        .layer(IdempotencyLayer::new(store))
        .service(subscriber);

    let msg = message_without_id("no-id");
    svc.ready().await.unwrap();
    svc.call(msg.clone()).await.unwrap();
    svc.ready().await.unwrap();
    svc.call(msg).await.unwrap();

    assert_eq!(
        *calls.lock().unwrap(),
        2,
        "sem message_id idempotência é bypass"
    );
}
