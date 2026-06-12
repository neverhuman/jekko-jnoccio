use crate::providers::{array_or_empty, client, response_from_items};
use crate::types::*;
use async_trait::async_trait;
use reqwest::Url;
use serde_json::Value;

pub struct SemanticScholarProvider {
    client: reqwest::Client,
    api_key: String,
}

impl SemanticScholarProvider {
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
            ProviderId::SemanticScholar,
            "fixture",
            &items,
            &["title"],
            &["url", "paperId"],
            &["abstract", "summary"],
            Some("semanticscholar"),
        )
    }
}

#[async_trait]
impl SearchProvider for SemanticScholarProvider {
    fn id(&self) -> ProviderId {
        ProviderId::SemanticScholar
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(false, true, false, false, false, true, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let mut url = Url::parse("https://api.semanticscholar.org/graph/v1/paper/search")?;
        url.query_pairs_mut()
            .append_pair("query", &req.query)
            .append_pair("limit", &req.limit.to_string())
            .append_pair("fields", "title,abstract,url,year");
        let json: Value = self
            .client
            .get(url)
            .header("x-api-key", &self.api_key)
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
