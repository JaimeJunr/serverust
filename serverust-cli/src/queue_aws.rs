//! Chamadas AWS SQS para `serverust queue inspect/tail`.

use aws_sdk_sqs::types::{MessageSystemAttributeName, QueueAttributeName};
use aws_sdk_sqs::Client;

use crate::queue::{MessageSummary, QueueAttributes};

async fn client() -> Client {
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    Client::new(&config)
}

pub async fn get_attributes(url: &str) -> anyhow::Result<QueueAttributes> {
    let sqs = client().await;
    let out = sqs
        .get_queue_attributes()
        .queue_url(url)
        .attribute_names(QueueAttributeName::All)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("GetQueueAttributes: {e}"))?;

    let find = |name: &str| -> Option<String> {
        out.attributes()
            .and_then(|m| {
                m.iter()
                    .find(|(k, _)| k.as_str() == name)
                    .map(|(_, v)| v.clone())
            })
    };

    Ok(QueueAttributes {
        url: url.to_string(),
        approx_messages: find("ApproximateNumberOfMessages"),
        approx_age_oldest: find("ApproximateAgeOfOldestMessage"),
        redrive_policy: find("RedrivePolicy"),
    })
}

pub async fn receive_messages(url: &str, max: i32) -> anyhow::Result<Vec<MessageSummary>> {
    let sqs = client().await;
    let out = sqs
        .receive_message()
        .queue_url(url)
        .max_number_of_messages(max)
        .message_system_attribute_names(MessageSystemAttributeName::SentTimestamp)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("ReceiveMessage: {e}"))?;

    let msgs = out
        .messages()
        .iter()
        .map(|m| MessageSummary {
            message_id: m.message_id().unwrap_or("(no id)").to_string(),
            body_preview: m.body().unwrap_or("").to_string(),
            sent_timestamp: m
                .attributes()
                .and_then(|a| a.get(&MessageSystemAttributeName::SentTimestamp))
                .cloned(),
        })
        .collect();

    Ok(msgs)
}
