//! `#[authorize]` sem exigência nenhuma é decoração — e decoração com cara de
//! controle de acesso é pior do que nenhum controle.

use serverust_macros::{authorize, get};

#[authorize]
#[get("/pedidos")]
async fn pedidos() -> &'static str {
    "pedidos"
}

fn main() {}
