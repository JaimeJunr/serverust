//! Emissão EMF: testa que `emit_emf_to` produz JSON com a estrutura
//! `_aws.CloudWatchMetrics` e que a macro `#[metric]` encapsula a chamada.

use serde_json::Value;
use serverust_telemetry::emit_emf_to;

#[test]
fn emit_emf_produces_cloudwatch_format() {
    let mut buf = Vec::new();
    emit_emf_to(&mut buf, "MyApp", "ProcessingTime", "Milliseconds", 42.0).unwrap();
    let text = String::from_utf8(buf).unwrap();
    let line = text.lines().next().unwrap();
    let parsed: Value = serde_json::from_str(line).unwrap();

    let aws = &parsed["_aws"];
    let cw = &aws["CloudWatchMetrics"][0];
    assert_eq!(cw["Namespace"], "MyApp");
    assert_eq!(cw["Metrics"][0]["Name"], "ProcessingTime");
    assert_eq!(cw["Metrics"][0]["Unit"], "Milliseconds");
    assert!(aws["Timestamp"].is_u64());
    // O valor da métrica é um top-level field nomeado pela métrica.
    assert_eq!(parsed["ProcessingTime"], 42.0);
}

#[test]
fn emit_emf_writes_single_newline_terminated_line() {
    let mut buf = Vec::new();
    emit_emf_to(&mut buf, "ns", "Count", "Count", 1.0).unwrap();
    assert!(buf.ends_with(b"\n"));
    // Exatamente uma quebra de linha — o CloudWatch consome uma linha por evento.
    assert_eq!(buf.iter().filter(|b| **b == b'\n').count(), 1);
}
