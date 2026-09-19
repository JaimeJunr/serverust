//! `#[public]` abaixo da macro de rota não tem efeito — precisa não compilar.
//!
//! Se isto passasse a compilar, uma rota anotada como pública continuaria sob
//! o default deny sem nenhum aviso: a anotação estaria lá, o `grep` a
//! encontraria, e o comportamento seria o oposto do declarado.

use serverust_macros::{get, public};

#[get("/health")]
#[public]
async fn health() -> &'static str {
    "ok"
}

fn main() {}
