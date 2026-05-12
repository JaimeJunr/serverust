# Decision Log

Decisões arquiteturais fechadas durante a fase de discovery do PRD (extraído de [docs/product/prd.md](../product/prd.md), seção 10).

Novas decisões devem ser registradas no final deste documento conforme surgirem.

---

## Decisões fechadas (PRD)

| # | Tema | Decisão | Racional |
|---|---|---|---|
| 1 | Macros vs builders | Macros declarativas para rotas e DTOs (`#[get]`, `#[derive(Validate)]`); **DI e App via builders explícitos**. | Equilíbrio entre DX FastAPI-like e clareza/erro de compilação Rust. Macros excessivas em DI degradariam mensagens de erro e tempo de build. |
| 2 | OpenAPI: utoipa vs aide | **utoipa**. | Maturidade, derive macros estáveis, integração nativa `utoipa-axum`, melhor cobertura de extractors customizados. |
| 3 | DI strategy | **Híbrido**: `Arc<dyn Trait>` default, opt-in para generics via `#[injectable(static)]`. | Default cobre 95% dos casos com DX ergonômica; usuário pode optar por static dispatch quando performance for crítica. |
| 4 | Idempotência storage | Trait `IdempotencyStore` com **DynamoDB default**; impls custom (Redis/Postgres) pluggable. | Alinha com Lambda como ambiente primário, mas não amarra usuários ao AWS. |
| 5 | CLI lang | **Rust puro com `clap`**. Sem dependência Node. | Coerência com o framework (tudo Rust); binário único via `cargo install serverust-cli`. |
| 6 | Naming | **Codename interno "RustAPI"**; nome público definido próximo do release. | Evita commit antecipado a um nome que pode colidir no crates.io. |
| 7 | MSRV | **Rust 1.85+ (Edition 2024)**. | Alinhado a `aws-lambda-rust-runtime` 1.84+ (jan/2026) e Lambda Managed Instances (mar/2026). Habilita `async fn` em traits e async closures. |
| 8 | Hot reload | **`cargo-watch`** no MVP. `bacon` no roadmap pós-MVP. | Maduro, amplamente usado, integração simples. `bacon` (subsecond rebuilds) entra após MVP. |
| 9 | Powertools scope MVP | Logger + Tracing + Metrics no MVP. **Idempotência completa em fase 2** (trait exposta, macro `#[idempotent]` adiada). | Entrega 3 pilares de observabilidade; idempotência completa (schema DynamoDB + macro) é mais cara e fica para fase 2. |
| 10 | Escape hatch Axum | **`App::axum_router()`** exposto como API pública estável. | Usuários avançados caem para Axum puro quando precisarem; framework não vira gaiola. |

---

## Decisões adicionais (descobertas durante implementação)

Padrões e escolhas técnicas que emergiram durante o Ralph Loop e não estavam explícitas no PRD original. Para detalhes, ver [`ralph-progress.md`](./ralph-progress.md).

| Tema | Decisão | Origem |
|---|---|---|
| Swagger UI / ReDoc serving | HTML inline + CDN (jsdelivr) em vez de `utoipa-swagger-ui` crate. | US-004 — evita build script e mantém binário enxuto. |
| Container DI | `HashMap<TypeId, Arc<dyn Any+Send+Sync>>` como axum state; blanket `FromRef<Container> for Arc<T>` resolve `State<Arc<dyn Trait>>` automaticamente. | US-005. |
| Posição de `#[guard]` | `#[guard(Type)]` deve ficar **acima** de `#[get/post/...]` para que o extractor sintético seja inserido antes do macro de rota ver a assinatura. | US-006. |
| `AWS_LAMBDA_HTTP_IGNORE_STAGE_IN_PATH` | Definido automaticamente em `run_lambda()` para REST v1, HTTP v2 e Function URL funcionarem com as mesmas rotas. | US-007. |
| EMF format mínimo | `_aws.CloudWatchMetrics[].Metrics[]` com `Name`/`Unit` + campo top-level homônimo. Uma linha JSON por evento. | US-008. |
| OpenTelemetry SDK | `opentelemetry_sdk` 0.26 + `opentelemetry-aws` (X-Ray propagator) atrás da feature `otel`. | US-008. |
| CLI architecture | Crate dividida em `cli` / `commands` / `scaffold` / `templates` para permitir testes sem efeito colateral (validação de `Command` sem `spawn`). | US-009. |
| `figment` profile mode | `Toml::file(path).nested()` (top-level keys = profile names), não flat. | US-010. |
| Edition 2024 + env in tests | `std::env::set_var` é `unsafe` em Edition 2024 — funções puras (`detect_runtime(Option<&str>)`) substituem mutação de env em tests. | US-007. |

---

## Novas questões em aberto

_(adicionar conforme surgirem durante manutenção/evolução)_
