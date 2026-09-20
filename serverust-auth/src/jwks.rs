//! Verificador com chaves vindas do JWKS do emissor.
//!
//! # O aquecimento é garantido por construção
//!
//! Buscar o JWKS custa uma ida à rede. Em Lambda, onde isso acontece decide o
//! preço: na **fase de init** há burst de CPU e a busca sai de graça; na fase
//! de invocação, a mesma busca vira latência no primeiro request de cada
//! container frio — e some dos testes, porque em teste o container está sempre
//! quente.
//!
//! Por isso não existe construtor síncrono com busca preguiçosa. O único jeito
//! de obter um [`JwksAuth`] é `await` num construtor que já buscou:
//!
//! ```text
//! let auth = JwksAuth::discover("https://idp.exemplo.com/").await?;
//! ```
//!
//! (Exemplo compilado em [`JwksAuth::discover`]; aqui não, porque este bloco
//! de doc existe mesmo sem a feature `jwks`, e o construtor não.)
//!
//! Esquecer de aquecer não é um erro que se possa cometer: não há objeto antes
//! da busca. É a regra 3 do corolário da filosofia — não dependa de lembrar.

use std::collections::HashMap;

use jsonwebtoken::jwk::{AlgorithmParameters, Jwk, JwkSet, KeyAlgorithm, PublicKeyUse};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};

use crate::claims::Claims;
use crate::error::AuthError;
use crate::verifier::Verifier;

/// Chave do JWKS já convertida para o que a verificação precisa.
///
/// A `Validation` é montada aqui, na construção, e não a cada requisição:
/// `set_issuer`/`set_audience` alocam, e o caminho quente não pode pagar isso.
struct Chave {
    key: DecodingKey,
    validation: Validation,
}

/// Verificador que usa as chaves publicadas pelo emissor no JWKS.
///
/// A escolha da chave é pelo `kid` do header do token. O **algoritmo vem da
/// chave**, nunca do header do token: é o que fecha a confusão de algoritmo,
/// em que um token forjado declara um `alg` que faz o servidor usar o material
/// de chave de um jeito diferente do pretendido.
///
/// # Rotação de chaves — o limite desta versão
///
/// As chaves são carregadas na construção e **não mudam depois**. Se o emissor
/// rotacionar enquanto este processo vive, token assinado com a chave nova
/// recebe 401 com `unknown_key_id` até o container ser reciclado.
///
/// O que torna isso viável na prática: emissores sérios publicam a chave nova
/// no JWKS **antes** de começar a assinar com ela, justamente para que
/// verificadores com cache a tenham; e containers de Lambda vivem minutos a
/// horas, não semanas. O que torna isso um risco real: se a janela de
/// publicação do seu emissor for menor que a vida dos seus containers, haverá
/// 401 até reciclarem.
///
/// Por isso `unknown_key_id` tem código próprio, distinto de `invalid_token`:
/// dá para alertar sobre ele. Um pico desse código significa rotação, e a ação
/// é do operador — não do cliente que recebeu o 401.
///
/// O refresh sob demanda está previsto na ADR 0009 e fica para o incremento
/// seguinte: fazê-lo exige I/O no caminho de request, o que muda o future do
/// [`crate::AuthLayer`] para todo mundo, inclusive para quem usa chave
/// estática. É mudança que merece o seu próprio PR.
pub struct JwksAuth {
    /// `kid` → chave. Preenchido na construção e imutável depois.
    chaves: HashMap<String, Chave>,
    /// Emissor esperado, aplicado a toda `Validation`.
    issuer: Option<String>,
    audience: Option<String>,
    leeway: Option<u64>,
}

impl JwksAuth {
    /// Monta o verificador a partir do JSON de um JWKS já obtido.
    ///
    /// É o escape hatch de transporte: quem já tem um cliente HTTP
    /// configurado — com proxy corporativo, CA própria, credenciais de VPC —
    /// busca o JWKS com ele e entrega o corpo aqui, sem a feature `jwks` e sem
    /// o cliente HTTP deste crate no binário.
    ///
    /// O chamador é quem garante o aquecimento neste caminho, porque é ele quem
    /// faz a busca.
    pub fn from_jwks_json(json: &str) -> Result<Self, AuthError> {
        let set: JwkSet = serde_json::from_str(json)
            .map_err(|e| AuthError::InvalidKey(format!("JWKS não é um JSON válido: {e}")))?;

        let mut chaves = HashMap::new();
        let mut descartadas = 0usize;

        for jwk in &set.keys {
            let Some(kid) = jwk.common.key_id.clone() else {
                // Sem `kid` não há como o token apontar para esta chave.
                descartadas += 1;
                continue;
            };

            match converter(jwk) {
                Some(key) => {
                    chaves.insert(
                        kid,
                        Chave {
                            key,
                            // Placeholder: `Validation` definitiva é montada em
                            // `finalizar`, quando issuer/audience/leeway já foram
                            // escolhidos pelo builder.
                            validation: Validation::new(Algorithm::HS256),
                        },
                    );
                }
                None => descartadas += 1,
            }
        }

        if chaves.is_empty() {
            return Err(AuthError::InvalidKey(format!(
                "nenhuma chave utilizável no JWKS ({descartadas} descartada(s)): \
                 são aceitas apenas chaves de assinatura assimétricas com `kid`"
            )));
        }

        // Guarda o algoritmo de cada chave junto dela, recalculando na
        // finalização — `converter` já garantiu que existe.
        let mut auth = Self {
            chaves,
            issuer: None,
            audience: None,
            leeway: None,
        };
        auth.remontar_validations(&set);
        Ok(auth)
    }

    /// Exige que a claim `iss` case com o emissor informado.
    ///
    /// [`JwksAuth::discover`] já preenche isto com o `issuer` do documento de
    /// descoberta; chamar aqui sobrescreve.
    pub fn issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = Some(issuer.into());
        self.reaplicar();
        self
    }

    /// Exige que a claim `aud` case com a audiência informada.
    pub fn audience(mut self, audience: impl Into<String>) -> Self {
        self.audience = Some(audience.into());
        self.reaplicar();
        self
    }

    /// Tolerância, em segundos, na checagem de `exp` e `nbf`.
    ///
    /// O default herdado do `jsonwebtoken` é **60 segundos**, não zero.
    pub fn leeway(mut self, seconds: u64) -> Self {
        self.leeway = Some(seconds);
        self.reaplicar();
        self
    }

    /// Quantidade de chaves utilizáveis carregadas. Serve para afirmar em
    /// teste que a descoberta trouxe o que se esperava.
    pub fn key_count(&self) -> usize {
        self.chaves.len()
    }

    /// Se o `kid` informado está entre as chaves carregadas.
    pub fn has_key(&self, kid: &str) -> bool {
        self.chaves.contains_key(kid)
    }

    fn remontar_validations(&mut self, set: &JwkSet) {
        for jwk in &set.keys {
            let Some(kid) = jwk.common.key_id.as_deref() else {
                continue;
            };
            let Some(alg) = algoritmo(jwk) else { continue };
            if let Some(chave) = self.chaves.get_mut(kid) {
                chave.validation = Validation::new(alg);
            }
        }
        self.reaplicar();
    }

    /// Reaplica issuer/audience/leeway a todas as `Validation`.
    ///
    /// Chamado a cada passo do builder em vez de na primeira verificação: o
    /// custo é de construção, e o caminho de request fica sem nenhum ramo.
    fn reaplicar(&mut self) {
        for chave in self.chaves.values_mut() {
            if let Some(issuer) = &self.issuer {
                chave.validation.set_issuer(&[issuer]);
            }
            if let Some(audience) = &self.audience {
                chave.validation.set_audience(&[audience]);
            }
            if let Some(leeway) = self.leeway {
                chave.validation.leeway = leeway;
            }
        }
    }

    /// Verifica assinatura e claims registradas, devolvendo o payload tipado.
    pub fn verify<C: Claims>(&self, token: &str) -> Result<C, AuthError> {
        let header = decode_header(token).map_err(|_| AuthError::InvalidToken)?;

        // Sem `kid` no token, a única escolha não-arbitrária é quando o JWKS
        // tem exatamente uma chave. Com duas ou mais, escolher seria adivinhar.
        let chave = match header.kid.as_deref() {
            Some(kid) => self.chaves.get(kid).ok_or(AuthError::UnknownKeyId)?,
            None if self.chaves.len() == 1 => self
                .chaves
                .values()
                .next()
                .expect("len() == 1 garante um elemento"),
            None => return Err(AuthError::UnknownKeyId),
        };

        decode::<C>(token, &chave.key, &chave.validation)
            .map(|data| data.claims)
            .map_err(AuthError::from)
    }
}

impl Verifier for JwksAuth {
    fn verify<C: Claims>(&self, token: &str) -> Result<C, AuthError> {
        JwksAuth::verify(self, token)
    }
}

// Debug manual, pelo mesmo motivo do `JwtAuth`: o derivado exporia material de
// chave em log de panic ou em `dbg!`.
impl std::fmt::Debug for JwksAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwksAuth")
            .field("keys", &self.chaves.len())
            .field("issuer", &self.issuer)
            .finish_non_exhaustive()
    }
}

/// Converte uma JWK em chave de verificação, ou `None` se ela não serve.
fn converter(jwk: &Jwk) -> Option<DecodingKey> {
    algoritmo(jwk)?;

    // `use: enc` marca chave de criptografia, não de assinatura. Usá-la para
    // verificar assinatura é uso fora do que o emissor declarou.
    if matches!(jwk.common.public_key_use, Some(PublicKeyUse::Encryption)) {
        return None;
    }

    DecodingKey::from_jwk(jwk).ok()
}

/// Algoritmo de assinatura da chave, ou `None` se ela não é utilizável.
///
/// A regra que importa aqui é de segurança, não de compatibilidade:
///
/// - **`oct` (HMAC) é recusado sempre.** JWKS é um documento público. Uma
///   chave simétrica publicada ali é o próprio segredo de assinatura, e aceitá-la
///   significaria que qualquer um que lê o JWKS forja tokens. Um emissor que
///   publique isso está mal configurado, e o comportamento seguro é recusar, não
///   acompanhar.
/// - **O algoritmo nunca vem do token.** Sai do campo `alg` da JWK; se ele não
///   existir, é inferido do tipo de chave e da curva, que também são da JWK.
/// - **Algoritmos de criptografia (`RSA-OAEP`, `RSA1_5`) são recusados**, por
///   não serem de assinatura.
fn algoritmo(jwk: &Jwk) -> Option<Algorithm> {
    if let Some(declarado) = jwk.common.key_algorithm {
        return match declarado {
            KeyAlgorithm::RS256 => Some(Algorithm::RS256),
            KeyAlgorithm::RS384 => Some(Algorithm::RS384),
            KeyAlgorithm::RS512 => Some(Algorithm::RS512),
            KeyAlgorithm::PS256 => Some(Algorithm::PS256),
            KeyAlgorithm::PS384 => Some(Algorithm::PS384),
            KeyAlgorithm::PS512 => Some(Algorithm::PS512),
            KeyAlgorithm::ES256 => Some(Algorithm::ES256),
            KeyAlgorithm::ES384 => Some(Algorithm::ES384),
            KeyAlgorithm::EdDSA => Some(Algorithm::EdDSA),
            // HS* e os de criptografia caem aqui de propósito. Ver o doc acima.
            _ => None,
        };
    }

    // `alg` é opcional na RFC 7517. Sem ele, o tipo de chave decide — e
    // continua sendo informação da JWK, não do token.
    match &jwk.algorithm {
        AlgorithmParameters::RSA(_) => Some(Algorithm::RS256),
        AlgorithmParameters::EllipticCurve(ec) => {
            use jsonwebtoken::jwk::EllipticCurve;
            match ec.curve {
                EllipticCurve::P256 => Some(Algorithm::ES256),
                EllipticCurve::P384 => Some(Algorithm::ES384),
                EllipticCurve::Ed25519 => Some(Algorithm::EdDSA),
                // P-521 não é suportado pelo backend de cripto.
                EllipticCurve::P521 => None,
                // `EllipticCurve` é `#[non_exhaustive]`: curva que o
                // `jsonwebtoken` venha a acrescentar cai aqui. Recusar é a
                // direção certa — aceitar exigiria saber que algoritmo usar, e
                // adivinhar isso é a confusão de algoritmo por outro caminho.
                _ => None,
            }
        }
        AlgorithmParameters::OctetKeyPair(_) => Some(Algorithm::EdDSA),
        // `oct` (simétrica) e `Other` não entram. Ver o doc acima. O `_`
        // cobre o `#[non_exhaustive]` do enum, pelo mesmo motivo da curva.
        AlgorithmParameters::OctetKey(_) | AlgorithmParameters::Other(_) => None,
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Descoberta por HTTP (feature `jwks`)
// ---------------------------------------------------------------------------

/// Teto para a busca de descoberta e do JWKS, somadas.
///
/// A fase de init da Lambda tem orçamento de 10 s; estourar ali derruba o
/// container inteiro com uma mensagem que não explica nada. Falhar aqui, com
/// [`AuthError::Discovery`] dizendo o que não respondeu, é melhor do que ser
/// morto pelo runtime.
#[cfg(feature = "jwks")]
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(feature = "jwks")]
#[derive(serde::Deserialize)]
struct DocumentoDeDescoberta {
    issuer: String,
    jwks_uri: String,
}

#[cfg(feature = "jwks")]
impl JwksAuth {
    /// Descobre o emissor por OIDC e carrega o JWKS dele.
    ///
    /// Busca `{issuer}/.well-known/openid-configuration`, lê o `jwks_uri` de
    /// lá e carrega as chaves. O `issuer` do documento é aplicado como emissor
    /// esperado — validação de `iss` fica **ligada por default**, e quem quiser
    /// outro valor sobrescreve com [`JwksAuth::issuer`].
    ///
    /// ```no_run
    /// # async fn exemplo() -> Result<(), serverust_auth::AuthError> {
    /// use serverust_auth::JwksAuth;
    ///
    /// let auth = JwksAuth::discover("https://idp.exemplo.com/")
    ///     .await?
    ///     .audience("minha-api");
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Chame no `main`, antes de montar o `App`: é o que coloca a ida à rede
    /// na fase de init.
    pub async fn discover(issuer: &str) -> Result<Self, AuthError> {
        let issuer = issuer.trim_end_matches('/');
        exigir_transporte_seguro(issuer)?;

        let url = format!("{issuer}/.well-known/openid-configuration");
        let doc: DocumentoDeDescoberta = buscar_json(&url).await?;

        // A RFC 8414 §3.3 exige que o `issuer` do documento seja idêntico ao
        // que foi usado para chegar nele. Divergir significa que o documento
        // não descreve o emissor que pedimos — seja por má configuração, seja
        // porque alguém redirecionou a descoberta para outro lugar. Aceitar
        // faria a validação de `iss` passar a atestar o emissor errado.
        if doc.issuer.trim_end_matches('/') != issuer {
            return Err(AuthError::Discovery(format!(
                "documento de descoberta declara issuer `{}`, mas foi buscado em `{issuer}`",
                doc.issuer
            )));
        }

        exigir_transporte_seguro(&doc.jwks_uri)?;
        let auth = Self::from_jwks_uri_sem_checar(&doc.jwks_uri).await?;
        Ok(auth.issuer(doc.issuer))
    }

    /// Carrega o JWKS direto de uma URL, sem passar pela descoberta.
    ///
    /// Para emissores que não publicam `/.well-known/openid-configuration`.
    /// Como não há documento de descoberta, **não há emissor para aplicar**:
    /// chame [`JwksAuth::issuer`] explicitamente, ou a claim `iss` não será
    /// verificada.
    pub async fn from_jwks_uri(uri: &str) -> Result<Self, AuthError> {
        exigir_transporte_seguro(uri)?;
        Self::from_jwks_uri_sem_checar(uri).await
    }

    async fn from_jwks_uri_sem_checar(uri: &str) -> Result<Self, AuthError> {
        let corpo = buscar_texto(uri).await?;
        Self::from_jwks_json(&corpo)
    }
}

/// Recusa buscar material de chave por transporte sem autenticação de origem.
///
/// JWKS em texto claro é confiança depositada em quem estiver no caminho: quem
/// puder responder pela URL escolhe a chave pública com que a API vai validar
/// tokens, e portanto forja qualquer identidade. A exceção é loopback, onde
/// não há caminho para interceptar e onde os testes e o desenvolvimento local
/// vivem.
#[cfg(feature = "jwks")]
fn exigir_transporte_seguro(url: &str) -> Result<(), AuthError> {
    if url.starts_with("https://") {
        return Ok(());
    }

    if let Some(resto) = url.strip_prefix("http://") {
        let host = resto
            .split(['/', ':', '?'])
            .next()
            .unwrap_or_default()
            .trim_start_matches('[')
            .trim_end_matches(']');

        if host == "localhost" || host == "127.0.0.1" || host == "::1" {
            return Ok(());
        }

        return Err(AuthError::Discovery(format!(
            "`{url}` usa http sem TLS. Quem responder por essa URL escolhe a chave \
             pública que valida os tokens desta API — só loopback é aceito em texto claro."
        )));
    }

    Err(AuthError::Discovery(format!(
        "`{url}` não é uma URL http(s)"
    )))
}

#[cfg(feature = "jwks")]
async fn buscar_texto(url: &str) -> Result<String, AuthError> {
    let cliente = reqwest::Client::builder()
        .timeout(TIMEOUT)
        // Um redirecionamento pode levar a busca para fora do emissor, e é aí
        // que o `exigir_transporte_seguro` deixaria de valer: o destino final
        // não passa por ele. Sem redirecionamento, a URL verificada é a URL
        // buscada.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| AuthError::Discovery(format!("cliente HTTP: {e}")))?;

    let resposta = cliente
        .get(url)
        .send()
        .await
        .map_err(|e| AuthError::Discovery(format!("{url}: {e}")))?;

    let status = resposta.status();
    if !status.is_success() {
        return Err(AuthError::Discovery(format!("{url}: HTTP {status}")));
    }

    resposta
        .text()
        .await
        .map_err(|e| AuthError::Discovery(format!("{url}: corpo ilegível: {e}")))
}

#[cfg(feature = "jwks")]
async fn buscar_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, AuthError> {
    let corpo = buscar_texto(url).await?;
    serde_json::from_str(&corpo)
        .map_err(|e| AuthError::Discovery(format!("{url}: JSON inesperado: {e}")))
}
