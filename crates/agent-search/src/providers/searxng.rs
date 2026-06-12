use crate::providers::{array_or_empty, client, response_from_items};
use crate::types::*;
use async_trait::async_trait;
use reqwest::Url;
use serde_json::Value;

pub struct SearxngProvider {
    client: reqwest::Client,
    base_url: String,
}

impl SearxngProvider {
    pub fn new(base_url: String) -> Self {
        Self {
            client: client(),
            base_url,
        }
    }

    pub fn parse_fixture(value: &Value) -> Result<ProviderSearchResponse> {
        let items = match value.get("results") {
            Some(items) => array_or_empty(Some(items)),
            None => Vec::new(),
        };
        response_from_items(
            ProviderId::Searxng,
            "fixture",
            &items,
            &["title"],
            &["url"],
            &["content"],
            Some("searxng"),
        )
    }
}

#[async_trait]
impl SearchProvider for SearxngProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Searxng
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(true, true, true, true, false, false, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let mut url = Url::parse(&format!("{}/search", self.base_url.trim_end_matches('/')))?;
        url.query_pairs_mut()
            .append_pair("q", &req.query)
            .append_pair("format", "json")
            .append_pair("language", "en");
        let json: Value = self
            .client
            .get(url)
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
