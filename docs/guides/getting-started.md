# Getting Started

Em 5 minutos você sai do zero a uma API HTTP do serverust rodando local com Swagger UI funcional.

> Se o que você quer é levar isso para AWS Lambda, faça este guia primeiro e depois siga o [Tutorial Lambda](./lambda-tutorial.md).

## 1. Pré-requisitos

- **Rust 1.85+** (Edition 2024). Instale ou atualize com [rustup](https://rustup.rs):
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  rustup update stable
  rustc --version    # confirme >= 1.85
  ```
- **(Para `serverust dev` com hot-reload)** [`cargo-watch`](https://github.com/watchexec/cargo-watch):
  ```bash
  cargo install cargo-watch
  ```
- **(Opcional para Lambda)** [`cargo-lambda`](https://www.cargo-lambda.info/guide/installation.html):
  ```bash
  cargo install cargo-lambda
  ```

Você **não precisa** de Docker, Node.js, Python ou conta AWS para começar.

## 2. Crie um projeto novo

```bash
cargo new --bin hello-serverust
cd hello-serverust
```

Edite `Cargo.toml`:

```toml
[package]
name = "hello-serverust"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]
serverust-core   = "0.1"
serverust-macros = "0.1"
serverust-lambda = "0.1"

tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
serde = { version = "1", features = ["derive"] }
```

## 3. Escreva o primeiro handler

`src/main.rs`:

```rust
use serverust_core::App;
use serverust_lambda::AppRuntime;
use serverust_macros::get;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, serverust!"
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    App::new().route(hello).run().await?;
    Ok(())
}
```

Três peças aparecem aqui — entenda cada uma porque você vai usar em todo handler daqui pra frente:

| Item | O que faz |
|---|---|
| `#[get("/")]` | Macro de roteamento. Transforma a função `hello` em um `IntoRoute` que o `App` registra. Variantes: `#[post]`, `#[put]`, `#[patch]`, `#[delete]`. |
| `App::new().route(hello)` | Builder do framework. Registra a rota gerada pela macro. |
| `.run().await` | Detecta o ambiente: em local, sobe HTTP em `0.0.0.0:3000`. Em Lambda (`AWS_LAMBDA_RUNTIME_API` presente), consome eventos do API Gateway. Vem da trait `AppRuntime`. |

## 4. Rode

```bash
cargo run
```

Em outro terminal:

```bash
curl http://localhost:3000/
# → Hello, serverust!
```

E abra no navegador:

- <http://localhost:3000/docs> — Swagger UI (gerado automaticamente)
- <http://localhost:3000/redoc> — ReDoc
- <http://localhost:3000/openapi.json> — spec OpenAPI 3.1

A rota `/` aparece sem você ter escrito uma linha de spec.

## 5. Adicione validação automática

Troque `src/main.rs` por:

```rust
use serverust_core::{App, extract::Json};
use serverust_lambda::AppRuntime;
use serverust_macros::{get, post};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

#[derive(Debug, Deserialize, Validate, ToSchema)]
struct GreetRequest {
    #[validate(length(min = 1, max = 50))]
    #[schema(min_length = 1, max_length = 50)]
    name: String,
}

#[derive(Debug, Serialize, ToSchema)]
struct GreetResponse {
    message: String,
}

#[get("/")]
async fn hello() -> &'static str {
    "Hello, serverust!"
}

#[post("/greet")]
async fn greet(Json(req): Json<GreetRequest>) -> Json<GreetResponse> {
    Json(GreetResponse {
        message: format!("Hello, {}!", req.name),
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    App::new()
        .openapi_info("Hello API", "0.1.0")
        .register_schema::<GreetRequest>()
        .register_schema::<GreetResponse>()
        .route(hello)
        .route(greet)
        .run()
        .await?;
    Ok(())
}
```

Adicione no `Cargo.toml`:

```toml
serde     = { version = "1", features = ["derive"] }
serde_json = "1"
validator = { version = "0.20", features = ["derive"] }
utoipa    = { version = "5", features = ["macros"] }
```

`cargo run` de novo e teste:

```bash
# OK
curl -X POST http://localhost:3000/greet \
     -H 'content-type: application/json' \
     -d '{"name":"Jaime"}'
# → {"message":"Hello, Jaime!"}

# Falha de validação → HTTP 422 com JSON estruturado, sem você escrever validação:
curl -i -X POST http://localhost:3000/greet \
     -H 'content-type: application/json' \
     -d '{"name":""}'
# HTTP/1.1 422 Unprocessable Entity
# {"error":"validation_error","fields":{"name":["length"]}}
```

Os campos `#[schema(min_length = ...)]` espelham as regras de `#[validate(...)]` — o utoipa **não** lê os atributos do validator automaticamente. É um pouco de duplicação, mas garante que o Swagger mostre os constraints corretos.

## 6. Próximos passos

- **Quer levar isso a AWS Lambda?** Veja o [Tutorial Lambda](./lambda-tutorial.md). O mesmo binário roda local e em Lambda, sem mudar uma linha do código de negócio.
- **Quer arquitetura real (DI, errors, módulos)?** Estude o exemplo [`examples/todo-api`](../../examples/todo-api/) — é uma CRUD completa que o tutorial constrói passo a passo.
- **Quer referência completa de API?** Rode `cargo doc --workspace --no-deps --open`.
- **Quer entender as escolhas arquiteturais?** Leia o [Decision Log](../development/decisions.md).
