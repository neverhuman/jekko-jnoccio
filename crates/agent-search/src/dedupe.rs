use crate::types::{ProviderId, SearchHit};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use url::Url;

const TRACKING_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "utm_id",
    "gclid",
    "fbclid",
    "mc_cid",
    "mc_eid",
    "ref",
    "source",
    "cmpid",
];

pub fn normalize_url(input: &str) -> Option<String> {
    let mut url = Url::parse(input).ok()?;
    if let Some(host) = url.host_str() {
        url.set_host(Some(&host.trim_end_matches('.').to_ascii_lowercase()))
            .ok()?;
    }
    if url.path().is_empty() {
        url.set_path("/");
    }
    let keep: BTreeMap<String, String> = url
        .query_pairs()
        .into_owned()
        .filter(|(key, _)| !TRACKING_PARAMS.contains(&key.as_str()) && !key.starts_with("utm_"))
        .collect();
    url.set_query(None);
    if !keep.is_empty() {
        let query = keep
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("&");
        url.set_query(Some(&query));
    }
    url.set_fragment(None);
    Some(url.to_string().trim_end_matches('/').to_string())
}

pub fn hash_fingerprint(
    provider: &ProviderId,
    title: &str,
    normalized_url: &str,
    snippet: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(provider.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(title.as_bytes());
    hasher.update(b"\0");
    hasher.update(normalized_url.as_bytes());
    hasher.update(b"\0");
    if let Some(snippet) = snippet {
        hasher.update(snippet.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

pub fn hash_search_batch(provider: ProviderId, query: &str, hits: &[SearchHit]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(provider.as_str().as_bytes());
    hasher.update(query.as_bytes());
    for hit in hits {
        hasher.update(hit.content_hash.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

pub fn dedupe_hits(mut hits: Vec<SearchHit>) -> Vec<SearchHit> {
    let mut seen = BTreeSet::new();
    hits.retain(|hit| seen.insert(dedupe_key(hit)));
    hits.sort_by(|a, b| {
        a.provider
            .as_str()
            .cmp(b.provider.as_str())
            .then(a.title.cmp(&b.title))
    });
    hits
}

fn dedupe_key(hit: &SearchHit) -> String {
    if let Some(id) = hit.citation_ids.iter().find(|value| {
        value.starts_with("doi:")
            || value.starts_with("arxiv:")
            || value.starts_with("pmid:")
            || value.starts_with("pmcid:")
            || value.starts_with("openalex:")
            || value.starts_with("s2:")
    }) {
        return id.clone();
    }
    hit.normalized_url.clone()
}
