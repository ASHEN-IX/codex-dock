use codex_dock_core::rpc;

#[test]
fn encodes_newline_delimited_json_not_content_length() {
    // The app-server speaks JSONL on stdio. An LSP frame is accepted and then
    // the connection is closed with no reply, which is the bug this pins.
    let frame = rpc::encode_request(2, "account/rateLimits/read");
    assert!(frame.ends_with('\n'), "each message is one line: {frame:?}");
    assert_eq!(frame.lines().count(), 1, "must be a single line");
    assert!(
        !frame.contains("Content-Length"),
        "app-server does not use LSP framing: {frame:?}"
    );
    assert!(frame.contains("\"method\":\"account/rateLimits/read\""));
    assert!(frame.contains("\"id\":2"));
    assert!(frame.contains("\"jsonrpc\":\"2.0\""));
}

#[test]
fn initialize_carries_the_required_client_info() {
    let req = rpc::encode_request_with(
        1,
        "initialize",
        r#"{"clientInfo":{"name":"codex-dock","version":"0.1.0"}}"#,
    );
    assert!(
        req.contains("clientInfo"),
        "clientInfo is required: {req:?}"
    );
    assert!(req.contains("\"name\":\"codex-dock\""));
    assert!(req.contains("\"version\""));
}

#[test]
fn notification_has_no_id() {
    let n = rpc::encode_notification("initialized");
    assert!(!n.contains("\"id\""), "notifications carry no id: {n:?}");
    assert!(n.contains("initialized"));
}

#[test]
fn decodes_a_jsonl_response_by_id() {
    let body =
        r#"{"jsonrpc":"2.0","id":7,"result":{"rateLimits":{"primary":{"usedPercent":87.5}}}}"#;
    let v = rpc::decode_response(body, 7).unwrap();
    assert_eq!(
        v.get("rateLimits")
            .and_then(|r| r.get("primary"))
            .and_then(|p| p.get("usedPercent"))
            .and_then(|x| x.as_f64()),
        Some(87.5)
    );
}

#[test]
fn decode_skips_the_initialize_reply_and_matches_the_right_id() {
    let init = r#"{"jsonrpc":"2.0","id":1,"result":{"userAgent":"x"}}"#;
    let want =
        r#"{"jsonrpc":"2.0","id":2,"result":{"rateLimits":{"secondary":{"usedPercent":40}}}}"#;
    let raw = format!("{init}\n{want}\n");
    let v = rpc::decode_response(&raw, 2).unwrap();
    assert_eq!(
        v.get("rateLimits")
            .and_then(|r| r.get("secondary"))
            .and_then(|p| p.get("usedPercent"))
            .and_then(|x| x.as_f64()),
        Some(40.0)
    );
}

#[test]
fn decode_ignores_non_json_and_notification_lines() {
    let raw = concat!(
        "some log line the server printed\n",
        r#"{"jsonrpc":"2.0","method":"account/updated","params":{}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":4,"result":{"summary":{"lifetimeTokens":755580136}}}"#,
        "\n"
    );
    let v = rpc::decode_response(raw, 4).unwrap();
    assert_eq!(
        v.get("summary")
            .and_then(|s| s.get("lifetimeTokens"))
            .and_then(|x| x.as_i64()),
        Some(755_580_136)
    );
}

#[test]
fn decode_errors_clearly_when_the_response_never_arrives() {
    let err = rpc::decode_response("", 5).unwrap_err();
    assert!(err.to_string().contains("no response"), "got {err}");
}

#[test]
fn decode_surfaces_a_jsonrpc_error_payload() {
    let body = r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"method not found"}}"#;
    let err = rpc::decode_response(body, 3).unwrap_err();
    assert!(err.to_string().contains("method not found"), "got {err}");
}
