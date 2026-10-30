use crate::error::{Error, Result};

/// LSP-style framing: `Content-Length: N\r\n\r\n` then N bytes of JSON.
pub fn encode_request(id: u64, method: &str) -> String {
    let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{}}}}"#);
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
}

fn frames(raw: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = raw;
    loop {
        let Some(hdr_end) = rest.find("\r\n\r\n") else { break };
        let header = &rest[..hdr_end];
        let Some(len) = header
            .lines()
            .find_map(|l| l.strip_prefix("Content-Length: "))
            .and_then(|v| v.trim().parse::<usize>().ok())
        else {
            break;
        };
        let body_start = hdr_end + 4;
        if rest.len() < body_start + len {
            break;
        }
        out.push(&rest[body_start..body_start + len]);
        rest = &rest[body_start + len..];
    }
    out
}

pub fn decode_response(raw: &str, want_id: u64) -> Result<serde_json::Value> {
    let mut saw_any = false;
    for body in frames(raw) {
        let v: serde_json::Value = match serde_json::from_str(body) {
            Ok(v) => v,
            Err(_) => continue,
        };
        saw_any = true;
        if v.get("id").and_then(|i| i.as_u64()) != Some(want_id) {
            continue;
        }
        if let Some(err) = v.get("error") {
            let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error");
            return Err(Error::Msg(format!("app-server error: {msg}")));
        }
        return Ok(v.get("result").cloned().unwrap_or(serde_json::Value::Null));
    }
    if saw_any {
        Err(Error::Msg(format!("no response for request id {want_id}")))
    } else {
        Err(Error::Msg("no response from app-server".into()))
    }
}
