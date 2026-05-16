//! Testes de contrato dos DTOs do handler kafka-wallet.
//!
//! Estes testes garantem que o payload publicado no tópico `wallet.events` é
//! decodificado corretamente pelo extractor `KafkaRecord<WalletEvent>` e que
//! `Wallet` satisfaz o contrato `DynamoTable` exigido pelo `DynamoRepo<Wallet>`.

use aws_lambda_events::event::kafka::{KafkaEvent, KafkaRecord as RawKafkaRecord};
use base64::Engine;
use kafka_wallet::{Wallet, WalletEvent};
use serverust_events::kafka::KafkaRecord;
use serverust_telemetry::dynamo::DynamoTable;

fn make_event(json_payload: &str, topic: &str) -> KafkaEvent {
    let encoded = base64::engine::general_purpose::STANDARD.encode(json_payload);
    let raw = serde_json::from_value::<RawKafkaRecord>(serde_json::json!({
        "topic": topic,
        "partition": 0,
        "offset": 0,
        "timestamp": 0,
        "timestampType": "CREATE_TIME",
        "key": null,
        "value": encoded,
        "headers": [],
    }))
    .unwrap();
    let mut ev = KafkaEvent::default();
    ev.records
        .entry(format!("{topic}-0"))
        .or_default()
        .push(raw);
    ev
}

#[test]
fn wallet_event_decodes_from_base64_kafka_record() {
    let event = make_event(
        r#"{"user_id":"u-1","amount":100,"operation":"credit"}"#,
        "wallet.events",
    );
    let records: Vec<KafkaRecord<WalletEvent>> = KafkaRecord::from_kafka_event(&event).unwrap();
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.payload.user_id, "u-1");
    assert_eq!(r.payload.amount, 100);
    assert_eq!(r.payload.operation, "credit");
    assert_eq!(r.topic, "wallet.events");
}

#[test]
fn wallet_dynamo_table_metadata_match_handler_table() {
    assert_eq!(Wallet::TABLE_NAME, "Wallets");
    assert_eq!(Wallet::PK_FIELD, "user_id");
    assert_eq!(Wallet::SK_FIELD, None);
}

#[test]
fn wallet_pk_value_is_user_id() {
    let w = Wallet {
        user_id: "u-2".into(),
        balance: 42,
    };
    assert_eq!(w.pk_value(), serde_json::Value::String("u-2".into()));
}
