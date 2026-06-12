#[test]
#[ignore]
fn live_research_smoke_test_requires_explicit_env() {
    assert!(
        std::env::var("AGENT_SEARCH_LIVE").is_ok(),
        "set AGENT_SEARCH_LIVE=1 to run live tests"
    );
}
