use agent_search::providers::{
    arxiv::ArxivProvider, brave::BraveProvider, crossref::CrossrefProvider, exa::ExaProvider,
    firecrawl::FirecrawlProvider, gdelt::GdeltProvider, github::GithubProvider, jina::JinaProvider,
    openalex::OpenAlexProvider, pubmed::PubMedProvider, searxng::SearxngProvider,
    semantic_scholar::SemanticScholarProvider, tavily::TavilyProvider,
    unpaywall::UnpaywallProvider,
};
use agent_search::ProviderId;
use serde_json::json;

fn assert_single_hit(
    result: agent_search::Result<agent_search::ProviderSearchResponse>,
    provider: ProviderId,
) {
    let response = result.expect("fixture parses");
    assert_eq!(response.hits.len(), 1);
    assert_eq!(response.hits[0].provider, provider);
}

#[test]
fn parses_openalex_fixture() {
    assert_single_hit(
        OpenAlexProvider::parse_fixture(&json!({
            "results": [{
                "display_name": "OpenAlex title",
                "doi": "https://doi.org/10.1/example",
                "abstract": "abstract"
            }]
        })),
        ProviderId::OpenAlex,
    );
}

#[test]
fn parses_crossref_fixture() {
    assert_single_hit(
        CrossrefProvider::parse_fixture(&json!({
            "message": {
                "items": [{
                    "title": "Crossref title",
                    "DOI": "10.1/example",
                    "abstract": "abstract"
                }]
            }
        })),
        ProviderId::Crossref,
    );
}

#[test]
fn parses_arxiv_fixture() {
    assert_single_hit(
        ArxivProvider::parse_fixture(
            r#"<feed><entry><title>ArXiv title</title><id>https://arxiv.org/abs/1234.5678</id><summary>abstract</summary></entry></feed>"#,
        ),
        ProviderId::Arxiv,
    );
}

#[test]
fn parses_pubmed_fixture() {
    assert_single_hit(
        PubMedProvider::parse_fixture(&json!({
            "esearchresult": { "idlist": ["12345"] }
        })),
        ProviderId::PubMed,
    );
}

#[test]
fn parses_gdelt_fixture() {
    assert_single_hit(
        GdeltProvider::parse_fixture(&json!({
            "articles": [{
                "title": "GDELT title",
                "url": "https://example.com/news",
                "snippet": "news"
            }]
        })),
        ProviderId::Gdelt,
    );
}

#[test]
fn parses_brave_fixture() {
    assert_single_hit(
        BraveProvider::parse_fixture(&json!({
            "web": { "results": [{ "title": "Brave title", "url": "https://example.com", "description": "snippet" }] }
        })),
        ProviderId::Brave,
    );
}

#[test]
fn parses_tavily_fixture() {
    assert_single_hit(
        TavilyProvider::parse_fixture(&json!({
            "results": [{ "title": "Tavily title", "url": "https://example.com", "content": "snippet" }]
        })),
        ProviderId::Tavily,
    );
}

#[test]
fn parses_exa_fixture() {
    assert_single_hit(
        ExaProvider::parse_fixture(&json!({
            "results": [{ "title": "Exa title", "url": "https://example.com", "text": "snippet" }]
        })),
        ProviderId::Exa,
    );
}

#[test]
fn parses_searxng_fixture() {
    assert_single_hit(
        SearxngProvider::parse_fixture(&json!({
            "results": [{ "title": "Searxng title", "url": "https://example.com", "content": "snippet" }]
        })),
        ProviderId::Searxng,
    );
}

#[test]
fn parses_semantic_scholar_fixture() {
    assert_single_hit(
        SemanticScholarProvider::parse_fixture(&json!({
            "data": [{ "title": "Scholar title", "url": "https://example.com", "abstract": "snippet" }]
        })),
        ProviderId::SemanticScholar,
    );
}

#[test]
fn parses_unpaywall_fixture() {
    assert_single_hit(
        UnpaywallProvider::parse_fixture(&json!({
            "results": [{ "doi": "10.1/example", "title": "Unpaywall title", "best_oa_location": { "url": "https://example.com" } }]
        })),
        ProviderId::Unpaywall,
    );
}

#[test]
fn parses_github_fixture() {
    assert_single_hit(
        GithubProvider::parse_fixture(&json!({
            "items": [{ "full_name": "owner/repo", "html_url": "https://github.com/owner/repo", "description": "snippet" }]
        })),
        ProviderId::Github,
    );
}

#[test]
fn parses_firecrawl_fixture() {
    assert_single_hit(
        FirecrawlProvider::parse_fixture(&json!({
            "data": [{ "title": "Firecrawl title", "url": "https://example.com", "markdown": "snippet" }]
        })),
        ProviderId::Firecrawl,
    );
}

#[test]
fn parses_jina_fixture() {
    assert_single_hit(
        JinaProvider::parse_fixture(&json!({
            "data": [{ "title": "Jina title", "url": "https://example.com", "text": "snippet" }]
        })),
        ProviderId::Jina,
    );
}
