# Filosofia do serverust

> Última atualização: 2026-09-19

[`vision.md`](vision.md) descreve **o que** o framework é e quais metas ele persegue. Este documento descreve o **porquê** — a tese que orienta as decisões de design e os trade-offs que aceitamos conscientemente.

## A tese

Rust entrega controle de memória e CPU de linguagem de sistema com garantias que o **compilador** verifica: sem `null`, sem uso após liberação, sem data race. O preço histórico disso foi ergonomia — para subir uma API com validação, documentação e injeção de dependência, o desenvolvedor montava tudo à mão, a cada projeto.

A tese do serverust é que **esse preço não é obrigatório**. Dá para ter a segurança e a eficiência do Rust com a experiência de desenvolvimento de FastAPI, NestJS ou Rails. Quem migra de TypeScript ou Python não deveria trocar produtividade por performance — deveria receber as duas.

## Os três compromissos

### 1. Segurança vem da linguagem, não da disciplina do time

Ownership, ausência de `null` e liberdade de data race são garantias verificadas em compile-time — não itens de checklist em code review. Uma classe inteira de incidentes de produção deixa de existir porque o código não compila.

O framework estende essa postura para o que está acima da linguagem: rotas, schema OpenAPI e tabelas de providers são resolvidos em compile-time. Erro de contrato vira erro de build, não alerta às 3h da manhã.

### 2. Baixo nível sem escrever baixo nível

Controle fino de memória e CPU, com ergonomia de linguagem de alto nível. Em serverless isso deixa de ser vaidade técnica e vira conta no fim do mês: o custo em Lambda é **memória provisionada × tempo de execução**, e ambos os fatores caem.

Por isso os invariantes de cold start e tamanho de binário são compromissos públicos com gate em CI (ver [`CLAUDE.md`](../../CLAUDE.md)), não aspirações. Uma feature que os viole exige ADR aprovada.

### 3. DX é requisito, não enfeite

Rotas declarativas, validação automática, OpenAPI gerado dos tipos, DI por construtor, runtime dual HTTP ↔ Lambda: tudo já resolvido, na caixa.

O objetivo é onde a atenção do desenvolvedor é gasta. **O dia deve ser gasto no domínio do problema** — a regra de negócio, o produto — e não em reimplementar o que a linguagem e o framework já deveriam ter resolvido. Ergonomia aqui não é conforto; é para onde o tempo de engenharia vai.

## O corolário: defaults na era dos agentes

Quem escreve a maior parte do código mudou. Hoje uma fração crescente — em muitos times, a maior parte — é escrita por agentes de IA, com revisão humana por amostragem. Isso não substitui os três compromissos acima; muda o **peso** do primeiro, e tem consequência direta sobre como os defaults do framework são escolhidos.

O ponto de partida é como agentes erram. **Eles falham por omissão, não por comissão.** Um agente faz bem o que foi pedido: escreve o handler, escreve o teste, deixa o diff limpo. O que ele omite com frequência é a preocupação transversal que ninguém citou naquela tarefa específica — e segurança quase nunca é citada numa tarefa que pede "um endpoint para exportar faturas".

Disso saem três regras de design:

**1. O default falha fechado.** Sob um default permissivo, a omissão é invisível: o código compila, os testes passam, o diff parece igual ao de todas as outras rotas, e nada no sistema reclama. Sob um default restritivo, a mesma omissão quebra o caminho feliz imediatamente. A diferença é que **um buraco de segurança silencioso vira um bug funcional barulhento** — e bug funcional sempre é corrigido, porque bloqueia o trabalho. Buraco de segurança não é corrigido, porque não bloqueia nada.

**2. Segurança deve ser auditável por presença, não por ausência.** Se a exceção é marcada (`#[public]`), um `grep` devolve a superfície exposta inteira e completa, em segundos, para humano ou agente. Se a proteção é que é marcada, descobrir o que está exposto exige enumerar tudo e verificar a *ausência* de anotação em cada item — e não existe grep para ausência. A diferença é decisiva quando a revisão acontece por trecho, sem o sistema inteiro em contexto.

**3. Política que depende de lembrar degrada com o volume; política que o runtime recusa não degrada.** Conforme sobe a taxa de código novo e cai a fração lida com atenção por alguém que tem o sistema todo na cabeça, qualquer garantia que dependa de disciplina se deteriora. Garantia imposta por tipo, por compilador ou por default restritivo custa o mesmo no primeiro e no milésimo endpoint.

Nada disso é estranho ao Rust — é o idioma da linguagem. Item é privado até se escrever `pub`, binding é imutável até se escrever `mut`, código perigoso exige `unsafe` explícito. Rust é fechado por padrão e aberto por anotação visível, e o framework segue a mesma gramática.

A primeira aplicação concreta é a [ADR 0009](../development/decisions/0009-auth-authz-crate-separada-serverust-auth.md): com autenticação instalada, rota sem anotação é **negada**, e `#[public]` é o `pub` das rotas.

## A prova: um caso real em produção

Um serviço interno de e-mail transacional foi migrado de **NestJS + Node 20** para serverust. Não é brinquedo nem benchmark sintético: são ~2.900 linhas de Rust de produção (mais ~1.200 de teste), rodando em AWS Lambda ARM64, `provided.al2023`, atrás de API Gateway REST v1, com deploy por Serverless Framework.

O serviço expõe uma rota de envio e um receptor de webhook do provedor de e-mail; renderiza HTML com layout e blocos componíveis, mantém um catálogo de templates com schema por template e branding por tenant; e integra SSM (segredo), DynamoDB (estado de entrega) e o provedor externo por HTTP. Binário final: **4,2 MB** zipado.

Os números abaixo vêm de linhas `REPORT` reais do CloudWatch:

| | NestJS / Node 20 (512 MB) | serverust (128 MB) |
|---|---:|---:|
| Init Duration | 830 ms | **105 ms** |
| Caminho frio completo (init + 1º request) | ~1130 ms | **596 ms** |
| Tempo cobrado numa chamada fria | 1660 ms | **597 ms** |
| Memória usada | 141–148 MB | **32 MB** |
| Request quente (rota trivial) | 112–365 ms | **1,4 ms** |

Como o custo é memória × tempo, o resultado foi **~11x mais barato por chamada fria**. E o consumo real de 32 MB permitiu baixar a memória provisionada de 512 MB para 128 MB — uma decisão que ninguém toma sem medir antes.

### A ressalva honesta

O `Init Duration` isolado sugere "8x mais rápido". **Esse número é enganoso e não queremos que ele seja divulgado assim.**

Parte relevante do custo de uma chamada fria acontece *fora* do `Init Duration`: no caso medido, a primeira chamada ao SSM custava ~735 ms de handshake TLS e resolução de credencial, já na fase de invocação. Olhando o caminho frio inteiro, o ganho real é de **~2x** — expressivo, verificável e suficiente. Quem comparar apenas `Init Duration` publica número inflado.

Boa parte do ganho restante também não é mágica do framework, e sim técnica de Lambda: a fase de init recebe um burst de CPU que a fase de invocação não tem, então aquecer conexões (SSM, TLS) dentro do `main()` antes do `run()` move trabalho para onde ele custa menos. No caso medido, isso sozinho levou o caminho frio de ~1106 ms para 596 ms.

Publicar a ressalva junto com o número é parte da filosofia: um framework que se vende por performance perde a credibilidade no dia em que alguém reproduz a medição.

## O que esta filosofia NÃO afirma

- **Não é "Rust é sempre mais rápido".** Para cargas dominadas por I/O de rede, o gargalo é a rede. O ganho aqui está concentrado em cold start, uso de memória e latência de request quente.
- **Não é anti-JavaScript/TypeScript ou anti-Python.** NestJS, Rails e FastAPI são as referências explícitas de DX do projeto — o serverust quer levar essa experiência para outro runtime, não desqualificá-la.
- **Não é "reescreva tudo".** A migração descrita acima só fez sentido porque havia um gate de equivalência: replay de corpus contra o router, comparando resposta byte a byte com a implementação antiga.
- **Não é performance a qualquer custo.** Quando performance e DX colidem, a saída preferida é resolver em compile-time — macro ou builder — em vez de empurrar a complexidade para o usuário.

## Como isso vira decisão de design

| Situação | Como decidimos |
|---|---|
| Feature útil, mas pesa no cold start ou no binário | Vai atrás de feature flag opt-in (Kafka, DynamoDB, rdkafka) |
| Açúcar sintático conveniente | Macro é permitida, mas sempre com builder programático equivalente por baixo |
| Abstração que esconderia o Axum | Rejeitada — `App::axum_router()` continua sendo escape hatch de primeira classe |
| Default de uma feature com efeito de segurança | Falha fechado; a exceção é anotação explícita e auditável, nunca omissão |
| Regressão de invariante público | Exige ADR aprovada em [`../development/decisions/`](../development/decisions/) |

## Referências

- [product/vision.md](vision.md) — o que é, objetivos mensuráveis, princípios de design e não-objetivos
- [CLAUDE.md](../../CLAUDE.md) — invariantes públicos com seus gates de medição
- [product/metrics/](metrics/) — histórico de benchmarks por versão
- [guides/lambda-tutorial.md](../guides/lambda-tutorial.md) — do zero ao deploy em AWS Lambda
