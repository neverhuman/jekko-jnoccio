use crate::providers::{array_or_empty, client};
use crate::types::*;
use async_trait::async_trait;
use reqwest::Url;
use serde_json::Value;

pub struct UnpaywallProvider {
    client: reqwest::Client,
    email: String,
}

impl UnpaywallProvider {
    pub fn new(email: String) -> Self {
        Self {
            client: client(),
            email,
        }
    }

    pub fn parse_fixture(value: &Value) -> Result<ProviderSearchResponse> {
        let items = match value.get("results") {
            Some(items) => array_or_empty(Some(items)),
            None => Vec::new(),
        };
        let mut hits = Vec::new();
        for item in items {
            if let Some(doi) = item.get("doi").and_then(Value::as_str) {
                hits.push(crate::providers::hit_from_value(
                    ProviderId::Unpaywall,
                    item.get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("Unpaywall result")
                        .to_string(),
                    format!("https://doi.org/{doi}"),
                    item.get("best_oa_location")
                        .and_then(|v| v.get("url"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    vec![format!("doi:{doi}")],
                )?);
            }
        }
        Ok(ProviderSearchResponse {
            hits,
            evidence: Vec::new(),
            receipts: Vec::new(),
            warnings: Vec::new(),
        })
    }
}

#[async_trait]
impl SearchProvider for UnpaywallProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Unpaywall
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(false, true, false, false, false, false, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let mut url = Url::parse("https://api.unpaywall.org/v2/search")?;
        url.query_pairs_mut()
            .append_pair("query", &req.query)
            .append_pair("email", &self.email);
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
