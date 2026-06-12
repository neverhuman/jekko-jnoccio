use crate::providers::{array_or_empty, client, response_from_items};
use crate::types::*;
use async_trait::async_trait;
use serde_json::Value;

pub struct TavilyProvider {
    client: reqwest::Client,
    api_key: String,
}

impl TavilyProvider {
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
            ProviderId::Tavily,
            "fixture",
            &items,
            &["title"],
            &["url"],
            &["content"],
            Some("tavily"),
        )
    }
}

#[async_trait]
impl SearchProvider for TavilyProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Tavily
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(true, false, false, false, false, true, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let body = serde_json::json!({
            "query": req.query,
            "search_depth": if matches!(req.mode, QueryClass::Academic | QueryClass::Code) { "advanced" } else { "basic" },
            "topic": match req.mode {
                QueryClass::Academic => "general",
                QueryClass::News => "news",
                QueryClass::Code => "general",
                _ => "general",
            },
            "max_results": req.limit,
        });
        let json: Value = self
            .client
            .post("https://api.tavily.com/search")
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
