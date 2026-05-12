# Visão Geral da Arquitetura

> Documento curto e direto. Para detalhes, consulte os diagramas e o PRD ([`docs/product/prd.md`](../product/prd.md)).

## Workspace

O framework é um Cargo workspace com 5 crates principais e 2 exemplos:

| Crate | Responsabilidade |
|---|---|
| `serverust-core` | `App` builder, `Route`/`IntoRoute`, DI Container, extractors validantes, pipeline (Guard/Pipe/Interceptor), geração OpenAPI, config (figment). |
| `serverust-macros` | Proc-macros: `#[get]`/`#[post]`/`#[put]`/`#[patch]`/`#[delete]`, `#[derive(Validate)]`, `#[derive(ApiError)]`, `#[injectable]`, `#[guard]`, `#[metric]`. |
| `serverust-lambda` | Adapter `lambda_http`, detecção de runtime (Lambda vs HTTP local), trait `AppRuntime` para dot-chain (`App::new().run().await`). |
| `serverust-telemetry` | Logger JSON estruturado, middleware de correlation-id (X-Ray), métricas EMF, `IdempotencyStore` trait, feature opcional `otel` (OpenTelemetry + X-Ray propagator). |
| `serverust-cli` | CLI `serverust` com clap: `new`/`generate`/`dev`/`build`/`deploy`/`info`/`openapi`. |
| `examples/hello-world` | Binário mínimo para benchmark de cold start. |
| `examples/funds-api` | CRUD completo (validação, OpenAPI, DI, integration tests). |

## Diagramas

Os 3 diagramas abaixo argumentam visualmente a arquitetura. São arquivos `.excalidraw` (editáveis em [excalidraw.com](https://excalidraw.com)) com PNG renderizado ao lado para preview rápido.

- [**architecture**](diagrams/architecture.excalidraw) ([preview](diagrams/architecture.png)) — 4 camadas: app do usuário, crates do framework, dependências (Axum/Tokio/utoipa/...), integrações AWS (API Gateway, Lambda, CloudWatch, X-Ray, DynamoDB).
- [**sequence**](diagrams/sequence.excalidraw) ([preview](diagrams/sequence.png)) — fluxo temporal de uma requisição em Lambda: Client → API Gateway → Runtime → adapter → Router → Guards → Pipes → Handler → Service (DI) → Interceptors → Telemetry → Response.
- [**data-flow**](diagrams/data-flow.excalidraw) ([preview](diagrams/data-flow.png)) — transformação de dados ao longo do pipeline: raw bytes → ApiGatewayProxyRequest → http::Request → extractors → Validate → DTO → service → entity → mapper → JSON, com ramos de erro (HTTP 422 / ApiError) e telemetria paralela.

## Princípios Centrais

1. **Compile-time over runtime** — roteamento, DI graph e schema OpenAPI são resolvidos em compile-time. Zero overhead runtime, tipagem forte. Sem reflection.
2. **Macros para rotas/DTOs; builders para DI/App** — equilíbrio entre DX FastAPI-like e clareza de erros Rust. Detalhes em [`development/decisions.md`](../development/decisions.md), decisão #1.
3. **Layering sobre Axum, não substituição** — `App::axum_router()` exposto como escape hatch público para usuários avançados.
4. **Runtime dual transparente** — `App::new().run().await` detecta `AWS_LAMBDA_RUNTIME_API` e despacha entre `lambda_http` e servidor HTTP local. A mesma app roda em Lambda (REST v1, HTTP v2, Function URLs) ou localmente.
5. **Observabilidade out-of-the-box** — logs JSON, tracing X-Ray, métricas EMF habilitados por padrão. Features opcionais `otel` e `dynamodb` para deps mais pesadas.

## Stack Interna

Definida em [`development/decisions.md`](../development/decisions.md) (decisão #2 e seguintes):

- **HTTP/runtime**: Tokio · Axum 0.8 · Tower / tower-http.
- **Serialização**: Serde.
- **Validação**: validator 0.20.
- **OpenAPI**: utoipa 5.
- **Tracing/logs**: tracing 0.1 · tracing-subscriber 0.3 · (opcional) opentelemetry-sdk 0.26 + opentelemetry-aws.
- **Lambda**: lambda_http 1.2 · aws_lambda_events 1.2.
- **AWS SDK**: aws-sdk-rust (DynamoDB para idempotência, atrás de feature).
- **Config**: figment.
- **Erros**: thiserror.
- **CLI**: clap (derive).
