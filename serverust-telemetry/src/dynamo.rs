//! Repositório tipado e metadados para tabelas DynamoDB.
//!
//! # Visão geral
//!
//! - [`DynamoTable`] — trait *zero-deps* (apenas `serde` + `serde_json`)
//!   declarando o nome da tabela, o campo da partition key e, opcionalmente,
//!   o sort key. É implementada pela macro `#[dynamo_table(...)]` de
//!   `serverust-macros`.
//! - [`DynamoRepo<T>`] — repository pattern para CRUD básico, atrás da feature
//!   `dynamodb`. Reaproveita o `aws_sdk_dynamodb::Client` já carregado para
//!   `DynamoDbIdempotencyStore`.
//!
//! # Exemplo (com macro)
//!
//! ```ignore
//! use serverust_macros::dynamo_table;
//! use serverust_telemetry::dynamo::DynamoRepo;
//!
//! #[dynamo_table("Wallets", pk = "user_id")]
//! #[derive(serde::Serialize, serde::Deserialize)]
//! struct Wallet { user_id: String, balance: u64 }
//!
//! # async fn demo(client: aws_sdk_dynamodb::Client) -> anyhow::Result<()> {
//! let repo: DynamoRepo<Wallet> = DynamoRepo::new(client);
//! let _ = repo.get("u-1").await?;                // GetItem
//! repo.put(&Wallet { user_id: "u-1".into(), balance: 10 }).await?;
//! repo.delete("u-1").await?;
//! # Ok(()) }
//! ```

use serde::Serialize;
use serde::de::DeserializeOwned;

/// Metadados estruturais de uma tabela DynamoDB. Implementado pela macro
/// `#[dynamo_table(name, pk, sk?)]`.
///
/// As implementações default usam `serde_json` para extrair os valores dos
/// campos por nome — qualquer struct `Serialize + DeserializeOwned` funciona,
/// independentemente da ordem ou tipos dos campos restantes.
pub trait DynamoTable: Serialize + DeserializeOwned + Sized {
    /// Nome da tabela DynamoDB.
    const TABLE_NAME: &'static str;
    /// Nome do campo da partition key (deve existir em `Self` e ser serializável).
    const PK_FIELD: &'static str;
    /// Nome opcional do sort key.
    const SK_FIELD: ::core::option::Option<&'static str> = None;

    /// Lê o valor da partition key a partir da instância via `serde_json`.
    /// Retorna `Value::Null` se o campo não estiver presente — caso só
    /// possível se a macro for desalinhada com o schema da struct.
    fn pk_value(&self) -> serde_json::Value {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.get(Self::PK_FIELD).cloned())
            .unwrap_or(serde_json::Value::Null)
    }

    /// Lê o valor do sort key quando configurado.
    fn sk_value(&self) -> Option<serde_json::Value> {
        let sk = Self::SK_FIELD?;
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.get(sk).cloned())
    }
}

#[cfg(feature = "dynamodb")]
mod repo {
    use super::DynamoTable;
    use aws_sdk_dynamodb::Client;
    use aws_sdk_dynamodb::primitives::Blob;
    use aws_sdk_dynamodb::types::AttributeValue;
    use std::collections::HashMap;
    use std::marker::PhantomData;

    /// Erros do `DynamoRepo`.
    #[derive(Debug, thiserror::Error)]
    pub enum RepoError {
        #[error("dynamodb error: {0}")]
        Dynamo(String),
        #[error("serde error: {0}")]
        Serde(#[from] serde_json::Error),
        #[error("unsupported attribute conversion: {0}")]
        Conversion(String),
    }

    impl<E, R> From<aws_sdk_dynamodb::error::SdkError<E, R>> for RepoError
    where
        E: std::fmt::Debug,
        R: std::fmt::Debug,
    {
        fn from(value: aws_sdk_dynamodb::error::SdkError<E, R>) -> Self {
            RepoError::Dynamo(format!("{value:?}"))
        }
    }

    /// Repositório CRUD tipado.
    pub struct DynamoRepo<T> {
        client: Client,
        _marker: PhantomData<T>,
    }

    impl<T> DynamoRepo<T>
    where
        T: DynamoTable,
    {
        /// Constrói um repo reaproveitando um `Client` já inicializado
        /// (mesma instância usada por `DynamoDbIdempotencyStore`).
        pub fn new(client: Client) -> Self {
            Self {
                client,
                _marker: PhantomData,
            }
        }

        /// `GetItem` por partition key. Para tabelas com sort key configurado,
        /// veja [`get_with_sk`](Self::get_with_sk).
        pub async fn get(&self, pk: impl Into<serde_json::Value>) -> Result<Option<T>, RepoError> {
            let mut key = HashMap::new();
            key.insert(T::PK_FIELD.to_string(), json_to_attr(&pk.into()));
            let out = self
                .client
                .get_item()
                .table_name(T::TABLE_NAME)
                .set_key(Some(key))
                .send()
                .await?;
            match out.item {
                Some(item) => Ok(Some(from_item(item)?)),
                None => Ok(None),
            }
        }

        /// `GetItem` por (pk, sk). Útil quando a tabela tem sort key.
        pub async fn get_with_sk(
            &self,
            pk: impl Into<serde_json::Value>,
            sk: impl Into<serde_json::Value>,
        ) -> Result<Option<T>, RepoError> {
            let sk_field = T::SK_FIELD.ok_or_else(|| {
                RepoError::Conversion(format!("tabela {} não tem sort key", T::TABLE_NAME))
            })?;
            let mut key = HashMap::new();
            key.insert(T::PK_FIELD.to_string(), json_to_attr(&pk.into()));
            key.insert(sk_field.to_string(), json_to_attr(&sk.into()));
            let out = self
                .client
                .get_item()
                .table_name(T::TABLE_NAME)
                .set_key(Some(key))
                .send()
                .await?;
            match out.item {
                Some(item) => Ok(Some(from_item(item)?)),
                None => Ok(None),
            }
        }

        /// `PutItem` da struct inteira (serializa todos os campos via serde).
        pub async fn put(&self, item: &T) -> Result<(), RepoError> {
            let dynamo_item = to_item(item)?;
            self.client
                .put_item()
                .table_name(T::TABLE_NAME)
                .set_item(Some(dynamo_item))
                .send()
                .await?;
            Ok(())
        }

        /// `DeleteItem` por partition key. Falha em runtime se a tabela tem
        /// sort key — use [`delete_with_sk`](Self::delete_with_sk) nesse caso.
        pub async fn delete(&self, pk: impl Into<serde_json::Value>) -> Result<(), RepoError> {
            if T::SK_FIELD.is_some() {
                return Err(RepoError::Conversion(format!(
                    "tabela {} tem sort key; use delete_with_sk",
                    T::TABLE_NAME
                )));
            }
            let mut key = HashMap::new();
            key.insert(T::PK_FIELD.to_string(), json_to_attr(&pk.into()));
            self.client
                .delete_item()
                .table_name(T::TABLE_NAME)
                .set_key(Some(key))
                .send()
                .await?;
            Ok(())
        }

        /// `DeleteItem` por (pk, sk).
        pub async fn delete_with_sk(
            &self,
            pk: impl Into<serde_json::Value>,
            sk: impl Into<serde_json::Value>,
        ) -> Result<(), RepoError> {
            let sk_field = T::SK_FIELD.ok_or_else(|| {
                RepoError::Conversion(format!("tabela {} não tem sort key", T::TABLE_NAME))
            })?;
            let mut key = HashMap::new();
            key.insert(T::PK_FIELD.to_string(), json_to_attr(&pk.into()));
            key.insert(sk_field.to_string(), json_to_attr(&sk.into()));
            self.client
                .delete_item()
                .table_name(T::TABLE_NAME)
                .set_key(Some(key))
                .send()
                .await?;
            Ok(())
        }

        /// `Query` por partition key, retornando todos os itens. Segue
        /// `LastEvaluatedKey` até exaurir todas as páginas — DynamoDB devolve
        /// 1 MB por página, então datasets grandes geram múltiplas chamadas.
        pub async fn query_by_pk(
            &self,
            pk: impl Into<serde_json::Value>,
        ) -> Result<Vec<T>, RepoError> {
            let pk_attr = json_to_attr(&pk.into());
            let mut expr_values = HashMap::new();
            expr_values.insert(":pk".to_string(), pk_attr);
            let mut expr_names = HashMap::new();
            expr_names.insert("#pk".to_string(), T::PK_FIELD.to_string());

            let mut all: Vec<T> = Vec::new();
            let mut exclusive_start: Option<HashMap<String, AttributeValue>> = None;
            loop {
                let out = self
                    .client
                    .query()
                    .table_name(T::TABLE_NAME)
                    .key_condition_expression("#pk = :pk")
                    .set_expression_attribute_names(Some(expr_names.clone()))
                    .set_expression_attribute_values(Some(expr_values.clone()))
                    .set_exclusive_start_key(exclusive_start.clone())
                    .send()
                    .await?;
                let items = out.items.unwrap_or_default();
                for item in items {
                    all.push(from_item(item)?);
                }
                match out.last_evaluated_key {
                    Some(k) if !k.is_empty() => exclusive_start = Some(k),
                    _ => break,
                }
            }
            Ok(all)
        }
    }

    /// Converte `serde_json::Value` em `AttributeValue` cobrindo os tipos
    /// usuais: string (S), número (N), bool (BOOL), null (NULL), array (L),
    /// objeto (M).
    pub fn json_to_attr(value: &serde_json::Value) -> AttributeValue {
        match value {
            serde_json::Value::Null => AttributeValue::Null(true),
            serde_json::Value::Bool(b) => AttributeValue::Bool(*b),
            serde_json::Value::Number(n) => AttributeValue::N(n.to_string()),
            serde_json::Value::String(s) => AttributeValue::S(s.clone()),
            serde_json::Value::Array(arr) => {
                AttributeValue::L(arr.iter().map(json_to_attr).collect())
            }
            serde_json::Value::Object(map) => {
                let mut out = HashMap::with_capacity(map.len());
                for (k, v) in map {
                    out.insert(k.clone(), json_to_attr(v));
                }
                AttributeValue::M(out)
            }
        }
    }

    /// Converte `AttributeValue` de volta para `serde_json::Value`.
    /// Tipos exóticos do DynamoDB (SS, NS, BS, B) viram representações
    /// "best-effort" (string array, número-string, base64 string).
    pub fn attr_to_json(attr: &AttributeValue) -> serde_json::Value {
        match attr {
            AttributeValue::Null(_) => serde_json::Value::Null,
            AttributeValue::Bool(b) => serde_json::Value::Bool(*b),
            AttributeValue::S(s) => serde_json::Value::String(s.clone()),
            AttributeValue::N(n) => {
                if let Ok(i) = n.parse::<i64>() {
                    serde_json::Value::Number(i.into())
                } else if let Ok(u) = n.parse::<u64>() {
                    serde_json::Value::Number(u.into())
                } else if let Ok(f) = n.parse::<f64>() {
                    serde_json::Number::from_f64(f)
                        .map(serde_json::Value::Number)
                        .unwrap_or(serde_json::Value::Null)
                } else {
                    serde_json::Value::String(n.clone())
                }
            }
            AttributeValue::L(list) => {
                serde_json::Value::Array(list.iter().map(attr_to_json).collect())
            }
            AttributeValue::M(map) => {
                let mut out = serde_json::Map::with_capacity(map.len());
                for (k, v) in map {
                    out.insert(k.clone(), attr_to_json(v));
                }
                serde_json::Value::Object(out)
            }
            AttributeValue::Ss(items) => serde_json::Value::Array(
                items
                    .iter()
                    .map(|s| serde_json::Value::String(s.clone()))
                    .collect(),
            ),
            AttributeValue::Ns(items) => serde_json::Value::Array(
                items
                    .iter()
                    .map(|n| serde_json::Value::String(n.clone()))
                    .collect(),
            ),
            AttributeValue::B(Blob { .. }) => {
                let blob: &Blob = match attr {
                    AttributeValue::B(b) => b,
                    _ => unreachable!(),
                };
                serde_json::Value::String(base64_encode(blob.as_ref()))
            }
            AttributeValue::Bs(items) => serde_json::Value::Array(
                items
                    .iter()
                    .map(|b| serde_json::Value::String(base64_encode(b.as_ref())))
                    .collect(),
            ),
            _ => serde_json::Value::Null,
        }
    }

    /// Converte um item DynamoDB inteiro para `serde_json::Value`.
    pub fn attr_map_to_json(item: &HashMap<String, AttributeValue>) -> serde_json::Value {
        let mut out = serde_json::Map::with_capacity(item.len());
        for (k, v) in item {
            out.insert(k.clone(), attr_to_json(v));
        }
        serde_json::Value::Object(out)
    }

    /// Serializa qualquer `T: Serialize` em um item DynamoDB.
    pub fn to_item<T: serde::Serialize>(
        value: &T,
    ) -> Result<HashMap<String, AttributeValue>, RepoError> {
        let json = serde_json::to_value(value)?;
        match json {
            serde_json::Value::Object(map) => Ok(map
                .into_iter()
                .map(|(k, v)| (k, json_to_attr(&v)))
                .collect()),
            other => Err(RepoError::Conversion(format!(
                "esperado objeto, recebi {other:?}"
            ))),
        }
    }

    /// Deserializa um item DynamoDB em `T: DeserializeOwned`.
    pub fn from_item<T: serde::de::DeserializeOwned>(
        item: HashMap<String, AttributeValue>,
    ) -> Result<T, RepoError> {
        let mut out = serde_json::Map::with_capacity(item.len());
        for (k, v) in item.iter() {
            out.insert(k.clone(), attr_to_json(v));
        }
        let json = serde_json::Value::Object(out);
        Ok(serde_json::from_value(json)?)
    }

    fn base64_encode(bytes: &[u8]) -> String {
        // base64 sem dep adicional: tabela mínima inline para Blob.
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        let mut i = 0;
        while i + 3 <= bytes.len() {
            let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | bytes[i + 2] as u32;
            out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
            out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
            out.push(TABLE[(n & 0x3f) as usize] as char);
            i += 3;
        }
        let rem = bytes.len() - i;
        if rem == 1 {
            let n = (bytes[i] as u32) << 16;
            out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
            out.push('=');
            out.push('=');
        } else if rem == 2 {
            let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
            out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
            out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
            out.push('=');
        }
        out
    }
}

#[cfg(feature = "dynamodb")]
pub use repo::{
    DynamoRepo, RepoError, attr_map_to_json, attr_to_json, from_item, json_to_attr, to_item,
};
