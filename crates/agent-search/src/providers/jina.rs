use crate::providers::{array_or_empty, client, response_from_items};
use crate::types::*;
use async_trait::async_trait;
use reqwest::Url;
use serde_json::Value;

pub struct JinaProvider {
    client: reqwest::Client,
    api_key: String,
}

impl JinaProvider {
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
            ProviderId::Jina,
            "fixture",
            &items,
            &["title"],
            &["url"],
            &["text", "content"],
            Some("jina"),
        )
    }
}

#[async_trait]
impl SearchProvider for JinaProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Jina
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(true, true, false, true, true, true, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let mut url = Url::parse("https://api.jina.ai/search")?;
        url.query_pairs_mut()
            .append_pair("q", &req.query)
            .append_pair("limit", &req.limit.to_string());
        let json: Value = self
            .client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
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
