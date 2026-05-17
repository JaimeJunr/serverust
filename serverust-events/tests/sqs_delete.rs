//! Testes para `sqs::delete::DeleteManager` (US-003).
//!
//! `DeleteManager` abstrai o `DeleteMessageBatch` do AWS SDK via trait
//! `SqsDeleteClient`, implementando retry exponencial com log de warning.
//! Estes testes usam um mock in-memory para verificar o comportamento sem
//! dependência de AWS.

#![cfg(feature = "sqs")]

use std::sync::{Arc, Mutex};

use serverust_events::sqs::delete::{DeleteClient, DeleteEntry, DeleteManager, DeleteResult};

/// Mock que registra as chamadas e retorna falha nas IDs configuradas.
struct MockDeleteClient {
    calls: Arc<Mutex<Vec<Vec<DeleteEntry>>>>,
    fail_ids: Vec<String>,
    network_error: bool,
}

impl MockDeleteClient {
    fn new(fail_ids: Vec<&str>) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            fail_ids: fail_ids.iter().map(|s| s.to_string()).collect(),
            network_error: false,
        }
    }

    fn with_network_error() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            fail_ids: vec![],
            network_error: true,
        }
    }

    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl DeleteClient for MockDeleteClient {
    async fn delete_batch(
        &self,
        _queue_url: &str,
        entries: Vec<DeleteEntry>,
    ) -> Result<DeleteResult, String> {
        self.calls.lock().unwrap().push(entries.clone());

        if self.network_error {
            return Err("simulated network error".into());
        }

        let failed: Vec<String> = entries
            .iter()
            .filter(|e| self.fail_ids.contains(&e.id))
            .map(|e| e.id.clone())
            .collect();

        Ok(DeleteResult { failed })
    }
}

// --- Testes ---

#[tokio::test]
async fn delete_manager_sucesso_total_uma_chamada() {
    let client = Arc::new(MockDeleteClient::new(vec![]));
    let manager = DeleteManager::new(client.clone()).with_zero_backoff();

    let entries = vec![
        DeleteEntry::new("id-1", "rh-1"),
        DeleteEntry::new("id-2", "rh-2"),
    ];
    manager
        .delete_successful("https://sqs.us-east-1.amazonaws.com/123/orders", entries)
        .await;

    assert_eq!(client.call_count(), 1);
}

#[tokio::test]
async fn delete_manager_falha_parcial_retenta_apenas_os_que_falharam() {
    // id-2 falha na primeira chamada, succeeds after retry (fail_ids vazio a partir do 2o attempt
    // simulado por: fail_ids = ["id-2"] mas mock falha apenas 1x)
    // Para simplicidade usamos um mock que falha sempre a id "id-2".
    let client = Arc::new(MockDeleteClient::new(vec!["id-2"]));
    let manager = DeleteManager::new(client.clone())
        .with_zero_backoff()
        .with_max_retries(3);

    let entries = vec![
        DeleteEntry::new("id-1", "rh-1"),
        DeleteEntry::new("id-2", "rh-2"),
        DeleteEntry::new("id-3", "rh-3"),
    ];
    manager
        .delete_successful("https://sqs.us-east-1.amazonaws.com/123/orders", entries)
        .await;

    // Primeira chamada: 3 entradas. Retenta "id-2" mais 2 vezes => 3 chamadas total.
    assert_eq!(
        client.call_count(),
        3,
        "esperado 3 chamadas (1 + 2 retries de id-2)"
    );

    // Nas últimas 2 chamadas, só id-2 foi reenviada.
    let all_calls = client.calls.lock().unwrap().clone();
    assert_eq!(all_calls[0].len(), 3);
    assert_eq!(all_calls[1].len(), 1);
    assert_eq!(all_calls[1][0].id, "id-2");
}

#[tokio::test]
async fn delete_manager_erro_de_rede_retenta_e_loga_warning() {
    let client = Arc::new(MockDeleteClient::with_network_error());
    let manager = DeleteManager::new(client.clone())
        .with_zero_backoff()
        .with_max_retries(2);

    let entries = vec![DeleteEntry::new("id-1", "rh-1")];
    // Não deve panic; deve logar e encerrar após max_retries.
    manager
        .delete_successful("https://sqs.us-east-1.amazonaws.com/123/orders", entries)
        .await;

    assert_eq!(client.call_count(), 2, "deve tentar max_retries vezes");
}

#[tokio::test]
async fn delete_manager_entradas_vazias_nao_chamam_o_cliente() {
    let client = Arc::new(MockDeleteClient::new(vec![]));
    let manager = DeleteManager::new(client.clone()).with_zero_backoff();

    manager
        .delete_successful("https://sqs.us-east-1.amazonaws.com/123/orders", vec![])
        .await;

    assert_eq!(client.call_count(), 0);
}

#[tokio::test]
async fn delete_entry_new_preenche_campos() {
    let e = DeleteEntry::new("my-id", "my-receipt-handle");
    assert_eq!(e.id, "my-id");
    assert_eq!(e.receipt_handle, "my-receipt-handle");
}
