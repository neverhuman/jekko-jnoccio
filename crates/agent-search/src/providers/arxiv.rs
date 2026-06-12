use crate::providers::client;
use crate::types::*;
use async_trait::async_trait;
use regex::Regex;
use reqwest::Url;

pub struct ArxivProvider {
    client: reqwest::Client,
}

impl ArxivProvider {
    pub fn new() -> Self {
        Self { client: client() }
    }

    pub fn parse_fixture(xml: &str) -> Result<ProviderSearchResponse> {
        let mut hits = Vec::new();
        let entry_re = Regex::new(r"(?s)<entry>(.*?)</entry>").expect("regex");
        for entry in entry_re.captures_iter(xml) {
            let body = &entry[1];
            let Some(title) = capture_tag(body, "title") else {
                continue;
            };
            let Some(id) = capture_tag(body, "id") else {
                continue;
            };
            let summary = capture_tag(body, "summary");
            hits.push(crate::providers::hit_from_value(
                ProviderId::Arxiv,
                title,
                id,
                summary,
                vec!["arxiv".to_string()],
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

crate::providers::default_from_new!(ArxivProvider);

fn capture_tag(input: &str, tag: &str) -> Option<String> {
    let re = Regex::new(&format!(r"(?s)<{tag}[^>]*>(.*?)</{tag}>")).ok()?;
    re.captures(input)
        .and_then(|caps| caps.get(1))
        .map(|m| html_unescape(m.as_str().trim()))
        .filter(|value| !value.is_empty())
}

fn html_unescape(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[async_trait]
impl SearchProvider for ArxivProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Arxiv
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(false, true, false, false, false, false, true)
    }

    async fn search(&self, req: ProviderSearchRequest) -> Result<ProviderSearchResponse> {
        let mut url = Url::parse("https://export.arxiv.org/api/query")?;
        url.query_pairs_mut()
            .append_pair("search_query", &req.query)
            .append_pair("start", "0")
            .append_pair("max_results", &req.limit.to_string());
        let body = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let mut response = Self::parse_fixture(&body)?;
        response
            .receipts
            .push(ProviderReceipt::ok(self.id(), &req.query, &response.hits));
        Ok(response)
    }
}
