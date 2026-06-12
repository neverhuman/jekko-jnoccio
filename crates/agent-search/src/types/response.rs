use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub provider: crate::types::ProviderId,
    pub title: String,
    pub url: String,
    pub normalized_url: String,
    pub snippet: Option<String>,
    pub retrieved_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
    pub content_hash: String,
    pub citation_ids: Vec<String>,
    pub tainted: bool,
}

impl SearchHit {
    pub fn new(
        provider: crate::types::ProviderId,
        title: impl Into<String>,
        url: impl Into<String>,
        snippet: Option<String>,
        citation_ids: Vec<String>,
    ) -> crate::types::Result<Self> {
        let url = url.into();
        let title = title.into();
        let normalized_url = match crate::dedupe::normalize_url(&url) {
            Some(value) => value,
            None => url.clone(),
        };
        let retrieved_at = Utc::now();
        let content_hash =
            crate::dedupe::hash_fingerprint(&provider, &title, &normalized_url, snippet.as_deref());
        Ok(Self {
            provider,
            title,
            url,
            normalized_url,
            snippet,
            retrieved_at,
            published_at: None,
            content_hash,
            citation_ids,
            tainted: false,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub provider: crate::types::ProviderId,
    pub citation_id: String,
    pub url: String,
    pub normalized_url: String,
    pub title: String,
    pub retrieved_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
    pub content_hash: String,
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReceiptStatus {
    Ok,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderReceipt {
    pub provider: crate::types::ProviderId,
    pub query: String,
    pub retrieved_at: DateTime<Utc>,
    pub status: ReceiptStatus,
    pub reason: Option<String>,
    pub normalized_query: String,
    pub content_hash: String,
    pub citation_ids: Vec<String>,
    pub url_count: usize,
}

impl ProviderReceipt {
    pub fn skipped(
        provider: crate::types::ProviderId,
        query: &str,
        reason: impl Into<String>,
    ) -> Self {
        let normalized_query = query.trim().to_string();
        Self {
            provider,
            query: query.to_string(),
            retrieved_at: Utc::now(),
            status: ReceiptStatus::Skipped,
            reason: Some(reason.into()),
            normalized_query,
            content_hash: String::new(),
            citation_ids: Vec::new(),
            url_count: 0,
        }
    }

    pub fn ok(provider: crate::types::ProviderId, query: &str, hits: &[SearchHit]) -> Self {
        Self {
            provider,
            query: query.to_string(),
            retrieved_at: Utc::now(),
            status: ReceiptStatus::Ok,
            reason: None,
            normalized_query: query.trim().to_string(),
            content_hash: crate::dedupe::hash_search_batch(provider, query, hits),
            citation_ids: hits
                .iter()
                .flat_map(|hit| hit.citation_ids.clone())
                .collect(),
            url_count: hits.len(),
        }
    }

    pub fn failed(
        provider: crate::types::ProviderId,
        query: &str,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            query: query.to_string(),
            retrieved_at: Utc::now(),
            status: ReceiptStatus::Failed,
            reason: Some(reason.into()),
            normalized_query: query.trim().to_string(),
            content_hash: String::new(),
            citation_ids: Vec::new(),
            url_count: 0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderSearchResponse {
    pub hits: Vec<SearchHit>,
    pub evidence: Vec<EvidenceRecord>,
    pub receipts: Vec<ProviderReceipt>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResearchResponse {
    pub hits: Vec<SearchHit>,
    pub evidence: Vec<EvidenceRecord>,
    pub receipts: Vec<ProviderReceipt>,
    pub warnings: Vec<String>,
}
