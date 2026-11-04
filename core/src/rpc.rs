use crate::error::{Error, Result};

/// The app-server speaks newline-delimited JSON over stdio: one complete JSON
/// object per line, no `Content-Length` header. Verified against
/// `codex app-server` 0.164.0 — an LSP-style frame is accepted and then the
/// connection is closed without a reply, which is why usage read as empty.
pub fn encode_request(id: u64, method: &str) -> String {
    encode_request_with(id, method, "{}")
}

/// A request with an explicit params object, already serialised as JSON.
pub fn encode_request_with(id: u64, method: &str, params: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"{method}\",\"params\":{params}}}\n")
}

/// A notification carries no id and expects no reply (`initialized`).
pub fn encode_notification(method: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"method\":\"{method}\",\"params\":{{}}}}\n")
}

/// Parse one line of server output. `None` means the line is not a reply to
/// `want_id` (another id, a notification, or noise the server logged).
pub fn decode_line(line: &str, want_id: u64) -> Option<Result<serde_json::Value>> {
    let trimmed = line.trim();
    if trimmed.is_empty() || !trimmed.starts_with('{') {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    if v.get("id").and_then(|i| i.as_u64()) != Some(want_id) {
        return None;
    }
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Some(Err(Error::Msg(format!("app-server error: {msg}"))));
    }
    Some(Ok(v
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null)))
}

/// Scan a whole captured transcript for the reply to `want_id`.
pub fn decode_response(raw: &str, want_id: u64) -> Result<serde_json::Value> {
    let mut saw_any = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('{') {
            continue;
        }
        saw_any = true;
        if let Some(reply) = decode_line(trimmed, want_id) {
            return reply;
        }
    }
    if saw_any {
        Err(Error::Msg(format!("no response for request id {want_id}")))
    } else {
        Err(Error::Msg("no response from app-server".into()))
    }
}
