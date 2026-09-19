//! A mesma contradição na outra ordem: `#[authorize]` abaixo da macro de
//! rota. Quem detecta aqui é a macro de rota, não a `#[authorize]`.

use serverust_macros::{authorize, get, public};

#[public]
#[get("/pedidos")]
#[authorize(scope = "orders:read")]
async fn pedidos() -> &'static str {
    "pedidos"
}

fn main() {}
