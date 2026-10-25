use clisage::{Config, Mode, build_messages, request};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

fn server(status: &str, body: &str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let handle = thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut data = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let n = socket.read(&mut buffer).unwrap();
            if n == 0 {
                break;
            }
            data.extend_from_slice(&buffer[..n]);
            if let Some(index) = data.windows(4).position(|s| s == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&data[..index]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_lowercase()
                            .strip_prefix("content-length: ")
                            .and_then(|n| n.parse::<usize>().ok())
                    })
                    .unwrap();
                if data.len() >= index + 4 + length {
                    break;
                }
            }
        }
        socket.write_all(response.as_bytes()).unwrap();
        String::from_utf8(data).unwrap()
    });
    (address, handle)
}

#[test]
fn transport_sends_bearer_scope_privacy_setting_and_pipeline_content() {
    let (url, handle) = server(
        "200 OK",
        "data: {\"choices\":[{\"delta\":{\"content\":\"answer\"}}]}\n\ndata: [DONE]\n\n",
    );
    let mut output = Vec::new();
    request(
        &reqwest::blocking::Client::new(),
        &url,
        "fake-test-key",
        &Config::default(),
        build_messages(Mode::Prompt, "summarize", "diff data").unwrap(),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, b"answer");
    let captured = handle.join().unwrap();
    assert!(captured.contains("Bearer fake-test-key"));
    let body: serde_json::Value =
        serde_json::from_str(captured.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert_eq!(body["model"], "gpt-6-luna");
    assert_eq!(body["reasoning_effort"], "none");
    assert!(
        body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("diff data")
    );
}

#[test]
fn model_override_does_not_inherit_luna_specific_parameters() {
    let (url, handle) = server("200 OK", "data: [DONE]\n\n");
    let config = Config {
        model: "test-custom-model".into(),
        ..Config::default()
    };
    request(
        &reqwest::blocking::Client::new(),
        &url,
        "fake-test-key",
        &config,
        vec![],
        &mut Vec::new(),
    )
    .unwrap();
    let captured = handle.join().unwrap();
    let body: serde_json::Value =
        serde_json::from_str(captured.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(body["model"], "test-custom-model");
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn provider_errors_are_classified_without_echoing_response_or_key() {
    for (status, expected) in [
        ("401 Unauthorized", "credentials"),
        ("429 Too Many Requests", "quota"),
        ("503 Unavailable", "temporarily"),
        ("400 Bad Request", "model"),
    ] {
        let (url, handle) = server(status, "fake-sensitive-provider-payload");
        let error = request(
            &reqwest::blocking::Client::new(),
            &url,
            "fake-test-key",
            &Config::default(),
            vec![],
            &mut Vec::new(),
        )
        .unwrap_err()
        .to_string();
        handle.join().unwrap();
        assert!(error.contains(expected));
        assert!(!error.contains("fake-sensitive-provider-payload"));
        assert!(!error.contains("fake-test-key"));
    }
}
