//! `#[authorize]` abaixo da macro de rota não compila mais.
//!
//! A verificação em runtime até aconteceria — o `GuardCheck` é injetado de
//! qualquer forma. O que se perde é o `security` do OpenAPI: a rota já foi
//! construída quando a `#[authorize]` roda, então os escopos não chegam ao
//! documento. Documento que descreve como aberta uma rota que exige escopo é
//! pior do que documento nenhum.

use serverust_macros::{authorize, get};

#[get("/pedidos")]
#[authorize(scope = "orders:read")]
async fn pedidos() -> &'static str {
    "pedidos"
}

fn main() {}
