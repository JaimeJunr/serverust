//! Testes da geração de AsyncAPI 3.0 a partir do builder.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serverust_events::asyncapi::{Action, AsyncApiBuilder};

#[derive(Serialize, Deserialize, JsonSchema)]
struct OrderCreated {
    id: String,
    total: f64,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct OrderProcessed {
    id: String,
    status: String,
}

#[test]
fn builder_emits_asyncapi_3_root_fields() {
    let spec = AsyncApiBuilder::new()
        .title("Orders API")
        .version("1.0.0")
        .description("Documenta eventos do domínio de ordens")
        .build();

    assert_eq!(spec.asyncapi, "3.0.0");
    assert_eq!(spec.info.title, "Orders API");
    assert_eq!(spec.info.version, "1.0.0");
    assert_eq!(
        spec.info.description.as_deref(),
        Some("Documenta eventos do domínio de ordens")
    );
}

#[test]
fn add_receive_creates_channel_and_operation() {
    let spec = AsyncApiBuilder::new()
        .title("Orders API")
        .version("1.0.0")
        .add_receive::<OrderCreated>("orders.created")
        .build();

    assert!(spec.channels.contains_key("orders.created"));
    let op = spec
        .operations
        .values()
        .find(|o| matches!(o.action, Action::Receive))
        .expect("receive operation");
    assert_eq!(op.channel.reference, "#/channels/orders.created");
    assert!(!op.messages.is_empty());
}

#[test]
fn add_send_creates_send_operation() {
    let spec = AsyncApiBuilder::new()
        .title("Orders API")
        .version("1.0.0")
        .add_send::<OrderProcessed>("orders.processed")
        .build();

    let op = spec
        .operations
        .values()
        .find(|o| matches!(o.action, Action::Send))
        .expect("send operation");
    assert_eq!(op.channel.reference, "#/channels/orders.processed");
}

#[test]
fn schema_for_type_is_embedded_in_components() {
    let spec = AsyncApiBuilder::new()
        .title("Orders API")
        .version("1.0.0")
        .add_receive::<OrderCreated>("orders.created")
        .build();

    assert!(
        spec.components.schemas.contains_key("OrderCreated"),
        "schemas: {:?}",
        spec.components.schemas.keys().collect::<Vec<_>>()
    );
    assert!(spec.components.messages.contains_key("OrderCreated"));

    let schema = &spec.components.schemas["OrderCreated"];
    let yaml_str = serde_json::to_string(schema).expect("serialize");
    assert!(
        yaml_str.contains("\"id\""),
        "schema deve conter campo id: {yaml_str}"
    );
    assert!(
        yaml_str.contains("\"total\""),
        "schema deve conter campo total: {yaml_str}"
    );
}

#[test]
fn yaml_output_contains_required_asyncapi_fields() {
    let spec = AsyncApiBuilder::new()
        .title("Orders API")
        .version("1.0.0")
        .add_receive::<OrderCreated>("orders.created")
        .add_send::<OrderProcessed>("orders.processed")
        .build();

    let yaml = spec.to_yaml().expect("yaml");

    // AsyncAPI 3.0 exige: asyncapi, info, channels, operations
    assert!(
        yaml.contains("asyncapi: 3.0.0"),
        "yaml falta version: {yaml}"
    );
    assert!(yaml.contains("info:"), "yaml falta info: {yaml}");
    assert!(
        yaml.contains("title: Orders API"),
        "yaml falta title: {yaml}"
    );
    assert!(yaml.contains("channels:"), "yaml falta channels: {yaml}");
    assert!(
        yaml.contains("operations:"),
        "yaml falta operations: {yaml}"
    );
    assert!(
        yaml.contains("orders.created"),
        "yaml falta tópico receive: {yaml}"
    );
    assert!(
        yaml.contains("orders.processed"),
        "yaml falta tópico send: {yaml}"
    );
    assert!(
        yaml.contains("components:"),
        "yaml falta components: {yaml}"
    );
}

#[test]
fn yaml_round_trips_to_valid_asyncapi_3_structure() {
    let spec = AsyncApiBuilder::new()
        .title("Orders API")
        .version("1.0.0")
        .add_receive::<OrderCreated>("orders.created")
        .build();

    let yaml = spec.to_yaml().expect("yaml");
    let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml).expect("parse yaml");

    assert_eq!(parsed["asyncapi"].as_str(), Some("3.0.0"));
    assert!(parsed["info"]["title"].is_string());
    assert!(parsed["info"]["version"].is_string());
    assert!(parsed["channels"]["orders.created"].is_mapping());
    assert!(parsed["channels"]["orders.created"]["address"].is_string());

    // operations devem ter action receive/send + channel ref
    let ops = parsed["operations"].as_mapping().expect("operations map");
    assert!(!ops.is_empty(), "operations não pode ser vazio");
    for (_id, op) in ops {
        let action = op["action"].as_str().expect("action string");
        assert!(
            action == "receive" || action == "send",
            "action inválida: {action}"
        );
        let chan_ref = op["channel"]["$ref"].as_str().expect("channel $ref");
        assert!(chan_ref.starts_with("#/channels/"));
    }
}

#[test]
fn operation_id_distinguishes_subscribe_publish_for_same_topic() {
    let spec = AsyncApiBuilder::new()
        .title("Orders API")
        .version("1.0.0")
        .add_receive::<OrderCreated>("orders.events")
        .add_send::<OrderProcessed>("orders.events")
        .build();

    let actions: Vec<_> = spec.operations.values().map(|o| &o.action).collect();
    assert!(actions.iter().any(|a| matches!(a, Action::Receive)));
    assert!(actions.iter().any(|a| matches!(a, Action::Send)));
    assert_eq!(
        spec.operations.len(),
        2,
        "duas operations distintas para o mesmo channel"
    );
}

#[test]
fn duplicate_topic_with_same_action_is_idempotent() {
    let spec = AsyncApiBuilder::new()
        .title("Orders API")
        .version("1.0.0")
        .add_receive::<OrderCreated>("orders.created")
        .add_receive::<OrderCreated>("orders.created")
        .build();

    // Mesma combinação topic+action+message não deve duplicar
    assert_eq!(spec.channels.len(), 1);
    assert_eq!(spec.operations.len(), 1);
}
