# Ralph Progress Log

Snapshot do log de execução do Ralph Loop que gerou o MVP do framework. Cada seção representa uma user story completada com aprendizados de implementação.

**Fonte viva**: `.ralph/serverust-framework/progress.txt` — este documento é uma cópia para publicação. Para atualizações da fonte, edite o original.

**Stories completadas**: US-001 a US-011 (todas com `passes: true`).

---

## Codebase Patterns

Padrões reusáveis descobertos durante a implementação, consultar antes de iterar novamente:

- **PRD HAZARD**: o hook `ctx-rewrite` (`~/.claude/hooks/ctx-rewrite.sh`) envolve `jq` em `ctx exec jq`, que TRUNCA o output. Comandos tipo `jq '...' file > /tmp/out && mv /tmp/out file` corrompem o arquivo (escrevem só ~100 linhas truncadas). Para editar `prd.json` use **python3** (`json.load`/`json.dump`). Para ler, use `python3 -c '...'` ou `awk`, nunca `Read` direto se > 100 linhas.
- `App` carrega um `Container` (`TypeId → Arc<dyn Any+Send+Sync>`) como state do `Router<Container>`; blanket `FromRef<Container> for Arc<T: ?Sized + Send + Sync + 'static>` resolve `State<Arc<dyn Trait>>` em handlers automaticamente.
- `App::provide::<T>(Arc<T>)` registra (turbofish necessário para `dyn Trait`); `App::r#override::<T>(...)` substitui (raw identifier porque `override` é palavra reservada).
- `MethodRouter<Container>` é o tipo concreto em `Route`; handlers sem `State` inferem `Container` via `Handler<_, S>` polimórfico.
- `#[injectable]` em struct/enum gera `impl serverust_core::Injectable` marker (sem efeito runtime); aceita `#[injectable(static)]` como opt-in para hint de dispatch estático (apenas valida o atributo).
- `edition = "2024"`, `rust-version = "1.85"` em todos os crates.
- `proc-macro = true` declarado em `[lib]` do `serverust-macros/Cargo.toml`.
- `workspace.resolver = "2"` na raiz.
- Macros de rota geram unit struct com mesmo nome do handler, implementando `serverust_core::IntoRoute`.
- `serverust_core::extract::Json` é a versão validante (requer `T: Validate`); demais extractors (`Path`, `Query`, `State`) são re-export direto do axum.
- `serverust_core::__private` re-exporta `axum`, `http`, `serde_json` e `utoipa` para uso interno das macros.
- Versões: axum 0.8, tokio 1, syn 2, quote 1, http 1, serde 1, tower 0.5, validator 0.20, utoipa 5.
- Path params em axum 0.8 usam sintaxe `{param}` (não `:param`).
- Testes de integração em `serverust-core/tests/` usam `tower::ServiceExt::oneshot` para validar rotas sem bind real.
- `ApiError` trait + derive: variantes anotadas com `#[status(N)]` e `#[message("...")]`; derive emite `ApiError` + `IntoResponse` simultaneamente, com payload JSON `{ "error": "<message>" }`.
- Validação: `validation_error_response(&ValidationErrors)` constrói o payload 422 padrão `{ error: "validation_error", fields: { campo: [mensagens] } }`.
- `Route` carrega `path` + `HttpMethod` + `MethodRouter` + `Operation`; `App` acumula tudo em `OpenApiState` e materializa `/openapi.json` + `/docs` + `/redoc` em `into_router()`.
- Swagger UI e ReDoc servidos via HTML inline carregando assets via CDN (sem `utoipa-swagger-ui` crate — evita build script e mantém binário enxuto).
- Imports utoipa: `HttpMethod`/`PathItem`/`Paths` estão em `utoipa::openapi`; `Operation`/`OperationBuilder`/`PathItemBuilder` estão em `utoipa::openapi::path`; `ResponseBuilder` em `utoipa::openapi`.
- `ToSchema` + `Validate` em DTOs requer `#[schema(min_length=..)]` espelhando `#[validate(length(min=..))]` — utoipa NÃO lê atributos do validator automaticamente.
- `App` expõe: `openapi_info(title, version)`, `register_schema::<T>()`, `docs(path)`, `redoc(path)`, `route(handler)`, `into_router()`, `run_http(addr)`, `interceptor(I)`.
- Pipeline: trait `Guard` (async check sobre `&Parts`), `Pipe<I>` (transform I → Output, retorna `Response` em erro) e `Interceptor` (async wrap req/next) em serverust-core.
- Macro `#[guard(Type)]` colocada **acima** de `#[get/post/...]` injeta `GuardCheck<Type>` como `FromRequestParts` no início da assinatura — múltiplos `#[guard]` empilháveis.
- Interceptors são aplicados em `into_router()` **antes** das rotas de docs (`/openapi.json` `/docs` `/redoc`) — middleware do usuário não envelopa documentação.
- `clippy::result_large_err` é suprimido em `serverust-core/src/pipeline.rs` porque `Response` é a moeda de erro idiomática do axum.
- Inserir extractor sintético na assinatura: usar `syn::FnArg` via `parse_quote!` + `func.sig.inputs.insert(0, ...)` para manter o body-consuming extractor (`Json<T>`) sempre por último.
- Runtime dual mora em `serverust-lambda` (extensão trait `AppRuntime`) para não poluir `serverust-core` com dependência pesada `lambda_http`; `App::new().run().await` funciona via `use serverust_lambda::AppRuntime;`.
- `detect_runtime(env_value: Option<&str>) -> Runtime` é função pura — evita mutação de env em testes (em Rust 2024 `std::env::set_var` é `unsafe`).
- Fixtures de eventos AWS em `tests/fixtures/*.json` + `lambda_http::request::from_str` permitem testar roteamento sem boot do runtime.
- `lambda_http::Body` é `#[non_exhaustive]` — patterns precisam de arm `_`.
- `lambda_http::run(router)` requer apenas `+ Future` no impl trait (não `+ Send`); bound `Send` quebra porque axum body dyn não é `Sync`.
- `axum::middleware::from_fn(f)` exige fn item type / closure inferido (não aceita fn pointer com `Pin<Box<dyn Future>>`); para expor uma "layer pronta" sem amarrar o tipo, use `macro_rules!` que faz o wiring no call site.
- `tracing::Instrument::instrument(span)` no future é obrigatório para preservar contexto através de await points; `span.enter()` com guard só funciona em código síncrono.
- `tracing_subscriber` 0.3 `.json()`: mensagem fica em `fields.message`; campos de span pai aparecem em `span.<field>` quando `.with_current_span(true)` está ativo.
- EMF mínimo: `{"_aws":{"Timestamp":ms,"CloudWatchMetrics":[{"Namespace","Dimensions":[[]],"Metrics":[{"Name","Unit"}]}]},"<Name>":<value>}` — uma linha JSON por evento em stdout.
- `opentelemetry_sdk` 0.26: `TracerProvider` (sem prefixo `Sdk`); id generator vai em `Config::default().with_id_generator(...)` passado via `.with_config(...)`; propagador X-Ray vem de `opentelemetry-aws`.
- clippy 1.94+: `func.block = Box::new(new_block)` em macros proc dispara `clippy::replace_box`; usar `*func.block = new_block`.
- Features opcionais (`otel`, `dynamodb`) com deps pesadas (`aws-sdk`, `opentelemetry`) preservam binário enxuto no build default — documentar em Cargo.toml o que cada feature traz.

---

## 2026-05-12 — US-001: Cargo workspace e crates base

- Criado Cargo workspace com 5 membros: `serverust-core`, `serverust-macros`, `serverust-cli`, `serverust-lambda`, `serverust-telemetry`.
- Cada crate com `edition = "2024"` e `rust-version = "1.85"`.
- `serverust-macros` com `proc-macro = true` em `[lib]`.
- Testes mínimos escritos primeiro (TDD): 1 teste por crate executável.
- `cargo build --workspace` e `cargo test --workspace` passando sem erros.
- **Files**: `Cargo.toml`, `serverust-*/Cargo.toml`, `serverust-*/src/lib.rs` (ou `main.rs`).
- **Learnings**:
  - `serverust-macros` não aceita testes unitários diretos (proc-macro crate) — sem testes ali.
  - Estrutura modular criada e pronta para US-002.

---

## 2026-05-12 01:12:28 — US-002: App builder e roteamento declarativo via macros

- Implementado `App` builder em `serverust-core`: `App::new().route(handler).into_router()` / `run_http(addr)`.
- `Route` + `IntoRoute` trait permitem registrar handlers tipados sem reflection.
- Macros `#[get]`, `#[post]`, `#[put]`, `#[patch]`, `#[delete]` em `serverust-macros` transformam `fn` em unit struct com `IntoRoute` impl.
- Extractors re-exportados em `serverust_core::extract`: `Path`, `Query`, `Json`, `State`.
- 6 testes de integração validam: GET estático, `Path<u32>`, `Query<T>`, `Json<T>` POST round-trip, múltiplas rotas compostas, `run_http` bind.
- **Files**: `serverust-core/{Cargo.toml, src/lib.rs, src/app.rs, src/route.rs, tests/routes.rs}`, `serverust-macros/{Cargo.toml, src/lib.rs}`.
- **Learnings**:
  - axum 0.8 mudou sintaxe de path params de `:id` para `{id}`.
  - `quote!`/`parse_macro_input` fluxo padrão: parse `LitStr` do attr, `ItemFn` do item, gerar struct + impl `IntoRoute` com fn original aninhada em `into_route()`.
  - Função interna shadow-ada dentro de `into_route()` permite passar fn ao `axum::routing::METHOD(fn)` sem conflito com a struct externa.
  - dev-dependency `serverust-macros = { path = "../serverust-macros" }` evita ciclo do workspace.

---

## 2026-05-12 01:19:14 — US-003: Validação automática com `#[derive(Validate)]` e erros padronizados

- Implementada validação automática via `serverust_core::extract::Json<T>` (T: `DeserializeOwned + Validate`). Falha de validação retorna HTTP 422 com payload padronizado antes do handler ser invocado.
- Implementada derive `ApiError` em `serverust-macros`: lê `#[status(N)]` e `#[message("...")]` por variante e emite impl `ApiError` + impl `IntoResponse`, permitindo `?` em handlers `Result<T, E: ApiError>`.
- 5 testes de integração novos em `serverust-core/tests/validation.rs` cobrem: happy path, falha de validação com payload estruturado, duas variantes de `ApiError` (404, 409) e `IntoResponse` direto.
- Existing `routes.rs` test atualizado para derivar `Validate` em `CreateUser` (consequência do novo `Json` validante).
- **Files**: `serverust-core/{Cargo.toml, src/{lib.rs,error.rs,validation.rs}, tests/{routes.rs,validation.rs}}`, `serverust-macros/src/lib.rs`.
- **Learnings**:
  - validator 0.20 expõe `ValidationErrors::field_errors()` retornando `HashMap<&'static str, &Vec<ValidationError>>`; campo `message` é `Option<Cow<'static, str>>`, cai em `code` quando ausente.
  - Substituir o re-export `extract::Json` por um wrapper validante quebra DTOs sem `#[derive(Validate)]`; solução é derivar `Validate` em todos os payloads (no-op quando sem regras).
  - Em axum 0.8 a trait `FromRequest` é nativa async (sem `#[async_trait]`); rejection do tipo `Response` deixa o extrator devolver qualquer status/payload sem cerimônia.
  - Derive de `proc_macro2` sobre enums com variantes Unit/Tuple/Named exige patterns distintos (`Self::V`, `Self::V(..)`, `Self::V { .. }`).
  - `LitInt` em `#[status(404)]` aceita literal inteiro qualquer; conversão para `u16` acontece em runtime via `StatusCode::from_u16`.

---

## 2026-05-12 01:27:22 — US-004: OpenAPI automático com utoipa + Swagger UI

- Integrei utoipa 5 no `serverust-core`. `App` agora gera OpenAPI 3.1 dinâmico:
  - `openapi_info(title, version)` customiza Info; defaults: `"serverust"` / `"0.1.0"`.
  - `register_schema::<T: ToSchema>()` registra T em `components.schemas`.
  - `docs(path)` / `redoc(path)` customizam paths (defaults `/docs` e `/redoc`).
  - `into_router()` injeta automaticamente `/openapi.json`, `/docs` e `/redoc`.
- Macros de rota (`#[get/post/put/patch/delete]`) agora geram, além do `IntoRoute`, uma `utoipa::openapi::path::Operation` com `operation_id` = nome da fn e resposta 200 default.
- `/docs` e `/redoc` servidos via HTML inline carregando Swagger UI e ReDoc via CDN (jsdelivr) — decisão consciente para evitar build script de `utoipa-swagger-ui` e manter binário enxuto.
- DTOs precisam de `#[derive(ToSchema)]` + atributos `#[schema(...)]` espelhando os `#[validate(...)]` para constraints aparecerem no spec.
- 6 testes novos em `serverust-core/tests/openapi.rs`.
- **Files**: `serverust-core/{Cargo.toml, src/{lib.rs, app.rs, route.rs, openapi.rs}, tests/openapi.rs}`, `serverust-macros/src/lib.rs`.
- **INFRA FIX (recorrente)**: `prd.json` corrompido novamente pelo marcador de truncamento do `Read` após linha 100. Padrão: ler `prd.json` via `jq` ou `awk`, nunca via `Read` direto se passar de 100 linhas.
- **Learnings**:
  - utoipa 5: `Operation`/`OperationBuilder`/`PathItemBuilder` em `utoipa::openapi::path`; `HttpMethod`/`PathItem`/`Paths`/`ResponseBuilder` em `utoipa::openapi`.
  - `PathsBuilder.path()` faz merge automático quando a mesma key aparece duas vezes — acumular paths por `(path, method)` → group em `BTreeMap` antes de `build()`.
  - `utoipa-swagger-ui` crate baixa assets via build script; para MVP evitamos isso servindo HTML via CDN.

---

## 2026-05-12 01:40:00 — US-005: Dependency Injection híbrido via builder

- `serverust_core::Container` (`HashMap<TypeId, Arc<dyn Any+Send+Sync>>`) é o axum state do `App`.
- `App::provide::<T>(Arc<T>)` e `App::r#override::<T>(Arc<T>)` registram/substituem services.
- Blanket `impl<T: ?Sized + Send + Sync + 'static> FromRef<Container> for Arc<T>` faz handlers extraírem `State<Arc<dyn Trait>>` automaticamente.
- `Route.method_router` agora é `MethodRouter<Container>` (macros existentes inferem via `Handler` trait polimórfica).
- Macro `#[injectable]` (struct/enum, opcional `(static)`) emite `impl serverust_core::Injectable` como marker — registro é explícito via builder.
- 4 testes novos em `tests/di.rs`: injeção via `State<Arc<dyn Trait>>`, override com mock, Singleton compartilhado, marker trait em compile-time.
- **Files**: `serverust-core/src/{container.rs, lib.rs, app.rs, route.rs}`, `serverust-core/tests/di.rs`, `serverust-macros/src/lib.rs`.
- **Learnings**:
  - `TypeId::of::<Arc<dyn Trait>>()` é único por trait — permite armazenar múltiplos `Arc<dyn _>` no mesmo HashMap sem colisão.
  - Orphan rule permite blanket `FromRef<Container>` porque `Container` é local.
  - `override` é palavra reservada em Rust; método precisa ser `r#override`.

---

## 2026-05-12 01:45:51 — US-006: Guards, Pipes e Interceptors

- Implementadas as 3 primitivas de pipeline em `serverust-core` (módulo novo `pipeline.rs`):
  - `Guard` (async fn check em `&Parts` → `Result<(), Response>`) + extractor zero-cost `GuardCheck<G>` (`FromRequestParts`).
  - `Pipe<I>` com associated type `Output`; `ParseUuidPipe: Pipe<String, Output=Uuid>` como exemplo canônico; extractor `PipePath<P>` aplica o pipe sobre `Path<String>`.
  - `Interceptor` (async wrap `(Request, Next) -> Response`) registrado via `App::interceptor(I)`.
- Macro `#[guard(Type)]` em `serverust-macros`: insere `GuardCheck<Type>` na posição 0 do `func.sig.inputs`. Múltiplos guards empilháveis.
- 6 testes novos em `serverust-core/tests/middleware.rs` cobrindo cada primitiva isolada + composição completa.
- **Files**: `serverust-core/{Cargo.toml, src/{lib.rs, app.rs, pipeline.rs}, tests/middleware.rs}`, `serverust-macros/src/lib.rs`.
- **Learnings**:
  - Atributos macro empilhados expandem **top-down** (outermost primeiro). Para `#[guard]` modificar a função ANTES de `#[get]` ver, precisa estar ACIMA da macro de rota.
  - Inserir extractor sintético em posição 0 é crítico: handlers com body-consuming extractor (`Json<T>`) só funcionam se este for o ÚLTIMO param. `GuardCheck`/`PipePath` são `FromRequestParts` e devem vir antes.
  - Testes não devem depender de estado global (`AtomicUsize` estático) — testes rodam em paralelo.
  - `Router::layer()` afeta apenas rotas registradas **antes** da chamada — guardar interceptors em `Vec<RouterMutator>` e aplicar em `into_router()` antes de adicionar `/openapi.json` `/docs` `/redoc`.

---

## 2026-05-12 01:55:21 — US-007: Runtime dual Lambda + HTTP com detecção automática

- `Runtime` enum + função pura `detect_runtime(Option<&str>) -> Runtime`.
- `run_http(app, addr)`, `run_lambda(app)`, `run(app)` (despacha por env).
- Trait `AppRuntime` adiciona `App::new().run().await` via dot-chain (`use serverust_lambda::AppRuntime`).
- `run_lambda` define `AWS_LAMBDA_HTTP_IGNORE_STAGE_IN_PATH=true` automaticamente para rotas funcionarem idêntico em REST v1, HTTP v2 e Function URL.
- 3 unit tests + 3 integration tests com fixtures JSON (apigw v1 GET, apigw v2 POST, Function URL).
- **Files**: `serverust-lambda/{Cargo.toml, src/lib.rs, tests/lambda_to_axum.rs, tests/fixtures/*.json}`, `serverust-cli/src/main.rs`.
- **Learnings**:
  - lambda_http 1.2 + axum 0.8 + http 1: integração direta sem adapter explícito.
  - Para v1 (REST API Gateway), `requestContext.path` inclui o prefixo do stage (`/prod/hello`) — sem `AWS_LAMBDA_HTTP_IGNORE_STAGE_IN_PATH=true` o axum recebe `/prod/hello` e devolve 404.
  - `+ Send` no impl Trait quebrou porque o future de `lambda_http::run(router)` contém `dyn HttpBody` que não é `Sync`.
  - `lambda_http::request::from_str(json)` faz a desambiguação entre v1/v2/Function URL pelos campos presentes.

---

## 2026-05-12 — US-008: Telemetria nativa (logger + tracing + metrics)

- `serverust-telemetry`:
  - `logger::init()` instala `tracing-subscriber` JSON estruturado (env filter via `RUST_LOG`).
  - `correlation::extract_or_generate_correlation_id(&HeaderMap)` (extrai `Root=` de `X-Amzn-Trace-Id`, fallback `X-Correlation-Id`, ou gera no formato X-Ray).
  - Middleware `correlation_id_middleware` injeta header e abre span tracing.
  - `emf::emit_emf(namespace, name, unit, value)` escreve linha JSON única no formato EMF.
  - `idempotency`: trait `IdempotencyStore` + `IdempotencyRecord` + `IdempotencyError`, `InMemoryIdempotencyStore`, `DynamoDbIdempotencyStore` atrás da feature `dynamodb`.
  - Feature `otel` adiciona `otel::init_xray(service)` que configura `XrayIdGenerator` + `XrayPropagator`.
- Macro `#[metric(name, unit, namespace)]` em `serverust-macros`: cronometra função sync/async e emite EMF com tempo em ms.
- 14 testes novos.
- **Files**: `serverust-telemetry/{Cargo.toml, src/{lib.rs,logger.rs,correlation.rs,emf.rs,idempotency.rs,otel.rs}, tests/*}`, `serverust-macros/src/lib.rs`.
- **Learnings**:
  - `tracing-subscriber` 0.3 `.json()`: mensagem fica em `fields.message`; campos de span pai em `span.<field>` quando `.with_current_span(true)`.
  - `tracing::Instrument::instrument(span)` no future preserva contexto através de await points.
  - `axum::middleware::from_fn(f)` exige fn item type — solução: async fn público + macro `correlation_id_layer!()` para wiring.
  - Format EMF mínimo: bloco `_aws.CloudWatchMetrics[].Metrics[]` com `Name`/`Unit` + campo top-level homônimo com o valor.
  - Macro `#[metric]` precisa diferenciar sync vs async via `func.sig.asyncness.is_some()`.

---

## 2026-05-12 — US-009: CLI Rust com clap (`new`/`generate`/`dev`/`build`/`deploy`/`info`/`openapi`)

- CLI `serverust` em `serverust-cli` com clap derive (`Parser` + `Subcommand` + `ValueEnum`):
  - `new <name>` gera scaffold completo (recusa se diretório existe).
  - `generate <kind> <name>` para 8 kinds: `resource`, `module`, `controller`, `service`, `pipe`, `guard`, `interceptor`, `filter`.
  - `dev` → `cargo watch -x run`.
  - `build [--release]` → `cargo build [--release]`.
  - `deploy lambda [--arch arm64|x86_64]` → `cargo lambda deploy`.
  - `info` → versão da CLI + arch + features.
  - `openapi --out <path>` → exporta spec sem subir servidor.
- Crate dividida em `cli` / `commands` / `scaffold` / `templates` para permitir testes sem efeito colateral.
- 20 testes novos (parse, command-builders, scaffolding em tempdir).
- **Files**: `serverust-cli/{Cargo.toml, src/{main.rs, lib.rs, cli.rs, commands.rs, scaffold.rs, templates.rs}, tests/*}`.
- **Learnings**:
  - clap `ValueEnum` com underscores produz kebab-case default — usar `#[value(name = "x86_64", alias = "x86-64")]`.
  - `[[bin]]` + `[lib]` no mesmo `Cargo.toml` mantém o binário disponível como `serverust` e expõe a lib para tests.
  - `#[path]` mapeia filename com `.` (Nest convention) para mod identifier válido em Rust.
  - `std::process::Command` permite validar invocação SEM spawn via `.get_program()`/`.get_args()`.

---

## 2026-05-12 — US-010: Configuração via `serverust.toml` + figment

- `serverust-core/src/config.rs`:
  - Structs: `ServerConfig`, `LambdaConfig`, `TelemetryConfig`, `OpenApiConfig`, `ServerustConfig`.
  - `ServerustConfig::load()` lê `serverust.toml` no profile `"default"`.
  - `ServerustConfig::load_for_profile(profile)` seleciona perfil (herda de `default`).
  - Env override via `SERVERUST_*` com separador `__` para campos aninhados.
- `App::config(ServerustConfig)` armazena no `Container`; handlers extraem via `State<Arc<ServerustConfig>>`.
- 8 testes em `tests/config.rs` serializados via `ENV_MUTEX` estático.
- **Files**: `serverust-core/{Cargo.toml, src/config.rs, src/lib.rs, src/app.rs, tests/config.rs}`, `serverust-cli/src/templates.rs`.
- **Learnings**:
  - `figment::Toml::file(path).nested()` é o modo correto para TOML profile-aware (top-level keys = profile names).
  - Env tests paralelos com `std::env::set_var` contaminam testes de file-loading — `ENV_MUTEX` em TODOS que chamam `load_from`.
  - figment profile inheritance: `select("dev")` herda do `default` automaticamente.

---

## 2026-05-12 — US-011: Exemplo `funds-api` end-to-end + benchmark hello-world

- Adicionados `examples/hello-world` e `examples/funds-api` como membros do workspace.
- `hello-world`: binário mínimo para benchmark de cold start.
- `funds-api`: CRUD completo de Fundos de Investimento (5 handlers, validação, OpenAPI, DI, 5 integration tests).
- `scripts/bench.sh`: mede tamanho stripped (alvo < 10 MB), startup local, cold start Lambda via AWS CLI (alvo < 50 ms, opcional `--lambda`).
- README.md completo (features, requisitos, estrutura, quick start, config, exemplos, deploy, benchmark, CLI).
- **Files**: `README.md`, `scripts/bench.sh`, `examples/hello-world/*`, `examples/funds-api/*`.
- **Learnings**:
  - Disk full (100% usage): `cargo build` falha silenciosamente com exit code 1 e zero output — `ctx-rewrite` hook trunca o output. Solução: `cargo clean` libera o `target/`.
  - Exemplo `funds-api` precisa de `[lib]` + `[[bin]]` no `Cargo.toml` para expor lib para integration tests.
  - Handlers com `Path<u64>` exigem `axum` no `Cargo.toml` do exemplo (não vem re-exportado via `serverust-core`).
