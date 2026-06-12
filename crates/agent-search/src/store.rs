use crate::types::{EvidenceRecord, SearchHit};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub struct ProvenanceStore {
    conn: Connection,
}

impl ProvenanceStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init()?;
        Ok(store)
    }

    fn init(&self) -> Result<(), rusqlite::Error> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS provenance (
                content_hash TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                query TEXT NOT NULL,
                title TEXT NOT NULL,
                url TEXT NOT NULL,
                normalized_url TEXT NOT NULL,
                snippet TEXT,
                citation_ids TEXT NOT NULL,
                retrieved_at TEXT NOT NULL,
                published_at TEXT,
                expires_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS provenance_provider_idx ON provenance(provider);
            CREATE INDEX IF NOT EXISTS provenance_expires_idx ON provenance(expires_at);
            "#,
        )?;
        Ok(())
    }

    pub fn insert_hit(
        &self,
        hit: &SearchHit,
        query: &str,
        ttl_days: i64,
    ) -> Result<bool, rusqlite::Error> {
        let expires_at = (Utc::now() + Duration::days(ttl_days)).to_rfc3339();
        let citation_ids = serde_json::json!(hit.citation_ids).to_string();
        let changed = self.conn.execute(
            r#"
            INSERT OR IGNORE INTO provenance (
                content_hash, provider, query, title, url, normalized_url, snippet,
                citation_ids, retrieved_at, published_at, expires_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            params![
                hit.content_hash,
                hit.provider.as_str(),
                query,
                hit.title,
                hit.url,
                hit.normalized_url,
                hit.snippet,
                citation_ids,
                hit.retrieved_at.to_rfc3339(),
                hit.published_at.map(|value| value.to_rfc3339()),
                expires_at,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn insert_evidence(
        &self,
        evidence: &EvidenceRecord,
        ttl_days: i64,
    ) -> Result<bool, rusqlite::Error> {
        let expires_at = (Utc::now() + Duration::days(ttl_days)).to_rfc3339();
        let citation_ids = serde_json::json!([evidence.citation_id.clone()]).to_string();
        let changed = self.conn.execute(
            r#"
            INSERT OR IGNORE INTO provenance (
                content_hash, provider, query, title, url, normalized_url, snippet,
                citation_ids, retrieved_at, published_at, expires_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            params![
                evidence.content_hash,
                evidence.provider.as_str(),
                evidence.citation_id,
                evidence.title,
                evidence.url,
                evidence.normalized_url,
                evidence.snippet,
                citation_ids,
                evidence.retrieved_at.to_rfc3339(),
                evidence.published_at.map(|value| value.to_rfc3339()),
                expires_at,
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn prune_expired(&self, now: DateTime<Utc>) -> Result<usize, rusqlite::Error> {
        self.conn.execute(
            "DELETE FROM provenance WHERE expires_at < ?1",
            params![now.to_rfc3339()],
        )
    }

    pub fn contains_hash(&self, hash: &str) -> Result<bool, rusqlite::Error> {
        let found: Option<String> = self
            .conn
            .query_row(
                "SELECT content_hash FROM provenance WHERE content_hash = ?1 LIMIT 1",
                params![hash],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }
}
