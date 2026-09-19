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
//!         .layer(AuthLayer::<StandardClaims>::new(auth))
//!         .route(me)
//!         .run_http("127.0.0.1:3000")
//!         .await
//! }
//! ```
//!
//! # Estado atual
//!
//! Esta versão cobre **chave estática**: segredo simétrico ou chave pública
//! conhecida no boot. Ainda não estão implementados, e virão em incrementos
//! seguintes previstos pela ADR 0009:
//!
//! - descoberta de JWKS/OIDC, com a busca aquecida na fase de init;
//! - `#[authorize(scope = "...")]` e o default deny por rota;
//! - `security` automático no OpenAPI.
//!
//! Enquanto o default deny não existe, **uma rota só é protegida se pedir
//! [`Auth`]** na assinatura.
//!
//! # Cripto
//!
//! O backend é Rust puro (feature `rust_crypto` do `jsonwebtoken`). A
//! alternativa `aws_lc_rs` exigiria cmake e toolchain C, quebrando a
//! cross-compilação x86_64 para aarch64 usada no build ARM64 de Lambda.

mod claims;
mod error;
mod extract;
mod jwt;
mod layer;

pub use claims::{AuthzFacts, Claims, StandardClaims};
pub use error::{AuthError, unauthorized};
pub use extract::{Auth, MaybeAuth};
pub use jwt::JwtAuth;
pub use layer::{AuthLayer, AuthService};
