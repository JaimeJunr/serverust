# funds-api — Referência CRUD mínima

Exemplo de referência rápida: CRUD de `Fund` sem validações extras, DI ou OpenAPI schemas explícitos.

## O que demonstra

- Rotas HTTP com macros `#[get]`, `#[post]`, `#[put]`, `#[delete]`.
- Integração básica com `serverust-lambda` para rodar em AWS Lambda.
- Testes de integração contra o `axum::Router` diretamente.

## Diferença em relação ao `todo-api`

Veja a tabela comparativa no [README do todo-api](../todo-api/README.md). Em resumo: `funds-api` é mais simples, sem validação, DI ou OpenAPI schemas explícitos — ideal para uma referência rápida de como estruturar um handler.

## Como rodar

```bash
cargo run -p funds-api
cargo test -p funds-api
```
