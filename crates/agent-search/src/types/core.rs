use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QueryClass {
    Web,
    Academic,
    News,
    Code,
    #[default]
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    OpenAlex,
    Crossref,
    Arxiv,
    PubMed,
    Gdelt,
    Brave,
    Tavily,
    Exa,
    Searxng,
    SemanticScholar,
    Unpaywall,
    Github,
    Firecrawl,
    Jina,
}

impl ProviderId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAlex => "openalex",
            Self::Crossref => "crossref",
            Self::Arxiv => "arxiv",
            Self::PubMed => "pubmed",
            Self::Gdelt => "gdelt",
            Self::Brave => "brave",
            Self::Tavily => "tavily",
            Self::Exa => "exa",
            Self::Searxng => "searxng",
            Self::SemanticScholar => "semantic_scholar",
            Self::Unpaywall => "unpaywall",
            Self::Github => "github",
            Self::Firecrawl => "firecrawl",
            Self::Jina => "jina",
        }
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProviderId {
    type Err = SearchError;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openalex" => Ok(Self::OpenAlex),
            "crossref" => Ok(Self::Crossref),
            "arxiv" => Ok(Self::Arxiv),
            "pubmed" => Ok(Self::PubMed),
            "gdelt" => Ok(Self::Gdelt),
            "brave" => Ok(Self::Brave),
            "tavily" => Ok(Self::Tavily),
            "exa" => Ok(Self::Exa),
            "searxng" => Ok(Self::Searxng),
            "semantic_scholar" | "semanticscholar" | "semantic-scholar" => {
                Ok(Self::SemanticScholar)
            }
            "unpaywall" => Ok(Self::Unpaywall),
            "github" => Ok(Self::Github),
            "firecrawl" => Ok(Self::Firecrawl),
            "jina" => Ok(Self::Jina),
            _ => Err(SearchError::UnknownProvider(value.to_string())),
        }
    }
}

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
    #[error("provider unavailable: {0}")]
    Unavailable(String),
    #[error("request failed: {0}")]
    Request(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("policy blocked request: {0}")]
    Policy(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Url(#[from] url::ParseError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Time(#[from] chrono::ParseError),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, SearchError>;
