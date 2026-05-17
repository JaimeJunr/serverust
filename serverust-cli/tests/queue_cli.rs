use clap::Parser;
use serverust_cli::cli::{Cli, Command, QueueCommand};
use serverust_cli::queue::{MessageSummary, QueueAttributes, format_inspect, format_messages};

const SAMPLE_URL: &str =
    "https://sqs.us-east-1.amazonaws.com/123456789012/orders";

// ── CLI parse tests ──────────────────────────────────────────────────────────

#[test]
fn parses_queue_inspect() {
    let cli = Cli::try_parse_from(["serverust", "queue", "inspect", SAMPLE_URL])
        .expect("parse");
    match cli.command {
        Command::Queue {
            command: QueueCommand::Inspect { url },
        } => assert_eq!(url, SAMPLE_URL),
        other => panic!("expected Queue::Inspect, got {other:?}"),
    }
}

#[test]
fn parses_queue_tail_default_max() {
    let cli =
        Cli::try_parse_from(["serverust", "queue", "tail", SAMPLE_URL]).expect("parse");
    match cli.command {
        Command::Queue {
            command: QueueCommand::Tail { url, max },
        } => {
            assert_eq!(url, SAMPLE_URL);
            assert_eq!(max, 5);
        }
        other => panic!("expected Queue::Tail, got {other:?}"),
    }
}

#[test]
fn parses_queue_tail_custom_max() {
    let cli =
        Cli::try_parse_from(["serverust", "queue", "tail", SAMPLE_URL, "--max", "10"])
            .expect("parse");
    match cli.command {
        Command::Queue {
            command: QueueCommand::Tail { max, .. },
        } => assert_eq!(max, 10),
        other => panic!("{other:?}"),
    }
}

#[test]
fn rejects_queue_tail_max_above_limit() {
    let result =
        Cli::try_parse_from(["serverust", "queue", "tail", SAMPLE_URL, "--max", "11"]);
    assert!(result.is_err(), "max > 10 deve ser rejeitado pelo clap (value_parser)");
}

// ── format_inspect tests ─────────────────────────────────────────────────────

#[test]
fn format_inspect_shows_all_fields() {
    let attrs = QueueAttributes {
        url: SAMPLE_URL.to_string(),
        approx_messages: Some("42".to_string()),
        approx_age_oldest: Some("3600".to_string()),
        redrive_policy: Some(
            r#"{"deadLetterTargetArn":"arn:aws:sqs:us-east-1:123:orders-dlq","maxReceiveCount":"3"}"#
                .to_string(),
        ),
    };
    let output = format_inspect(&attrs);
    assert!(output.contains("42"), "contagem de mensagens ausente");
    assert!(output.contains("3600"), "age ausente");
    assert!(output.contains("orders-dlq"), "redrive policy ausente");
    assert!(output.contains(SAMPLE_URL), "URL ausente");
}

#[test]
fn format_inspect_handles_none_fields() {
    let attrs = QueueAttributes {
        url: SAMPLE_URL.to_string(),
        approx_messages: None,
        approx_age_oldest: None,
        redrive_policy: None,
    };
    let output = format_inspect(&attrs);
    // com campos ausentes, deve exibir placeholder (ex.: "—")
    assert!(output.contains('—'), "placeholder ausente para campos None");
}

#[test]
fn format_inspect_includes_age_unit() {
    let attrs = QueueAttributes {
        url: SAMPLE_URL.to_string(),
        approx_messages: Some("1".to_string()),
        approx_age_oldest: Some("120".to_string()),
        redrive_policy: None,
    };
    let output = format_inspect(&attrs);
    assert!(output.contains("120"), "age value ausente");
    // deve incluir unidade de tempo ("s" ou "seconds")
    assert!(
        output.contains(" s") || output.contains("seconds"),
        "unidade de tempo ausente"
    );
}

// ── format_messages tests ─────────────────────────────────────────────────────

#[test]
fn format_messages_shows_ids_and_preview() {
    let msgs = vec![
        MessageSummary {
            message_id: "msg-aaa".to_string(),
            body_preview: "Hello, World!".to_string(),
            sent_timestamp: Some("1716000000000".to_string()),
        },
        MessageSummary {
            message_id: "msg-bbb".to_string(),
            body_preview: "Foo bar".to_string(),
            sent_timestamp: None,
        },
    ];
    let output = format_messages(&msgs);
    assert!(output.contains("msg-aaa"), "msg-aaa ausente");
    assert!(output.contains("msg-bbb"), "msg-bbb ausente");
    assert!(output.contains("Hello, World!"), "body preview ausente");
    assert!(output.contains("Foo bar"), "body preview ausente");
}

#[test]
fn format_messages_empty_returns_placeholder() {
    let output = format_messages(&[]);
    assert!(!output.is_empty(), "deve retornar texto para fila vazia");
    // deve mencionar "0" ou "vazia" ou "empty" ou equivalente
    assert!(
        output.contains('0') || output.contains("vaz") || output.contains("empty"),
        "placeholder para fila vazia ausente"
    );
}

#[test]
fn format_messages_truncates_long_body() {
    let long_body = "x".repeat(300);
    let msgs = vec![MessageSummary {
        message_id: "msg-long".to_string(),
        body_preview: long_body.clone(),
        sent_timestamp: None,
    }];
    let output = format_messages(&msgs);
    // preview não deve vazar além de 200 chars de body
    assert!(
        !output.contains(&long_body),
        "body longo não foi truncado"
    );
}
