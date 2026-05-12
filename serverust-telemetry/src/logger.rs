//! Configuração do logger estruturado JSON.
//!
//! [`init`] instala globalmente um `tracing-subscriber` JSON com filtro
//! controlado por `RUST_LOG` (default `info`). Idempotente: chamadas
//! sucessivas após a primeira são no-op silenciosos.
//!
//! Para testes (ou cenários que precisem inspecionar o output), use
//! [`init_with_writer`] ou monte um subscriber via [`json_subscriber`] e
//! instale-o com `tracing::subscriber::with_default`.

use std::io;
use std::sync::OnceLock;

use tracing::Subscriber;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::{MakeWriter, format::JsonFields};

static INIT: OnceLock<()> = OnceLock::new();

/// Inicializa o logger JSON global. Chamadas subsequentes são no-op para
/// permitir uso em testes sem precisar coordenar ordem.
pub fn init() {
    INIT.get_or_init(|| {
        let subscriber = json_subscriber(env_filter(), io::stdout);
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

/// Variante de [`init`] que aceita um writer customizado. Útil para testes
/// que precisam capturar o output em buffer.
pub fn init_with_writer<W>(writer: W)
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    INIT.get_or_init(|| {
        let subscriber = json_subscriber(env_filter(), writer);
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

/// Constrói um subscriber JSON sem instalá-lo. Permite uso com
/// `tracing::subscriber::with_default` em testes para evitar estado global.
pub fn json_subscriber<W>(filter: EnvFilter, writer: W) -> impl Subscriber + Send + Sync + 'static
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .json()
        .fmt_fields(JsonFields::new())
        .with_current_span(true)
        .with_span_list(false)
        .with_target(true)
        .with_writer(writer)
        .with_env_filter(filter)
        .finish()
}

fn env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}
