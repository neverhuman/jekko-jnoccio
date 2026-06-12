use crate::providers::{array_or_empty, client, response_from_items};
use crate::types::*;
use async_trait::async_trait;
use reqwest::Url;
use serde_json::Value;

pub struct CrossrefProvider {
    client: reqwest::Client,
    mailto: Option<String>,
}

impl CrossrefProvider {
    pub fn new(mailto: Option<String>) -> Self {
        Self {
            client: client(),
            mailto,
        }
    }

    pub fn parse_fixture(value: &Value) -> Result<ProviderSearchResponse> {
        let items = match value
            .get("message")
            .and_then(|message| message.get("items"))
        {
            Some(items) => array_or_empty(Some(items)),
            None => Vec::new(),
        };
        response_from_items(
            ProviderId::Crossref,
            "fixture",
            &items,
            &["title", "subtitle"],
            &["URL", "DOI"],
            &["abstract", "publisher"],
            Some("crossref"),
        )
    }
}

#[async_trait]
impl SearchProvider for CrossrefProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Crossref
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(false, true, false, false, false, false, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let mut url = Url::parse("https://api.crossref.org/works")?;
        url.query_pairs_mut()
            .append_pair("query", &req.query)
            .append_pair("rows", &req.limit.to_string());
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
        let items = match json.get("message").and_then(|message| message.get("items")) {
            Some(items) => array_or_empty(Some(items)),
            None => Vec::new(),
        };
        let mut response = response_from_items(
            self.id(),
            &req.query,
            &items,
            &["title", "subtitle"],
            &["URL", "DOI"],
            &["abstract", "publisher"],
            Some("doi"),
        )?;
        response
            .receipts
            .push(ProviderReceipt::ok(self.id(), &req.query, &response.hits));
        Ok(response)
    }
}
