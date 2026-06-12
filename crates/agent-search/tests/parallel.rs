use agent_search::parallel::search_parallel;
use agent_search::types::*;
use agent_search::{ProviderCapabilities, ProviderEntry};
use async_trait::async_trait;
use chrono::Utc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::time::Duration;

struct TestProvider {
    id: ProviderId,
    delay_ms: u64,
    fail: bool,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

#[async_trait]
impl SearchProvider for TestProvider {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(true, true, true, true, false, false, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let query = req.query.clone();
        let current = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(current, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        if self.fail {
            return Err(SearchError::Request("boom".to_string()));
        }
        Ok(ProviderSearchResponse {
            hits: vec![SearchHit {
                provider: self.id,
                title: format!("{} hit", self.id.as_str()),
                url: format!("https://{}.example.com", self.id.as_str()),
                normalized_url: format!("https://{}.example.com", self.id.as_str()),
                snippet: Some(query.clone()),
                retrieved_at: Utc::now(),
                published_at: None,
                content_hash: format!("hash-{}", self.id.as_str()),
                citation_ids: vec![],
                tainted: false,
            }],
            evidence: vec![],
            receipts: vec![ProviderReceipt::ok(self.id, &query, &[])],
            warnings: vec![],
        })
    }
}

#[tokio::test]
async fn honors_max_parallel_and_surfaces_partial_failures() {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let providers = vec![
        ProviderEntry::new(Arc::new(TestProvider {
            id: ProviderId::OpenAlex,
            delay_ms: 30,
            fail: false,
            active: active.clone(),
            peak: peak.clone(),
        })),
        ProviderEntry::new(Arc::new(TestProvider {
            id: ProviderId::Brave,
            delay_ms: 30,
            fail: true,
            active: active.clone(),
            peak: peak.clone(),
        })),
    ];
    let request = ResearchRequest {
        query: "parallel test".to_string(),
        objective: None,
        mode: QueryClass::Mixed,
        providers: ProviderPolicy::default(),
        limits: ResearchLimits {
            max_queries: 4,
            max_pages: 1,
            max_parallel: 1,
            timeout_seconds: 5,
            max_cost_usd: 1.0,
        },
        extraction: ExtractionPolicy::default(),
        evidence: EvidencePolicy::default(),
        safety: SafetyPolicy::default(),
    };

    let response = search_parallel(providers, request, QueryClass::Mixed).await;
    assert_eq!(peak.load(Ordering::SeqCst), 1);
    assert_eq!(response.hits.len(), 1);
    assert!(response
        .warnings
        .iter()
        .any(|warning| warning.contains("boom")));
    assert!(response
        .receipts
        .iter()
        .any(|receipt| matches!(receipt.status, ReceiptStatus::Failed)));
}
