//! Testes de integracao para o subscriber FIFO e SqsFifoProducer (US-005).
//!
//! Cobre os criterios de aceitacao:
//! - SqsFifoMetadata extractor expoe message_group_id, message_deduplication_id, sequence_number
//! - SqsFifoMetadata em mensagem sem MessageGroupId retorna Err descritivo
//! - SqsFifoProducer com builder type-state exige message_group_id em compile-time
//!   (testes de compile-fail estao em serverust-macros/tests/ui/fail_*.rs)
//! - SqsFifoProducer envia entrada com message_group_id populado

#![cfg(feature = "sqs")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use aws_lambda_events::event::sqs::SqsEvent;
use serverust_events::{
    broker::BrokerMessage,
    extract::FromExtractor,
    router::EventRouter,
    sqs::{
        consumer::SqsBroker,
        extract::SqsFifoMetadata,
        fifo_producer::SqsFifoProducer,
        producer::{ProducerConfig, SendClient, SendEntry, SendResult},
    },
};

// ---------------------------------------------------------------------------
// Fixture FIFO
// ---------------------------------------------------------------------------

fn fifo_record_event(body: &str, group_id: &str, dedup_id: &str, seq: &str) -> SqsEvent {
    let raw = format!(
        r#"{{"Records":[{{"messageId":"fifo-mid-1","receiptHandle":"rh-fifo","body":{body_json},"attributes":{{"ApproximateReceiveCount":"1","SentTimestamp":"1678000000000","MessageGroupId":{group_json},"MessageDeduplicationId":{dedup_json},"SequenceNumber":{seq_json}}},"messageAttributes":{{}},"eventSourceARN":"arn:aws:sqs:us-east-1:123456789012:orders.fifo","eventSource":"aws:sqs","awsRegion":"us-east-1"}}]}}"#,
        body_json = serde_json::to_string(body).unwrap(),
        group_json = serde_json::to_string(group_id).unwrap(),
        dedup_json = serde_json::to_string(dedup_id).unwrap(),
        seq_json = serde_json::to_string(seq).unwrap(),
    );
    serde_json::from_str(&raw).expect("fifo record deve ser SqsEvent valido")
}

fn standard_record_event(body: &str) -> SqsEvent {
    let raw = format!(
        r#"{{"Records":[{{"messageId":"std-mid","receiptHandle":"rh","body":{body_json},"attributes":{{"ApproximateReceiveCount":"1","SentTimestamp":"1678000000000"}},"messageAttributes":{{}},"eventSourceARN":"arn:aws:sqs:us-east-1:1:orders","eventSource":"aws:sqs","awsRegion":"us-east-1"}}]}}"#,
        body_json = serde_json::to_string(body).unwrap()
    );
    serde_json::from_str(&raw).unwrap()
}

// ---------------------------------------------------------------------------
// AC: SqsFifoMetadata expoe os campos FIFO (happy path)
// ---------------------------------------------------------------------------

type FifoCaptured = Arc<Mutex<Vec<(String, Option<String>, Option<String>)>>>;

#[tokio::test]
async fn sqs_fifo_metadata_expoe_message_group_id_dedup_e_sequence_number() {
    let broker = Arc::new(SqsBroker::new());
    let captured: FifoCaptured = Arc::new(Mutex::new(Vec::new()));

    let router = {
        let captured = captured.clone();
        EventRouter::new().subscribe_with(
            "orders.fifo",
            move |_event: serde_json::Value, meta: SqsFifoMetadata| {
                let captured = captured.clone();
                async move {
                    captured.lock().unwrap().push((
                        meta.message_group_id.clone(),
                        meta.message_deduplication_id.clone(),
                        meta.sequence_number.clone(),
                    ));
                    Ok(())
                }
            },
        )
    };

    router.attach(broker.clone()).await.unwrap();

    let event = fifo_record_event(
        r#"{"orderId":"o-1","amount":1}"#,
        "user-42",
        "dedup-abc",
        "18866521389998784500",
    );
    let resp = broker.handle_sqs_event(&event).await;
    assert!(
        resp.batch_item_failures.is_empty(),
        "esperava sem falhas, got: {:?}",
        resp.batch_item_failures
    );

    let got = captured.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, "user-42");
    assert_eq!(got[0].1, Some("dedup-abc".to_string()));
    assert_eq!(got[0].2, Some("18866521389998784500".to_string()));
}

// ---------------------------------------------------------------------------
// AC: SqsFifoMetadata retorna Err quando MessageGroupId ausente (queue standard)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sqs_fifo_metadata_em_queue_standard_falha_mensagem() {
    let broker = Arc::new(SqsBroker::new());

    EventRouter::new()
        .subscribe_with("orders", |_: serde_json::Value, _: SqsFifoMetadata| async {
            Ok(())
        })
        .attach(broker.clone())
        .await
        .unwrap();

    let event = standard_record_event(r#"{}"#);
    let resp = broker.handle_sqs_event(&event).await;
    assert_eq!(
        resp.batch_item_failures.len(),
        1,
        "SqsFifoMetadata em queue standard deve falhar a mensagem em runtime"
    );
}

#[test]
fn sqs_fifo_metadata_err_descreve_message_group_id_ausente() {
    // BrokerMessage sintetico com header presente mas sem MessageGroupId em attributes.
    let sqs_msg_json = r#"{
        "messageId": "x",
        "receiptHandle": "rh",
        "body": "{}",
        "attributes": {},
        "messageAttributes": {},
        "md5OfBody": "",
        "eventSource": "aws:sqs",
        "eventSourceARN": "arn:aws:sqs:us-east-1:1:orders.fifo",
        "awsRegion": "us-east-1"
    }"#;
    let mut headers = HashMap::new();
    headers.insert(
        "__serverust_sqs_message".to_string(),
        sqs_msg_json.as_bytes().to_vec(),
    );
    let msg = BrokerMessage {
        topic: "orders.fifo".into(),
        partition: None,
        offset: None,
        key: None,
        payload: b"{}".to_vec(),
        headers,
        timestamp: None,
    };

    let result = SqsFifoMetadata::from_message(&msg, None);
    let err = result.expect_err("esperava Err");
    let err_str = format!("{err}");
    assert!(
        err_str.contains("MessageGroupId") || err_str.contains("FIFO"),
        "erro deve mencionar MessageGroupId/FIFO, got: {err_str}"
    );
}

#[test]
fn sqs_fifo_metadata_err_quando_header_ausente() {
    let msg = BrokerMessage {
        topic: "orders.fifo".into(),
        partition: None,
        offset: None,
        key: None,
        payload: b"{}".to_vec(),
        headers: HashMap::new(),
        timestamp: None,
    };
    let result = SqsFifoMetadata::from_message(&msg, None);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// AC: SqsFifoProducer envia entrada com message_group_id populado
// ---------------------------------------------------------------------------

struct CapturingFifoClient {
    calls: Arc<Mutex<Vec<Vec<SendEntry>>>>,
}

#[async_trait]
impl SendClient for CapturingFifoClient {
    async fn send_batch(
        &self,
        _queue_url: &str,
        entries: Vec<SendEntry>,
    ) -> Result<SendResult, String> {
        self.calls.lock().unwrap().push(entries.clone());
        let successful = entries
            .iter()
            .map(|e| (e.id.clone(), format!("aws-msg-{}", e.id)))
            .collect();
        Ok(SendResult {
            successful,
            failed: vec![],
        })
    }
}

#[tokio::test]
async fn sqs_fifo_producer_envia_com_message_group_id() {
    let calls = Arc::new(Mutex::new(Vec::<Vec<SendEntry>>::new()));
    let client = Arc::new(CapturingFifoClient {
        calls: calls.clone(),
    });
    let (producer, task) = SqsFifoProducer::new(
        client as Arc<dyn SendClient>,
        "https://sqs.us-east-1.amazonaws.com/123/orders.fifo",
        ProducerConfig {
            max_batch_size: 10,
            max_linger: Duration::from_millis(20),
            base_backoff: Duration::ZERO,
            ..Default::default()
        },
    );

    let result = producer
        .send_builder("hello")
        .message_group_id("group-A")
        .deduplication_id("dedup-1")
        .send()
        .await;

    assert!(result.is_ok(), "esperava Ok(MessageId), got {result:?}");

    drop(producer);
    task.await.unwrap();

    let log = calls.lock().unwrap();
    assert_eq!(log.len(), 1);
    let entry = &log[0][0];
    assert_eq!(entry.message_group_id.as_deref(), Some("group-A"));
    assert_eq!(entry.message_deduplication_id.as_deref(), Some("dedup-1"));
    assert_eq!(entry.message_body, "hello");
}

#[tokio::test]
async fn sqs_fifo_producer_dedup_opcional() {
    let calls = Arc::new(Mutex::new(Vec::<Vec<SendEntry>>::new()));
    let client = Arc::new(CapturingFifoClient {
        calls: calls.clone(),
    });
    let (producer, task) = SqsFifoProducer::new(
        client as Arc<dyn SendClient>,
        "https://sqs.us-east-1.amazonaws.com/123/orders.fifo",
        ProducerConfig {
            max_batch_size: 10,
            max_linger: Duration::from_millis(20),
            base_backoff: Duration::ZERO,
            ..Default::default()
        },
    );

    let _ = producer
        .send_builder("body")
        .message_group_id("group-B")
        .send()
        .await
        .unwrap();

    drop(producer);
    task.await.unwrap();

    let log = calls.lock().unwrap();
    let entry = &log[0][0];
    assert_eq!(entry.message_group_id.as_deref(), Some("group-B"));
    assert!(
        entry.message_deduplication_id.is_none(),
        "deduplication_id deve ser opcional"
    );
}

#[tokio::test]
async fn sqs_fifo_producer_atributos_e_dedup_antes_do_group_id() {
    let calls = Arc::new(Mutex::new(Vec::<Vec<SendEntry>>::new()));
    let client = Arc::new(CapturingFifoClient {
        calls: calls.clone(),
    });
    let (producer, task) = SqsFifoProducer::new(
        client as Arc<dyn SendClient>,
        "https://sqs.us-east-1.amazonaws.com/123/orders.fifo",
        ProducerConfig {
            max_batch_size: 10,
            max_linger: Duration::from_millis(20),
            base_backoff: Duration::ZERO,
            ..Default::default()
        },
    );

    // Builder fluente: dedup/attribute no estado NoGroupId, depois message_group_id transita para HasGroupId.
    let _ = producer
        .send_builder("payload")
        .deduplication_id("dedup-pre")
        .attribute("custom-key", "v1")
        .message_group_id("group-C")
        .send()
        .await
        .unwrap();

    drop(producer);
    task.await.unwrap();

    let log = calls.lock().unwrap();
    let entry = &log[0][0];
    assert_eq!(entry.message_group_id.as_deref(), Some("group-C"));
    assert_eq!(entry.message_deduplication_id.as_deref(), Some("dedup-pre"));
    assert_eq!(
        entry
            .message_attributes
            .get("custom-key")
            .map(|s| s.as_str()),
        Some("v1")
    );
}

#[tokio::test]
async fn sqs_fifo_producer_signal_shutdown_drena_pendentes() {
    let calls = Arc::new(Mutex::new(Vec::<Vec<SendEntry>>::new()));
    let client = Arc::new(CapturingFifoClient {
        calls: calls.clone(),
    });
    let (producer, task) = SqsFifoProducer::new(
        client as Arc<dyn SendClient>,
        "https://sqs.us-east-1.amazonaws.com/123/orders.fifo",
        ProducerConfig {
            max_batch_size: 100,
            max_linger: Duration::from_secs(60),
            base_backoff: Duration::ZERO,
            ..Default::default()
        },
    );

    let mut join_set = tokio::task::JoinSet::new();
    for i in 0..3usize {
        let p = producer.clone();
        join_set.spawn(async move {
            p.send_builder(format!("m-{i}"))
                .message_group_id("g")
                .send()
                .await
        });
    }

    tokio::task::yield_now().await;
    producer.signal_shutdown();
    drop(producer);
    task.await.unwrap();

    let mut results = Vec::new();
    while let Some(r) = join_set.join_next().await {
        results.push(r.expect("join ok"));
    }
    for r in &results {
        assert!(r.is_ok(), "esperava Ok após shutdown, got {r:?}");
    }
}

// Benchmark FIFO ElasticMQ — placeholder ignorado por padrao (US-010 adicionara aws-sdk-sqs real).
#[tokio::test]
#[ignore]
async fn fifo_ordering_garantido_em_elasticmq() {
    let _url = match std::env::var("ELASTICMQ_URL") {
        Ok(u) => u,
        Err(_) => {
            eprintln!("ELASTICMQ_URL nao definida; teste FIFO ElasticMQ ignorado");
            return;
        }
    };
    // Verificacao end-to-end com aws-sdk-sqs real chega em US-010.
    eprintln!("FIFO ordering ElasticMQ — a implementar com aws-sdk-sqs em US-010");
}
