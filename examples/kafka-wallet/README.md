# kafka-wallet

Exemplo end-to-end **Kafka → DynamoDB → Kafka** usando o framework serverust.

O handler do usuário (`src/lib.rs`) tem **75 LOC** total (limite do PRD: ≤ 80 LOC),
incluindo as 3 structs DTO e as anotações `#[kafka_consumer]` + `#[dynamo_table]`.

## Fluxo

1. Lambda recebe um `KafkaEvent` contendo registros publicados no tópico
   `wallet.events` (carteira a ser creditada/debitada).
2. Para cada registro, o handler:
   - Lê o estado atual da carteira no DynamoDB (tabela `Wallets`, pk = `user_id`).
   - Aplica `credit` ou `debit`.
   - Persiste o novo saldo via `DynamoRepo<Wallet>`.
   - Publica o resultado (`WalletResult`) no tópico `wallet.results` via
     `KafkaProducer`.

## Setup local

Suba Redpanda (broker Kafka-compatível) e DynamoDB Local:

```bash
docker compose up -d
```

Crie a tabela `Wallets` no DynamoDB Local:

```bash
aws --endpoint-url http://localhost:8000 dynamodb create-table \
  --table-name Wallets \
  --attribute-definitions AttributeName=user_id,AttributeType=S \
  --key-schema AttributeName=user_id,KeyType=HASH \
  --billing-mode PAY_PER_REQUEST
```

Crie os tópicos Kafka:

```bash
docker exec kafka-wallet-redpanda rpk topic create wallet.events wallet.results
```

## Rodar localmente (cargo lambda)

```bash
KAFKA_BROKERS=localhost:9092 \
AWS_ENDPOINT_URL=http://localhost:8000 \
AWS_REGION=us-east-1 \
AWS_ACCESS_KEY_ID=local \
AWS_SECRET_ACCESS_KEY=local \
cargo lambda watch
```

Em outro terminal, publique um evento de teste:

```bash
docker exec -i kafka-wallet-redpanda rpk topic produce wallet.events <<< \
  '{"user_id":"u-1","amount":100,"operation":"credit"}'
```

Confirme o resultado:

```bash
docker exec kafka-wallet-redpanda rpk topic consume wallet.results --num 1
aws --endpoint-url http://localhost:8000 dynamodb get-item \
  --table-name Wallets --key '{"user_id":{"S":"u-1"}}'
```

## Deploy AWS

```bash
cargo lambda build --release --arm64
cargo lambda deploy kafka-wallet \
  --iam-role arn:aws:iam::ACCOUNT:role/kafka-wallet \
  --env-var MSK_BOOTSTRAP_SERVERS=b-1.cluster.kafka.region.amazonaws.com:9098 \
  --env-var MSK_IAM_ROLE=enabled
```

Em seguida, anexe um *event source mapping* MSK (Console AWS ou Terraform)
apontando para o tópico `wallet.events`.

## Variáveis de ambiente

| Variável | Uso | Obrigatória |
|---|---|---|
| `KAFKA_BROKERS` | bootstrap servers locais | Local |
| `MSK_BOOTSTRAP_SERVERS` | bootstrap MSK | Produção |
| `MSK_IAM_ROLE` | ativa SASL_SSL + OAUTHBEARER IAM | Produção |
| `AWS_REGION` | região AWS para Dynamo + signer MSK | Sempre |
| `AWS_ENDPOINT_URL` | endpoint custom (DynamoDB Local) | Local |
