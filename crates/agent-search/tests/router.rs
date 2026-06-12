use agent_search::providers::brave::BraveProvider;
use agent_search::providers::openalex::OpenAlexProvider;
use agent_search::{
    plan_providers, ProviderEntry, ProviderId, ProviderPolicy, QueryClass, QueryRouter,
};
use std::sync::Arc;

#[test]
fn classifies_academic_queries() {
    let router = QueryRouter::new();
    assert_eq!(
        router.classify("find papers on retrieval augmented generation", None),
        QueryClass::Academic
    );
}

#[test]
fn prefers_academic_and_web_providers_for_academic_queries() {
    let providers = vec![
        ProviderEntry::new(Arc::new(OpenAlexProvider::new(None))),
        ProviderEntry::new(Arc::new(BraveProvider::new("key".to_string()))),
    ];
    let planned = plan_providers(&providers, QueryClass::Academic, &ProviderPolicy::default());
    assert!(planned
        .iter()
        .any(|entry| entry.provider.id() == ProviderId::OpenAlex));
    assert!(planned
        .iter()
        .any(|entry| entry.provider.id() == ProviderId::Brave));
}
