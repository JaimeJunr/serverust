//! Testes do módulo `dynamo`: trait `DynamoTable` (sempre disponível) e
//! conversores `json↔AttributeValue` (atrás da feature `dynamodb`).
//!
//! A integração ponta-a-ponta com DynamoDB real não roda aqui — exigiria
//! LocalStack/network. O contrato testado é:
//! - macro `#[dynamo_table]` emite constantes corretas;
//! - conversores JSON → AttributeValue → JSON fazem roundtrip sem perda.

use serde::{Deserialize, Serialize};
use serverust_macros::dynamo_table;
use serverust_telemetry::dynamo::DynamoTable;

#[dynamo_table("Wallets", pk = "user_id")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Wallet {
    user_id: String,
    balance: u64,
}

#[dynamo_table("Orders", pk = "customer_id", sk = "order_id")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Order {
    customer_id: String,
    order_id: String,
    total: u64,
}

#[test]
fn macro_expoe_constantes_de_tabela_sem_sk() {
    assert_eq!(Wallet::TABLE_NAME, "Wallets");
    assert_eq!(Wallet::PK_FIELD, "user_id");
    assert_eq!(Wallet::SK_FIELD, None);
}

#[test]
fn macro_expoe_constantes_de_tabela_com_sk() {
    assert_eq!(Order::TABLE_NAME, "Orders");
    assert_eq!(Order::PK_FIELD, "customer_id");
    assert_eq!(Order::SK_FIELD, Some("order_id"));
}

#[test]
fn pk_value_extrai_campo_pela_serializacao() {
    let w = Wallet {
        user_id: "u-1".into(),
        balance: 42,
    };
    assert_eq!(w.pk_value(), serde_json::json!("u-1"));
    assert_eq!(w.sk_value(), None);
}

#[test]
fn sk_value_quando_tabela_e_composite() {
    let o = Order {
        customer_id: "c-1".into(),
        order_id: "o-9".into(),
        total: 100,
    };
    assert_eq!(o.pk_value(), serde_json::json!("c-1"));
    assert_eq!(o.sk_value(), Some(serde_json::json!("o-9")));
}

#[cfg(feature = "dynamodb")]
mod with_dynamodb {
    use super::*;
    use aws_sdk_dynamodb::types::AttributeValue;
    use serverust_telemetry::dynamo::{attr_map_to_json, attr_to_json, json_to_attr};
    use std::collections::HashMap;

    #[test]
    fn json_to_attr_roundtrip_primitivos() {
        let cases = vec![
            serde_json::json!("hello"),
            serde_json::json!(42),
            serde_json::json!(1.5),
            serde_json::json!(true),
            serde_json::json!(false),
            serde_json::Value::Null,
        ];
        for value in cases {
            let attr = json_to_attr(&value);
            let roundtrip = attr_to_json(&attr);
            assert_eq!(value, roundtrip, "valor {value:?} sofreu mudança");
        }
    }

    #[test]
    fn json_to_attr_array_e_objeto() {
        let value = serde_json::json!({
            "user_id": "u-1",
            "balance": 42,
            "tags": ["a", "b"],
            "active": true,
        });
        let attr = json_to_attr(&value);
        match &attr {
            AttributeValue::M(map) => {
                assert!(map.contains_key("user_id"));
                assert!(map.contains_key("balance"));
            }
            other => panic!("esperado AttributeValue::M, recebido {other:?}"),
        }
        let back = attr_to_json(&attr);
        assert_eq!(back, value);
    }

    #[test]
    fn to_item_e_from_item_preservam_struct() {
        let w = Wallet {
            user_id: "u-1".into(),
            balance: 42,
        };
        let item = serverust_telemetry::dynamo::to_item(&w).expect("serialize");
        assert!(item.contains_key("user_id"));
        assert!(item.contains_key("balance"));
        let back: Wallet = serverust_telemetry::dynamo::from_item(item).expect("deserialize");
        assert_eq!(back, w);
    }

    #[test]
    fn attr_map_to_json_descarta_valores_null() {
        let mut map = HashMap::new();
        map.insert("a".to_string(), AttributeValue::S("x".into()));
        map.insert("b".to_string(), AttributeValue::Null(true));
        let json = attr_map_to_json(&map);
        assert_eq!(json["a"], serde_json::json!("x"));
        assert!(json["b"].is_null());
    }

    /// Verifica que `DynamoRepo<T>` compõe `new()` + métodos com `T: DynamoTable`.
    /// Não executa I/O — só prova typecheck.
    #[allow(dead_code)]
    fn _typecheck_dyn_repo(client: aws_sdk_dynamodb::Client) {
        let repo: serverust_telemetry::dynamo::DynamoRepo<Wallet> =
            serverust_telemetry::dynamo::DynamoRepo::new(client);
        let _fut = async move {
            let _opt: Option<Wallet> = repo.get("u-1").await.unwrap();
            let _put: Result<(), _> = repo
                .put(&Wallet {
                    user_id: "u-1".into(),
                    balance: 0,
                })
                .await;
            let _del: Result<(), _> = repo.delete("u-1").await;
            let _q: Vec<Wallet> = repo.query_by_pk("u-1").await.unwrap();
        };
    }
}
