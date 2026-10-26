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
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
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

#[test]
fn command_only_mode_requests_a_single_plain_command() {
    let messages = build_messages(Mode::ShellCommand, "find large files", "").unwrap();
    let instruction = messages[0]["content"].as_str().unwrap();
    assert!(instruction.contains("single line"));
    assert!(instruction.contains("no Markdown"));
    assert!(instruction.contains("Never execute"));
}

#[test]
fn command_only_output_rejects_control_sequences_multiline_and_markdown() {
    use clisage::command_suggestion;
    assert_eq!(command_suggestion("  ls -lh\n").unwrap(), "ls -lh");
    assert_eq!(
        command_suggestion("printf 'café'").unwrap(),
        "printf 'café'"
    );
    for invalid in [
        "",
        "  ",
        "ls\nwhoami",
        "ls\rwhoami",
        "ls\t-l",
        "\u{1b}[31mls",
        "```sh\nls\n```",
        "```ls```",
        "ls\u{202e}abc",
    ] {
        assert!(command_suggestion(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn streaming_requires_a_successful_nonempty_completion() {
    for reason in ["length", "content_filter", "tool_calls", "unknown"] {
        let input = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"find . -delete\"}},\"finish_reason\":\"{reason}\"}}]}}\n\ndata: [DONE]\n\n"
        );
        assert!(
            stream_response(input.as_bytes(), &mut Vec::new()).is_err(),
            "accepted {reason}"
        );
    }
    for input in [
        "data: [DONE]\n\n",
        "data: {}\n\ndata: [DONE]\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\ndata: [DONE]\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        "data: {\"choices\":[{\"delta\":{\"refusal\":\"fake refusal\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
    ] {
        assert!(stream_response(input.as_bytes(), &mut Vec::new()).is_err());
    }
}

#[test]
fn streaming_bounds_lines_events_and_total_output() {
    let oversized_line = format!("data: {}", "x".repeat(65_537));
    assert!(
        stream_response(oversized_line.as_bytes(), &mut Vec::new())
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
    let oversized_event = format!("{}\n", "data: x\n".repeat(40_000));
    assert!(
        stream_response(oversized_event.as_bytes(), &mut Vec::new())
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
    let chunk = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{}\"}}}}]}}\n\n",
        "a".repeat(32_768)
    );
    let mut output = Vec::new();
    assert!(
        stream_response(chunk.repeat(33).as_bytes(), &mut output)
            .unwrap_err()
            .to_string()
            .contains("limit")
    );
    assert!(output.len() <= 1_048_576);
}

#[test]
fn successful_completion_allows_usage_but_rejects_additional_content() {
    let completed =
        "data: {\"choices\":[{\"delta\":{\"content\":\"answer\"},\"finish_reason\":\"stop\"}]}\n\n";
    let usage = format!(
        "{completed}data: {{\"choices\":[],\"usage\":{{\"total_tokens\":1}}}}\n\ndata: [DONE]\n\n"
    );
    let mut output = Vec::new();
    stream_response(usage.as_bytes(), &mut output).unwrap();
    assert_eq!(output, b"answer");
    let extra = format!(
        "{completed}data: {{\"choices\":[{{\"delta\":{{\"content\":\"unexpected\"}}}}]}}\n\ndata: [DONE]\n\n"
    );
    assert!(stream_response(extra.as_bytes(), &mut Vec::new()).is_err());
}

#[test]
fn keepalives_cannot_bypass_the_total_stream_limit() {
    let stream = ": keepalive\n\n".repeat(700_000);
    let error = stream_response(stream.as_bytes(), &mut Vec::new()).unwrap_err();
    assert!(error.to_string().contains("size limit"));
}
