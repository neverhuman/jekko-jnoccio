use crate::providers::{array_or_empty, client, response_from_items};
use crate::types::*;
use async_trait::async_trait;
use serde_json::Value;

pub struct FirecrawlProvider {
    client: reqwest::Client,
    api_key: String,
}

impl FirecrawlProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: client(),
            api_key,
        }
    }

    pub fn parse_fixture(value: &Value) -> Result<ProviderSearchResponse> {
        let items = match value.get("data") {
            Some(items) => array_or_empty(Some(items)),
            None => Vec::new(),
        };
        response_from_items(
            ProviderId::Firecrawl,
            "fixture",
            &items,
            &["title"],
            &["url"],
            &["markdown", "text", "content"],
            Some("firecrawl"),
        )
    }
}

#[async_trait]
impl SearchProvider for FirecrawlProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Firecrawl
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(true, true, false, true, true, true, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let body = serde_json::json!({
            "query": req.query,
            "limit": req.limit,
        });
        let json: Value = self
            .client
            .post("https://api.firecrawl.dev/v1/search")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mut response = Self::parse_fixture(&json)?;
        response
            .receipts
            .push(ProviderReceipt::ok(self.id(), &req.query, &response.hits));
        Ok(response)
    }
}
