//! Validação do logger JSON: garantir que `tracing::info!` produz uma linha
//! JSON parseável contendo o campo `correlation_id` quando estamos dentro de
//! um span aberto pelo middleware.

use std::sync::{Arc, Mutex};

use serverust_telemetry::{json_subscriber, logger};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Buffer {
    fn snapshot(&self) -> Vec<u8> {
        self.0.lock().unwrap().clone()
    }
}

impl std::io::Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Buffer {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn logger_emits_json_with_correlation_field() {
    let buffer = Buffer::default();
    let subscriber = json_subscriber(EnvFilter::new("info"), buffer.clone());

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("request", correlation_id = "1-abc-def");
        let _guard = span.enter();
        tracing::info!(action = "processed", "handler concluído");
    });

    let raw = buffer.snapshot();
    let line = std::str::from_utf8(&raw)
        .expect("output deve ser UTF-8")
        .lines()
        .next()
        .expect("ao menos uma linha de log");
    let value: serde_json::Value = serde_json::from_str(line).expect("output deve ser JSON válido");

    assert_eq!(value["level"], "INFO");
    assert!(
        value["fields"]["message"]
            .as_str()
            .map(|m| m.contains("handler concluído"))
            .unwrap_or(false),
        "mensagem ausente: {value}"
    );
    assert_eq!(
        value["span"]["correlation_id"], "1-abc-def",
        "correlation_id deve aparecer no span: {value}"
    );
}

#[test]
fn init_is_idempotent() {
    // Não devemos panicar nem retornar erro em chamadas múltiplas.
    logger::init();
    logger::init();
}
