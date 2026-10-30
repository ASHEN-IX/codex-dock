use codex_dock_core::rpc;

#[test]
fn encodes_a_content_length_framed_request() {
    let frame = rpc::encode_request(1, "account/rateLimits/read");
    assert!(frame.starts_with("Content-Length: "), "got {:?}", frame);
    assert!(frame.contains("\r\n\r\n"), "needs header/body separator");
    let body = frame.split("\r\n\r\n").nth(1).unwrap();
    assert!(body.contains("\"method\":\"account/rateLimits/read\""), "got {body:?}");
    assert!(body.contains("\"id\":1"));
    assert!(body.contains("\"jsonrpc\":\"2.0\""));
    let declared: usize = frame
        .lines()
        .find(|l| l.starts_with("Content-Length: "))
        .unwrap()
        .trim_start_matches("Content-Length: ")
        .parse()
        .unwrap();
    assert_eq!(declared, body.len(), "length must match the body's byte count");
}

#[test]
fn decodes_a_framed_response_by_id() {
    let body = r#"{"jsonrpc":"2.0","id":7,"result":{"five_hour_limit_percent":87.5}}"#;
    let raw = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    let v = rpc::decode_response(&raw, 7).unwrap();
    assert_eq!(v.get("five_hour_limit_percent").and_then(|x| x.as_f64()), Some(87.5));
}

#[test]
fn decode_skips_the_initialize_reply_and_matches_the_right_id() {
    let init = r#"{"jsonrpc":"2.0","id":0,"result":{}}"#;
    let want = r#"{"jsonrpc":"2.0","id":2,"result":{"weekly_limit_percent":40}}"#;
    let raw = format!(
        "Content-Length: {}\r\n\r\n{}Content-Length: {}\r\n\r\n{}",
        init.len(), init, want.len(), want
    );
    let v = rpc::decode_response(&raw, 2).unwrap();
    assert_eq!(v.get("weekly_limit_percent").and_then(|x| x.as_f64()), Some(40.0));
}

#[test]
fn decode_errors_clearly_when_the_response_never_arrives() {
    let err = rpc::decode_response("", 5).unwrap_err();
    assert!(err.to_string().contains("no response"), "got {err}");
}

#[test]
fn decode_surfaces_a_jsonrpc_error_payload() {
    let body = r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"method not found"}}"#;
    let raw = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
    let err = rpc::decode_response(&raw, 3).unwrap_err();
    assert!(err.to_string().contains("method not found"), "got {err}");
}
