use crate::providers;
use crate::types::*;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;

#[derive(Clone)]
pub struct ProviderEntry {
    pub provider: Arc<dyn SearchProvider>,
    pub capabilities: ProviderCapabilities,
}

impl ProviderEntry {
    pub fn new(provider: Arc<dyn SearchProvider>) -> Self {
        let capabilities = provider.capabilities();
        Self {
            provider,
            capabilities,
        }
    }
}

#[derive(Clone)]
pub struct SearchConfig {
    pub providers: Vec<ProviderEntry>,
    pub skipped: Vec<ProviderReceipt>,
    pub provider_policy: ProviderPolicy,
    pub limits: ResearchLimits,
    pub extraction: ExtractionPolicy,
    pub evidence: EvidencePolicy,
    pub safety: SafetyPolicy,
    pub store_path: Option<std::path::PathBuf>,
}

impl SearchConfig {
    pub fn from_env() -> Self {
        Self::from_env_map(None)
    }

    pub fn from_env_map(env_map: Option<&HashMap<String, String>>) -> Self {
        let mut provider_policy = ProviderPolicy::default();
        if let Some(allow) = env_var("AGENT_SEARCH_ALLOW", env_map) {
            provider_policy.allow = allow
                .split(',')
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
                .collect();
        }
        if let Some(prefer) = env_var("AGENT_SEARCH_PREFER", env_map) {
            provider_policy.prefer = prefer
                .split(',')
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect();
        }
        let limits = ResearchLimits::default();
        let extraction = ExtractionPolicy::default();
        let evidence = EvidencePolicy::default();
        let safety = SafetyPolicy::default();
        let mut providers = Vec::new();
        let mut skipped = Vec::new();

        providers.push(ProviderEntry::new(Arc::new(
            providers::openalex::OpenAlexProvider::new(env_var(
                "AGENT_SEARCH_OPENALEX_MAILTO",
                env_map,
            )),
        )));
        providers.push(ProviderEntry::new(Arc::new(
            providers::crossref::CrossrefProvider::new(env_var(
                "AGENT_SEARCH_CROSSREF_MAILTO",
                env_map,
            )),
        )));
        providers.push(ProviderEntry::new(Arc::new(
            providers::arxiv::ArxivProvider::new(),
        )));
        if let Some(email) = env_var("AGENT_SEARCH_PUBMED_EMAIL", env_map) {
            providers.push(ProviderEntry::new(Arc::new(
                providers::pubmed::PubMedProvider::new(email),
            )));
        } else {
            skipped.push(ProviderReceipt::skipped(
                ProviderId::PubMed,
                "",
                "missing AGENT_SEARCH_PUBMED_EMAIL",
            ));
        }
        providers.push(ProviderEntry::new(Arc::new(
            providers::gdelt::GdeltProvider::new(),
        )));

        if let Some(api_key) = env_var("BRAVE_API_KEY", env_map) {
            providers.push(ProviderEntry::new(Arc::new(
                providers::brave::BraveProvider::new(api_key),
            )));
        } else {
            skipped.push(ProviderReceipt::skipped(
                ProviderId::Brave,
                "",
                "missing BRAVE_API_KEY",
            ));
        }

        if let Some(api_key) = env_var("TAVILY_API_KEY", env_map) {
            providers.push(ProviderEntry::new(Arc::new(
                providers::tavily::TavilyProvider::new(api_key),
            )));
        } else {
            skipped.push(ProviderReceipt::skipped(
                ProviderId::Tavily,
                "",
                "missing TAVILY_API_KEY",
            ));
        }

        if let Some(api_key) = env_var("EXA_API_KEY", env_map) {
            providers.push(ProviderEntry::new(Arc::new(
                providers::exa::ExaProvider::new(api_key),
            )));
        } else {
            skipped.push(ProviderReceipt::skipped(
                ProviderId::Exa,
                "",
                "missing EXA_API_KEY",
            ));
        }

        if let Some(base_url) = env_var("SEARXNG_BASE_URL", env_map) {
            providers.push(ProviderEntry::new(Arc::new(
                providers::searxng::SearxngProvider::new(base_url),
            )));
        } else {
            skipped.push(ProviderReceipt::skipped(
                ProviderId::Searxng,
                "",
                "missing SEARXNG_BASE_URL",
            ));
        }

        if let Some(api_key) = env_var("SEMANTIC_SCHOLAR_API_KEY", env_map) {
            providers.push(ProviderEntry::new(Arc::new(
                providers::semantic_scholar::SemanticScholarProvider::new(api_key),
            )));
        } else {
            skipped.push(ProviderReceipt::skipped(
                ProviderId::SemanticScholar,
                "",
                "missing SEMANTIC_SCHOLAR_API_KEY",
            ));
        }

        if let Some(email) = env_var("UNPAYWALL_EMAIL", env_map) {
            providers.push(ProviderEntry::new(Arc::new(
                providers::unpaywall::UnpaywallProvider::new(email),
            )));
        } else {
            skipped.push(ProviderReceipt::skipped(
                ProviderId::Unpaywall,
                "",
                "missing UNPAYWALL_EMAIL",
            ));
        }

        providers.push(ProviderEntry::new(Arc::new(
            providers::github::GithubProvider::new(env_var("GITHUB_TOKEN", env_map)),
        )));

        if let Some(api_key) = env_var("FIRECRAWL_API_KEY", env_map) {
            providers.push(ProviderEntry::new(Arc::new(
                providers::firecrawl::FirecrawlProvider::new(api_key),
            )));
        } else {
            skipped.push(ProviderReceipt::skipped(
                ProviderId::Firecrawl,
                "",
                "missing FIRECRAWL_API_KEY",
            ));
        }

        if let Some(api_key) = env_var("JINA_API_KEY", env_map) {
            providers.push(ProviderEntry::new(Arc::new(
                providers::jina::JinaProvider::new(api_key),
            )));
        } else {
            skipped.push(ProviderReceipt::skipped(
                ProviderId::Jina,
                "",
                "missing JINA_API_KEY",
            ));
        }

        Self {
            providers,
            skipped,
            provider_policy,
            limits,
            extraction,
            evidence,
            safety,
            store_path: env_var("AGENT_SEARCH_SQLITE", env_map).map(Into::into),
        }
    }

    pub fn provider_ids(&self) -> Vec<ProviderId> {
        self.providers
            .iter()
            .map(|entry| entry.provider.id())
            .collect()
    }
}

fn env_var(name: &str, env_map: Option<&HashMap<String, String>>) -> Option<String> {
    let value = match env_map.and_then(|map| map.get(name).cloned()) {
        Some(value) => Some(value),
        None => env::var(name).ok(),
    }?;
    let value = value.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
