//! Runtime dual do framework serverust: HTTP local e AWS Lambda.
//!
//! Use a trait [`AppRuntime`] (importada deste crate) para chamar `.run()`
//! diretamente em [`serverust_core::App`] — a função escolhe entre Lambda e
//! HTTP local olhando para `AWS_LAMBDA_RUNTIME_API`.

use std::net::SocketAddr;

use lambda_http::Error as LambdaError;
use serverust_core::App;
use tokio::net::ToSocketAddrs;

pub use aws_lambda_events;
pub use lambda_http;

/// Tipo de runtime escolhido pela detecção de ambiente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    /// AWS Lambda detectado (`AWS_LAMBDA_RUNTIME_API` presente).
    Lambda,
    /// Execução local em servidor HTTP.
    Http,
}

/// Função pura usada pelo runtime dispatcher e por testes.
///
/// Considera Lambda apenas quando a variável existe e não está vazia, para
/// evitar falso-positivo se algum operador exportar `AWS_LAMBDA_RUNTIME_API=`
/// sem valor.
pub fn detect_runtime(env_value: Option<&str>) -> Runtime {
    match env_value {
        Some(value) if !value.is_empty() => Runtime::Lambda,
        _ => Runtime::Http,
    }
}

fn current_runtime() -> Runtime {
    detect_runtime(std::env::var("AWS_LAMBDA_RUNTIME_API").ok().as_deref())
}

/// Sobe a App no runtime Lambda (consumindo eventos do API Gateway / Function URL).
///
/// Define `AWS_LAMBDA_HTTP_IGNORE_STAGE_IN_PATH=true` automaticamente para que
/// rotas funcionem idênticas em todos os triggers (REST v1 incluiria o `stage`
/// no path por default). A var só é definida se ainda não estiver presente,
/// permitindo override pelo operador.
pub async fn run_lambda(app: App) -> Result<(), LambdaError> {
    if std::env::var_os("AWS_LAMBDA_HTTP_IGNORE_STAGE_IN_PATH").is_none() {
        // SAFETY: chamado uma única vez na inicialização do binário Lambda,
        // antes do runtime spawnar worker tasks. Não há concorrência com
        // outras leituras/escritas de env.
        unsafe { std::env::set_var("AWS_LAMBDA_HTTP_IGNORE_STAGE_IN_PATH", "true") };
    }
    let router = app.into_router();
    lambda_http::run(router).await
}

/// Sobe a App em modo HTTP local atado a `addr`.
pub async fn run_http<A: ToSocketAddrs>(app: App, addr: A) -> std::io::Result<()> {
    app.run_http(addr).await
}

/// Dispatcher: escolhe entre Lambda e HTTP local conforme o ambiente.
///
/// Em modo HTTP local, utiliza o endereço default `0.0.0.0:3000`. Quem precisar
/// customizar deve chamar [`run_http`] diretamente.
pub async fn run(app: App) -> Result<(), LambdaError> {
    match current_runtime() {
        Runtime::Lambda => run_lambda(app).await,
        Runtime::Http => {
            let addr: SocketAddr = "0.0.0.0:3000".parse().expect("addr literal sempre parseia");
            run_http(app, addr).await.map_err(LambdaError::from)
        }
    }
}

/// Extensão que permite chamar `.run()` e `.run_lambda()` em [`App`] via dot-chain.
///
/// A função [`run`](Self::run) detecta automaticamente o ambiente: em AWS
/// Lambda (`AWS_LAMBDA_RUNTIME_API` presente), invoca `lambda_http::run`; em
/// outros casos, sobe servidor HTTP local em `0.0.0.0:3000`.
///
/// ```no_run
/// use serverust_core::App;
/// use serverust_lambda::AppRuntime;
/// use serverust_macros::get;
///
/// #[get("/")]
/// async fn hello() -> &'static str { "hello" }
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
///     App::new().route(hello).run().await?;
///     Ok(())
/// }
/// ```
pub trait AppRuntime {
    /// Detecta o ambiente e despacha entre Lambda e HTTP local.
    fn run(self) -> impl std::future::Future<Output = Result<(), LambdaError>>;
    /// Força runtime Lambda (`lambda_http::run`) independentemente do ambiente.
    fn run_lambda(self) -> impl std::future::Future<Output = Result<(), LambdaError>>;
}

impl AppRuntime for App {
    fn run(self) -> impl std::future::Future<Output = Result<(), LambdaError>> {
        run(self)
    }
    fn run_lambda(self) -> impl std::future::Future<Output = Result<(), LambdaError>> {
        run_lambda(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_runtime_returns_lambda_when_env_var_is_set() {
        assert_eq!(
            detect_runtime(Some("127.0.0.1:9001")),
            Runtime::Lambda,
            "presença da var indica execução Lambda"
        );
    }

    #[test]
    fn detect_runtime_returns_http_when_env_var_is_absent() {
        assert_eq!(detect_runtime(None), Runtime::Http);
    }

    #[test]
    fn detect_runtime_returns_http_when_env_var_is_empty() {
        assert_eq!(
            detect_runtime(Some("")),
            Runtime::Http,
            "valor vazio não deve ativar runtime Lambda"
        );
    }
}
