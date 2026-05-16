# Análise Competitiva — Rocket

> Última atualização: 2026-05-13
> Versão analisada: Rocket v0.5.1 (maio 2024)
> Fonte: https://rocket.rs · https://github.com/rwf2/Rocket

---

## O que é

Rocket é o framework web Rust mais conhecido do ecossistema. Proposta de valor central: **type safety radical** — o compilador Rust valida rotas, guards e extractors antes do binário sequer rodar. A DX é altamente ergonômica para quem já conhece Rust, com macros familiares (`#[get]`, `#[post]`) e tratamento de erros integrado ao sistema de tipos.

É um framework para aplicações web tradicionais: servidores HTTP long-running, APIs REST clássicas, apps full-stack com templates. Não foi projetado para serverless.

---

## O que tem de bom

| Aspecto | Avaliação |
|---|---|
| Type safety | Excelente — guards e extractors falham em tempo de compilação |
| Documentação | Entre as melhores do ecossistema Rust |
| Comunidade | Grande, ativa, muitos exemplos disponíveis |
| Maturidade | Estável, v0.5 levou anos para ser finalizada |
| Async nativo | Sim, desde v0.5 com Tokio |
| Tratamento de erros | Elegante via `Responder` e catchers customizáveis |

Rocket é uma escolha sólida para quem quer um framework web Rust opinativo com boa DX para aplicações tradicionais.

---

## O que falta vs serverust

| Feature | serverust | Rocket |
|---|:---:|:---:|
| AWS Lambda nativo | ✅ | ❌ |
| Runtime dual HTTP ↔ Lambda | ✅ | ❌ |
| OpenAPI 3.1 automático | ✅ | ❌ (rocket_okapi externo) |
| Scalar / docs UI embutido | ✅ | ❌ |
| Validação → HTTP 422 padronizado | ✅ | ❌ (rocket-validation externo) |
| Dependency Injection nativo | ✅ | ❌ (coi-rocket externo) |
| CLI scaffolding (`new`, `generate`) | ✅ | ❌ |
| Cold start < 50 ms (Lambda ARM64) | ✅ | ✗ não aplicável |
| Binário stripped < 10 MB | ✅ | ✗ não otimizado |

## Kafka & Event Sources

Rocket não suporta Kafka event source. É um framework HTTP-only por design. Não há issues ou PRs no tracker oficial propondo suporte a Kafka, SQS ou outros event sources ([github.com/rwf2/Rocket/issues](https://github.com/rwf2/Rocket/issues)). Para Lambda + Kafka, o desenvolvedor precisa abandonar Rocket e usar lambda_runtime diretamente.

---

**Ponto crítico**: o maior gap é o suporte a serverless. `rocket_lamb`, o único adapter disponível, foi publicado em 2019 e está desatualizado — não recebe manutenção. Para AWS Lambda, usar Rocket em produção hoje exige soluções improvisadas.

**Segundo gap**: tudo que faz Rocket parecido com serverust (OpenAPI, validação, DI) vem de plugins de terceiros, não mantidos pela equipe core. Isso significa versões defasadas, APIs inconsistentes e risco de abandono.

---

## Quando faz sentido usar Rocket

- Aplicação web tradicional (servidor long-running, não Lambda)
- Time já familiarizado com Rocket e sem necessidade de serverless
- Projeto sem requisito de OpenAPI automático
- Preferência por framework com grande comunidade e documentação extensa
- Casos onde type safety máximo e zero runtime errors são prioridade absoluta

**Não faz sentido usar Rocket se**: o deploy alvo é AWS Lambda, se você quer OpenAPI automático sem configuração manual, ou se precisa de DI e scaffolding de fábrica.

---

## Posicionamento serverust vs Rocket

Rocket e serverust compartilham a filosofia de macros para roteamento (`#[get]`, `#[post]`), mas divergem completamente no alvo de deploy e na DX out-of-the-box.

serverust foi construído desde o início para o modelo serverless — runtime dual, binário enxuto, cold start otimizado. Rocket assume um servidor long-running. Essa diferença não é incidental; ela permeia cada decisão de design dos dois projetos.

Para times que precisam de AWS Lambda + OpenAPI + DI + CLI em Rust, serverust elimina semanas de configuração manual que Rocket exigiria.
