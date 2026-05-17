# todo-api — Exemplo didático do tutorial Lambda

Este é o exemplo principal do tutorial [`docs/guides/lambda-tutorial.md`](../../docs/guides/lambda-tutorial.md).

## O que demonstra

- CRUD completo de `Task` com validação de entrada via `#[derive(Validate)]`.
- Erros padronizados com `#[derive(ApiError)]`.
- Dependency Injection de `TaskService` via `App::provide`.
- OpenAPI 3.1 + Scalar UI automáticos (sem anotações extras).
- Mesmo binário roda local (HTTP em `0.0.0.0:3000`) e em AWS Lambda (detecção automática).

## Diferença em relação ao `funds-api`

| Aspecto | `todo-api` | `funds-api` |
|---|---|---|
| Propósito | Tutorial guiado (passo a passo) | CRUD mínimo de referência |
| Validação | Sim — `#[derive(Validate)]` | Não |
| Erros customizados | Sim — `#[derive(ApiError)]` | Não |
| DI container | Sim — `TaskService` injetado | Não |
| OpenAPI schemas | Registrados explicitamente | Não |
| Complexidade | Média (didático) | Baixa (referência rápida) |

Use `todo-api` quando quiser entender o framework do zero, seguindo o tutorial.
Use `funds-api` como referência rápida de um handler CRUD sem dependências extras.

## Como rodar

```bash
# Local (HTTP em :3000)
cargo run -p todo-api

# Testes
cargo test -p todo-api
```
