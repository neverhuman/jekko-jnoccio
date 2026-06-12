use agent_search::SearchConfig;
use agent_search::extract::extract_url;
use agent_search::parallel::search_parallel;
use agent_search::router::QueryRouter;
use agent_search::store::ProvenanceStore;
use agent_search::types::{
    EvidenceRecord, QueryClass, ResearchLimits, ResearchRequest, ResearchResponse,
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;

pub async fn jnoccio_search(arguments: Value, env: &HashMap<String, String>) -> Result<Value> {
    let response = run_search(arguments, SearchMode::Search, env).await?;
    Ok(serialize_search_response("jnoccio_search", response))
}

pub async fn jnoccio_research(arguments: Value, env: &HashMap<String, String>) -> Result<Value> {
    let response = run_search(arguments, SearchMode::Research, env).await?;
    Ok(serialize_search_response("jnoccio_research", response))
}

pub async fn jnoccio_extract(arguments: Value, env: &HashMap<String, String>) -> Result<Value> {
    let url = argument_string(&arguments, "url")?;
    let config = SearchConfig::from_env_map(Some(env));
    let allowed = config.extraction.allowed_extractors.clone();
    let page = extract_url(&url, &allowed).await?;
    Ok(json!({
        "url": page.url,
        "text": page.text,
        "quarantined": page.quarantined,
        "receipts": [],
        "warnings": page.quarantined.then_some("content quarantined"),
    }))
}

enum SearchMode {
    Search,
    Research,
}

async fn run_search(
    arguments: Value,
    mode: SearchMode,
    env: &HashMap<String, String>,
) -> Result<ResearchResponse> {
    let config = SearchConfig::from_env_map(Some(env));
    let query = argument_string(&arguments, "query")?;
    let objective = argument_string_optional(&arguments, "objective");
    let query_mode = parse_mode(argument_string_optional(&arguments, "mode").as_deref());
    let max_parallel = argument_u64(&arguments, "max_parallel")
        .or_else(|| arguments.get("maxParallel").and_then(Value::as_u64))
        .unwrap_or(config.limits.max_parallel as u64)
        .clamp(1, 20) as usize;
    let timeout_seconds = argument_u64(&arguments, "timeout_seconds")
        .or_else(|| arguments.get("timeoutSeconds").and_then(Value::as_u64))
        .unwrap_or(config.limits.timeout_seconds)
        .max(1);

    let router = QueryRouter::new();
    let lane = if matches!(query_mode, QueryClass::Mixed)
        && argument_string_optional(&arguments, "mode").as_deref() == Some("auto")
    {
        router.classify(&query, objective.as_deref())
    } else {
        query_mode
    };

    let request = ResearchRequest {
        query: query.clone(),
        objective: objective.clone(),
        mode: lane,
        providers: config.provider_policy.clone(),
        limits: ResearchLimits {
            max_queries: config.limits.max_queries,
            max_pages: arguments
                .get("max_pages")
                .and_then(Value::as_u64)
                .or_else(|| arguments.get("maxPages").and_then(Value::as_u64))
                .unwrap_or(config.limits.max_pages as u64) as usize,
            max_parallel,
            timeout_seconds,
            max_cost_usd: config.limits.max_cost_usd,
        },
        extraction: config.extraction.clone(),
        evidence: config.evidence.clone(),
        safety: config.safety.clone(),
    };
    let require_citations = request.evidence.require_citations;

    let mut response = search_parallel(config.providers.clone(), request, lane).await;
    response.receipts.extend(config.skipped);

    if let Some(path) = config.store_path {
        persist_hits(&path, &query, &response.hits)?;
    }

    if matches!(mode, SearchMode::Research) {
        response.evidence = evidence_from_hits(&response.hits);
        if response.evidence.is_empty() && require_citations {
            response
                .warnings
                .push("no citation-bearing hits were found".to_string());
        }
    }

    if response.receipts.is_empty() {
        response
            .warnings
            .push("no research providers were available".to_string());
    }

    Ok(response)
}

fn serialize_search_response(tool_name: &str, response: ResearchResponse) -> Value {
    json!({
        "tool": tool_name,
        "hits": response.hits,
        "evidence": response.evidence,
        "receipts": response.receipts,
        "warnings": response.warnings,
        "hit_count": response.hits.len(),
        "evidence_count": response.evidence.len(),
    })
}

fn evidence_from_hits(hits: &[agent_search::SearchHit]) -> Vec<EvidenceRecord> {
    hits.iter()
        .filter_map(|hit| {
            let citation_id = hit.citation_ids.first()?.clone();
            Some(EvidenceRecord {
                provider: hit.provider,
                citation_id,
                url: hit.url.clone(),
                normalized_url: hit.normalized_url.clone(),
                title: hit.title.clone(),
                retrieved_at: hit.retrieved_at,
                published_at: hit.published_at,
                content_hash: hit.content_hash.clone(),
                snippet: hit.snippet.clone(),
            })
        })
        .collect()
}

fn persist_hits(path: &PathBuf, query: &str, hits: &[agent_search::SearchHit]) -> Result<()> {
    let store = ProvenanceStore::open(path)
        .with_context(|| format!("open provenance store at {}", path.display()))?;
    for hit in hits {
        let _ = store.insert_hit(hit, query, 30)?;
    }
    Ok(())
}

fn parse_mode(value: Option<&str>) -> QueryClass {
    match value.unwrap_or("auto").to_ascii_lowercase().as_str() {
        "web" => QueryClass::Web,
        "academic" => QueryClass::Academic,
        "news" => QueryClass::News,
        "code" => QueryClass::Code,
        "mixed" => QueryClass::Mixed,
        _ => QueryClass::Mixed,
    }
}

fn argument_string(arguments: &Value, key: &str) -> Result<String> {
    let Some(value) = arguments.get(key).and_then(Value::as_str) else {
        bail!("missing {key}");
    };
    let value = value.trim();
    if value.is_empty() {
        bail!("{key} must not be empty");
    }
    Ok(value.to_string())
}

fn argument_string_optional(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn argument_u64(arguments: &Value, key: &str) -> Option<u64> {
    arguments.get(key).and_then(Value::as_u64)
}
