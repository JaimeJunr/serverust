use std::sync::{Arc, Mutex};

use serverust_core::{App, Container, EventError, EventHandler};

#[derive(Clone)]
struct PaymentEvent {
    amount: u64,
}

struct LedgerHandler {
    ledger: Arc<Mutex<u64>>,
}

impl EventHandler<PaymentEvent> for LedgerHandler {
    async fn handle(&self, event: PaymentEvent, _ctx: &Container) -> Result<(), EventError> {
        *self.ledger.lock().unwrap() += event.amount;
        Ok(())
    }
}

struct NotifyHandler {
    notified: Arc<Mutex<Vec<u64>>>,
}

impl EventHandler<PaymentEvent> for NotifyHandler {
    async fn handle(&self, event: PaymentEvent, _ctx: &Container) -> Result<(), EventError> {
        self.notified.lock().unwrap().push(event.amount);
        Ok(())
    }
}

struct ServiceCheckHandler;

impl EventHandler<PaymentEvent> for ServiceCheckHandler {
    async fn handle(&self, event: PaymentEvent, ctx: &Container) -> Result<(), EventError> {
        let threshold: Arc<u64> = ctx
            .get::<u64>()
            .ok_or_else(|| EventError("threshold not found".into()))?;
        if event.amount > *threshold {
            return Err(EventError(format!(
                "amount {} exceeds threshold {}",
                event.amount, threshold
            )));
        }
        Ok(())
    }
}

#[tokio::test]
async fn app_event_registra_handler_e_dispatcher_despacha() {
    let ledger = Arc::new(Mutex::new(0u64));
    let dispatcher = App::new()
        .event::<PaymentEvent, _>(LedgerHandler {
            ledger: Arc::clone(&ledger),
        })
        .into_event_dispatcher::<PaymentEvent>();

    dispatcher
        .dispatch_event(PaymentEvent { amount: 100 })
        .await
        .unwrap();

    assert_eq!(*ledger.lock().unwrap(), 100);
}

#[tokio::test]
async fn app_event_injeta_state_do_container_no_handler() {
    let threshold = Arc::new(500u64);
    let dispatcher = App::new()
        .provide::<u64>(Arc::clone(&threshold))
        .event::<PaymentEvent, _>(ServiceCheckHandler)
        .into_event_dispatcher::<PaymentEvent>();

    // Abaixo do threshold: ok
    dispatcher
        .dispatch_event(PaymentEvent { amount: 300 })
        .await
        .unwrap();

    // Acima do threshold: erro
    let err = dispatcher
        .dispatch_event(PaymentEvent { amount: 600 })
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn app_event_multiplos_handlers_executam_em_sequencia() {
    let ledger = Arc::new(Mutex::new(0u64));
    let notified = Arc::new(Mutex::new(Vec::<u64>::new()));

    let dispatcher = App::new()
        .event::<PaymentEvent, _>(LedgerHandler {
            ledger: Arc::clone(&ledger),
        })
        .event::<PaymentEvent, _>(NotifyHandler {
            notified: Arc::clone(&notified),
        })
        .into_event_dispatcher::<PaymentEvent>();

    dispatcher
        .dispatch_event(PaymentEvent { amount: 42 })
        .await
        .unwrap();

    assert_eq!(*ledger.lock().unwrap(), 42);
    assert_eq!(*notified.lock().unwrap(), vec![42]);
}

#[tokio::test]
async fn http_tests_nao_sao_afetados_por_event_handlers() {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use serverust_macros::get;
    use tower::ServiceExt;

    #[get("/ping")]
    async fn ping() -> &'static str {
        "pong"
    }

    let router = App::new()
        .event::<PaymentEvent, _>(LedgerHandler {
            ledger: Arc::new(Mutex::new(0)),
        })
        .route(ping)
        .into_router();

    let resp = router
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/ping")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}
