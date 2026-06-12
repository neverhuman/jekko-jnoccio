use super::openai_compatible::OpenAICompatibleClient;

pub fn client(http: reqwest::Client, base_url: String, api_key: String) -> OpenAICompatibleClient {
    OpenAICompatibleClient::new(
        http,
        base_url,
        api_key,
        "cloudflare-workers-ai".to_string(),
        "cloudflare_workers_ai".to_string(),
    )
}
