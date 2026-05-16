use aws_lambda_events::event::kafka::KafkaEvent;
use serde::Deserialize;
use serverust_events::kafka::{KafkaPayloadError, KafkaRecord};

#[derive(Debug, Deserialize, PartialEq)]
struct CreditPayload {
    amount: u64,
    currency: String,
}

fn load_msk_fixture() -> KafkaEvent {
    let json = include_str!("fixtures/kafka/msk-v1.json");
    serde_json::from_str(json).expect("fixture deve ser válido")
}

#[test]
fn decodifica_fixture_msk_e_valida_payload() {
    let event = load_msk_fixture();
    let records: Vec<KafkaRecord<CreditPayload>> =
        KafkaRecord::from_kafka_event(&event).expect("deve decodificar sem erro");

    assert_eq!(records.len(), 2, "fixture tem 2 registros");

    let first = records.iter().find(|r| r.offset == 100).unwrap();
    assert_eq!(
        first.payload,
        CreditPayload {
            amount: 100,
            currency: "USD".into()
        }
    );
    assert_eq!(first.topic, "wallet.credits");
    assert_eq!(first.partition, 0);
    assert_eq!(first.key, Some(b"user-1".to_vec()));

    let second = records.iter().find(|r| r.offset == 101).unwrap();
    assert_eq!(
        second.payload,
        CreditPayload {
            amount: 250,
            currency: "EUR".into()
        }
    );
    assert_eq!(second.key, None);
}

#[test]
fn headers_decodificados_corretamente() {
    let event = load_msk_fixture();
    let records: Vec<KafkaRecord<CreditPayload>> =
        KafkaRecord::from_kafka_event(&event).expect("deve decodificar");

    let first = records.iter().find(|r| r.offset == 100).unwrap();
    assert!(first.headers.contains_key("correlationId"));
    assert!(first.headers.contains_key("source"));
}

#[test]
fn missing_value_retorna_erro_tipado() {
    let json = r#"{
        "records": {
            "test-0": [{
                "topic": "test",
                "partition": 0,
                "offset": 0,
                "timestamp": 0,
                "timestampType": "CREATE_TIME",
                "key": null,
                "value": null,
                "headers": []
            }]
        }
    }"#;
    let event: KafkaEvent = serde_json::from_str(json).unwrap();
    let result = KafkaRecord::<CreditPayload>::from_kafka_event(&event);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        KafkaPayloadError::MissingValue { .. }
    ));
}

#[test]
fn base64_invalido_retorna_erro_tipado() {
    let json = r#"{
        "records": {
            "test-0": [{
                "topic": "test",
                "partition": 0,
                "offset": 0,
                "timestamp": 0,
                "timestampType": "CREATE_TIME",
                "key": null,
                "value": "nao_e_base64!!!",
                "headers": []
            }]
        }
    }"#;
    let event: KafkaEvent = serde_json::from_str(json).unwrap();
    let result = KafkaRecord::<CreditPayload>::from_kafka_event(&event);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        KafkaPayloadError::Base64Decode { .. }
    ));
}

#[test]
fn json_invalido_retorna_erro_tipado() {
    // Base64 de "not json"
    let not_json_b64 =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b"not json");
    let json = format!(
        r#"{{
        "records": {{
            "test-0": [{{
                "topic": "test",
                "partition": 0,
                "offset": 0,
                "timestamp": 0,
                "timestampType": "CREATE_TIME",
                "key": null,
                "value": "{not_json_b64}",
                "headers": []
            }}]
        }}
    }}"#
    );
    let event: KafkaEvent = serde_json::from_str(&json).unwrap();
    let result = KafkaRecord::<CreditPayload>::from_kafka_event(&event);
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        KafkaPayloadError::JsonDeserialize { .. }
    ));
}
