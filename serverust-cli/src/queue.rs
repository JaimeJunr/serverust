//! Tipos e formatação para `serverust queue inspect/tail`.

const SEP: &str = "───────────────────────────────────────────────────────────";
const BOLD: &str = "\x1b[1m";
const CYAN: &str = "\x1b[36m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const BODY_PREVIEW_LEN: usize = 120;

pub struct QueueAttributes {
    pub url: String,
    pub approx_messages: Option<String>,
    pub approx_age_oldest: Option<String>,
    pub redrive_policy: Option<String>,
}

pub struct MessageSummary {
    pub message_id: String,
    pub body_preview: String,
    pub sent_timestamp: Option<String>,
}

pub fn format_inspect(attrs: &QueueAttributes) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "{BOLD}Queue:{RESET} {CYAN}{}{RESET}\n{SEP}\n",
        attrs.url
    ));

    let messages = attrs.approx_messages.as_deref().unwrap_or("—");
    let age_raw = attrs.approx_age_oldest.as_deref();
    let age = match age_raw {
        Some(v) => format!("{v} s"),
        None => "—".to_string(),
    };
    let redrive = attrs.redrive_policy.as_deref().unwrap_or("—");

    out.push_str(&row("Messages available:", messages));
    out.push_str(&row("Oldest message age:", &age));
    out.push_str(&row("Redrive policy:", redrive));
    out
}

pub fn format_messages(msgs: &[MessageSummary]) -> String {
    if msgs.is_empty() {
        return format!("{DIM}0 messages received (queue empty or all invisible){RESET}\n");
    }
    let mut out = String::new();
    for (i, msg) in msgs.iter().enumerate() {
        let preview = truncate(&msg.body_preview, BODY_PREVIEW_LEN);
        let ts = msg
            .sent_timestamp
            .as_deref()
            .map(|t| format!(" {DIM}(sent: {t}){RESET}"))
            .unwrap_or_default();
        out.push_str(&format!(
            "{BOLD}[{}]{RESET} {CYAN}{}{RESET}{ts}\n    {}\n",
            i + 1,
            msg.message_id,
            preview
        ));
    }
    out
}

fn row(label: &str, value: &str) -> String {
    format!("  {:<22} {value}\n", label)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
