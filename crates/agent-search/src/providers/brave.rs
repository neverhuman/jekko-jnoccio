use crate::providers::{array_or_empty, client, response_from_items};
use crate::types::*;
use async_trait::async_trait;
use reqwest::Url;
use serde_json::Value;

pub struct BraveProvider {
    client: reqwest::Client,
    api_key: String,
}

impl BraveProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: client(),
            api_key,
        }
    }

    pub fn parse_fixture(value: &Value) -> Result<ProviderSearchResponse> {
        let items = match value.get("web").and_then(|v| v.get("results")) {
            Some(items) => array_or_empty(Some(items)),
            None => Vec::new(),
        };
        response_from_items(
            ProviderId::Brave,
            "fixture",
            &items,
            &["title"],
            &["url"],
            &["description"],
            Some("brave"),
        )
    }
}

#[async_trait]
impl SearchProvider for BraveProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Brave
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(true, false, false, false, false, true, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let mut url = Url::parse("https://api.search.brave.com/res/v1/web/search")?;
        url.query_pairs_mut()
            .append_pair("q", &req.query)
            .append_pair("count", &req.limit.to_string())
            .append_pair("safesearch", "strict")
            .append_pair("search_lang", "en");
        let json: Value = self
            .client
            .get(url)
            .header("X-Subscription-Token", &self.api_key)
            .header("Accept", "application/json")
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
