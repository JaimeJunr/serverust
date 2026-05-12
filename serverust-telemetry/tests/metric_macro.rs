//! Valida que a macro `#[serverust_macros::metric(...)]` instrumenta funções
//! sync e async — não conseguimos capturar `stdout` confiavelmente em testes
//! integrados, então o teste apenas exercita o caminho de expansão e
//! confirma o retorno preservado.

use serverust_macros::metric;

#[metric(name = "SyncMetric", unit = "Milliseconds")]
fn sync_doubled(x: i32) -> i32 {
    x * 2
}

#[metric(name = "AsyncMetric", unit = "Milliseconds", namespace = "TestApp")]
async fn async_doubled(x: i32) -> i32 {
    tokio::task::yield_now().await;
    x * 2
}

#[test]
fn sync_metric_preserves_return_value() {
    assert_eq!(sync_doubled(21), 42);
}

#[tokio::test]
async fn async_metric_preserves_return_value() {
    assert_eq!(async_doubled(21).await, 42);
}
