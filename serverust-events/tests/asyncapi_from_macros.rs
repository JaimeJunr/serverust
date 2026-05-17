//! Testes US-013: geracao automatica de AsyncAPI 3.0 a partir das macros
//! `#[subscriber]` / `#[publisher]`.
//!
//! Garante:
//! - `Sub::register_asyncapi(builder)` existe quando `serverust-events/asyncapi`
//!   esta on (macro emite o metodo).
//! - Drivers `kafka` e `sqs` registram no mesmo spec.
//! - `#[derive(JsonSchema)]` no T faz o schema aparecer em `components.schemas`.
//! - `#[publisher]` empilhado gera operation `send` para o tipo do handler.
//! - Helper `emit_asyncapi_if_requested` detecta `--serverust-emit-asyncapi`
//!   na lista de args e grava o YAML.

#![cfg(all(feature = "asyncapi", feature = "sqs"))]

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serverust_events::asyncapi::{Action, AsyncApiBuilder, emit_asyncapi_if_requested};
use serverust_events::broker::BrokerError;
// `publisher` aparece como atributo aninhado em `#[publisher(topic = "...")]`,
// mas é consumido pelo macro externo `#[subscriber]` — Rust não detecta uso.
#[allow(unused_imports)]
use serverust_macros::{publisher, subscriber};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct OrderCreated {
    id: String,
    total: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct OrderShipped {
    id: String,
    tracking: String,
}

#[subscriber(driver = "kafka", topic = "orders.created", asyncapi)]
async fn process_order_kafka(_event: OrderCreated) -> Result<(), BrokerError> {
    Ok(())
}

#[subscriber(driver = "sqs", queue = "orders-shipped", asyncapi)]
async fn process_order_sqs(_event: OrderShipped) -> Result<(), BrokerError> {
    Ok(())
}

#[subscriber(driver = "kafka", topic = "orders.in", asyncapi)]
#[publisher(topic = "orders.out")]
async fn route_order(event: OrderCreated) -> Result<OrderCreated, BrokerError> {
    Ok(event)
}

// Subscriber sem flag asyncapi — confirma que back-compat se mantém.
#[subscriber(driver = "kafka", topic = "orders.no-spec")]
async fn process_no_spec(_event: OrderCreated) -> Result<(), BrokerError> {
    Ok(())
}

#[test]
fn constante_has_asyncapi_reflete_flag() {
    #[allow(clippy::assertions_on_constants)]
    {
        assert!(process_order_kafka::HAS_ASYNCAPI);
        assert!(process_order_sqs::HAS_ASYNCAPI);
        assert!(route_order::HAS_ASYNCAPI);
        assert!(!process_no_spec::HAS_ASYNCAPI);
    }
}

#[test]
fn register_asyncapi_kafka_subscriber_adds_receive_channel() {
    let builder = AsyncApiBuilder::new().title("Orders API").version("1.0.0");
    let builder = process_order_kafka::register_asyncapi(builder);
    let spec = builder.build();

    assert!(
        spec.channels.contains_key("orders.created"),
        "channel ausente: {:?}",
        spec.channels.keys().collect::<Vec<_>>()
    );
    let has_receive = spec
        .operations
        .values()
        .any(|op| matches!(op.action, Action::Receive));
    assert!(has_receive, "operacao receive deve existir");
}

#[test]
fn register_asyncapi_sqs_subscriber_adds_receive_channel() {
    let builder = AsyncApiBuilder::new().title("Orders API").version("1.0.0");
    let builder = process_order_sqs::register_asyncapi(builder);
    let spec = builder.build();

    assert!(spec.channels.contains_key("orders-shipped"));
    assert!(spec.components.schemas.contains_key("OrderShipped"));
}

#[test]
fn drivers_kafka_e_sqs_coexistem_no_mesmo_spec() {
    let builder = AsyncApiBuilder::new().title("Mixed").version("1.0.0");
    let builder = process_order_kafka::register_asyncapi(builder);
    let builder = process_order_sqs::register_asyncapi(builder);
    let spec = builder.build();

    assert!(spec.channels.contains_key("orders.created"));
    assert!(spec.channels.contains_key("orders-shipped"));
    assert_eq!(spec.channels.len(), 2);

    let yaml = spec.to_yaml().expect("yaml");
    assert!(yaml.contains("orders.created"));
    assert!(yaml.contains("orders-shipped"));
    // Schemas dos dois payloads devem estar embutidos.
    assert!(yaml.contains("OrderCreated"));
    assert!(yaml.contains("OrderShipped"));
}

#[test]
fn schemas_jsonschema_aparecem_em_components_schemas() {
    let builder = AsyncApiBuilder::new().title("X").version("1.0.0");
    let builder = process_order_kafka::register_asyncapi(builder);
    let spec = builder.build();

    let schema = spec
        .components
        .schemas
        .get("OrderCreated")
        .expect("schema OrderCreated");
    let s = serde_json::to_string(schema).unwrap();
    assert!(s.contains("\"id\""), "schema deve conter campo id: {s}");
    assert!(
        s.contains("\"total\""),
        "schema deve conter campo total: {s}"
    );
}

#[test]
fn subscriber_com_publisher_empilhado_gera_send_e_receive() {
    let builder = AsyncApiBuilder::new().title("Pub").version("1.0.0");
    let builder = route_order::register_asyncapi(builder);
    let spec = builder.build();

    assert!(spec.channels.contains_key("orders.in"));
    assert!(spec.channels.contains_key("orders.out"));

    let has_receive = spec
        .operations
        .values()
        .any(|op| matches!(op.action, Action::Receive));
    let has_send = spec
        .operations
        .values()
        .any(|op| matches!(op.action, Action::Send));
    assert!(has_receive, "deve existir operacao receive");
    assert!(has_send, "deve existir operacao send");
}

#[test]
fn yaml_resultante_roundtrip_em_estrutura_asyncapi_3() {
    let builder = AsyncApiBuilder::new().title("RT").version("2.0.0");
    let builder = process_order_kafka::register_asyncapi(builder);
    let builder = process_order_sqs::register_asyncapi(builder);
    let spec = builder.build();
    let yaml = spec.to_yaml().expect("yaml");

    let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("parse yaml");
    assert_eq!(parsed["asyncapi"].as_str(), Some("3.0.0"));
    assert_eq!(parsed["info"]["title"].as_str(), Some("RT"));
    assert!(parsed["channels"]["orders.created"].is_mapping());
    assert!(parsed["channels"]["orders-shipped"].is_mapping());

    let ops = parsed["operations"].as_mapping().expect("operations");
    for (_id, op) in ops {
        let action = op["action"].as_str().expect("action str");
        assert!(action == "receive" || action == "send");
    }
}

#[test]
fn emit_asyncapi_if_requested_grava_yaml_quando_flag_presente() {
    let tmp = std::env::temp_dir().join("serverust-asyncapi-test.yaml");
    let _ = std::fs::remove_file(&tmp);

    let builder = AsyncApiBuilder::new().title("E2E").version("1.0.0");
    let builder = process_order_kafka::register_asyncapi(builder);
    let builder = process_order_sqs::register_asyncapi(builder);

    let args = vec![
        "serverust".to_string(),
        "--serverust-emit-asyncapi".to_string(),
        tmp.to_string_lossy().to_string(),
    ];

    let handled = emit_asyncapi_if_requested(builder, &args).expect("emit ok");
    assert!(handled, "flag presente => handled=true");

    let written = std::fs::read_to_string(&tmp).expect("arquivo gravado");
    assert!(written.contains("asyncapi: 3.0.0"));
    assert!(written.contains("orders.created"));
    assert!(written.contains("orders-shipped"));
    let _ = std::fs::remove_file(&tmp);
}

#[test]
fn emit_asyncapi_if_requested_retorna_false_quando_flag_ausente() {
    let builder = AsyncApiBuilder::new().title("No flag").version("1.0.0");
    let builder = process_order_kafka::register_asyncapi(builder);
    let args = vec!["serverust".to_string()];

    let handled = emit_asyncapi_if_requested(builder, &args).expect("emit ok");
    assert!(!handled);
}
