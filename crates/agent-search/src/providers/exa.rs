use crate::providers::{array_or_empty, client, response_from_items};
use crate::types::*;
use async_trait::async_trait;
use serde_json::Value;

pub struct ExaProvider {
    client: reqwest::Client,
    api_key: String,
}

impl ExaProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: client(),
            api_key,
        }
    }

    pub fn parse_fixture(value: &Value) -> Result<ProviderSearchResponse> {
        let items = match value.get("results") {
            Some(items) => array_or_empty(Some(items)),
            None => Vec::new(),
        };
        response_from_items(
            ProviderId::Exa,
            "fixture",
            &items,
            &["title"],
            &["url"],
            &["text", "content"],
            Some("exa"),
        )
    }
}

#[async_trait]
impl SearchProvider for ExaProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Exa
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(true, true, false, false, true, true, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let body = serde_json::json!({
            "query": req.query,
            "type": if matches!(req.mode, QueryClass::Academic | QueryClass::Code) { "deep" } else { "auto" },
            "numResults": req.limit,
            "livecrawl": "preferred",
            "contextMaxCharacters": 10_000usize,
        });
        let json: Value = self
            .client
            .post("https://api.exa.ai/search")
            .bearer_auth(&self.api_key)
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
