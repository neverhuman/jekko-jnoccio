use agent_search::{ProvenanceStore, ProviderId, SearchHit};
use chrono::Utc;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn inserts_deduplicates_and_prunes_rows() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let path = std::env::current_dir()
        .expect("cwd")
        .join("target")
        .join("agent-search")
        .join(format!("provenance-{unique}.sqlite"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    let store = ProvenanceStore::open(&path).expect("open store");
    let hit = SearchHit {
        provider: ProviderId::OpenAlex,
        title: "Example".to_string(),
        url: "https://example.com".to_string(),
        normalized_url: "https://example.com".to_string(),
        snippet: Some("snippet".to_string()),
        retrieved_at: Utc::now(),
        published_at: None,
        content_hash: "hash-1".to_string(),
        citation_ids: vec!["doi:10.1/example".to_string()],
        tainted: false,
    };

    assert!(store.insert_hit(&hit, "example query", 1).expect("insert"));
    assert!(!store.insert_hit(&hit, "example query", 1).expect("dedupe"));
    assert!(store.contains_hash("hash-1").expect("contains"));
    assert_eq!(
        store
            .prune_expired(Utc::now() + chrono::Duration::days(2))
            .expect("prune"),
        1
    );
    let _ = fs::remove_file(path);
}
