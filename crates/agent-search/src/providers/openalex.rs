use crate::providers::{array_or_empty, client, response_from_items};
use crate::types::*;
use async_trait::async_trait;
use reqwest::Url;
use serde_json::Value;

pub struct OpenAlexProvider {
    client: reqwest::Client,
    mailto: Option<String>,
}

impl OpenAlexProvider {
    pub fn new(mailto: Option<String>) -> Self {
        Self {
            client: client(),
            mailto,
        }
    }

    pub fn parse_fixture(value: &Value) -> Result<ProviderSearchResponse> {
        let items = array_or_empty(value.get("results"));
        response_from_items(
            ProviderId::OpenAlex,
            "fixture",
            &items,
            &["display_name", "title"],
            &["doi", "id", "primary_location"],
            &["abstract", "publication_year"],
            Some("openalex"),
        )
    }
}

#[async_trait]
impl SearchProvider for OpenAlexProvider {
    fn id(&self) -> ProviderId {
        ProviderId::OpenAlex
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(false, true, false, false, false, false, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let mut url = Url::parse("https://api.openalex.org/works")?;
        url.query_pairs_mut()
            .append_pair("search", &req.query)
            .append_pair("per-page", &req.limit.to_string());
        if let Some(email) = &self.mailto {
            url.query_pairs_mut().append_pair("mailto", email);
        }
        let json: Value = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let items = array_or_empty(json.get("results"));
        let mut response = response_from_items(
            self.id(),
            &req.query,
            &items,
            &["display_name", "title"],
            &["doi", "id"],
            &["abstract", "biblio"],
            Some("openalex"),
        )?;
        response
            .receipts
            .push(ProviderReceipt::ok(self.id(), &req.query, &response.hits));
        Ok(response)
    }
}
