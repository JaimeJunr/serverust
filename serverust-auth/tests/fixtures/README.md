# Fixtures de chave para teste

Pares de chave **descartáveis**, gerados exclusivamente para a suíte de testes
de `serverust-auth` (`tests/asymmetric_keys.rs`), que precisa de material real
para exercitar RS256 e ES256 e para provar que a tentativa de confusão de
algoritmo é rejeitada.

**Estas chaves não são segredo.** Nunca protegeram nada, não existem fora deste
diretório e podem ser regeneradas a qualquer momento:

```bash
# RSA 2048 (RS256)
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out rs256_private.pem
openssl rsa -in rs256_private.pem -pubout -out rs256_public.pem

# ECDSA P-256 (ES256)
openssl ecparam -name prime256v1 -genkey -noout -out es256_private.pem
openssl ec -in es256_private.pem -pubout -out es256_public.pem
```

## Por que estão versionadas

O `.gitignore` da raiz ignora `*.pem` para impedir commit acidental de chave de
verdade. Estes arquivos entram por uma exceção **estreita**, declarada logo
abaixo daquela regra e limitada a este diretório — a proteção continua valendo
em todo o resto do repositório.

A alternativa seria gerar os pares em tempo de teste, mas a geração de RSA-2048
em build de debug custa segundos e tornaria a suíte lenta sem ganho de
segurança, já que o material aqui não tem valor.

**Não adicione neste diretório nenhuma chave que proteja algo.**
