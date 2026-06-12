use agent_search::types::*;
use std::str::FromStr;

#[test]
fn provider_id_parsing_and_display_stay_stable() {
    assert_eq!(
        ProviderId::from_str("semantic-scholar").unwrap(),
        ProviderId::SemanticScholar
    );
    assert_eq!(ProviderId::SemanticScholar.to_string(), "semantic_scholar");
}

#[test]
fn provider_defaults_keep_the_expected_allowlist_and_receipts_shape() {
    let policy = ProviderPolicy::default();
    assert!(policy.allow.contains(&"openalex".to_string()));
    assert!(policy.allow.contains(&"jina".to_string()));
    assert_eq!(ProviderSearchResponse::default().hits.len(), 0);
    assert_eq!(ResearchResponse::default().warnings.len(), 0);
}
