# ADR 0001 — HTTP-first via Axum + cargo-lambda

- **Status:** Accepted
- **Date:** 2024-01-01 (retroativa — decisão original do v0.1.0)
- **Deciders:** maintainers serverust

---

## Contexto e Problema

O serverust nasceu como framework para APIs HTTP em AWS Lambda com Rust. Era necessário escolher o stack HTTP e a toolchain de deploy.

## Drivers de Decisão

- Cold start e tamanho de binário são SLOs públicos (< 50 ms / < 10 MB).
- AWS Lambda é o ambiente-alvo primário, mas o framework deve rodar localmente sem emulação de Lambda.
- A comunidade Rust já tem Axum como stack HTTP mais maduro e amplamente adotado.
- `cargo-lambda` é a ferramenta de referência para build/deploy de funções Lambda Rust.

## Opções Consideradas

1. **Axum + cargo-lambda** (escolhida)
2. Actix-web + cross-compilation manual
3. Framework HTTP próprio (zero deps externas)

## Decisão

Usar **Axum 0.8** como layer HTTP e **cargo-lambda** como toolchain de build/deploy.

- `serverust-lambda` encapsula o adapter `lambda_http` e detecta automaticamente se está rodando em Lambda ou local.
- `App::run().await` funciona nos dois ambientes sem mudança no código do usuário.
- Roteamento, DI e pipeline ficam em `serverust-core`, independentes de Axum; o adapter Lambda é opt-in via `serverust-lambda`.

## Consequências

### Positivas
- Stack HTTP testado em produção por milhares de projetos.
- `cargo-lambda` cobre cross-compilation para ARM64 e provê `cargo lambda watch` para hot reload.
- Axum é "layering sobre Tower" — middleware ecosystem rico disponível imediatamente.
- `App::axum_router()` como escape hatch: usuários avançados acessam Axum puro quando necessário.

### Negativas / Trade-offs
- Acoplamento a Axum como runtime HTTP (não pluggable por design no MVP).
- Upgrade de Axum (ex.: 0.7→0.8) pode requerer ajustes em `serverust-core`.

## Links e Referências

- [Axum 0.8 changelog](https://github.com/tokio-rs/axum/blob/main/axum/CHANGELOG.md)
- [cargo-lambda docs](https://www.cargo-lambda.info/)
- [lambda_http crate](https://crates.io/crates/lambda_http)
- Decisão #1 e #7 em [`../decisions.md`](../decisions.md)
