use super::*;

#[test]
fn serializes_chat_completion_request() {
    let provider = OpenAiLikeProvider::new(
        OpenAiLikeProviderConfig {
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: Some("ollama".to_string()),
            api_key_env: None,
        },
        ProviderKind::Ollama,
    )
    .unwrap();

    let body = provider.request_body(&GenerateTextRequest {
        system_prompt: "system".to_string(),
        user_prompt: "user".to_string(),
        model: "llama3.2".to_string(),
        temperature: 0.4,
        max_tokens: 100,
    });

    assert_eq!(body.model, "llama3.2");
    assert_eq!(body.messages.len(), 2);
    assert_eq!(body.messages[0].role, "system");
    assert!(!body.stream);
}
