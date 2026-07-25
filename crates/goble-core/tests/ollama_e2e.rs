use std::process::{Command, Stdio};
use std::time::Duration;

use futures::StreamExt;
use goble_core::llm::{CompletionRequest, LlmProvider};

fn find_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().unwrap().port()
}

#[tokio::test]
async fn test_ollama_provider_e2e() {
    let port = find_free_port();
    let mut child = Command::new("python3")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ollama_server.py"))
        .arg(port.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ollama mock server");

    // Wait for server to start
    let base_url = format!("http://127.0.0.1:{}", port);
    let mut ready = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Ok(resp) = reqwest::get(format!("{}/api/generate", base_url)).await {
            if resp.status().as_u16() != 404 {
                ready = true;
                break;
            }
        }
    }
    assert!(ready, "mock server did not start");

    let provider = goble_core::llm::OllamaProvider::new(base_url.clone());
    let request = CompletionRequest::new("ollama", "llama3.1").with_messages(vec![goble_core::llm::Message { role: goble_core::llm::Role::User, content: "hi".to_string(), tool_calls: None }]);
    let mut stream = provider.complete_stream(request).await.expect("stream");
    let mut content = String::new();
    while let Some(event) = stream.next().await {
        match event {
            goble_core::llm::CompletionStreamEvent::AssistantDelta(delta) => content.push_str(&delta),
            goble_core::llm::CompletionStreamEvent::Done => break,
            goble_core::llm::CompletionStreamEvent::Error(msg) => eprintln!("stream error: {}", msg),
            _ => {}
        }
    }

    let _ = child.kill();
    eprintln!("content: {:?}", content);
    assert!(content.contains("Hello from llama3.1"));
    assert!(content.contains("Prompt: hi"));
}
