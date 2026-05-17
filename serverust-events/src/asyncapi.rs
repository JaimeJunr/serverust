//! Geração de schema [AsyncAPI 3.0](https://www.asyncapi.com/docs/reference/specification/v3.0.0)
//! a partir dos tipos de evento da aplicação.
//!
//! O builder programático aceita tipos `serde::Serialize`/`Deserialize` com
//! `schemars::JsonSchema` e emite YAML AsyncAPI 3.0 com:
//!
//! - `channels` — um por tópico Kafka/SQS/SNS;
//! - `operations` — uma `receive` por subscriber e uma `send` por publisher;
//! - `components.messages` + `components.schemas` — JSON Schema embarcado de cada tipo.
//!
//! # Exemplo
//!
//! ```ignore
//! use schemars::JsonSchema;
//! use serde::{Deserialize, Serialize};
//! use serverust_events::asyncapi::AsyncApiBuilder;
//!
//! #[derive(Serialize, Deserialize, JsonSchema)]
//! struct OrderCreated { id: String, total: f64 }
//!
//! let yaml = AsyncApiBuilder::new()
//!     .title("Orders API")
//!     .version("1.0.0")
//!     .add_receive::<OrderCreated>("orders.created")
//!     .build()
//!     .to_yaml()
//!     .unwrap();
//! ```

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use schemars::JsonSchema;
use serde::Serialize;

/// Itens reexportados para uso pelas macros `serverust-macros`. Não é parte
/// do contrato público — pode mudar sem bump de major.
#[doc(hidden)]
pub mod __private {
    pub use schemars::JsonSchema;
}

/// Spec AsyncAPI 3.0 completo, pronto para serialização YAML/JSON.
#[derive(Debug, Serialize)]
pub struct AsyncApiSpec {
    pub asyncapi: String,
    pub info: Info,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub channels: BTreeMap<String, Channel>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub operations: BTreeMap<String, Operation>,
    #[serde(skip_serializing_if = "Components::is_empty")]
    pub components: Components,
}

/// Metadados obrigatórios do spec (objeto `info`).
#[derive(Debug, Default, Serialize)]
pub struct Info {
    pub title: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Canal AsyncAPI — um por tópico físico.
#[derive(Debug, Serialize)]
pub struct Channel {
    pub address: String,
    pub messages: BTreeMap<String, Reference>,
}

/// Operação AsyncAPI — `receive` (consumer) ou `send` (producer).
#[derive(Debug, Serialize)]
pub struct Operation {
    pub action: Action,
    pub channel: Reference,
    pub messages: Vec<Reference>,
}

/// Ação da operação. Em AsyncAPI 3.0 só existem dois valores: `send` e `receive`.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    /// Operação que envia mensagens para o canal (publisher).
    Send,
    /// Operação que recebe mensagens do canal (subscriber).
    Receive,
}

/// Wrapper de `{ "$ref": "..." }` para qualquer ponteiro AsyncAPI/JSON Schema.
#[derive(Debug, Serialize)]
pub struct Reference {
    #[serde(rename = "$ref")]
    pub reference: String,
}

/// Definições reusáveis: mensagens e schemas indexados por nome.
#[derive(Debug, Default, Serialize)]
pub struct Components {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub messages: BTreeMap<String, MessageDef>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub schemas: BTreeMap<String, serde_json::Value>,
}

impl Components {
    fn is_empty(&self) -> bool {
        self.messages.is_empty() && self.schemas.is_empty()
    }
}

/// Definição de uma mensagem no `components.messages`.
#[derive(Debug, Serialize)]
pub struct MessageDef {
    pub name: String,
    pub payload: Reference,
}

impl AsyncApiSpec {
    /// Serializa o spec em YAML AsyncAPI 3.0.
    pub fn to_yaml(&self) -> Result<String, serde_yaml::Error> {
        serde_yaml::to_string(self)
    }
}

/// Builder fluente que acumula channels, operations e schemas.
///
/// O builder é idempotente: registrar a mesma tupla `(topic, action,
/// message)` mais de uma vez não duplica entradas.
#[derive(Debug, Default)]
pub struct AsyncApiBuilder {
    info: Info,
    channels: BTreeMap<String, Channel>,
    operations: BTreeMap<String, Operation>,
    components: Components,
}

impl AsyncApiBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.info.title = title.into();
        self
    }

    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.info.version = version.into();
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.info.description = Some(description.into());
        self
    }

    /// Registra uma operação `receive` para o tipo `T` no `topic`.
    ///
    /// Adiciona o canal, a operação e o JSON Schema de `T` em
    /// `components.schemas` (com `components.messages` referenciando).
    pub fn add_receive<T: JsonSchema>(self, topic: &str) -> Self {
        self.add_operation::<T>(topic, Action::Receive)
    }

    /// Registra uma operação `send` para o tipo `T` no `topic`.
    pub fn add_send<T: JsonSchema>(self, topic: &str) -> Self {
        self.add_operation::<T>(topic, Action::Send)
    }

    fn add_operation<T: JsonSchema>(mut self, topic: &str, action: Action) -> Self {
        let message_name = schema_name::<T>();
        let schema_value = serde_json::to_value(schemars::schema_for!(T))
            .expect("schemars::schema_for produz JSON serializável");

        // 1) Canal — cria sob demanda, adiciona referência da mensagem.
        let channel = self
            .channels
            .entry(topic.to_string())
            .or_insert_with(|| Channel {
                address: topic.to_string(),
                messages: BTreeMap::new(),
            });
        channel
            .messages
            .entry(message_name.clone())
            .or_insert_with(|| Reference {
                reference: format!("#/components/messages/{message_name}"),
            });

        // 2) Operação — id único por (action, topic).
        // Múltiplas mensagens no mesmo canal+ação são adicionadas à lista
        // messages da operação existente em vez de criar uma nova operação.
        let action_str = match action {
            Action::Send => "send",
            Action::Receive => "receive",
        };
        let op_id = format!("{action_str}_{topic}");
        let msg_ref = Reference {
            reference: format!("#/channels/{topic}/messages/{message_name}"),
        };
        let op = self.operations.entry(op_id).or_insert_with(|| Operation {
            action,
            channel: Reference {
                reference: format!("#/channels/{topic}"),
            },
            messages: vec![],
        });
        if !op.messages.iter().any(|r| r.reference == msg_ref.reference) {
            op.messages.push(msg_ref);
        }

        // 3) Componentes — message + schema.
        self.components
            .messages
            .entry(message_name.clone())
            .or_insert_with(|| MessageDef {
                name: message_name.clone(),
                payload: Reference {
                    reference: format!("#/components/schemas/{message_name}"),
                },
            });
        self.components
            .schemas
            .entry(message_name)
            .or_insert(schema_value);

        self
    }

    pub fn build(self) -> AsyncApiSpec {
        AsyncApiSpec {
            asyncapi: "3.0.0".to_string(),
            info: self.info,
            channels: self.channels,
            operations: self.operations,
            components: self.components,
        }
    }
}

/// Flag CLI que o `serverust info --asyncapi` injeta no binário do projeto.
pub const EMIT_FLAG: &str = "--serverust-emit-asyncapi";

/// Erros possíveis ao gravar o spec a partir da CLI.
#[derive(Debug, thiserror::Error)]
pub enum EmitError {
    #[error("flag `{flag}` requer um caminho de saída", flag = EMIT_FLAG)]
    MissingPath,
    #[error("falha ao serializar AsyncAPI YAML: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("falha ao gravar arquivo {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
}

/// Detecta a flag [`EMIT_FLAG`] nos `args` e, se presente, grava o spec do
/// `builder` em YAML no caminho indicado e retorna `Ok(true)`. Se ausente,
/// devolve `Ok(false)` sem efeitos colaterais.
///
/// Use no `main` do projeto antes de qualquer setup pesado de runtime para
/// suportar `serverust info --asyncapi` sem subir handlers.
pub fn emit_asyncapi_if_requested<I, S>(
    builder: AsyncApiBuilder,
    args: I,
) -> Result<bool, EmitError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        if arg.as_ref() == EMIT_FLAG {
            let out = iter.next().ok_or(EmitError::MissingPath)?;
            let path = out.as_ref();
            write_spec_to(builder, Path::new(path))?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn write_spec_to(builder: AsyncApiBuilder, path: &Path) -> Result<(), EmitError> {
    let yaml = builder.build().to_yaml()?;
    std::fs::write(path, yaml).map_err(|source| EmitError::Io {
        path: path.display().to_string(),
        source,
    })
}

/// Nome do schema preferindo `JsonSchema::schema_name()` (geralmente o ident do tipo)
/// com fallback para o último segmento de `type_name()`.
fn schema_name<T: JsonSchema>() -> String {
    let name = T::schema_name();
    if name.is_empty() {
        std::any::type_name::<T>()
            .rsplit("::")
            .next()
            .unwrap_or("Message")
            .to_string()
    } else {
        name
    }
}
