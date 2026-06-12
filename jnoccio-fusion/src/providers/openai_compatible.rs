use crate::openai::{ChatChoiceMessage, ChatCompletionRequest, ChatUsage, sanitize_messages};
pub(crate) use crate::providers::completion_common::{
    CompletionTransport, ProviderError, UpstreamCompletion, build_tool_call,
    build_upstream_completion, parse_json_response,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::fmt;

pub(crate) fn has_tools(value: &Value) -> bool {
    match value {
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        Value::Null => false,
        _ => true,
    }
}

pub(crate) fn apply_common_completion_body_fields(
    map: &mut Map<String, Value>,
    request: &ChatCompletionRequest,
    tools: Option<Value>,
    stream: bool,
    completion_tokens_param: Option<&str>,
) {
    let has_tools = tools.as_ref().map(has_tools).unwrap_or(false);
    if let Some(value) = request.temperature {
        map.insert("temperature".to_string(), json!(value));
    }
    if let Some(value) = request.top_p {
        map.insert("top_p".to_string(), json!(value));
    }
    if let Some(value) = request.max_completion_tokens.or(request.max_tokens) {
        let key = completion_tokens_param.unwrap_or("max_tokens");
        map.insert(key.to_string(), json!(value));
    }
    if has_tools && let Some(value) = tools {
        map.insert("tools".to_string(), value);
    }
    if has_tools && let Some(value) = &request.tool_choice {
        map.insert("tool_choice".to_string(), value.clone());
    }
    if let Some(value) = &request.reasoning_effort {
        map.insert("reasoning_effort".to_string(), value.clone());
    }
    if let Some(value) = &request.response_format {
        map.insert("response_format".to_string(), value.clone());
    }
    if stream && let Some(value) = &request.stream_options {
        map.insert("stream_options".to_string(), value.clone());
    }
}

#[derive(Clone)]
pub struct OpenAICompatibleClient {
    transport: CompletionTransport,
}

impl UpstreamCompletion {
    pub fn into_response(self, model: &str) -> crate::openai::ChatCompletionResponse {
        let reasoning = self.message.reasoning_text.clone();
        let finish_reason = match self.finish_reason.clone() {
            Some(reason) => Some(reason),
            None => Some("stop".to_string()),
        };
        let usage = self.usage.clone();
        crate::openai::build_response(model, self.message, finish_reason, usage, reasoning)
    }
}

impl OpenAICompatibleClient {
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
        let response = self.transport.send_json("/chat/completions", body).await?;
        parse_chat_completion(
            response,
            self.transport.provider(),
            self.transport.api_style(),
            "/chat/completions",
        )
        .await
    }
}

pub async fn parse_chat_completion(
    response: reqwest::Response,
    provider: &str,
    api_style: &str,
    endpoint: &str,
) -> Result<UpstreamCompletion, ProviderError> {
    let parsed = parse_json_response(response, provider, api_style, endpoint).await?;
    parse_completion(parsed.raw).map_err(|err| {
        ProviderError::parse_failure(
            provider,
            api_style,
            endpoint,
            parsed.status,
            parsed.headers,
            parsed.text,
            &err,
        )
    })
}

#[derive(Debug, Deserialize)]
struct CompletionEnvelope {
    choices: Vec<CompletionChoice>,
    usage: Option<ChatUsage>,
    #[serde(flatten)]
    _extra: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct CompletionChoice {
    message: CompletionMessage,
    finish_reason: Option<String>,
    #[serde(flatten)]
    _extra: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct CompletionMessage {
    role: Option<String>,
    content: Option<String>,
    reasoning_text: Option<String>,
    reasoning_content: Option<String>,
    reasoning_opaque: Option<String>,
    tool_calls: Option<Vec<CompletionToolCall>>,
    #[serde(flatten)]
    _extra: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct CompletionToolCall {
    id: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    function: CompletionToolCallFunction,
    #[serde(flatten)]
    _extra: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct CompletionToolCallFunction {
    name: String,
    arguments: String,
    #[serde(flatten)]
    _extra: Map<String, Value>,
}

#[derive(Debug)]
enum CompletionParseError {
    InvalidEnvelope(String),
    EmptyChoices,
}

impl From<serde_json::Error> for CompletionParseError {
    fn from(err: serde_json::Error) -> Self {
        CompletionParseError::InvalidEnvelope(err.to_string())
    }
}

impl fmt::Display for CompletionParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CompletionParseError::InvalidEnvelope(err) => {
                write!(f, "invalid completion envelope: {err}")
            }
            CompletionParseError::EmptyChoices => {
                f.write_str("upstream response contained no choices")
            }
        }
    }
}

impl std::error::Error for CompletionParseError {}

fn first_choice(choices: Vec<CompletionChoice>) -> Result<CompletionChoice, CompletionParseError> {
    let Some(choice) = choices.into_iter().next() else {
        return Err(CompletionParseError::EmptyChoices);
    };
    Ok(choice)
}

fn parse_completion(raw: Value) -> Result<UpstreamCompletion, CompletionParseError> {
    let CompletionEnvelope {
        choices,
        usage,
        _extra: _,
    } = serde_json::from_value(raw.clone())?;
    let choice = first_choice(choices)?;
    let tools = match choice.message.tool_calls {
        Some(tool_calls) => tool_calls
            .into_iter()
            .map(|call| {
                build_tool_call(
                    call.id.as_deref(),
                    call.kind.as_deref(),
                    &call.function.name,
                    Some(call.function.arguments.as_str()),
                )
            })
            .collect::<Vec<_>>(),
        None => Vec::new(),
    };
    let reasoning = match (
        choice.message.reasoning_text,
        choice.message.reasoning_content,
        choice.message.reasoning_opaque.clone(),
    ) {
        (Some(text), _, _) => Some(text),
        (None, Some(content), _) => Some(content),
        (None, None, opaque) => opaque,
    };
    let message = ChatChoiceMessage {
        role: match choice.message.role {
            Some(role) => role,
            None => "assistant".to_string(),
        },
        content: choice.message.content,
        tool_calls: if tools.is_empty() { None } else { Some(tools) },
        reasoning_text: reasoning.clone(),
        reasoning_content: reasoning,
        reasoning_opaque: choice.message.reasoning_opaque,
        extra: choice.message._extra,
    };
    Ok(build_upstream_completion(
        raw,
        message,
        usage,
        choice.finish_reason,
    ))
}

pub fn build_body(
    request: &ChatCompletionRequest,
    model: &str,
    stream: bool,
    tools: Option<Value>,
    messages: Vec<Value>,
    completion_tokens_param: Option<&str>,
    api_style: &str,
) -> Value {
    let messages = sanitize_messages(messages);
    if api_style == "openai_responses" {
        return crate::providers::responses::build_body(
            request,
            model,
            stream,
            tools,
            messages,
            completion_tokens_param,
        );
    }

    let mut body = json!({
      "model": model,
      "messages": messages,
      "stream": stream,
    });
    if let Some(map) = body.as_object_mut() {
        apply_common_completion_body_fields(map, request, tools, stream, completion_tokens_param);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::{CompletionParseError, build_tool_call, build_upstream_completion, first_choice};
    use crate::openai::{ChatChoiceMessage, ChatUsage};
    use serde_json::Map;

    #[test]
    fn first_choice_requires_at_least_one_choice() {
        let err = first_choice(vec![]).unwrap_err();
        assert!(matches!(err, CompletionParseError::EmptyChoices));
    }

    #[test]
    fn build_tool_call_applies_default_id_and_kind() {
        let call = build_tool_call(None, None, "list_projects", None);
        assert_eq!(call.kind, "function");
        assert_eq!(call.function.name, "list_projects");
        assert!(call.function.arguments.is_empty());
        assert!(!call.id.is_empty());
    }

    #[test]
    fn build_upstream_completion_preserves_fields() {
        let raw = serde_json::json!({"id":"resp_123"});
        let message = ChatChoiceMessage {
            role: "assistant".to_string(),
            content: Some("answer".to_string()),
            tool_calls: None,
            reasoning_text: Some("trace".to_string()),
            reasoning_content: Some("trace".to_string()),
            reasoning_opaque: None,
            extra: Map::new(),
        };
        let usage = ChatUsage {
            prompt_tokens: Some(1),
            completion_tokens: Some(2),
            total_tokens: Some(3),
            ..Default::default()
        };

        let completion = build_upstream_completion(
            raw.clone(),
            message.clone(),
            Some(usage),
            Some("stop".to_string()),
        );

        assert_eq!(completion.raw, raw);
        assert_eq!(completion.message.role, message.role);
        assert_eq!(completion.message.content, message.content);
        assert_eq!(completion.message.reasoning_text, message.reasoning_text);
        assert_eq!(
            completion.message.reasoning_content,
            message.reasoning_content
        );
        assert_eq!(completion.finish_reason.as_deref(), Some("stop"));
        assert_eq!(completion.usage.unwrap().total_tokens, Some(3));
    }
}
