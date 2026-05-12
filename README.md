# serverust Framework

Framework Rust opinativo para APIs HTTP e AWS Lambda, inspirado em Axum + FastAPI + NestJS.

📚 **Documentação completa**: [`docs/INDEX.md`](docs/INDEX.md) — PRD, arquitetura, diagramas, decisões e progresso.

## Features

- Roteamento declarativo via macros (`#[get]`, `#[post]`, `#[put]`, `#[patch]`, `#[delete]`)
- Validação automática de payloads com `#[derive(Validate)]` → HTTP 422 padronizado
- OpenAPI 3.1 + Swagger UI + ReDoc automáticos via utoipa
- Dependency Injection híbrido (`Arc<dyn Trait>` + builder)
- Guards, Pipes e Interceptors para cross-cutting concerns
- Runtime dual: detecta automaticamente HTTP local vs AWS Lambda
- Telemetria nativa: logs JSON, tracing X-Ray, métricas EMF
- CLI: `serverust new/generate/dev/build/deploy/info/openapi`
- Configuração via `serverust.toml` + env vars (figment)

## Requisitos

- Rust 1.85+ (Edition 2024)
- Para deploy Lambda: [`cargo-lambda`](https://www.cargo-lambda.info/)

## Estrutura do Workspace

```
serverust-core/       # App builder, Route, validação, DI, pipeline
serverust-macros/     # Proc-macros: #[get], #[post], #[injectable], etc.
serverust-cli/        # CLI serverust com clap
serverust-lambda/     # Runtime dual Lambda + HTTP via AppRuntime trait
serverust-telemetry/  # Logger JSON, tracing, métricas EMF
examples/
  hello-world/      # Mínimo para benchmark de cold start
  funds-api/        # CRUD completo: validação, OpenAPI, DI
scripts/
  bench.sh          # Benchmark de tamanho e cold start
```

## Início Rápido

### Criar um novo projeto

```bash
cargo install serverust-cli
serverust new my-api
cd my-api
serverust dev  # sobe com hot-reload via cargo-watch
```

### Exemplo mínimo

```rust
use serverust_core::App;
use serverust_lambda::AppRuntime;
use serverust_macros::get;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, World!"
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    App::new().route(hello).run().await
}
```

### CRUD com validação e DI

```rust
use std::sync::Arc;
use serverust_core::{App, extract::Json};
use serverust_lambda::AppRuntime;
use serverust_macros::{get, post, injectable};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct User { pub id: u64, pub name: String }

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateUserDto {
    #[validate(length(min = 1))]
    #[schema(min_length = 1)]
    pub name: String,
}

#[injectable]
pub struct UserService;

#[get("/users")]
async fn list_users(axum::extract::State(svc): axum::extract::State<Arc<UserService>>) -> axum::Json<Vec<User>> {
    axum::Json(vec![])
}

#[post("/users")]
async fn create_user(
    axum::extract::State(svc): axum::extract::State<Arc<UserService>>,
    Json(dto): Json<CreateUserDto>,
) -> impl axum::response::IntoResponse {
    (axum::http::StatusCode::CREATED, axum::Json(User { id: 1, name: dto.name }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    App::new()
        .openapi_info("Users API", "1.0.0")
        .provide::<UserService>(Arc::new(UserService))
        .route(list_users)
        .route(create_user)
        .run()
        .await
}
```

## Configuração (serverust.toml)

Usa [figment](https://docs.rs/figment) com suporte a profiles e override por env vars:

```toml
[default.server]
host = "127.0.0.1"
port = 3000

[default.lambda]
memory_size = 128
timeout_seconds = 30

[default.telemetry]
log_level = "info"
format = "json"

[default.openapi]
title = "My API"
version = "0.1.0"
docs_path = "/docs"
redoc_path = "/redoc"

# Overrides por perfil:
[dev.server]
port = 3001

[prod.server]
host = "0.0.0.0"
port = 8080
```

Seleção de perfil: `SERVERUST_PROFILE=prod` ou `ServerustConfig::load_for_profile("prod")`.

Override por env: `SERVERUST_SERVER__PORT=8080`, `SERVERUST_TELEMETRY__LOG_LEVEL=debug`.

Carregar no handler:
```rust
use serverust_core::ServerustConfig;
let cfg = ServerustConfig::load().unwrap_or_default();
App::new().config(cfg).route(handler)
// handler: State<Arc<ServerustConfig>>
```

## Exemplos

### Rodar funds-api localmente

```bash
cd examples/funds-api
cargo run
# Acesse: http://localhost:3000/docs (Swagger UI)
# Acesse: http://localhost:3000/openapi.json
```

### Rodar hello-world localmente

```bash
cd examples/hello-world
cargo run
# GET http://localhost:3000/
```

## Deploy em Lambda

```bash
# Instalar cargo-lambda
cargo install cargo-lambda

# Deploy do funds-api (ARM64)
serverust deploy lambda --arch arm64 -p funds-api

# Ou manualmente:
cargo lambda build --release --arm64 -p funds-api
cargo lambda deploy funds-api --memory-size 128
```

## Benchmark

```bash
# Tamanho do binário stripped e startup local:
./scripts/bench.sh

# Incluir benchmark de cold start em Lambda (requer AWS CLI):
LAMBDA_FUNCTION_NAME=serverust-hello-world-bench ./scripts/bench.sh --lambda
```

Alvos de performance:
- Cold start Lambda ARM64 128MB: **< 50ms**
- Binário stripped: **< 10 MB**

## Rodar Testes

```bash
cargo test --workspace
```

## CLI

```bash
serverust new <name>                          # novo projeto
serverust generate resource <name>           # CRUD completo
serverust generate module|controller|service|pipe|guard|interceptor|filter <name>
serverust dev                                # hot-reload local
serverust build [--release]                  # cargo build
serverust deploy lambda [--arch arm64|x86_64]
serverust info                               # versões e features
serverust openapi --out openapi.json         # exportar spec
```
