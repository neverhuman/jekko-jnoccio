pub mod capacity;
pub mod cli;
pub mod config;
pub mod failure_log;
pub mod fusion;
pub mod limits;
pub mod mcp;
pub mod metrics;
pub mod openai;
pub mod providers;
pub mod quality_band;
pub mod router;
pub mod routing;
pub mod rpc_shim;
pub mod search;
pub mod state;
pub mod telemetry;

pub use config::{
    AppConfig, InstanceRole, ModelEntry, Registry, ResolvedModel, RuntimeSettings, ScalingSettings,
    ServerConfig,
};
pub use fusion::{DashboardMessage, Gateway, GatewayError, GatewayResult};
pub use metrics::{DashboardModel, DashboardSnapshot, DashboardTotals};
pub use openai::{
    AssistantMessage, ChatChoiceDelta, ChatChoiceMessage, ChatCompletionChoice,
    ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, ChatErrorBody,
    ChatErrorResponse, ChatUsage, EmbeddingObject, EmbeddingsInput, EmbeddingsRequest,
    EmbeddingsResponse, EmbeddingsUsage, StreamReceipt, ToolCall, ToolCallFunction,
};
