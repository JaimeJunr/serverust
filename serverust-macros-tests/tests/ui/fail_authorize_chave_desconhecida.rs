//! Chave inexistente em `#[authorize]` precisa falhar: escrita errada que
//! compila é permissão que ninguém está checando.

use serverust_macros::{authorize, get};

#[authorize(permission = "orders:read")]
#[get("/pedidos")]
async fn pedidos() -> &'static str {
    "pedidos"
}

fn main() {}
