use crate::providers::{array_or_empty, client, response_from_items};
use crate::types::*;
use async_trait::async_trait;
use reqwest::Url;
use serde_json::Value;

pub struct GithubProvider {
    client: reqwest::Client,
    token: Option<String>,
}

impl GithubProvider {
    pub fn new(token: Option<String>) -> Self {
        Self {
            client: client(),
            token,
        }
    }

    pub fn parse_fixture(value: &Value) -> Result<ProviderSearchResponse> {
        let items = match value.get("items") {
            Some(items) => array_or_empty(Some(items)),
            None => Vec::new(),
        };
        response_from_items(
            ProviderId::Github,
            "fixture",
            &items,
            &["full_name", "name"],
            &["html_url", "url"],
            &["description"],
            Some("github"),
        )
    }
}

#[async_trait]
impl SearchProvider for GithubProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Github
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(false, false, false, true, false, false, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let mut url = Url::parse("https://api.github.com/search/repositories")?;
        url.query_pairs_mut()
            .append_pair("q", &req.query)
            .append_pair("sort", "stars")
            .append_pair("order", "desc")
            .append_pair("per_page", &req.limit.to_string());
        let mut request = self
            .client
            .get(url)
            .header("User-Agent", "agent-search/0.1")
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let json: Value = request.send().await?.error_for_status()?.json().await?;
        let mut response = Self::parse_fixture(&json)?;
        response
            .receipts
            .push(ProviderReceipt::ok(self.id(), &req.query, &response.hits));
        Ok(response)
    }
}
