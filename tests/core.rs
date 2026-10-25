use clisage::{Config, Mode, build_messages, stream_response};

#[test]
fn pipeline_and_prompt_are_both_sent_with_mode_instruction() {
    let messages = build_messages(Mode::Shell, "find large files", "input data").unwrap();
    assert!(
        messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("Never execute")
    );
    assert!(
        messages[1]["content"]
            .as_str()
            .unwrap()
            .contains("find large files")
    );
    assert!(
        messages[1]["content"]
            .as_str()
            .unwrap()
            .contains("input data")
    );
    assert!(build_messages(Mode::Prompt, "  ", "").is_err());
    assert!(build_messages(Mode::Explain, "", "ls -l").is_ok());
}

#[test]
fn streaming_decodes_chunks_and_requires_completion() {
    let input = b": keepalive\n\ndata: {";
    assert!(stream_response(&input[..], &mut Vec::new()).is_err());
    let input = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\r\n\r\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
        "data: [DONE]\n\n"
    );
    let mut output = Vec::new();
    stream_response(input.as_bytes(), &mut output).unwrap();
    assert_eq!(output, b"hello world");
    assert!(stream_response(&b"data: {}\n\n"[..], &mut Vec::new()).is_err());
    assert!(
        stream_response(
            &b"data: {\"error\":{\"message\":\"secret\"}}\n\n"[..],
            &mut Vec::new()
        )
        .unwrap_err()
        .to_string()
        .find("secret")
        .is_none()
    );
}

#[test]
fn config_has_fixed_provider_and_credential_scope() {
    let config = Config::default();
    let encoded = toml::to_string(&config).unwrap();
    assert!(encoded.contains("config_version = 1"));
    assert!(encoded.contains("pass"));
    assert!(
        toml::from_str::<Config>(
            "config_version=99\nmodel='fake-model'\ncredential_backend='pass'"
        )
        .is_err()
    );
}
