use crate::openai::ChatCompletionRequest;
use crate::providers::openai_compatible::{
    OpenAICompatibleClient, ProviderError, UpstreamCompletion,
};

pub mod cloudflare_workers;
pub mod completion_common;
pub mod openai_compatible;
pub mod responses;

pub enum ProviderClient {
    OpenAI(OpenAICompatibleClient),
    Responses(responses::ResponsesClient),
}

impl ProviderClient {
    pub async fn complete(
        &self,
        request: &ChatCompletionRequest,
        body: serde_json::Value,
    ) -> Result<UpstreamCompletion, ProviderError> {
        match self {
            ProviderClient::OpenAI(client) => client.complete(request, body).await,
            ProviderClient::Responses(client) => client.complete(request, body).await,
        }
    }
}

pub fn client(
    http: reqwest::Client,
    style: &str,
    base_url: String,
    api_key: String,
    provider: String,
) -> ProviderClient {
    if style == "cloudflare_workers_ai" || style == "cloudflare_openai" {
        return ProviderClient::OpenAI(cloudflare_workers::client(http, base_url, api_key));
    }
    if style == "openai_responses" {
        return ProviderClient::Responses(responses::ResponsesClient::new(
            http,
            base_url,
            api_key,
            provider,
            style.to_string(),
        ));
    }
    ProviderClient::OpenAI(OpenAICompatibleClient::new(
        http,
        base_url,
        api_key,
        provider,
        style.to_string(),
    ))
}
