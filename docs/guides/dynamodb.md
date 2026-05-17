# Guia DynamoDB com serverust

Este guia mostra como integrar o `DynamoRepo<T>` e a macro `#[dynamo_table]` no seu projeto serverust.

## Pré-requisitos

- Projeto serverust com `serverust-telemetry` e `serverust-macros` como dependências.
- Credenciais AWS configuradas (ver seção [Credenciais](#credenciais)).

## Setup de dependências

Adicione `serverust-telemetry` com a feature `dynamodb` no seu `Cargo.toml`:

```toml
[dependencies]
serverust-telemetry = { version = "0.2", features = ["dynamodb"] }
serverust-macros = { version = "0.2" }
serde = { version = "1", features = ["derive"] }
```

A feature `dynamodb` ativa `aws-sdk-dynamodb` e `aws-config` — elas ficam **fora do build padrão** para preservar o binário enxuto (< 10 MB) exigido pelo invariante de cold start.

## Declarando uma tabela

Use `#[dynamo_table]` para associar uma struct à tabela DynamoDB:

```rust
use serverust_macros::dynamo_table;
use serde::{Serialize, Deserialize};

// Tabela com apenas partition key
#[dynamo_table("Products", pk = "product_id")]
#[derive(Debug, Serialize, Deserialize)]
pub struct Product {
    pub product_id: String,
    pub name: String,
    pub price: f64,
}

// Tabela com partition key + sort key
#[dynamo_table("Orders", pk = "user_id", sk = "order_id")]
#[derive(Debug, Serialize, Deserialize)]
pub struct Order {
    pub user_id: String,
    pub order_id: String,
    pub total: f64,
    pub status: String,
}
```

A macro implementa a trait `DynamoTable` definindo `TABLE_NAME`, `PK_FIELD` e (opcionalmente) `SK_FIELD`.

## Inicializando o cliente DynamoDB

Em ambiente Lambda, use `aws-config` para construir o cliente a partir das credenciais do ambiente:

```rust
use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::Client;
use serverust_telemetry::dynamo::DynamoRepo;
use std::sync::Arc;

async fn build_repo() -> Arc<DynamoRepo<Product>> {
    let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
    let client = Client::new(&config);
    Arc::new(DynamoRepo::new(client))
}
```

### Warm-start seguro em Lambda

Use `OnceLock` para inicializar o repo apenas uma vez:

```rust
use std::sync::{Arc, OnceLock};

static REPO: OnceLock<Arc<DynamoRepo<Product>>> = OnceLock::new();

async fn get_repo() -> &'static Arc<DynamoRepo<Product>> {
    if REPO.get().is_none() {
        let repo = build_repo().await;
        let _ = REPO.set(repo);
    }
    REPO.get().unwrap()
}
```

## CRUD básico

```rust
use serverust_telemetry::dynamo::DynamoRepo;

// PutItem — insere ou substitui o item inteiro
repo.put(&Product {
    product_id: "p-001".into(),
    name: "Teclado mecânico".into(),
    price: 299.90,
}).await?;

// GetItem — busca por partition key
let product: Option<Product> = repo.get("p-001").await?;

// DeleteItem — remove por partition key
repo.delete("p-001").await?;

// Query — retorna todos os itens com uma partition key (útil para tabelas com SK)
let orders: Vec<Order> = order_repo.query_by_pk("user-42").await?;
```

### Tabelas com sort key

```rust
// GetItem com PK + SK
let order: Option<Order> = order_repo.get_with_sk("user-42", "order-99").await?;

// DeleteItem com PK + SK
order_repo.delete_with_sk("user-42", "order-99").await?;
```

## Credenciais

### Desenvolvimento local (env vars)

```bash
export AWS_ACCESS_KEY_ID=your-key
export AWS_SECRET_ACCESS_KEY=your-secret
export AWS_DEFAULT_REGION=us-east-1
```

### Lambda (IAM Role)

Atribua à função Lambda uma IAM Role com permissões sobre a tabela:

```json
{
  "Effect": "Allow",
  "Action": [
    "dynamodb:GetItem",
    "dynamodb:PutItem",
    "dynamodb:DeleteItem",
    "dynamodb:Query"
  ],
  "Resource": "arn:aws:dynamodb:us-east-1:*:table/Products"
}
```

O SDK da AWS detecta automaticamente as credenciais via IMDSv2 quando a função executa em Lambda — nenhuma variável de ambiente extra é necessária.

### ECS / EC2

Igual ao Lambda: atribua uma Task Role (ECS) ou Instance Profile (EC2) com as permissões necessárias. O SDK faz a resolução automática via cadeia de providers.

## Testes locais com DynamoDB Local

Suba o DynamoDB Local com Docker:

```bash
docker run -d -p 8000:8000 amazon/dynamodb-local
```

Configure o endpoint local no seu código de teste:

```rust
#[cfg(test)]
mod tests {
    use aws_config::BehaviorVersion;
    use aws_sdk_dynamodb::{Client, config::Builder};
    use aws_types::region::Region;

    async fn local_client() -> Client {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        let local_config = Builder::from(&config)
            .endpoint_url("http://localhost:8000")
            .region(Region::new("us-east-1"))
            .build();
        Client::from_conf(local_config)
    }

    #[tokio::test]
    async fn test_product_crud() {
        let client = local_client().await;
        // crie a tabela manualmente antes:
        // aws dynamodb create-table --endpoint-url http://localhost:8000 \
        //   --table-name Products \
        //   --attribute-definitions AttributeName=product_id,AttributeType=S \
        //   --key-schema AttributeName=product_id,KeyType=HASH \
        //   --billing-mode PAY_PER_REQUEST

        let repo = DynamoRepo::<Product>::new(client);
        let item = Product { product_id: "t-1".into(), name: "Test".into(), price: 1.0 };

        repo.put(&item).await.unwrap();
        let got = repo.get("t-1").await.unwrap();
        assert_eq!(got.unwrap().name, "Test");

        repo.delete("t-1").await.unwrap();
        assert!(repo.get("t-1").await.unwrap().is_none());
    }
}
```

## Troubleshooting

### `ResourceNotFoundException` ao acessar a tabela

- Verifique se o nome da tabela em `#[dynamo_table("...")]` bate exatamente com o nome criado na AWS (case-sensitive).
- Confirme que `AWS_DEFAULT_REGION` corresponde à região onde a tabela existe.

### `AccessDeniedException`

- A IAM Role ou as credenciais locais não têm permissão para a ação (`GetItem`, `PutItem`, etc.).
- Em testes locais com DynamoDB Local, credenciais podem ser fictícias (`AWS_ACCESS_KEY_ID=test`).

### Tipos numéricos perdendo precisão

DynamoDB armazena números como string. O `attr_to_json` prioriza `i64` → `u64` → `f64` ao deserializar. Se o seu campo for `f32` ou `Decimal`, use `String` no schema e faça a conversão na sua lógica de negócio.

### Sort key obrigatório mas não informado

`repo.delete("pk")` retorna `RepoError::Conversion` se a tabela declarar `sk`. Use `delete_with_sk("pk", "sk")`.

## Veja também

- [`serverust-telemetry/src/dynamo.rs`](../../serverust-telemetry/src/dynamo.rs) — implementação completa do `DynamoRepo`.
- [`examples/kafka-wallet/src/lib.rs`](../../examples/kafka-wallet/src/lib.rs) — exemplo real com `DynamoRepo<Wallet>` + `OnceLock` em Lambda.
- [ADR 0002](../development/decisions/0002-dynamodb-opt-in.md) — decisão de tornar DynamoDB opt-in via feature flag.
