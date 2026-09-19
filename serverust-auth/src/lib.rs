//! Autenticação e autorização para o framework **serverust**.
//!
//! O escopo deste crate é verificar tokens emitidos por um provedor de
//! identidade externo — Cognito, Auth0, Clerk, Keycloak, Logto. Ele
//! deliberadamente **não** implementa login, hash de senha nem sessão: isso
//! exige estado e banco, encaixa mal em Lambda, e é território de crates como
//! `torii` e `axum-login`. O raciocínio completo está na
//! [ADR 0009](https://github.com/JaimeJunr/serverust/blob/main/docs/development/decisions/0009-auth-authz-crate-separada-serverust-auth.md).
//!
//! # Como as peças se encaixam
//!
//! ```text
//! AuthLayer  valida o token UMA vez e deposita o resultado nas extensions
//!     │
//!     ├── Auth<C>       extractor tipado — exige identidade, 401 sem ela
//!     └── MaybeAuth<C>  extractor opcional — identidade quando houver
//! ```
//!
//! Validar no layer e ler no extractor evita revalidar o mesmo token a cada
//! peça do pipeline que precise dele.
//!
//! # Exemplo
//!
//! ```no_run
//! use serverust_auth::{Auth, AuthLayer, JwtAuth, StandardClaims};
//! use serverust_core::App;
//! use serverust_macros::get;
//!
//! #[get("/me")]
//! async fn me(user: Auth<StandardClaims>) -> String {
//!     format!("olá, {}", user.sub)
//! }
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     let secret = std::env::var("JWT_SECRET").expect("JWT_SECRET");
//!     let auth = JwtAuth::hs256(secret.as_bytes()).issuer("https://idp.exemplo.com/");
//!
//!     App::new()
//!         .auth(AuthLayer::<StandardClaims>::new(auth))
//!         .route(me)
//!         .run_http("127.0.0.1:3000")
//!         .await
//! }
//! ```
//!
//! # Origem das chaves
//!
//! Duas, e a escolha é do emissor que você usa:
//!
//! - [`JwtAuth`] — **chave estática** conhecida no boot: segredo simétrico de
//!   variável de ambiente ou Secrets Manager, ou chave pública embutida no
//!   binário. Não toca a rede.
//! - [`JwksAuth`] — **chaves do JWKS do emissor**, com descoberta de OIDC. O
//!   construtor é `async` de propósito: é o que põe a ida à rede na fase de
//!   init da Lambda, onde há burst de CPU, em vez de na primeira requisição de
//!   cada container frio. Não existe construtor síncrono com busca preguiçosa,
//!   então esquecer de aquecer não é um erro que se possa cometer.
//!
//! As duas implementam [`Verifier`], e é isso que o [`AuthLayer`] consome —
//! inclusive uma terceira, escrita por você.
//!
//! # O que ainda não está implementado
//!
//! - **Refresh do JWKS sob demanda.** As chaves são carregadas na construção e
//!   não mudam depois. Se o emissor rotacionar enquanto o processo vive, token
//!   assinado com a chave nova recebe 401 com `unknown_key_id` até o container
//!   ser reciclado. Ver a nota sobre rotação em [`JwksAuth`].
//! - **`security` automático no OpenAPI.** O botão "Authorize" do
//!   Scalar/Swagger UI ainda precisa de configuração manual.
//!
//! # Cripto
//!
//! O backend é Rust puro (feature `rust_crypto` do `jsonwebtoken`). A
//! alternativa `aws_lc_rs` exigiria cmake e toolchain C, quebrando a
//! cross-compilação x86_64 para aarch64 usada no build ARM64 de Lambda.

mod claims;
mod error;
mod extract;
mod jwks;
mod jwt;
mod layer;
mod verifier;

pub use claims::{AuthzFacts, Claims, StandardClaims};
pub use error::{AuthError, unauthorized};
pub use extract::{Auth, MaybeAuth};
pub use jwks::JwksAuth;
pub use jwt::JwtAuth;
pub use layer::{AuthLayer, AuthService};
pub use verifier::Verifier;
