//! Terceira ordem possível da contradição: `#[authorize]` acima de
//! `#[public]`, com a macro de rota embaixo.
//!
//! Esta ordem chegou a compilar. A rota não ficava insegura — `#[authorize]`
//! nega sem identidade de qualquer forma — mas entrava no `grep '#[public]'`
//! e no inventário de rotas públicas do log de init sem ser uma delas, que é
//! justamente a corrupção da auditoria que esta checagem existe para impedir.

use serverust_macros::{authorize, get, public};

#[authorize(scope = "orders:read")]
#[public]
#[get("/pedidos")]
async fn pedidos() -> &'static str {
    "pedidos"
}

fn main() {}
