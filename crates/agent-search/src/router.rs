use crate::config::ProviderEntry;
use crate::types::*;
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct QueryRouter {
    pub mode: Option<QueryClass>,
}

impl QueryRouter {
    pub fn new() -> Self {
        Self { mode: None }
    }

    pub fn classify(&self, query: &str, objective: Option<&str>) -> QueryClass {
        if let Some(mode) = self.mode {
            return mode;
        }
        let mut text = query.to_ascii_lowercase();
        if let Some(objective) = objective {
            text.push(' ');
            text.push_str(&objective.to_ascii_lowercase());
        }
        if text.contains("arxiv")
            || text.contains("pubmed")
            || text.contains("doi")
            || text.contains("paper")
            || text.contains("citation")
        {
            return QueryClass::Academic;
        }
        if text.contains("github")
            || text.contains("code")
            || text.contains("repository")
            || text.contains("rust")
            || text.contains("python")
        {
            return QueryClass::Code;
        }
        if text.contains("breaking")
            || text.contains("latest")
            || text.contains("today")
            || text.contains("news")
        {
            return QueryClass::News;
        }
        if text.contains("web") || text.contains("search") {
            return QueryClass::Web;
        }
        QueryClass::Mixed
    }
}

crate::providers::default_from_new!(QueryRouter);

pub fn plan_providers(
    entries: &[ProviderEntry],
    query_class: QueryClass,
    policy: &ProviderPolicy,
) -> Vec<ProviderEntry> {
    let allow: HashSet<String> = policy
        .allow
        .iter()
        .map(|entry| entry.to_ascii_lowercase())
        .collect();
    let mut ranked: Vec<_> = entries
        .iter()
        .filter(|entry| allow.contains(entry.provider.id().as_str()))
        .filter(|entry| match query_class {
            QueryClass::Academic => entry.capabilities.academic || entry.capabilities.web,
            QueryClass::Code => entry.capabilities.code || entry.capabilities.web,
            QueryClass::News => entry.capabilities.news || entry.capabilities.web,
            QueryClass::Web => entry.capabilities.web,
            QueryClass::Mixed => true,
        })
        .cloned()
        .collect();

    ranked.sort_by_key(|entry| {
        let id = entry.provider.id().as_str();
        let prefer_rank = policy
            .prefer
            .iter()
            .position(|p| {
                p.eq_ignore_ascii_case("official_api") && !entry.capabilities.requires_key
            })
            .unwrap_or(usize::MAX);
        let stability = if entry.capabilities.privacy_first {
            0
        } else {
            1
        };
        (prefer_rank, stability, id.to_string())
    });
    ranked
}
