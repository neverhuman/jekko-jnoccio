use crate::dedupe::normalize_url;
use crate::types::*;
use chrono::Utc;
use reqwest::Client;
use serde_json::Value;

pub mod arxiv;
pub mod brave;
pub mod crossref;
pub mod exa;
pub mod firecrawl;
pub mod gdelt;
pub mod github;
pub mod jina;
pub mod openalex;
pub mod pubmed;
pub mod searxng;
pub mod semantic_scholar;
pub mod tavily;
pub mod unpaywall;

macro_rules! default_from_new {
    ($ty:ty) => {
        impl Default for $ty {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

pub(crate) use default_from_new;

pub(crate) fn client() -> Client {
    Client::builder()
        .user_agent("agent-search/0.1")
        .build()
        .expect("reqwest client")
}

pub(crate) fn value_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = value_string(value, key) {
            return Some(value);
        }
    }
    None
}

pub(crate) fn array_or_empty(value: Option<&Value>) -> Vec<Value> {
    match value.and_then(Value::as_array) {
        Some(items) => items.clone(),
        None => Vec::new(),
    }
}

pub(crate) fn hit_from_value(
    provider: ProviderId,
    title: impl Into<String>,
    url: impl Into<String>,
    snippet: Option<String>,
    citation_ids: Vec<String>,
) -> Result<SearchHit> {
    let url = url.into();
    let normalized_url = match normalize_url(&url) {
        Some(value) => value,
        None => url.clone(),
    };
    let title = title.into();
    let content_hash =
        crate::dedupe::hash_fingerprint(&provider, &title, &normalized_url, snippet.as_deref());
    Ok(SearchHit {
        provider,
        title,
        url,
        normalized_url,
        snippet,
        retrieved_at: Utc::now(),
        published_at: None,
        content_hash,
        citation_ids,
        tainted: false,
    })
}

pub(crate) fn response_from_items(
    provider: ProviderId,
    query: &str,
    items: &[Value],
    title_keys: &[&str],
    url_keys: &[&str],
    snippet_keys: &[&str],
    citation_prefix: Option<&str>,
) -> Result<ProviderSearchResponse> {
    let mut hits = Vec::new();
    for item in items {
        let title = match first_string(item, title_keys) {
            Some(value) => value,
            None => query.to_string(),
        };
        let url = match first_string(item, url_keys) {
            Some(value) => value,
            None => title.clone(),
        };
        let snippet = first_string(item, snippet_keys);
        let mut citation_ids = Vec::new();
        if let Some(prefix) = citation_prefix {
            citation_ids.push(format!(
                "{prefix}:{}",
                title.to_lowercase().replace(' ', "-")
            ));
        }
        hits.push(hit_from_value(provider, title, url, snippet, citation_ids)?);
    }
    Ok(ProviderSearchResponse {
        hits,
        evidence: Vec::new(),
        receipts: Vec::new(),
        warnings: Vec::new(),
    })
}
