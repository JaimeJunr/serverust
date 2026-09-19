//! `#[public]` numa função que não é rota não significa nada.

use serverust_macros::public;

#[public]
async fn nao_e_rota() -> &'static str {
    "ok"
}

fn main() {}
