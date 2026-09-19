//! `#[public]` e `#[authorize]` na mesma rota se contradizem (ADR 0009,
//! Emenda 1). O `#[public]` não abriria nada — `#[authorize]` nega sem
//! identidade de qualquer forma — e ainda assim faria a rota aparecer na
//! auditoria de endpoints anônimos. Precisa não compilar.

use serverust_macros::{authorize, get, public};

#[public]
#[authorize(scope = "orders:read")]
#[get("/pedidos")]
async fn pedidos() -> &'static str {
    "pedidos"
}

fn main() {}
