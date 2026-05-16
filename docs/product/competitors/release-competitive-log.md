# Release Competitive Log

Histórico de benchmarks competitivos por release do serverust.
Gerado via `scripts/benchmark_competitive.sh`; números de binário/cold start via `scripts/benchmark_ci.sh`.

---

## v0.1.0 — 2024-06-01

### Versões dos concorrentes
| Biblioteca | Versão | Fonte |
|---|---|---|
| Rocket | v0.5.1 | https://github.com/rwf2/Rocket/releases |
| Loco.rs | v0.3.x | https://github.com/loco-rs/loco/releases |
| actix-web | v4.5.1 | https://github.com/actix/actix-web/releases |
| axum | v0.7.x | https://github.com/tokio-rs/axum/releases |
| lambda_runtime | v0.8.x | https://github.com/awslabs/aws-lambda-rust-runtime/releases |

### Números serverust hello-world
| Métrica | Valor | Observação |
|---|---|---|
| stripped_size_bytes | null | benchmark_ci.sh não existia nesta release |
| cold_start_p95_ms | null | não medido |
| loc_handler | 28 | main.rs completo |

### Suporte Kafka event source
| Framework | Kafka nativo | Observação |
|---|:---:|---|
| serverust | ❌ | não implementado nesta versão (v0.2.0) |
| Rocket | ❌ | HTTP-only; sem suporte Kafka |
| Loco.rs | ❌ | HTTP-only; sem suporte Kafka |
| actix-web | ❌ | HTTP-only; sem suporte Kafka |

---

## v0.1.1 — 2024-10-01

_Patch de manutenção. Métricas não coletadas (release sem tag git — ver CHANGELOG)._

---

## v0.1.2 — 2025-05-01

_Patch de manutenção. Métricas não coletadas (release sem tag git — ver CHANGELOG)._

---

## v0.2.0 — 2026-05-16

### Versões dos concorrentes
| Biblioteca | Versão | Fonte |
|---|---|---|
| Rocket | v0.5.1 | https://github.com/rwf2/Rocket/releases/tag/v0.5.1 |
| Loco.rs | v0.16.3 | https://github.com/loco-rs/loco/releases/tag/v0.16.3 |
| actix-web | v4.9.0 | https://github.com/actix/actix-web/releases/tag/web-v4.9.0 |
| axum | v0.8.x | https://github.com/tokio-rs/axum/releases |
| lambda_runtime | v1.2 | https://github.com/awslabs/aws-lambda-rust-runtime/releases |
| rdkafka | v0.35 | https://github.com/fede1024/rust-rdkafka/releases |

### Números serverust v0.2.0
| Métrica | hello-world | kafka-wallet | Observação |
|---|---|---|---|
| stripped_size_bytes | null* | 19 040 272 B (~18 MB) | *hello-world: release não compilado nesta iteração; SLO < 10 MB mantido por CI gate |
| cold_start_p95_ms | null | null | não medido localmente (cargo-lambda indisponível no ambiente de dev) |
| loc_handler | 13 | 16 | linhas não-vazias excluindo #[cfg(test)]; src/main.rs e src/lib.rs respectivamente |
| quality_gates lint | ✅ | ✅ | clippy --deny warnings verde |
| quality_gates complexity | ✅ | ✅ | complexidade ciclomática verde |
| quality_gates cycles | ✅ | ✅ | sem dependências cíclicas |
| quality_gates coverage | n/a | n/a | coverage rodado nos crates core, não nos exemplos |
| quality_gates mutation | n/a | n/a | mutation testing nos crates core |

### Baseline axum-raw-kafka (vanilla) lado a lado
| Métrica | serverust kafka-wallet | axum-raw-kafka baseline | Ratio |
|---|---|---|---|
| loc_handler | 16 | 64 | **4,0×** menos LOC com serverust |
| stripped_size_bytes | 19 040 272 | null* | *baseline: rdkafka-sys requer libcurl dev no ambiente; build local não concluído |
| cold_start_local_ms | null | null | não medido |

_Ratio LOC calculado pelo script `scripts/benchmark_competitive.sh` — fonte auditável no repo._

### Suporte Kafka event source
| Framework | Kafka nativo | Fonte / Evidência |
|---|:---:|---|
| serverust | ✅ | `serverust-events` crate, macro `#[kafka_consumer]`, `KafkaRecord<T>` |
| Rocket | ❌ | HTTP-only por design; nenhuma issue ou PR de Kafka no tracker (https://github.com/rwf2/Rocket/issues) |
| Loco.rs | ❌ | HTTP-only por design; roadmap público não menciona event sources (https://github.com/loco-rs/loco/issues) |
| actix-web | ❌ | HTTP-only por design; sem planos documentados de event sources (https://github.com/actix/actix-web/issues) |

### Observações desta release
- serverust-events introduz `KafkaRecord<T>` (decode Base64 automático), `KafkaProducer` (opt-in feature `kafka-producer`), macro `#[kafka_consumer]` e `DynamoRepo<T>`.
- O exemplo `kafka-wallet` demonstra o ciclo completo Kafka→DynamoDB→Kafka em 16 LOC de handler.
- O baseline `examples/baselines/axum-raw-kafka` é a implementação vanilla auditável equivalente (64 LOC handler, sem abstrações do framework).
- hello-world HTTP-first preservado: zero deps de Kafka/DynamoDB, CI gate `quality_hello_world.sh` adicionado.
