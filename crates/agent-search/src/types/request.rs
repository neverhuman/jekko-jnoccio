use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchRequest {
    pub query: String,
    pub objective: Option<String>,
    pub mode: crate::types::QueryClass,
    pub providers: crate::types::ProviderPolicy,
    pub limits: crate::types::ResearchLimits,
    pub extraction: crate::types::ExtractionPolicy,
    pub evidence: crate::types::EvidencePolicy,
    pub safety: crate::types::SafetyPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSearchRequest {
    pub query: String,
    pub objective: Option<String>,
    pub mode: crate::types::QueryClass,
    pub limit: usize,
    pub timeout_seconds: u64,
    pub extraction: crate::types::ExtractionPolicy,
    pub evidence: crate::types::EvidencePolicy,
    pub safety: crate::types::SafetyPolicy,
}
