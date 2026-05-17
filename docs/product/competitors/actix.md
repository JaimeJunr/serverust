# Análise Competitiva — actix-web

> Última atualização: 2026-05-16
> Versão analisada: actix-web v4.13.0 (maio 2026)
> Fonte: https://actix.rs · https://github.com/actix/actix-web/releases

---

## O que é

actix-web é o framework web Rust mais utilizado em produção. Proposta de valor central: **máxima performance HTTP** — consistentemente no topo de benchmarks como TechEmpower, com throughput que rivaliza com Go e C++. A API é ergonômica, com extractors tipados (`web::Json<T>`, `web::Path<T>`) e middleware composável.

É um framework para servidores HTTP long-running de alta vazão: APIs REST de alto throughput, proxies, gateways, microservices. Não foi projetado para serverless nem para event sources não-HTTP.

---

## O que tem de bom

| Aspecto | Avaliação |
|---|---|
| Throughput HTTP | Excepcional — top-3 em benchmarks TechEmpower |
| Maturidade | Alta — v4 estável há anos, usado em produção em larga escala |
| Ecossistema | Amplo — actix-extras, middleware de auth, rate limiting, sessions |
| Documentação | Boa, com exemplos práticos no site oficial |
| Async nativo | Sim, Tokio desde v4 |
| Extractors | Ergonômicos e composáveis, similar a axum |
| WebSockets | Suporte nativo via actix-web-actors |

actix-web é a escolha certa quando o requisito é throughput HTTP máximo em servidor long-running.

### Mudanças notáveis em v4.10 – v4.13 (atualização 2026-05-16)

- **MSRV elevado para Rust 1.88** (a partir de v4.12): projetos com toolchain fixo precisam atualizar.
- **Throughput HTTP/2 melhorado**: novos parâmetros `h2_initial_window_size` e `h2_initial_connection_window_size` no `HttpServer` permitem ajuste fino do controle de fluxo — ganhos mensuráveis em streams de alta throughput.
- **Security fix actix-http v3.12.1**: corrigido request smuggling via cabeçalhos `Content-Length` + `Transfer-Encoding` simultâneos. Atualização obrigatória para quem expõe actix-web diretamente à internet.
- **`experimental-introspection` feature**: lista rotas registradas em runtime — funcionalidade comparável ao OpenAPI automático do serverust, mas experimental, opt-in, e sem schema de tipos nem Scalar UI integrado.

---

## O que falta vs serverust

| Feature | serverust | actix-web |
|---|:---:|:---:|
| AWS Lambda nativo | ✅ | ❌ |
| Runtime dual HTTP ↔ Lambda | ✅ | ❌ |
| Kafka event source nativo | ✅ | ❌ |
| SQS / EventBridge / S3 event source | ✅ | ❌ |
| OpenAPI 3.1 automático | ✅ | ❌ (utoipa externo) |
| Scalar / docs UI embutido | ✅ | ❌ |
| CLI scaffolding (`new`, `generate`) | ✅ | ❌ |
| Binário stripped < 10 MB | ✅ | ✗ não otimizado para Lambda |
| Cold start < 50 ms (Lambda ARM64) | ✅ | ✗ não aplicável ao modelo |

**Ponto crítico sobre Lambda**: actix-web pode rodar em Lambda via `lambda_web` (crate community, não oficial). Entretanto, cold starts típicos são >100ms — o próprio repositório do projeto documenta overhead de inicialização em discussões de issues ([actix/actix-web#2785](https://github.com/actix/actix-web/issues/2785)) e benchmarks comunitários consistentemente registram >150ms no ARM64 128MB. Isso ultrapassa o SLO de 50ms p95 do serverust. A razão é estrutural: actix-web não foi projetado para inicialização única — assume um servidor persistente.

**Segundo gap**: event sources. actix-web é HTTP-only por design. Kafka, SQS, EventBridge e S3 events simplesmente não existem no modelo de programação. Para cobrir esses casos, o desenvolvedor precisa de um segundo binário com outra stack (lambda_runtime + rdkafka crus), duplicando boilerplate e context-switching.

---

## Kafka & Event Sources

actix-web não suporta Kafka event source nativo. Não há planos documentados para adicioná-lo — a filosofia do projeto é HTTP. Para Lambda + Kafka, a única opção é abandonar actix-web e usar lambda_runtime diretamente, perdendo todo o ecossistema do framework.

serverust resolve isso com `serverust-events` (opt-in): o mesmo projeto, o mesmo DI container, o mesmo padrão de macros — agora cobrindo tanto HTTP quanto Kafka/SQS.

---

## Quando faz sentido usar actix-web vs serverust

| Cenário | Recomendação |
|---|---|
| API REST de alta vazão, servidor long-running (EC2/ECS/k8s) | **actix-web** |
| Proxy reverso ou gateway HTTP com baixa latência de processamento | **actix-web** |
| WebSockets em escala | **actix-web** |
| AWS Lambda (HTTP e/ou Kafka/SQS) | **serverust** |
| HTTP + Kafka no mesmo handler/projeto | **serverust** |
| Cold start otimizado (edge, Lambda@Edge) | **serverust** |
| OpenAPI automático sem config manual | **serverust** |

---

## Posicionamento serverust vs actix-web

actix-web e serverust não são concorrentes diretos em foco: actix-web maximiza throughput em servidor persistente; serverust maximiza DX e cold start em Lambda serverless.

O risco real é um time que conhece actix-web e tenta portá-lo para Lambda — a experiência é frustrante (cold start alto, sem event sources, lambda_web não oficial). serverust foi desenhado para o modelo Lambda desde o primeiro commit, o que elimina esse atrito por construção.
