use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Value>,
    pub stream: Option<bool>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<u64>,
    pub max_completion_tokens: Option<u64>,
    pub tools: Option<Value>,
    pub tool_choice: Option<Value>,
    pub reasoning_effort: Option<Value>,
    pub response_format: Option<Value>,
    pub stream_options: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ChatUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub prompt_tokens_details: Option<TokenDetails>,
    pub completion_tokens_details: Option<CompletionTokenDetails>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct TokenDetails {
    pub cached_tokens: Option<u64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct CompletionTokenDetails {
    pub reasoning_tokens: Option<u64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolCallFunction,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ChatChoiceMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub reasoning_text: Option<String>,
    pub reasoning_content: Option<String>,
    pub reasoning_opaque: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

pub type AssistantMessage = ChatChoiceMessage;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ChatCompletionChoice {
    pub index: u64,
    pub message: ChatChoiceMessage,
    pub finish_reason: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ChatCompletionResponse {
    pub id: String,
    #[serde(rename = "object")]
    pub kind: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionChoice>,
    pub usage: Option<ChatUsage>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ChatChoiceDelta {
    pub role: Option<String>,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_opaque: Option<String>,
    pub tool_calls: Option<Vec<ToolCallDelta>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ToolCallDelta {
    pub index: u64,
    pub id: Option<String>,
    pub r#type: Option<String>,
    pub function: ToolCallFunctionDelta,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ToolCallFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ChatCompletionChunkChoice {
    pub index: u64,
    pub delta: ChatChoiceDelta,
    pub finish_reason: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ChatCompletionChunk {
    pub id: String,
    #[serde(rename = "object")]
    pub kind: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionChunkChoice>,
    pub usage: Option<ChatUsage>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatErrorResponse {
    pub error: ChatErrorBody,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamReceipt {
    pub request_id: String,
    pub phase: String,
    pub text: String,
}

/// OpenAI-compatible `/v1/embeddings` request body. `input` may be either a
/// single string or an array of strings — Phase E2 keeps both shapes because
/// some upstream embedders + smoke harnesses send each.
#[derive(Clone, Debug, Deserialize)]
pub struct EmbeddingsRequest {
    pub model: String,
    pub input: EmbeddingsInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum EmbeddingsInput {
    Single(String),
    Batch(Vec<String>),
}

impl EmbeddingsInput {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            EmbeddingsInput::Single(text) => vec![text],
            EmbeddingsInput::Batch(items) => items,
        }
    }

    pub fn as_slice(&self) -> Vec<&str> {
        match self {
            EmbeddingsInput::Single(text) => vec![text.as_str()],
            EmbeddingsInput::Batch(items) => items.iter().map(|s| s.as_str()).collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbeddingObject {
    #[serde(rename = "object")]
    pub kind: String,
    pub embedding: Vec<f32>,
    pub index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct EmbeddingsUsage {
    pub prompt_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbeddingsResponse {
    #[serde(rename = "object")]
    pub kind: String,
    pub data: Vec<EmbeddingObject>,
    pub model: String,
    pub usage: EmbeddingsUsage,
}

pub fn error_response(
    message: impl Into<String>,
    kind: impl Into<String>,
    code: Option<String>,
) -> ChatErrorResponse {
    ChatErrorResponse {
        error: ChatErrorBody {
            message: message.into(),
            kind: kind.into(),
            code,
        },
    }
}

pub fn sse_data(payload: &impl Serialize) -> String {
    format!(
        "data: {}\n\n",
        serde_json::to_string(payload).expect("serializable SSE payload")
    )
}

pub fn sse_event(event: &str, payload: &impl Serialize) -> String {
    format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(payload).expect("serializable SSE payload")
    )
}

pub fn sse_done() -> String {
    "data: [DONE]\n\n".to_string()
}

pub fn build_chunk(
    model: &str,
    delta: ChatChoiceDelta,
    finish_reason: Option<String>,
    usage: Option<ChatUsage>,
) -> ChatCompletionChunk {
    ChatCompletionChunk {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        kind: "chat.completion.chunk".to_string(),
        created: chrono::Utc::now().timestamp() as u64,
        model: model.to_string(),
        choices: vec![ChatCompletionChunkChoice {
            index: 0,
            delta,
            finish_reason,
            extra: Map::new(),
        }],
        usage,
        extra: Map::new(),
    }
}

pub fn build_response(
    model: &str,
    message: ChatChoiceMessage,
    finish_reason: Option<String>,
    usage: Option<ChatUsage>,
    reasoning: Option<String>,
) -> ChatCompletionResponse {
    let mut message = message;
    if let Some(reasoning) = reasoning {
        message.reasoning_text = Some(if let Some(existing) = message.reasoning_text.take() {
            format!("{}\n{}", existing, reasoning)
        } else {
            reasoning.clone()
        });
        if message.reasoning_content.is_none() {
            message.reasoning_content = message.reasoning_text.clone();
        }
    }
    ChatCompletionResponse {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        kind: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp() as u64,
        model: model.to_string(),
        choices: vec![ChatCompletionChoice {
            index: 0,
            message,
            finish_reason,
            extra: Map::new(),
        }],
        usage,
        extra: Map::new(),
    }
}

pub fn merge_reasoning(receipts: &[String], upstream_reasoning: Option<&str>) -> Option<String> {
    let mut parts = receipts.to_vec();
    if let Some(value) = upstream_reasoning.filter(|value| !value.is_empty()) {
        parts.push(value.to_string());
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("\n"))
}

pub fn stringify_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        _ => value.to_string(),
    }
}

pub fn sanitize_messages(messages: Vec<Value>) -> Vec<Value> {
    messages.into_iter().filter_map(sanitize_message).collect()
}

pub fn clamp_output_tokens(
    request: &ChatCompletionRequest,
    max_output_tokens: u64,
) -> ChatCompletionRequest {
    let mut request = request.clone();
    request.max_tokens = request.max_tokens.map(|value| value.min(max_output_tokens));
    request.max_completion_tokens = request
        .max_completion_tokens
        .map(|value| value.min(max_output_tokens));
    request
}

fn sanitize_message(message: Value) -> Option<Value> {
    let object = message.as_object()?;
    let mut out = Map::new();
    for key in ["role", "content", "name", "tool_call_id"] {
        if let Some(value) = object.get(key) {
            out.insert(key.to_string(), value.clone());
        }
    }
    if let Some(tool_calls) = object.get("tool_calls").and_then(Value::as_array) {
        let tool_calls = tool_calls
            .iter()
            .filter_map(sanitize_tool_call)
            .collect::<Vec<_>>();
        if !tool_calls.is_empty() {
            out.insert("tool_calls".to_string(), Value::Array(tool_calls));
        }
    }
    if out.is_empty() {
        return None;
    }
    Some(Value::Object(out))
}

fn sanitize_tool_call(value: &Value) -> Option<Value> {
    let object = value.as_object()?;
    let function = object.get("function")?.as_object()?;
    let name = function.get("name")?.as_str()?;
    let mut function_out = Map::new();
    function_out.insert("name".to_string(), Value::String(name.to_string()));
    function_out.insert(
        "arguments".to_string(),
        match function.get("arguments") {
            Some(Value::String(text)) => Value::String(text.clone()),
            Some(value) => Value::String(value.to_string()),
            None => Value::String(String::new()),
        },
    );

    let mut out = Map::new();
    if let Some(id) = object.get("id").and_then(Value::as_str) {
        out.insert("id".to_string(), Value::String(id.to_string()));
    }
    out.insert(
        "type".to_string(),
        Value::String(
            object
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("function")
                .to_string(),
        ),
    );
    out.insert("function".to_string(), Value::Object(function_out));
    Some(Value::Object(out))
}
