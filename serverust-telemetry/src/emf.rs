//! Emissão de métricas no Embedded Metric Format (EMF) do CloudWatch.
//!
//! O EMF combina um payload JSON arbitrário com um bloco `_aws` que descreve
//! quais campos são métricas. CloudWatch extrai automaticamente quando a
//! linha aparece em stdout/stderr de uma Lambda (ou for ingerida no Logs).
//!
//! Para emitir de forma idiomática a partir de uma função, use a macro
//! `#[serverust_macros::metric(name = "...", unit = "...")]`.

use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::json;

/// Unidades aceitas pelo CloudWatch EMF. Mantemos como `&'static str` para
/// integração trivial com a macro `#[metric]`.
pub mod units {
    pub const MILLISECONDS: &str = "Milliseconds";
    pub const SECONDS: &str = "Seconds";
    pub const COUNT: &str = "Count";
    pub const BYTES: &str = "Bytes";
    pub const PERCENT: &str = "Percent";
}

/// Representação tipada de uma métrica EMF antes da serialização.
#[derive(Debug, Clone, Serialize)]
pub struct EmfMetric<'a> {
    pub namespace: &'a str,
    pub name: &'a str,
    pub unit: &'a str,
    pub value: f64,
}

/// Emite a métrica como uma linha JSON em `stdout`. Em ambiente Lambda o
/// CloudWatch ingere e extrai automaticamente.
pub fn emit_emf(namespace: &str, name: &str, unit: &str, value: f64) {
    let _ = emit_emf_to(io::stdout(), namespace, name, unit, value);
}

/// Variante de [`emit_emf`] com writer customizado — usado nos testes para
/// capturar e inspecionar o JSON gerado.
pub fn emit_emf_to<W: Write>(
    mut writer: W,
    namespace: &str,
    name: &str,
    unit: &str,
    value: f64,
) -> io::Result<()> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let payload = json!({
        "_aws": {
            "Timestamp": timestamp,
            "CloudWatchMetrics": [{
                "Namespace": namespace,
                "Dimensions": [[]],
                "Metrics": [{
                    "Name": name,
                    "Unit": unit,
                }],
            }],
        },
        name: value,
    });

    let mut line = serde_json::to_vec(&payload)?;
    line.push(b'\n');
    writer.write_all(&line)
}
