use crate::config::ProviderEntry;
use crate::dedupe::dedupe_hits;
use crate::router::plan_providers;
use crate::types::*;
use futures::future::join_all;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{timeout, Duration};

pub async fn search_parallel(
    providers: Vec<ProviderEntry>,
    request: ResearchRequest,
    query_class: QueryClass,
) -> ResearchResponse {
    let planned = plan_providers(&providers, query_class, &request.providers);
    let max_parallel = request.limits.max_parallel.max(1);
    let semaphore = Arc::new(Semaphore::new(max_parallel));
    let mut tasks = Vec::new();

    for (index, entry) in planned.into_iter().enumerate() {
        let permit = semaphore.clone();
        let req = ProviderSearchRequest {
            query: request.query.clone(),
            objective: request.objective.clone(),
            mode: query_class,
            limit: request.limits.max_pages.max(1),
            timeout_seconds: request.limits.timeout_seconds,
            extraction: request.extraction.clone(),
            evidence: request.evidence.clone(),
            safety: request.safety.clone(),
        };
        tasks.push(tokio::spawn(async move {
            let _permit = permit.acquire_owned().await.ok()?;
            let timeout_window = Duration::from_secs(req.timeout_seconds.max(1));
            let result = timeout(timeout_window, entry.provider.search(req)).await;
            Some((index, entry.provider.id(), result))
        }));
    }

    let mut hits = Vec::new();
    let mut evidence = Vec::new();
    let mut receipts = Vec::new();
    let mut warnings = Vec::new();

    let mut outputs = join_all(tasks).await;
    outputs.sort_by_key(|item| {
        item.as_ref()
            .ok()
            .and_then(|value| value.as_ref().map(|(index, _, _)| *index))
            .unwrap_or(usize::MAX)
    });

    for task in outputs {
        let Ok(Some((_index, provider_id, result))) = task else {
            continue;
        };
        match result {
            Ok(Ok(mut response)) => {
                hits.append(&mut response.hits);
                evidence.append(&mut response.evidence);
                receipts.extend(response.receipts);
                warnings.extend(response.warnings);
            }
            Ok(Err(err)) => {
                receipts.push(ProviderReceipt::failed(
                    provider_id,
                    &request.query,
                    err.to_string(),
                ));
                warnings.push(format!("{provider_id}: {err}"));
            }
            Err(_) => {
                receipts.push(ProviderReceipt::failed(
                    provider_id,
                    &request.query,
                    "provider timeout",
                ));
                warnings.push(format!("{provider_id}: provider timeout"));
            }
        }
    }

    hits = dedupe_hits(hits);
    ResearchResponse {
        hits,
        evidence,
        receipts,
        warnings,
    }
}
