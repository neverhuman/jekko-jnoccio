use crate::providers::{array_or_empty, client};
use crate::types::*;
use async_trait::async_trait;
use reqwest::Url;
use serde_json::Value;

pub struct PubMedProvider {
    client: reqwest::Client,
    email: String,
}

impl PubMedProvider {
    pub fn new(email: String) -> Self {
        Self {
            client: client(),
            email,
        }
    }

    pub fn parse_fixture(value: &Value) -> Result<ProviderSearchResponse> {
        let ids = match value.get("esearchresult").and_then(|v| v.get("idlist")) {
            Some(items) => array_or_empty(Some(items)),
            None => Vec::new(),
        };
        let mut hits = Vec::new();
        for id in ids
            .into_iter()
            .filter_map(|value| value.as_str().map(str::to_string))
        {
            hits.push(crate::providers::hit_from_value(
                ProviderId::PubMed,
                format!("PubMed PMID {id}"),
                format!("https://pubmed.ncbi.nlm.nih.gov/{id}/"),
                Some("PubMed search result".to_string()),
                vec![format!("pmid:{id}")],
            )?);
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
impl SearchProvider for PubMedProvider {
    fn id(&self) -> ProviderId {
        ProviderId::PubMed
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(false, true, false, false, false, false, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let mut url = Url::parse("https://eutils.ncbi.nlm.nih.gov/entrez/eutils/esearch.fcgi")?;
        url.query_pairs_mut()
            .append_pair("db", "pubmed")
            .append_pair("term", &req.query)
            .append_pair("retmode", "json")
            .append_pair("retmax", &req.limit.to_string())
            .append_pair("tool", "agent_search")
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
