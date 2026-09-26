# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- MAINTENANCE: When bumping workspace.version in Cargo.toml, add a new ## [x.y.z] section
     above [Unreleased] with date YYYY-MM-DD and move relevant [Unreleased] entries there. -->

## Week ending 2026-05-31

> Digest da auditoria semanal (commits `92cdd3a`..`c6c35ab`). Itens de produto acumulam em `[Unreleased]` abaixo.

### Fixed

- `EventRouter` com `RetryPolicy::Exponential`: backoff com `Duration::saturating_mul` e expoente limitado a 31 — sem panic por overflow (#13).

### Changed

- `serverust-auth`: o future do `AuthLayer` passou de `S::Future` para um enum de duas variantes. A variante rápida repassa o future do inner service sem box nem alocação, como antes, e é por onde passa toda requisição que não precise de rebusca; a lenta só é construída quando o verificador diz que vale tentar de novo. Quem usa `JwtAuth` nunca a alcança — `can_revalidate` devolve `false` antes de qualquer alocação acontecer. O `examples/hello-world`, que mede o KPI de cold start, não linka `serverust-auth`: o impacto no gate é zero, verificado.

- `serverust-auth`: nova dependência `tokio` com `default-features = false, features = ["sync"]`, para o mutex assíncrono que serializa as revalidações. É Rust puro, sem runtime e sem I/O; o runtime do tokio já está em qualquer binário serverust por via do core. Também `pin-project-lite`, pelo mesmo motivo do `serverust-core`: o future do layer é um enum e precisa projetar `Pin`.


- `serverust-macros`: **`#[authorize]` passa a exigir posição acima da macro de rota**, como `#[public]` e `#[guard]`. Abaixo não compila mais. A verificação em runtime continuaria valendo naquela posição, mas a rota já teria sido construída quando a macro roda, então os escopos não chegariam ao `security` do OpenAPI — e um documento que descreve como aberta uma rota fechada é pior do que documento nenhum. Mudança de comportamento em relação ao que foi anunciado no `[Unreleased]` anterior; nada publicado a alcança.


- `serverust-cli`: `version.workspace = true` em `Cargo.toml`, alinhado aos demais crates publicáveis (#19).

### Documentation

- SQS v0.3: guia [event-driven.md](docs/guides/event-driven.md), roadmap v0.3 entregue, `INDEX.md` e overview (#16).
- Sync semanal: comportamento de backoff documentado no guia; patches pós-0.3.0 no roadmap (#17).

## [Unreleased]

### Added

- `serverust-auth`: **revalidação do JWKS sob demanda**. Quando chega um token com `kid` desconhecido, o verificador rebusca o JWKS e tenta de novo — a requisição que encontrou a chave nova é atendida, não rejeitada. Fecha o último item em aberto da [ADR 0009](docs/development/decisions/0009-auth-authz-crate-separada-serverust-auth.md), que o [#54](https://github.com/JaimeJunr/serverust/pull/54) tinha declarado fora de escopo.

  **O limite de uma rebusca por minuto é controle de segurança, não afinação.** Este caminho é alcançável por requisição não autenticada — basta inventar um `kid` — e sem limite o serviço vira um amplificador contra o próprio emissor. Com o limite, o pior caso é uma busca por janela, independentemente do volume do ataque. Ajustável por `JwksAuth::revalidation_interval`.

  Requisições concorrentes com o mesmo `kid` desconhecido rendem **uma** busca, não N: o mutex é mantido através da busca, e quem chega depois encontra o trabalho feito. Importa quando a rotação pega um serviço sob carga — que é justamente quando ela acontece.

  Só `unknown_key_id` dispara a rebusca. Token ausente, expirado, de outro emissor ou com assinatura inválida continuariam inválidos depois dela, então tentar seria I/O garantidamente inútil em caminho que qualquer requisição alcança.

  Erro de rebusca nunca vaza para o cliente: se o emissor não responde, a resposta segue `401 unknown_key_id`. `discovery_failed` descreveria um problema de infraestrutura nossa, sobre uma credencial que é do cliente.

  Vale só para `discover` e `from_jwks_uri`, que guardam de onde rebuscar. `from_jwks_json` não tem URL e se comporta exatamente como antes.

- `serverust-auth`: `Verifier` ganha `can_revalidate` e `revalidate`, ambos com default que não revalida nada. Chave estática não muda de comportamento nem paga por esta capacidade.

- `serverust-auth`: `JwksAuth::revalidation_interval` e a constante pública `INTERVALO_MINIMO_DE_REVALIDACAO` (60 s).


- `serverust-core`: **`security` automático no OpenAPI** — etapa 5 e última da [ADR 0009](docs/development/decisions/0009-auth-authz-crate-separada-serverust-auth.md). Com `App::auth(...)` instalado, o `/openapi.json` passa a declarar o esquema `bearerAuth` (`type: http`, `bearerFormat: JWT`) e a exigência de cada operação, então o botão "Authorize" do Scalar e do Swagger UI aparece configurado sozinho.

  A exigência global espelha o default deny — o padrão do serviço é pedir credencial, e cada operação só aparece no documento quando diverge disso. Rota `#[public]` recebe `security: []`, um vetor vazio e não a ausência do campo: ausência herdaria o requisito global e faria o documento afirmar que a rota aberta é protegida. Rota com `#[authorize]` recebe os escopos exigidos.

  **Sem autenticação instalada o documento não fala de segurança**, porque declarar um esquema ali descreveria uma exigência que não existe.

  A derivação parte das mesmas marcações que decidem o comportamento em runtime (`is_public` e os escopos do `#[authorize]`), e há teste conferindo rota a rota que o que o documento declara aberto é exatamente o que responde sem credencial. Um documento que mente sobre segurança é pior do que um documento omisso, porque alguém confia nele.

  **Não foi preciso mexer na trait `Guard`**, ao contrário do que a ADR previa: os escopos viajam da `#[authorize]` para a macro de rota pelo mesmo mecanismo de marcador do `#[public]`. O custo é que um `#[guard]` escrito à mão não aparece no documento — descrevê-lo exigiria uma associated const na trait, mais superfície pública para todo mundo por um caso que já é escape hatch.

- `serverust-core`: `Route` ganha `required_scopes` e o builder `Route::scopes()`, preenchidos pela macro. Declaram o que a rota exige **para o documento**; quem aplica a exigência continua sendo o guard.


- `serverust-auth`: **`JwksAuth`** — verificação com as chaves publicadas pelo emissor, com descoberta de OIDC. Etapa 4 da [ADR 0009](docs/development/decisions/0009-auth-authz-crate-separada-serverust-auth.md).

  ```rust
  let auth = JwksAuth::discover("https://idp.exemplo.com/").await?.audience("minha-api");
  App::new().auth(AuthLayer::<StandardClaims, _>::new(auth))
  ```

  **O construtor é `async` de propósito, e não existe versão síncrona.** É o que põe a ida à rede na fase de init da Lambda, onde há burst de CPU, em vez de na primeira requisição de cada container frio — onde ela viraria latência que some dos testes, porque em teste o container está sempre quente. Não dá para ter o objeto sem ter buscado, então esquecer de aquecer não é um erro que se possa cometer.

  Decisões de segurança embutidas: o `issuer` do documento de descoberta é aplicado como emissor esperado (validação de `iss` ligada sem pedir) e precisa bater com a URL usada para chegar nele (RFC 8414 §3.3); **o algoritmo vem da chave, nunca do header do token**; chave simétrica (`oct`) no JWKS é recusada na construção, porque JWKS é documento público e uma chave simétrica ali é o próprio segredo de assinatura; `http://` sem TLS é recusado fora de loopback; redirecionamentos são desligados, para que a URL verificada seja a URL buscada.

  Dois códigos novos em `AuthError`: `unknown_key_id` (o `kid` do token não está no JWKS carregado — tipicamente rotação) e `discovery_failed` (só na inicialização). `unknown_key_id` é distinto de `invalid_token` para ser alertável: um pico dele significa rotação, e quem precisa agir é o operador.

- `serverust-auth`: feature **`jwks`**, desligada por default, com o cliente HTTP e a pilha TLS da descoberta. Quem usa chave estática não paga isso em binário nem em cold start. `JwksAuth::from_jwks_json` fica disponível **sem** a feature, para quem prefere buscar o JWKS com o próprio cliente.

- `serverust-auth`: trait **`Verifier`**, o que o `AuthLayer` consome. `JwtAuth` e `JwksAuth` a implementam, e nada impede uma terceira implementação escrita pelo usuário. A trait é síncrona de propósito: todo o I/O acontece na construção, então o layer continua repassando o future do inner service sem box nem alocação no caminho quente.

### Changed

- Release: o `release-plz` passa a seguir a versão única do workspace (tag `vX.Y.Z`, todos os crates `serverust-*` no mesmo `version_group`, incluindo o `serverust-auth`) e só publica a partir de Release PR mergeada (`release_always = false`). Antes, o job `release` publicava qualquer crate cuja versão local faltasse no crates.io — tentou subir o `serverust-auth` sem PR e só não subiu porque não compilou. O workflow agora exige o secret `RELEASE_PLZ_TOKEN` e reprova sem ele, em vez de usar o `GITHUB_TOKEN`, que não pode abrir a PR nem dispara os checks exigidos do `main`.

- `serverust-auth`: `AuthLayer<C>` virou `AuthLayer<C, V = JwtAuth>`, genérico sobre o verificador. O default mantém `AuthLayer::<C>` significando o que sempre significou, então nenhum código existente quebra; com outro verificador, use `AuthLayer::<C, _>::new(auth)` e deixe a inferência resolver.

- **CI**: `serverust-auth` com a feature `jwks` entrou nas combinações extras da matriz. Sem isso os testes de JWKS não rodariam no CI — exatamente o buraco que a matriz derivada fechou para crates, aplicado a uma feature.


- `serverust-auth`: a trait `AuthzFacts` **mudou de casa** para `serverust-core` e é reexportada por `serverust-auth`, então `use serverust_auth::AuthzFacts` segue funcionando e nenhum código de usuário quebra. O motivo é de contrato, não de organização: `AuthzFacts` não toca cripto, e `#[authorize]` precisa gerar código contra ela sem arrastar o crate de autenticação para dentro de quem só usa a macro. O core continua sem dependência de cripto.

- `serverust-core`: `Route` ganha o campo público `is_public`. `Route::new()` mantém a assinatura e o default `false`, então as macros de rota e todo código que constrói por `new()` seguem compilando — mas **construção por struct literal (`Route { .. }`) quebra** e precisa passar a usar `new()`. Mudança de API pública, aceitável em `0.x` e mitigada por `Route` ser quase sempre gerada por macro.
- `serverust-core`: nova dependência `pin-project-lite`. O future do `AuthGate` é um enum (seguiu / negado) e precisa projetar `Pin`. A alternativa sem dependência nova seria `axum::middleware::from_fn`, que boxa um future por requisição em toda rota não-pública — inaceitável num crate cujo invariante é request quente na casa de 1 ms. `pin-project-lite` é `macro_rules` puro, sem dependências transitivas e sem custo de runtime, e já era usado por `serverust-events`.
- Testes de integração das macros (`trybuild` + `kafka_consumer_runtime`) saíram de `serverust-macros` para o crate interno `serverust-macros-tests` (`publish = false`). `serverust-macros` deixa de ter dev-deps em `serverust-core`/`serverust-events`/`serverust-telemetry`, quebrando os 3 ciclos de dependência que o `cargo-cycles` detectava. Os ~18 testes de core/events/telemetry que usam as macros continuam onde estão.

### Added

- **CI**: a matriz de testes passa a ser derivada do workspace ([`scripts/ci_test_matrix.sh`](scripts/ci_test_matrix.sh)) em vez de escrita à mão em `tests.yml`.

  Escrita à mão, ela era uma allowlist por presença: crate novo que ninguém lembrasse de adicionar não reprovava — sumia. Foi o que aconteceu com o `serverust-auth` (65 testes, incluindo todos os de default deny), o `funds-api` e o `todo-api` (13 testes): nunca rodaram no CI, porque faltava uma linha de YAML que nenhum `grep` procura. É a regra 3 do corolário da filosofia — *não dependa de lembrar* — aplicada à própria pipeline.

  O que sobrou de decisão humana são duas listas no gerador, escolhidas para que nenhuma consiga esconder um crate: esquecer uma dispensa em `SEM_TESTES` faz o `nextest` reprovar por 0 testes, e esquecer uma combinação de features reduz cobertura sem tirar o crate da matriz. A necessidade de toolchain de C (librdkafka) é derivada das dependências reais, não de uma lista.

  [`scripts/test_ci_test_matrix.sh`](scripts/test_ci_test_matrix.sh) guarda as invariantes no próprio CI, inclusive a dispensa obsoleta — um crate que ganhe testes e continue em `SEM_TESTES` seria de novo o verde vazio.

- `serverust-core`: **`App::auth(layer)`** e o log de inicialização que lista as rotas públicas — mitigação 2 da decisão 5 da [ADR 0009](docs/development/decisions/0009-auth-authz-crate-separada-serverust-auth.md). É o mesmo `App::layer`, com duas diferenças: o nome, porque essa linha é a decisão mais consequente do serviço e esconder isso num ponto de extensão genérico não ajuda quem lê o `main`; e o log, que imprime no stderr a superfície anônima a cada boot.

  Lista o que é **aberto**, nunca o que é protegido: a lista curta é a que se lê, e inverter produziria um log do tamanho do serviço. `App::public_routes()` expõe o mesmo conteúdo, para afirmar a superfície anônima num teste em vez de confiar na leitura do log — e há teste conferindo que o inventário bate com o que de fato responde sem token, porque declaração de segurança que ninguém verifica é a categoria de problema que a ADR inteira trata.

  `.layer(AuthLayer::new(...))` continua ativando o default deny; quem faz isso são os marcadores que o layer insere, não o método. O que se perde é a lista no boot. Sem autenticação instalada nada é impresso.

- `serverust-core`: **`App::allow_unannotated()`**, o escape hatch de migração da decisão 5. Desliga o default deny para que um serviço existente seja migrado rota a rota, em vez de marcar dezenas de `#[public]` num único PR — que é onde o erro entra. É uma linha visível no builder em vez de omissão distribuída, e o log de init a denuncia em voz alta a cada boot.

  Não desarma `#[authorize]`: afrouxar o default não é abrir mão da permissão que alguém pediu de propósito. Implementado como marcador de request (`AllowUnannotated`), e não como flag de montagem, para que a ordem do builder continue irrelevante.

- `serverust-macros`: macro **`#[public]`**, a anotação que abre uma rota declarada por macro. É a exceção explícita ao default deny da [ADR 0009](docs/development/decisions/0009-auth-authz-crate-separada-serverust-auth.md) — o `pub` do Rust, para rotas — e fecha o vão em que só a via programática `Route::public()` conseguia declarar uma rota aberta.

  Vem **acima** da macro de rota, como `#[guard]`. Abaixo ela não teria efeito, e em vez de ser ignorada em silêncio **não compila**: `#[public]` deixa um marcador que a macro de rota consome, e o marcador é ele próprio um erro de compilação se ninguém o consumir. Uma rota que se diz pública sem ser é exatamente a falha que o default deny existe para evitar — anunciá-la e não aplicá-la seria pior do que não ter a anotação.

- `serverust-macros`: macro **`#[authorize(scope = "...", role = "...")]`**, autorização por escopo e papel sobre os fatos que o crate de autenticação publica nas extensions. `scope` e `role` são repetíveis e conjuntivos (AND); empilhar `#[authorize]` também conjunta. Sem identidade na requisição a resposta é 401 `authentication_required` — **inclusive em rota `#[public]` e inclusive sem `AuthLayer` instalado**, porque "pedi permissão e não tenho de onde lê-la" não pode resultar em acesso. Com identidade e sem a permissão, 403 `insufficient_scope` (RFC 6750 §3.1).

  `#[public]` e `#[authorize]` na mesma rota **não compilam**, nas três ordens possíveis de atributo: `#[authorize]` já nega sem identidade, então o `#[public]` não abriria nada — só faria a rota aparecer na auditoria de endpoints anônimos sem ser um. Era o que a Emenda 1 da ADR pedia.

  Exigência alternativa (`any_of`) ficou de fora de propósito: um "qualquer um destes" ambíguo é o tipo de default que a filosofia do projeto pede para não existir. O caso genuinamente alternativo cabe num `#[guard]` escrito à mão.

- `serverust-core`: portão de rota que implementa o **default deny** da [ADR 0009, Emenda 1](docs/development/decisions/0009-auth-authz-crate-separada-serverust-auth.md). `Route` ganha o flag `is_public` com o builder `Route::public()`, e `App::route()` embrulha com o novo `AuthGate` toda rota que não seja pública. Os marcadores `AuthEnabled` e `Authenticated` formam o contrato que uma implementação de autenticação insere nas extensions — o core não ganha dependência de cripto, e qualquer implementação, inclusive uma escrita pelo usuário, herda o default deny inserindo os mesmos marcadores.

  O portão é **inerte enquanto não houver autenticação instalada**: sem `AuthEnabled` nas extensions ele deixa tudo passar, então quem não usa autenticação não muda de comportamento, e a ordem do builder é irrelevante. A rejeição é 401 com `WWW-Authenticate: Bearer` (RFC 6750 §3) e `reason: "authentication_required"`. A dica que ensina a anotar a rota aparece **só em build de debug** — em release seria vazamento de detalhe interno para quem chama a API.

  O `serverust-auth` passa a inserir esses marcadores, o que **ativa o default deny de fato**: instalar o `AuthLayer` protege toda rota não marcada como pública, inclusive as que não pedem `Auth<C>` na assinatura. O motivo preciso da falha atravessa a rejeição via `AuthFailure`, então uma rota protegida só pelo portão ainda devolve `token_expired` ou `invalid_issuer` em vez de um genérico — a distinção importa, porque diz ao cliente se deve renovar ou reautenticar.

  Custo medido: binário stripped de `hello-world` vai de 3 550 128 para 3 585 888 bytes (**+35 KB, +1,0%**), dentro do gate estrito de 5% e a 34% do limite de 10 MB. Startup local não regrediu.

- Novo crate `serverust-auth`: verificação stateless de JWT emitido por IdP externo (Cognito, Auth0, Clerk, Keycloak, Logto), primeira parcela da [ADR 0009](docs/development/decisions/0009-auth-authz-crate-separada-serverust-auth.md). Inclui `JwtAuth` com chave estática (`hs256`, `rs256_pem`, `es256_pem`) e os ajustes `.issuer()`, `.audience()` e `.leeway()`; `AuthLayer<C>`, que valida o token uma única vez e deposita as claims nas extensions sem jamais rejeitar a requisição; os extractors `Auth<C>` (exige identidade, 401 sem ela) e `MaybeAuth<C>` (opcional); e `StandardClaims`, cobrindo os formatos comuns de OAuth 2.0/OIDC. O algoritmo é fixado no construtor e não lido do header, fechando o ataque de confusão de algoritmo. O backend de cripto é Rust puro (feature `rust_crypto` do `jsonwebtoken`), escolhido para não exigir cmake e não quebrar a cross-compilação x86_64 → aarch64. `serverust-core` continua sem dependência de cripto: quem não usa auth não paga nada.

  Ainda **não** implementados, e previstos para incrementos seguintes da mesma ADR: descoberta de JWKS/OIDC com a busca aquecida na fase de init, `#[authorize(scope = "...")]`, a macro `#[public]` e o `security` automático no OpenAPI. Enquanto a macro não existe, declarar rota aberta exige a via programática `Route::public()`.

### Documentation

- Guia [auth.md](docs/guides/auth.md): setup da dependência, construção do `JwtAuth`, instalação do `AuthLayer` via `App::layer`, handlers com `Auth`/`MaybeAuth`, como implementar `AuthzFacts` num tipo de claims próprio, tabela dos códigos `reason` devolvidos no 401 e a lista do que ainda não está implementado.
- [ADR 0009, Emenda 1](docs/development/decisions/0009-auth-authz-crate-separada-serverust-auth.md#emenda-1-2026-09-19--mecanismo-do-default-deny) (Accepted): fixa o **mecanismo** do default deny, que a decisão original deixou em aberto. Um layer global não distingue rota pública de rota esquecida, porque não sabe qual rota casou. A rota passa a carregar a própria classificação (`Route::public()`), e `App::route()` embrulha com um `AuthGate` só o que não é público — portão inerte enquanto não houver autenticação instalada, o que torna a ordem do builder irrelevante. O contrato (dois tipos-marcador + o `AuthGate`, sem dependência nova) fica em `serverust-core`, para que qualquer implementação de autenticação herde o default deny. Registra também o que o mecanismo **não** resolve: segue sendo runtime e não compile-time, e `App::axum_router()` o contorna.
- [ADR 0009](docs/development/decisions/0009-auth-authz-crate-separada-serverust-auth.md) (Accepted): auth/authz em crate separada `serverust-auth` — validação de JWT de IdP externo, RBAC/scopes em compile-time, JWKS aquecido na fase de init por construção da API, cripto em Rust puro e **default deny** (rota sem anotação é negada; `#[public]` é a exceção explícita).
- Filosofia: novo corolário [defaults na era dos agentes](docs/product/philosophy.md) — falhe fechado, torne a exceção auditável por presença e não dependa de lembrar. Regra espelhada no `CLAUDE.md`.
- Filosofia do projeto em [`docs/product/philosophy.md`](docs/product/philosophy.md): segurança pela linguagem, baixo nível sem escrever baixo nível e DX como requisito — com o caso real de migração NestJS → serverust em produção (Lambda ARM64), os números medidos e a ressalva de como lê-los (#40). Resumo no `README.md`, critério de trade-off e regra de divulgação de performance no `CLAUDE.md`, links em `INDEX.md` e `vision.md`.

## [0.4.2] - 2026-09-14

Duas adições à API pública de `serverust-core`, ambas **compatíveis** — nada muda para quem já usa a 0.4.x. O restante é infraestrutura do repositório e não alcança quem consome os crates.

Versão de patch, não minor, apesar de `feat`: em `0.x` o Cargo trata `0.5.0` como incompatível com `0.4.x`, e uma 0.5.0 exigiria que cada consumidor editasse o `Cargo.toml` para receber uma mudança que é puramente aditiva. Com `0.4.2`, quem declara `serverust-core = "0.4"` recebe automaticamente.

### Added

- `serverust-core`: `App::layer(...)` para aplicar qualquer `tower::Layer` genérico (ex.: `axum::extract::DefaultBodyLimit`, CORS, timeout, compressão) sobre as rotas do usuário, sem precisar implementar a trait `Interceptor` nem espelhar o `run()` do framework à mão (#39).
- `serverust-core`: `App::without_docs()` desabilita o registro de `/openapi.json`, `/docs` e `/redoc` em `into_router()`, para serviços internos que não querem expor essa superfície (#39).

### Changed

- `scripts/quality_kpi_gate.sh`: o eixo de startup local deixa de reprovar por comparação com o baseline e passa a informativo, com guarda absoluta em 2000 ms. Medições isoladas da mesma build, sem alteração de código, variaram de 11 ms a 62 ms conforme a carga da máquina — a tolerância de 20% sobre um baseline de 11 ms reprovava ruído. `stripped_bytes` continua gate estrito em 5%. Ver ADR 0008.
- `scripts/benchmark_ci.sh` e `scripts/metrics_append.sh`: o startup passa a ser a mediana de 5 medições (`STARTUP_SAMPLES`) em vez de uma amostra única, que deixava o baseline refém de onde caiu na distribuição.
- `docs/product/metrics/history.json`: o campo medido passa a se chamar `startup_local_p50_ms`, acompanhado de `startup_local_samples`. O que o script mede é o tempo até a primeira resposta HTTP de um binário local — não cold start de Lambda. `cold_start_p95_ms` permanece no schema como o campo do invariante público e fica `null` enquanto não houver invocação real na AWS. Leitores aceitam o nome antigo como fallback.

## [0.4.1] - 2026-09-13

Release de manutenção: atualiza duas dependências com advisory do RUSTSEC no `Cargo.lock`. **Sem mudança de API** — quem usa os crates como biblioteca pode pular.

**Quem deve atualizar:** quem instala o CLI com `cargo install serverust-cli --locked`, já que esse fluxo usa o `Cargo.lock` publicado no pacote. Quem depende dos crates como biblioteca resolve as próprias dependências e já pegava as versões corrigidas, por serem semver-compatíveis.

### Changed

- `serverust-events`: `handle_sqs_event` e `flush` decompostos em funções menores (complexidade cognitiva 32→6 e 27→3). Refactor interno, sem mudança de API nem de comportamento — os testes existentes passaram sem alteração.

### Security

- `Cargo.lock`: `h2` 0.4.14 → 0.4.19 (RUSTSEC-2026-0258, DoS por DATA frames vazios sem limite) e `anyhow` 1.0.102 → 1.0.104 (RUSTSEC-2026-0190, unsoundness em `Error::downcast_mut()`). Consumidores das bibliotecas não eram afetados: o `Cargo.lock` não participa da resolução de dependências de quem depende dos crates, e ambos os fixes são semver-compatíveis. O impacto real era em quem builda este repositório e em quem instala o CLI com `cargo install serverust-cli --locked`, fluxo que usa o `Cargo.lock` publicado no pacote.
- `deny.toml`: dois advisories sem ação possível deste lado passam a ter ignore documentado, em vez de deixar `cargo deny` vermelho mascarando achado novo — `h2` 0.3.27 (sem patch na linha 0.3.x, chega só pelo cliente HTTP do AWS SDK) e `proc-macro-error2` (unmaintained, proc-macro de build-time via `validator_derive`).

## [0.4.0] - 2026-09-13

`serverust-telemetry 0.4.0` traz uma **quebra de API** no `IdempotencyStore` e dois fixes que **mudam o comportamento em runtime sem quebrar a compilação** — leia "Migração" antes de subir em produção.

### Migração desde 0.3.x

**1. `IdempotencyStore` agora exige token de fencing.** Só afeta quem implementa o trait ou faz `match` em `AcquireOutcome` diretamente; quem só monta o `IdempotencyLayer` com `InMemoryIdempotencyStore` / `DynamoDbIdempotencyStore` não precisa mudar nada.

```rust
// antes
match store.try_acquire(&key, now, ttl).await? {
    AcquireOutcome::Acquired => { /* ... */ store.complete(&key, now, ttl).await?; }
    // ...
}

// depois — o token identifica o dono do lock
match store.try_acquire(&key, now, ttl).await? {
    AcquireOutcome::Acquired(token) => { /* ... */ store.complete(&key, &token, now, ttl).await?; }
    // ...
}
```

Quem implementa o trait: `release`/`complete` devem virar no-op de sucesso quando o token não bate com o do registro corrente — é o que impede um worker cujo TTL expirou de apagar o lock de outro. Na tabela DynamoDB isso vira uma `condition_expression` com estado **e** token; nenhuma migração de schema é necessária (o atributo `token` passa a ser gravado nos registros novos).

**2. Mudanças de comportamento em produção** — nada a alterar no código, mas o sistema passa a agir diferente:

| Cenário | 0.3.x | 0.4.0 |
|---|---|---|
| Handler falha com `IdempotencyLayer` ativo | Lock `InProgress` ficava até o TTL (24h por padrão); redeliveries do SQS não reexecutavam o handler e a mensagem ia para a DLQ sem nunca ser processada | Lock é liberado; a próxima redelivery reexecuta o handler |
| `EventRouter::with_dlq`, publish na DLQ bem-sucedido | Retornava `Err`, a Lambda não removia a mensagem da fila — loop de redelivery com escrita duplicada na DLQ | Retorna `Ok(())`, a mensagem original recebe ack (mesma semântica do `DlqLayer`) |

O efeito prático do primeiro é **mais reprocessamento** de mensagens que antes ficavam presas: se o handler não for idempotente por conta própria além do lock, verifique isso antes de subir. O do segundo é **menos escrita duplicada na DLQ**.

**3. `tracing` virou dependência não-opcional de `serverust-events`.** Antes vinha só com a feature `sqs`. Nenhuma ação necessária — apenas note o acréscimo na árvore de dependências se você audita footprint.

**4. Tópico Kafka sem handler continua sendo ignorado** (agora com `tracing::warn!` em vez de silêncio). Se preferir que isso falhe, é opt-in: `.with_unhandled_topic_policy(UnhandledTopicPolicy::Error)`.

### Added

- `UnhandledTopicPolicy` em `LambdaBroker` e `KafkaBroker` (`with_unhandled_topic_policy`): `WarnAndIgnore` (default) preserva o comportamento 0.3.x de pular o record sem handler e passa a emitir `tracing::warn!` com o tópico; `Error` retorna `BrokerError::Subscribe` com o tópico recebido e a lista de tópicos inscritos.

### Changed

- `serverust-events`: `tracing` deixa de ser dependência opcional (antes só sob a feature `sqs`) — `UnhandledTopicPolicy` e o log de falha da DLQ em `EventRouter` rodam em código sem a feature `sqs`.
- CI e desenvolvimento local passam a usar toolchain Rust pinada em `rust-toolchain.toml` (1.94.1) em vez de `stable` flutuante — os testes `trybuild` de `serverust-macros` comparam a saída literal do rustc e quebravam a cada mudança de formatação de diagnóstico.
- `serverust-cli`: passa a usar `version.workspace = true` em `Cargo.toml`, herdando `workspace.package.version` como os demais crates publicáveis (evita drift de versão do binário `serverust`).
- **BREAKING** (`serverust-telemetry`): `IdempotencyStore::try_acquire` devolve `AcquireOutcome::Acquired(LockToken)` em vez de `Acquired`, e `release`/`complete` passam a exigir o token da aquisição (fencing). Token divergente é no-op de sucesso. Implementações externas de `IdempotencyStore` e qualquer `match` sobre `AcquireOutcome::Acquired` precisam ser ajustados.

### Fixed

- Hooks `machete` e `cog-verify` no lefthook deixavam de bloquear quando a ferramenta existia e reprovava (deps não usadas / mensagem de commit inválida); só a ausência da ferramenta deve ser tolerada.
- `EventRouter` com `RetryPolicy::Exponential`: o atraso `base_delay * 2^n` passa a usar `Duration::saturating_mul` e expoente limitado a 31, evitando panic por overflow de `Duration` em retentativas longas ou `base_delay` grande.
- `SqsBroker::handle_sqs_event` (Lambda ESM + `ReportBatchItemFailures`): mensagens sem handler para a fila do ARN ou sem `event_source_arn` válido passam a entrar em `batchItemFailures` quando há `messageId`, em vez de serem tratadas como sucesso implícito (a Lambda removia da fila sem processamento).
- `IdempotencyLayer`: após falha do handler ou de `complete()`, libera o lock `InProgress` via `IdempotencyStore::release`, permitindo que redeliveries do SQS reexecutem o handler dentro do TTL (antes o lock bloqueava reprocessamento por até 24h e a mensagem ia para DLQ sem nova tentativa). `release`/`complete` só mutam o registro se o token bater com o dono corrente — um owner cujo TTL expirou não apaga nem completa o lock de outro worker. Falha de `release` é logada com `tracing::warn` (chave + erro), sem mascarar o erro do handler. Falha de `complete` após sucesso do handler propaga erro ao SQS e libera o lock.
- `EventRouter::with_dlq`: após publicação bem-sucedida no tópico DLQ, o wrapper retorna `Ok(())` (mesma semântica de `DlqLayer`), permitindo ack da mensagem original no Lambda SQS em vez de loop infinito de redelivery. Se o publish na DLQ falhar, o erro original do handler é retornado e ambos os erros (handler e DLQ) são registrados com `tracing::error!`.

## [0.3.0] - 2026-05-17

`serverust-events 0.3.0` — SqsBroker maduro: Lambda ESM + Standalone worker, FIFO type-safe, Tower pipeline, idempotency, DLQ declarativo, transport abstraction SQS↔Kafka, AsyncAPI, EMF, X-Ray e CLI inspector. 14 user stories (US-001..US-014) entregues.

### Added
- `SqsBroker` em `serverust-events/src/sqs/consumer.rs` (feature `sqs`) — consumer Lambda ESM com partial batch failure automático via `SqsBatchResponse.batchItemFailures` (US-001)
- Macro `#[subscriber(driver = "sqs", queue = "...")]` em `serverust-macros` — mesma macro suporta `driver = "kafka"` e `driver = "sqs"` sem alterar a lógica do handler (US-001, US-011)
- Extractors estilo Axum para SQS em `serverust-events/src/sqs/extract.rs` — `Json<T>`, `State<S>`, `SqsMetadata` (`message_id`, `receipt_handle`, `attributes`, `system_attributes`) (US-002)
- `DeleteManager` em `serverust-events/src/sqs/delete.rs` — agrupa `DeleteMessageBatch` no standalone worker; em Lambda ESM o ack/nack é controlado por `batchItemFailures` (US-003)
- `SqsProducer` em `serverust-events/src/sqs/producer.rs` — batching transparente (até 10 msgs / 200ms linger, configurável), retry exponencial em partial failure, graceful shutdown com flush (US-004)
- `SqsFifoMetadata` extractor expondo `message_group_id`, `message_deduplication_id`, `sequence_number` para subscribers FIFO (US-005)
- `SqsFifoProducer` com `FifoSendBuilder` type-state (`NoGroupId` → `HasGroupId`) — `send()` só compila após `.message_group_id(...)`, eliminando erros runtime de FIFO inválido (US-005)
- `#[subscriber(driver = "sqs", queue = "...", fifo)]` valida em compile-time que o handler declara `SqsFifoMetadata` (US-005)
- `SqsSubscriber` implementa `tower::Service<SqsMessage>` em `serverust-events/src/sqs/subscriber.rs` — pipeline `TracingLayer → MetricsLayer → IdempotencyLayer → RetryLayer → handler` reaproveita `serverust-telemetry` (US-006)
- `IdempotencyLayer` em `serverust-events/src/sqs/layers.rs` (feature `sqs`) — at-least-once → effectively-once com `IdempotencyStore` (in-memory + DynamoDB), protocolo InProgress/Completed + TTL configurável default 24h (US-007)
- `RetryLayer` + `DlqLayer` declarativos via macro `#[subscriber(retry = exponential(max = 5, base = "100ms"), dlq = "orders-dlq")]` — política em metadata, código de negócio limpo (US-008)
- `HeartbeatLayer` em `serverust-events/src/sqs/heartbeat.rs` — `ChangeMessageVisibility` automático em background quando 30% do timeout resta; ativo por default no standalone, opt-in em Lambda ESM (US-009)
- `StandaloneSqsBroker` em `serverust-events/src/sqs/standalone.rs` — long-poll worker para ECS/EC2/bare-metal, concorrência configurável, graceful shutdown drenando in-flight, backoff exponencial em fila vazia. Mesma macro `#[subscriber]` funciona em Lambda ESM e standalone (US-010)
- Transport abstraction — `#[subscriber(driver = "kafka|sqs")]` no mesmo handler; brokers heterogêneos no mesmo app via `EventRouter::attach`; example `examples/transport-swap` (US-011)
- Observability EMF + X-Ray automáticos: métricas `messages_received`, `processing_duration`, `partial_failures`, `dlq_routed`, `idempotency_hits` por queue/handler; span por mensagem com `AWSTraceHeader` propagado outbound pelo producer (US-012)
- Flag opt-in `asyncapi` em `#[subscriber(...)]` — emite método associado `register_asyncapi(builder)` que adiciona `receive` (e `send` se `#[publisher]` empilhado) no `AsyncApiBuilder`; `HAS_ASYNCAPI: bool` exposto (US-013)
- `serverust-events::asyncapi::emit_asyncapi_if_requested(builder, args)` — detecta `--serverust-emit-asyncapi <path>` em `args` e grava spec YAML; integra com `serverust info --asyncapi` (US-013)
- `serverust queue inspect/tail` em `serverust-cli` — lista subscribers/publishers declarados, valida queues + permissões IAM + DLQ stats; saída tabela humana ou `--json` (US-014)

### Changed
- `EventRouter::attach` aceita qualquer `impl Broker` (não só `KafkaBroker`)
- Feature `aws_lambda_events/sqs` ativada pela feature flag `sqs` no `serverust-events`

### Fixed
- Structured logging consistente: substituído `eprintln!` por `tracing::{warn, error}` em `consumer.rs`, `producer.rs`, `layers.rs` — evita vazamento ad-hoc em CloudWatch
- Graceful degradation em `serde_json::to_vec(SqsMessage)` — falha de alocação não causa panic; metadata header omitido e `SqsMetadata` extractor falha com erro claro
- `IdempotencyLayer` agora emite `tracing::warn` quando bypassa por `message_id` vazio (era silencioso)

### Preserved
- `serverust-core` continua sem deps de SQS (invariante CLAUDE.md verificada via `cargo tree`)
- `examples/hello-world` sem dep transitiva de SQS
- Cold start ARM64 128MB < 50ms p95 mantido (feature `sqs` é opt-in)

## [0.2.0] - 2026-05-16

### Added
- Trait `EventHandler<E>` em `serverust-core` paralela ao Router HTTP (US-001)
- Dispatcher multi-trigger em `serverust-lambda` detectando HTTP vs Event automaticamente (US-002)
- Nova crate `serverust-events` com extractor `KafkaRecord<T>` (US-003)
- Macro `#[kafka_consumer(topic, group)]` em `serverust-macros` (US-004)
- `KafkaProducer` injetável atrás de feature `kafka-producer` opt-in (US-005)
- `DynamoRepo<T>` repository pattern + macro `#[dynamo_table]` (US-006)
- Exemplo `examples/kafka-wallet` end-to-end Kafka→Dynamo→Kafka (US-007)
- Baseline competitivo `examples/baselines/axum-raw-kafka` (US-009)
- `CHANGELOG.md` versionado (Keep a Changelog 1.1.0) + gate `quality_changelog.sh` (US-013)
- `CLAUDE.md` na raiz + 5 ADRs MADR em `docs/development/decisions/` (US-015)
- `docs/development/for-ai-agents.md` — guia máquina-legível (US-017)
- `docs/product/metrics/history.json` + schema + scripts `metrics_append.sh`/`metrics_regression_check.sh` (US-014)
- Gate `scripts/quality_kpi_gate.sh` no pre-push (US-016)
- Análise competitiva de `actix-web` em `docs/product/competitors/actix.md` (US-018)
- Entrada v0.2.0 em `release-competitive-log.md` com números reais + baseline (US-010)
- Tabelas competitivas em README + `rocket.md`/`loco.md`/`actix.md` (US-011)
- `docs/development/release-checklist.md` + issue template GitHub com itens `required:true` (US-012)

### Changed
- Renomeado `AGENTS.md` → `CLAUDE.md` (projeto usa Claude Code, não Codex)
- Tabela comparativa do README inclui colunas Rocket / Loco / actix-web

### Preserved (não-regressão)
- `examples/hello-world` mantém SLOs históricos (< 10 MB stripped, < 2000 ms cold start) (US-008)
- Pitch HTTP-first intacto: tudo event-driven em crate opt-in `serverust-events`

## [0.1.2] - 2026-05-16

### Added
- IaC compatibility contract for Serverless Framework, SST and Terraform (`docs/guides/iac-compatibility.md`)
- Release checklist, competitive log and issue template (`docs/product/competitors/release-competitive-log.md`)

### Changed
- Pre-push lefthook hooks scoped to `serverust-core` only (coverage + mutation)
- Quality gates added to pre-commit: lint, complexity, cycle detection, formatting

### Fixed
- CLI scaffold templates now reference crates.io instead of local path
- Friendly CLI message when `cargo-watch` or `cargo-lambda` are missing

## [0.1.1] - 2026-05-14

### Added
- Branding: Ferris 🦀 mascot, startup feedback and first-compilation output

## [0.1.0] - 2026-05-12

### Added
- Cargo workspace with crates: `serverust-core`, `serverust-macros`, `serverust-lambda`, `serverust-cli`, `serverust-telemetry`
- HTTP routing via declarative macros (`#[get]`, `#[post]`, `#[put]`, `#[delete]`)
- App builder and Lambda/HTTP dual-runtime with auto-detection
- Dependency injection via builder pattern
- OpenAPI automatic generation with utoipa + Swagger UI
- Request validation with `#[derive(Validate)]` and standardised error shapes
- Guards, Pipes and Interceptors middleware
- AWS Powertools telemetry: structured logger, tracing and metrics
- CLI (`serverust-cli`): `new`, `generate`, `dev`, `build`, `deploy`, `info`, `openapi` commands
- Configuration via `rustapi.toml` with figment
- Examples: `hello-world`, `funds-api`, `todo-api`
- Essential rustdoc on all public APIs
- MIT OR Apache-2.0 dual license

[Unreleased]: https://github.com/JaimeJunr/serverust/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/JaimeJunr/serverust/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/JaimeJunr/serverust/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/JaimeJunr/serverust/releases/tag/v0.1.0
