use crate::safety::{block_internal_url, quarantine_content, strip_active_html};
use crate::types::{ExtractorId, Result, SearchError};
use reqwest::Client;

#[derive(Debug, Clone)]
pub struct ExtractedPage {
    pub url: String,
    pub text: String,
    pub quarantined: bool,
}

pub async fn extract_url(url: &str, allowed: &[ExtractorId]) -> Result<ExtractedPage> {
    if !allowed.iter().any(|id| {
        matches!(
            id,
            ExtractorId::BuiltIn | ExtractorId::Jina | ExtractorId::Firecrawl
        )
    }) {
        return Err(SearchError::Policy(
            "no extractors allowed by policy".to_string(),
        ));
    }
    block_internal_url(url)?;
    let client = Client::builder()
        .user_agent("agent-search/0.1")
        .build()
        .map_err(SearchError::Http)?;
    let body = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let text = strip_active_html(&body);
    let (text, quarantined) = quarantine_content(&text);
    Ok(ExtractedPage {
        url: url.to_string(),
        text,
        quarantined,
    })
}
