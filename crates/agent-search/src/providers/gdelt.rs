use crate::providers::{array_or_empty, client, response_from_items};
use crate::types::*;
use async_trait::async_trait;
use reqwest::Url;
use serde_json::Value;

pub struct GdeltProvider {
    client: reqwest::Client,
}

impl GdeltProvider {
    pub fn new() -> Self {
        Self { client: client() }
    }

    pub fn parse_fixture(value: &Value) -> Result<ProviderSearchResponse> {
        let items = match value.get("articles") {
            Some(items) => array_or_empty(Some(items)),
            None => match value.get("results") {
                Some(items) => array_or_empty(Some(items)),
                None => Vec::new(),
            },
        };
        response_from_items(
            ProviderId::Gdelt,
            "fixture",
            &items,
            &["title", "sourceCountry"],
            &["url", "sourceUrl"],
            &["seendate", "snippet"],
            Some("gdelt"),
        )
    }
}

crate::providers::default_from_new!(GdeltProvider);

#[async_trait]
impl SearchProvider for GdeltProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Gdelt
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(true, false, true, false, false, false, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let mut url = Url::parse("https://api.gdeltproject.org/api/v2/doc/doc")?;
        url.query_pairs_mut()
            .append_pair("query", &req.query)
            .append_pair("mode", "artlist")
            .append_pair("format", "json")
            .append_pair("maxrecords", &req.limit.to_string());
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
