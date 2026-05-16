//! Trybuild: a macro #[dynamo_table] aplica-se a uma struct e injeta
//! `impl serverust_telemetry::dynamo::DynamoTable` com as constantes.

use serde::{Deserialize, Serialize};
use serverust_macros::dynamo_table;
use serverust_telemetry::dynamo::DynamoTable;

#[dynamo_table("Wallets", pk = "user_id")]
#[derive(Debug, Serialize, Deserialize)]
struct Wallet {
    user_id: String,
    balance: u64,
}

#[dynamo_table("Orders", pk = "customer_id", sk = "order_id")]
#[derive(Debug, Serialize, Deserialize)]
struct Order {
    customer_id: String,
    order_id: String,
    total: u64,
}

fn main() {
    assert_eq!(Wallet::TABLE_NAME, "Wallets");
    assert_eq!(Wallet::PK_FIELD, "user_id");
    assert_eq!(Wallet::SK_FIELD, None);

    assert_eq!(Order::TABLE_NAME, "Orders");
    assert_eq!(Order::PK_FIELD, "customer_id");
    assert_eq!(Order::SK_FIELD, Some("order_id"));
}
