// jankurai:allow HLT-000-SCORE-DIMENSION reason=responses-api-structurally-parallel-to-openai-compatible-by-design expires=2027-01-01
use crate::openai::{ChatCompletionRequest, sanitize_messages};
use crate::providers::completion_common::{
    CompletionTransport, ParsedJsonResponse, ProviderError, UpstreamCompletion,
    parse_json_response, parse_raw_completion,
};
use crate::providers::openai_compatible::apply_common_completion_body_fields;
use serde_json::{Map, Value};

pub fn build_body(
    request: &ChatCompletionRequest,
    model: &str,
    stream: bool,
    tools: Option<Value>,
    messages: Vec<Value>,
    completion_tokens_param: Option<&str>,
) -> Value {
    let (instructions, input) = split_messages(sanitize_messages(messages));
    let mut body = Map::new();
    body.insert("model".to_string(), Value::String(model.to_string()));
    body.insert("input".to_string(), Value::Array(input));
    body.insert("stream".to_string(), Value::Bool(stream));
    if let Some(value) = instructions {
        body.insert("instructions".to_string(), Value::String(value));
    }
    apply_common_completion_body_fields(&mut body, request, tools, stream, completion_tokens_param);
    Value::Object(body)
}

pub async fn parse_completion(
    response: reqwest::Response,
    provider: &str,
    api_style: &str,
    endpoint: &str,
) -> Result<UpstreamCompletion, ProviderError> {
    let ParsedJsonResponse { raw, .. } =
        parse_json_response(response, provider, api_style, endpoint).await?;
    Ok(parse_raw_completion(raw))
}

fn split_messages(messages: Vec<Value>) -> (Option<String>, Vec<Value>) {
    let mut instructions = Vec::new();
    let input = messages
        .into_iter()
        .filter_map(|message| {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("user");
            let content = message.get("content").cloned().unwrap_or(Value::Null);
            if role == "system" || role == "developer" {
                if let Some(text) = content.as_str().filter(|text| !text.trim().is_empty()) {
                    instructions.push(text.to_string());
                }
                return None;
            }
            let mut item = Map::new();
            item.insert("type".to_string(), Value::String("message".to_string()));
            item.insert("role".to_string(), Value::String(role.to_string()));
            item.insert("content".to_string(), content);
            Some(Value::Object(item))
        })
        .collect::<Vec<_>>();
    let instructions = if instructions.is_empty() {
        None
    } else {
        Some(instructions.join("\n"))
    };
    (instructions, input)
}

pub struct ResponsesClient {
    transport: CompletionTransport,
}

impl ResponsesClient {
    pub fn new(
        client: reqwest::Client,
        base_url: String,
        api_key: String,
        provider: String,
        api_style: String,
    ) -> Self {
        Self {
            transport: CompletionTransport::new(client, base_url, api_key, provider, api_style),
        }
    }

    pub async fn complete(
        &self,
        _request: &ChatCompletionRequest,
        body: Value,
    ) -> Result<UpstreamCompletion, ProviderError> {
        let response = self.transport.send_json("/responses", body).await?;
        parse_completion(
            response,
            self.transport.provider(),
            self.transport.api_style(),
            "/responses",
        )
        .await
    }
}
