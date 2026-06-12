use agent_search::{dedupe_hits, normalize_url, ProviderId, SearchHit};
use chrono::Utc;

#[test]
fn normalizes_urls_and_removes_tracking_params() {
    let normalized = normalize_url("https://example.com/article?utm_source=x&gclid=y#fragment")
        .expect("url normalizes");
    assert_eq!(normalized, "https://example.com/article");
}

#[test]
fn dedupes_by_citation_identifier_before_url() {
    let hit_a = SearchHit {
        provider: ProviderId::OpenAlex,
        title: "Paper A".to_string(),
        url: "https://example.com/a".to_string(),
        normalized_url: "https://example.com/a".to_string(),
        snippet: None,
        retrieved_at: Utc::now(),
        published_at: None,
        content_hash: "hash-a".to_string(),
        citation_ids: vec!["doi:10.1/abc".to_string()],
        tainted: false,
    };
    let hit_b = SearchHit {
        provider: ProviderId::Crossref,
        title: "Paper B".to_string(),
        url: "https://example.com/b".to_string(),
        normalized_url: "https://example.com/b".to_string(),
        snippet: None,
        retrieved_at: Utc::now(),
        published_at: None,
        content_hash: "hash-b".to_string(),
        citation_ids: vec!["doi:10.1/abc".to_string()],
        tainted: false,
    };
    let deduped = dedupe_hits(vec![hit_a, hit_b]);
    assert_eq!(deduped.len(), 1);
}
