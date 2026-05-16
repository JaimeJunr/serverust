use aws_lambda_events::event::kafka::KafkaEvent;
use lambda_runtime::LambdaEvent;
use serverust_core::{App, Container, EventError, EventHandler};
use serverust_lambda::run_event_lambda_handler;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

struct CreditHandler {
    processed: Arc<Mutex<Vec<u64>>>,
}

impl EventHandler<KafkaEvent> for CreditHandler {
    async fn handle(&self, event: KafkaEvent, _ctx: &Container) -> Result<(), EventError> {
        for records in event.records.values() {
            for record in records {
                if let Some(value) = &record.value {
                    let amount: u64 = value
                        .parse()
                        .map_err(|_| EventError(format!("invalid amount: {value}")))?;
                    self.processed.lock().unwrap().push(amount);
                }
            }
        }
        Ok(())
    }
}

struct FailingCreditHandler;

impl EventHandler<KafkaEvent> for FailingCreditHandler {
    async fn handle(&self, _event: KafkaEvent, _ctx: &Container) -> Result<(), EventError> {
        Err(EventError("simulated processing failure".into()))
    }
}

fn make_kafka_event(records: Vec<(&str, &str)>) -> KafkaEvent {
    let json = {
        let mut entries: Vec<String> = Vec::new();
        let mut by_topic: HashMap<String, Vec<String>> = HashMap::new();
        for (topic, value) in records {
            by_topic.entry(topic.to_string()).or_default().push(
                format!(
                    r#"{{"topic":"{topic}","partition":0,"offset":0,"timestamp":0,"timestampType":"CREATE_TIME","key":null,"value":"{value}","headers":[]}}"#
                ),
            );
        }
        for (topic, recs) in &by_topic {
            entries.push(format!(r#""{topic}":[{}]"#, recs.join(",")));
        }
        format!(r#"{{"records":{{{}}}}}"#, entries.join(","))
    };
    serde_json::from_str::<KafkaEvent>(&json).expect("valid kafka event json")
}

#[tokio::test]
async fn kafka_event_handler_processa_records_e_retorna_sem_batch_failures() {
    let processed = Arc::new(Mutex::new(Vec::<u64>::new()));
    let dispatcher = App::new()
        .event::<KafkaEvent, _>(CreditHandler {
            processed: Arc::clone(&processed),
        })
        .into_event_dispatcher::<KafkaEvent>();

    let kafka_event = make_kafka_event(vec![("wallet.credits", "100"), ("wallet.credits", "200")]);
    dispatcher
        .dispatch_event(kafka_event)
        .await
        .expect("handler deve processar sem erro");

    let amounts = processed.lock().unwrap().clone();
    assert!(!amounts.is_empty(), "deve ter processado registros");
    assert!(amounts.contains(&100) || amounts.contains(&200));
}

#[tokio::test]
async fn kafka_event_handler_falho_retorna_event_error() {
    let dispatcher = App::new()
        .event::<KafkaEvent, _>(FailingCreditHandler)
        .into_event_dispatcher::<KafkaEvent>();

    let kafka_event = make_kafka_event(vec![("wallet.credits", "50")]);
    let result = dispatcher.dispatch_event(kafka_event).await;

    assert!(result.is_err(), "handler falho deve propagar EventError");
    let err = result.unwrap_err();
    assert!(err.0.contains("simulated processing failure"));
}

#[tokio::test]
async fn app_has_event_handlers_reflete_registro() {
    let app_sem_handlers = App::new();
    assert!(!app_sem_handlers.has_event_handlers());

    let app_com_handler = App::new().event::<KafkaEvent, _>(FailingCreditHandler);
    assert!(app_com_handler.has_event_handlers());
}

#[tokio::test]
async fn run_event_lambda_handler_executa_handler_com_kafka_event() {
    let processed = Arc::new(Mutex::new(Vec::<u64>::new()));
    let dispatcher = App::new()
        .event::<KafkaEvent, _>(CreditHandler {
            processed: Arc::clone(&processed),
        })
        .into_event_dispatcher::<KafkaEvent>();

    let kafka_event = make_kafka_event(vec![("wallet.credits", "42")]);
    let ctx = serde_json::from_str::<lambda_runtime::Context>(r#"{"requestId":"test","deadlineMs":0,"invokedFunctionArn":"arn:aws:lambda:us-east-1:123:function:test","xrayTraceId":"trace","clientContext":null,"identity":null,"env":{}}"#).unwrap_or_default();
    let lambda_event = LambdaEvent::new(kafka_event, ctx);

    run_event_lambda_handler(dispatcher, lambda_event)
        .await
        .expect("deve processar sem erro");

    let amounts = processed.lock().unwrap().clone();
    assert!(amounts.contains(&42));
}
