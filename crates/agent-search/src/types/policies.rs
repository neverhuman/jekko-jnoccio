use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderCapabilities {
    pub web: bool,
    pub academic: bool,
    pub news: bool,
    pub code: bool,
    pub extraction: bool,
    pub requires_key: bool,
    pub privacy_first: bool,
}

impl ProviderCapabilities {
    pub const fn new(
        web: bool,
        academic: bool,
        news: bool,
        code: bool,
        extraction: bool,
        requires_key: bool,
        privacy_first: bool,
    ) -> Self {
        Self {
            web,
            academic,
            news,
            code,
            extraction,
            requires_key,
            privacy_first,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderPolicy {
    pub prefer: Vec<String>,
    pub allow: Vec<String>,
    pub missing_provider: MissingProviderPolicy,
}

impl Default for ProviderPolicy {
    fn default() -> Self {
        Self {
            prefer: vec![
                "official_api".to_string(),
                "primary_source".to_string(),
                "privacy_first".to_string(),
            ],
            allow: vec![
                "openalex".to_string(),
                "crossref".to_string(),
                "arxiv".to_string(),
                "pubmed".to_string(),
                "gdelt".to_string(),
                "brave".to_string(),
                "tavily".to_string(),
                "exa".to_string(),
                "searxng".to_string(),
                "semantic_scholar".to_string(),
                "unpaywall".to_string(),
                "github".to_string(),
                "firecrawl".to_string(),
                "jina".to_string(),
            ],
            missing_provider: MissingProviderPolicy::SkipWithReceipt,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissingProviderPolicy {
    SkipWithReceipt,
    Pause,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionPolicy {
    pub enabled: bool,
    pub max_pages: usize,
    pub allowed_extractors: Vec<ExtractorId>,
}

impl Default for ExtractionPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            max_pages: 12,
            allowed_extractors: vec![
                ExtractorId::BuiltIn,
                ExtractorId::Jina,
                ExtractorId::Firecrawl,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtractorId {
    BuiltIn,
    Jina,
    Firecrawl,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidencePolicy {
    pub require_citations: bool,
    pub claim_level: bool,
    pub store: EvidenceStore,
}

impl Default for EvidencePolicy {
    fn default() -> Self {
        Self {
            require_citations: true,
            claim_level: true,
            store: EvidenceStore::Sqlite,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStore {
    Sqlite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyPolicy {
    pub redact_secrets: bool,
    pub block_internal_urls: bool,
    pub prompt_injection: PromptInjectionPolicy,
    pub taint_label: String,
}

impl Default for SafetyPolicy {
    fn default() -> Self {
        Self {
            redact_secrets: true,
            block_internal_urls: true,
            prompt_injection: PromptInjectionPolicy::Quarantine,
            taint_label: "web_content".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptInjectionPolicy {
    Quarantine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchLimits {
    pub max_queries: usize,
    pub max_pages: usize,
    pub max_parallel: usize,
    pub timeout_seconds: u64,
    pub max_cost_usd: f64,
}

impl Default for ResearchLimits {
    fn default() -> Self {
        Self {
            max_queries: 24,
            max_pages: 20,
            max_parallel: 6,
            timeout_seconds: 30,
            max_cost_usd: 1.0,
        }
    }
}
