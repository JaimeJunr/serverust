# Compatibilidade com IaC (Serverless, SST, Terraform e afins)

Este guia define **o contrato oficial** de compatibilidade do `serverust` com ferramentas de IaC.

## Escopo oficial

O `serverust` **não** é uma plataforma de provisionamento de infraestrutura.  
Ele é um framework/runtime Rust que roda em AWS Lambda.

Compatibilidade garantida:
- Function URL
- API Gateway HTTP API (v2)
- API Gateway REST API (v1)

Isso cobre os gatilhos usados por:
- Serverless Framework
- SST
- Terraform
- SAM/CDK

## Como o contrato é validado

No crate `serverust-lambda`, o teste de integração [`lambda_to_axum.rs`](../../serverust-lambda/tests/lambda_to_axum.rs)
valida que eventos reais de Lambda são roteados corretamente para o `axum::Router`:

- `fixtures/apigw_v1_get.json` (REST v1)
- `fixtures/apigw_v2_post.json` (HTTP v2)
- `fixtures/lambda_function_url_get.json` (Function URL)

Esse teste roda em CI e é o gate mínimo para evitar regressão de compatibilidade.

## Exemplo: Serverless Framework

```yaml
service: my-api
provider:
  name: aws
  runtime: provided.al2023
functions:
  api:
    handler: bootstrap
    package:
      artifact: target/lambda/my-api/bootstrap.zip
    events:
      - httpApi: "*"
```

## Exemplo: SST (AWS CDK por baixo)

```ts
new sst.aws.Function("Api", {
  runtime: "provided.al2023",
  handler: "bootstrap",
  url: true,
});
```

## Exemplo: Terraform

```hcl
resource "aws_lambda_function" "api" {
  function_name = "my-api"
  role          = aws_iam_role.lambda_exec.arn
  runtime       = "provided.al2023"
  handler       = "bootstrap"
  filename      = "bootstrap.zip"
}
```

## Observações importantes

- O deploy/infra (IAM, API Gateway, DNS, VPC) é responsabilidade da ferramenta IaC.
- O `serverust` garante o comportamento da aplicação ao receber os formatos de evento suportados.
- Se você provisionar API Gateway REST v1, mantenha `AWS_LAMBDA_HTTP_IGNORE_STAGE_IN_PATH=true` (o `run_lambda()` já define automaticamente quando possível).
