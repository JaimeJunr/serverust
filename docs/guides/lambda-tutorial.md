# Tutorial: Lambda do zero ao deploy com serverust

Este tutorial te leva de **um diretório vazio** até **uma API REST de Tasks rodando em AWS Lambda**, passando pelos pontos que costumam quebrar para quem está começando: validação, DI, OpenAPI, rodar local, empacotar para Lambda, fazer deploy real, e testar o endpoint AWS com `curl`.

> **Tempo estimado**: 30–45 min se nunca usou cargo-lambda; 15 min se já mexeu.
>
> **Pré-requisitos**:
> - Já fez o [Getting Started](./getting-started.md), ou ao menos confirmou que `cargo run` funciona com o framework.
> - `cargo-lambda` instalado: `cargo install cargo-lambda`.
> - Para a parte de deploy: AWS CLI configurada (`aws configure`) e permissões para criar funções Lambda + roles IAM.
>
> **Destino**: o exemplo completo deste tutorial vive em [`examples/todo-api`](../../examples/todo-api/). Você pode copiar dali a qualquer momento se quiser pular adiante.

---

## Visão geral

Vamos construir uma API de tarefas (`Task`) com 5 endpoints:

| Método | Path | O que faz |
|---|---|---|
| `GET`    | `/tasks`      | Lista todas as tarefas |
| `POST`   | `/tasks`      | Cria uma tarefa (com validação) |
| `GET`    | `/tasks/{id}` | Busca por id (404 padronizado) |
| `PUT`    | `/tasks/{id}` | Atualiza |
| `DELETE` | `/tasks/{id}` | Remove |

Vamos seguir esta ordem:

1. **Setup**: criar o projeto Cargo, declarar dependências.
2. **Modelos**: `Task`, `CreateTaskDto`, `UpdateTaskDto`.
3. **Erros**: `TaskError` com `#[derive(ApiError)]`.
4. **Service**: `TaskService` com `#[injectable]`.
5. **Handlers**: 5 funções com `#[get]`/`#[post]`/etc.
6. **Wire**: `App::new()...build()` no `main.rs`.
7. **Rodar local** e testar com `curl`.
8. **Empacotar com cargo-lambda**.
9. **Deploy em AWS**.
10. **Testar o endpoint Lambda real**.

---

## 1. Setup

```bash
cargo new --bin todo-lambda
cd todo-lambda
```

`Cargo.toml`:

```toml
[package]
name = "todo-lambda"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[dependencies]
# Crates do serverust (use `path` enquanto o framework não está publicado;
# troque por `version = "x.y"` quando estiver no crates.io).
serverust-core   = { path = "../serverust-core" }
serverust-macros = { path = "../serverust-macros" }
serverust-lambda = { path = "../serverust-lambda" }

# Runtime e tipos auxiliares
tokio  = { version = "1", features = ["macros", "rt-multi-thread"] }
serde  = { version = "1", features = ["derive"] }
axum   = "0.8"

# Validação + OpenAPI
validator = { version = "0.20", features = ["derive"] }
utoipa    = { version = "5", features = ["macros"] }
```

> Por que precisamos de `axum` se o framework já depende dele?
>
> Porque o exemplo importa diretamente `axum::extract::{Path, State}` e `axum::http::StatusCode`. O serverust re-exporta `Path`, `Query`, `Json` e `State` em `serverust_core::extract`, mas o resto do axum você usa direto. É consciente: o framework **estende** o axum em vez de escondê-lo.

Confirme o build inicial:

```bash
cargo check
```

Se quebrar aqui, é dependência mal escrita ou `path` errado — resolva antes de seguir.

---

## 2. Modelos

Crie `src/model.rs`:

```rust
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub done: bool,
    pub created_at: u64,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateTaskDto {
    #[validate(length(min = 1, max = 200))]
    #[schema(min_length = 1, max_length = 200)]
    pub title: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateTaskDto {
    #[validate(length(min = 1, max = 200))]
    #[schema(min_length = 1, max_length = 200)]
    pub title: Option<String>,
    pub done: Option<bool>,
}
```

**Notas importantes**:

- Os campos `#[schema(...)]` espelham `#[validate(...)]`. O utoipa não lê atributos do validator automaticamente — você precisa duplicar para o OpenAPI mostrar os constraints. É um pouco chato; aceitamos como trade-off para evitar magic.
- Todo DTO de entrada **precisa** derivar `Validate`. Se não houver regras, o derive é no-op. Isso é uma decisão do framework: `serverust_core::extract::Json<T>` exige `T: Validate` para garantir que validação rode antes de chegar no handler.

---

## 3. Erros padronizados

Crie `src/errors.rs`:

```rust
use serverust_macros::ApiError;

#[derive(Debug, ApiError)]
pub enum TaskError {
    #[status(404)]
    #[message("Task não encontrada")]
    NotFound,
}
```

`#[derive(ApiError)]` emite simultaneamente uma `impl ApiError` (lê o status code) e uma `impl IntoResponse` (responde JSON `{"error":"Task não encontrada"}` com HTTP 404).

Resultado prático: você pode usar `?` em handlers `Result<T, TaskError>` e o framework converte para resposta HTTP automaticamente.

> **Adicionando mais variantes**: cada variante leva seu próprio `#[status(N)]` e `#[message("...")]`. Ex:
> ```rust
> #[status(409)] #[message("Title já existe")] DuplicateTitle,
> #[status(403)] #[message("Permissão negada")] Forbidden,
> ```

---

## 4. Service com DI

Crie `src/service.rs`:

```rust
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serverust_macros::injectable;

use crate::model::{CreateTaskDto, Task, UpdateTaskDto};

#[injectable]
pub struct TaskService {
    tasks: Mutex<Vec<Task>>,
    next_id: AtomicU64,
}

impl TaskService {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn list(&self) -> Vec<Task> {
        self.tasks.lock().unwrap().clone()
    }

    pub fn get(&self, id: u64) -> Option<Task> {
        self.tasks.lock().unwrap().iter().find(|t| t.id == id).cloned()
    }

    pub fn create(&self, dto: CreateTaskDto) -> Task {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let task = Task {
            id,
            title: dto.title,
            done: false,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };
        self.tasks.lock().unwrap().push(task.clone());
        task
    }

    pub fn update(&self, id: u64, dto: UpdateTaskDto) -> Option<Task> {
        let mut tasks = self.tasks.lock().unwrap();
        let task = tasks.iter_mut().find(|t| t.id == id)?;
        if let Some(title) = dto.title { task.title = title; }
        if let Some(done)  = dto.done  { task.done  = done; }
        Some(task.clone())
    }

    pub fn delete(&self, id: u64) -> bool {
        let mut tasks = self.tasks.lock().unwrap();
        let before = tasks.len();
        tasks.retain(|t| t.id != id);
        tasks.len() < before
    }
}

impl Default for TaskService {
    fn default() -> Self { Self::new() }
}
```

**Coisas a notar**:

- `#[injectable]` é um marker — não muda o struct em runtime. Ele só sinaliza que esse tipo é uma dependência do framework. Quem registra de fato é `App::provide::<TaskService>(Arc::new(...))` lá no `main`.
- Storage in-memory é só para o tutorial. Em produção, substitua por um repositório que fale com Postgres / Dynamo / etc.
- `Mutex<Vec<_>>` + `AtomicU64` é o suficiente para serializar acesso em Lambda. Para alta concorrência local, troque por `RwLock` ou um `dashmap`.

---

## 5. Handlers

Crie `src/handlers.rs`:

```rust
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serverust_core::extract::Json;
use serverust_macros::{delete, get, post, put};

use crate::errors::TaskError;
use crate::model::{CreateTaskDto, Task, UpdateTaskDto};
use crate::service::TaskService;

#[get("/tasks")]
pub async fn list_tasks(State(svc): State<Arc<TaskService>>) -> Json<Vec<Task>> {
    Json(svc.list())
}

#[post("/tasks")]
pub async fn create_task(
    State(svc): State<Arc<TaskService>>,
    Json(dto): Json<CreateTaskDto>,
) -> impl IntoResponse {
    let task = svc.create(dto);
    (StatusCode::CREATED, Json(task))
}

#[get("/tasks/{id}")]
pub async fn get_task(
    Path(id): Path<u64>,
    State(svc): State<Arc<TaskService>>,
) -> Result<Json<Task>, TaskError> {
    svc.get(id).map(Json).ok_or(TaskError::NotFound)
}

#[put("/tasks/{id}")]
pub async fn update_task(
    Path(id): Path<u64>,
    State(svc): State<Arc<TaskService>>,
    Json(dto): Json<UpdateTaskDto>,
) -> Result<Json<Task>, TaskError> {
    svc.update(id, dto).map(Json).ok_or(TaskError::NotFound)
}

#[delete("/tasks/{id}")]
pub async fn delete_task(
    Path(id): Path<u64>,
    State(svc): State<Arc<TaskService>>,
) -> Result<StatusCode, TaskError> {
    if svc.delete(id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(TaskError::NotFound)
    }
}
```

**Padrões essenciais**:

1. **Ordem dos parâmetros**: extractors que leem só os headers/path/state (como `Path`, `State`) vêm **antes** do `Json<T>`. O `Json` é body-consuming — só pode ser o último. O framework usa essa convenção do axum.
2. **Path params em axum 0.8**: sintaxe é `{id}`, não `:id`.
3. **State injetado**: `State(svc): State<Arc<TaskService>>` é resolvido automaticamente porque o container do serverust tem um blanket `FromRef<Container> for Arc<T>`.
4. **`Result<T, TaskError>`**: handlers que podem falhar retornam Result. O framework converte o `Err` via `IntoResponse` que veio do derive.

---

## 6. Wire (lib.rs + main.rs)

Crie `src/lib.rs` (para permitir testes de integração contra o `App`):

```rust
pub mod errors;
pub mod handlers;
pub mod model;
pub mod service;

use std::sync::Arc;

use serverust_core::App;

use crate::handlers::{create_task, delete_task, get_task, list_tasks, update_task};
use crate::model::{CreateTaskDto, Task, UpdateTaskDto};
use crate::service::TaskService;

pub fn build_app() -> App {
    App::new()
        .openapi_info("Todo API", "0.1.0")
        .register_schema::<Task>()
        .register_schema::<CreateTaskDto>()
        .register_schema::<UpdateTaskDto>()
        .provide::<TaskService>(Arc::new(TaskService::new()))
        .route(list_tasks)
        .route(create_task)
        .route(get_task)
        .route(update_task)
        .route(delete_task)
}
```

E `src/main.rs`:

```rust
use serverust_lambda::AppRuntime;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    todo_lambda::build_app().run().await?;
    Ok(())
}
```

Adicione no `Cargo.toml`:

```toml
[[bin]]
name = "todo-lambda"
path = "src/main.rs"

[lib]
name = "todo_lambda"
path = "src/lib.rs"
```

**O coração disto tudo é a linha `.run().await`**: a trait `AppRuntime` (vinda de `serverust-lambda`) faz `App` ter um `.run()` que:

- Detecta se está em Lambda olhando para `AWS_LAMBDA_RUNTIME_API`.
- Em Lambda, chama `lambda_http::run(router)` (consome eventos de API Gateway REST v1, HTTP v2 e Lambda Function URL).
- Em local, chama `axum::serve` em `0.0.0.0:3000`.

Você **não muda** o código para alternar entre os dois.

Build:

```bash
cargo build
```

---

## 7. Rodar local e testar com curl

```bash
cargo run
```

Em outro terminal:

```bash
# Listar (vazio)
curl http://localhost:3000/tasks
# → []

# Criar
curl -X POST http://localhost:3000/tasks \
     -H 'content-type: application/json' \
     -d '{"title":"escrever doc"}'
# → {"id":1,"title":"escrever doc","done":false,"created_at":1715000000}

# Falha de validação (title vazio) → 422 automático
curl -i -X POST http://localhost:3000/tasks \
     -H 'content-type: application/json' \
     -d '{"title":""}'
# HTTP/1.1 422 Unprocessable Entity
# {"error":"validation_error","fields":{"title":["length"]}}

# 404 padronizado
curl -i http://localhost:3000/tasks/999
# HTTP/1.1 404 Not Found
# {"error":"Task não encontrada"}

# Atualizar (marcar como concluída)
curl -X PUT http://localhost:3000/tasks/1 \
     -H 'content-type: application/json' \
     -d '{"done":true}'

# Remover
curl -i -X DELETE http://localhost:3000/tasks/1
# HTTP/1.1 204 No Content
```

E **veja a documentação OpenAPI gerada automaticamente**:

```bash
open http://localhost:3000/docs       # Swagger UI
open http://localhost:3000/redoc      # ReDoc
curl http://localhost:3000/openapi.json | jq .  # spec OpenAPI 3.1
```

Você não escreveu nenhuma linha de OpenAPI. Os schemas vieram dos `#[derive(ToSchema)]` + atributos `#[schema(...)]`, e os paths vieram das macros de rota.

---

## 8. Empacotar para Lambda com cargo-lambda

```bash
# ARM64 (Graviton) — recomendado: ~20% mais barato, cold start similar
cargo lambda build --release --arm64

# OU x86_64:
cargo lambda build --release
```

O binário fica em `target/lambda/todo-lambda/bootstrap`. Confira tamanho:

```bash
ls -lh target/lambda/todo-lambda/bootstrap
# Deve estar na casa de 3–8 MB. Stripped no release profile.
```

Para validar localmente (cargo-lambda emula o runtime):

```bash
# Terminal 1
cargo lambda watch

# Terminal 2 — manda evento simulando API Gateway
cargo lambda invoke --data-ascii '{
  "version":"2.0",
  "routeKey":"GET /tasks",
  "rawPath":"/tasks",
  "rawQueryString":"",
  "headers":{},
  "requestContext":{"http":{"method":"GET","path":"/tasks","sourceIp":"127.0.0.1"}},
  "isBase64Encoded":false
}'
```

---

## 9. Deploy em AWS

> Você precisa de credenciais AWS válidas em `~/.aws/credentials` ou nas env vars. `aws sts get-caller-identity` deve responder OK antes de seguir.

```bash
# Deploy direto: cria a função se não existir, atualiza se existir
cargo lambda deploy todo-lambda \
    --memory-size 128 \
    --timeout 10
```

O cargo-lambda cria:
- A função Lambda (`todo-lambda`).
- Um role IAM básico (apenas escrever em CloudWatch Logs).

Para acessar via HTTP, você precisa **adicionar um trigger**. Modo mais simples — **Function URL**:

```bash
aws lambda create-function-url-config \
    --function-name todo-lambda \
    --auth-type NONE

aws lambda add-permission \
    --function-name todo-lambda \
    --statement-id FunctionURLAllowPublicAccess \
    --action lambda:InvokeFunctionUrl \
    --principal '*' \
    --function-url-auth-type NONE

# Pega a URL gerada
aws lambda get-function-url-config \
    --function-name todo-lambda \
    --query FunctionUrl --output text
# → https://xxxxx.lambda-url.us-east-1.on.aws/
```

> **Alternativa**: se preferir API Gateway, use `aws apigatewayv2 create-api ... --target arn:aws:lambda:...`. O serverust suporta API Gateway REST v1, HTTP v2 **e** Function URL — o `lambda_http` faz o roteamento sozinho.

---

## 10. Testar o endpoint AWS real

Substitua `${URL}` pela Function URL retornada acima:

```bash
URL=https://xxxxx.lambda-url.us-east-1.on.aws

# Hello
curl ${URL}/tasks
# → []

# Criar (em Lambda agora)
curl -X POST ${URL}/tasks \
     -H 'content-type: application/json' \
     -d '{"title":"vai pra produção"}'

# Validação ainda funciona
curl -i -X POST ${URL}/tasks \
     -H 'content-type: application/json' \
     -d '{"title":""}'
# 422

# Swagger UI também!
open ${URL}/docs
```

**Observações importantes sobre o estado em Lambda**:

- `TaskService` está em memória do execution environment. Cada cold start começa zerado.
- Lambda mantém o ambiente quente por alguns minutos entre invocações — então duas chamadas seguidas geralmente compartilham state.
- Para state real, **substitua o `TaskService` por um repositório DynamoDB** (deixaremos isso para o próximo tutorial).

---

## Cleanup

Quando terminar:

```bash
aws lambda delete-function-url-config --function-name todo-lambda
aws lambda delete-function --function-name todo-lambda
```

---

## O que você acabou de fazer

- Definiu modelos com validação automática (`#[derive(Validate)]`).
- Padronizou erros HTTP com `#[derive(ApiError)]`.
- Injetou um service via `App::provide` + `State<Arc<...>>`.
- Escreveu 5 handlers concisos com macros de rota.
- Gerou OpenAPI / Swagger UI / ReDoc sem escrever spec.
- Rodou o mesmo binário em HTTP local e em AWS Lambda com Function URL.
- Tudo isso em **menos de 200 linhas** de código de aplicação.

## Onde ir a seguir

- [Decision Log](../development/decisions.md) — entenda por que o framework foi feito assim.
- [Diagramas de arquitetura](../architecture/overview.md) — fluxo de uma requisição em Lambda, componentes do framework.
- [PRD completo](../product/prd.md) — visão de longo prazo + features em roadmap.
- `cargo doc --workspace --no-deps --open` — referência completa de API.

## Troubleshooting comum

| Sintoma | Causa provável | Correção |
|---|---|---|
| `cargo lambda build` falha com "linker error" | Toolchain de cross-compile ausente | Em macOS/Linux: `cargo install --locked cargo-zigbuild` e use `--zigbuild`. Em Linux x86_64 → x86_64: deve funcionar direto. |
| Função em Lambda retorna 404 para todas as rotas | Stage prefix do API Gateway REST v1 | `run_lambda()` já define `AWS_LAMBDA_HTTP_IGNORE_STAGE_IN_PATH=true`. Se você criou a função manualmente sem usar `AppRuntime::run`, defina essa env var. |
| `cargo lambda deploy` reclama de permissão IAM | Role default não tem permissões suficientes | Adicione `lambda:CreateFunction`, `iam:CreateRole`, `iam:AttachRolePolicy` ao seu usuário. |
| Validação não dispara, body inválido vira erro 400 raw do axum | Esqueceu de `#[derive(Validate)]` no DTO | Todo DTO usado com `serverust_core::extract::Json<T>` precisa derivar `Validate`. |
| Swagger UI carrega vazio | Esqueceu de chamar `.register_schema::<T>()` para os DTOs | Registre todos os tipos que aparecem em request/response. |
