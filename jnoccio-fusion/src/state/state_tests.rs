use super::state_util::{minute_floor, now_unix};
use super::*;
use crate::limits::ErrorKind;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_seed_preserves_hard_failure_state() {
        let interim = tempfile::tempdir().unwrap();
        let db = StateDb::open(interim.path().join("state.sqlite")).unwrap();
        db.upsert_model("github/llama", "github", "ready").unwrap();
        db.record_failure(
            "request-1",
            "draft",
            "github/llama",
            "github",
            &ErrorKind::ModelUnavailable,
            12,
            Some(now_unix() + 86_400),
            Some("unknown_model"),
            &RouteEventMeta::default(),
            None,
        )
        .unwrap();

        db.upsert_model("github/llama", "github", "ready").unwrap();

        let state = db.state_for("github/llama").unwrap().unwrap();
        assert_eq!(state.status, "model_unavailable");
        assert_eq!(state.last_error_kind.as_deref(), Some("ModelUnavailable"));
        assert_eq!(state.last_error_message.as_deref(), Some("unknown_model"));
        assert!(state.disabled_until.is_some());
    }

    #[test]
    fn startup_seed_allows_missing_key_to_replace_failure_state() {
        let interim = tempfile::tempdir().unwrap();
        let db = StateDb::open(interim.path().join("state.sqlite")).unwrap();
        db.upsert_model("github/llama", "github", "ready").unwrap();
        db.record_failure(
            "request-1",
            "draft",
            "github/llama",
            "github",
            &ErrorKind::ModelUnavailable,
            12,
            Some(now_unix() + 86_400),
            Some("unknown_model"),
            &RouteEventMeta::default(),
            None,
        )
        .unwrap();

        db.upsert_model("github/llama", "github", "missing_key")
            .unwrap();

        let state = db.state_for("github/llama").unwrap().unwrap();
        assert_eq!(state.status, "missing_key");
        assert!(state.last_error_kind.is_none());
        assert!(state.last_error_message.is_none());
        assert!(state.disabled_until.is_none());
    }

    #[test]
    fn idempotent_migration_preserves_rows() {
        let interim = tempfile::tempdir().unwrap();
        let path = interim.path().join("state.sqlite");
        StateDb::open(&path)
            .unwrap()
            .upsert_model("provider/model", "provider", "ready")
            .unwrap();
        let reopened = StateDb::open(&path).unwrap();
        let state = reopened.state_for("provider/model").unwrap().unwrap();
        assert_eq!(state.status, "ready");
    }

    #[test]
    fn two_handles_pool_minute_usage_under_wal() {
        let interim = tempfile::tempdir().unwrap();
        let path = interim.path().join("state.sqlite");
        let first = StateDb::open(&path).unwrap();
        let second = StateDb::open(&path).unwrap();
        first
            .record_attempt(
                "request-1",
                "fast",
                "provider/model",
                "provider",
                &RouteEventMeta::default(),
                None,
            )
            .unwrap();
        second
            .record_attempt(
                "request-2",
                "fast",
                "provider/model",
                "provider",
                &RouteEventMeta::default(),
                None,
            )
            .unwrap();
        let usage = first.usage_since(now_unix() - 3600).unwrap();
        assert_eq!(usage[0].attempts, 2);
    }

    #[test]
    fn token_rate_estimate_projects_median_and_ten_minute_max() {
        let interim = tempfile::tempdir().unwrap();
        let db = StateDb::open(interim.path().join("state.sqlite")).unwrap();
        let now = minute_floor(1_800_000);
        {
            let conn = db.conn.lock().expect("sqlite mutex poisoned");
            for minutes_ago in 0..60 {
                let tokens = if minutes_ago < 10 { 5_000 } else { 1_000 };
                conn.execute(
                    r#"
          INSERT INTO model_usage_minute (
            model_id, provider, minute_ts, attempts, successes, failures, wins,
            prompt_tokens, completion_tokens, total_tokens, latency_count, latency_total_ms
          )
          VALUES (?1, ?2, ?3, 0, 1, 0, 0, 0, ?4, ?4, 0, 0)
          "#,
                    rusqlite::params![
                        "provider/model",
                        "provider",
                        now - (minutes_ago * 60),
                        tokens
                    ],
                )
                .unwrap();
            }
        }

        let estimate = db.token_rate_estimate(now, 60, 10).unwrap();

        assert_eq!(estimate.window_minutes, 60);
        assert_eq!(estimate.smoothing_minutes, 10);
        assert_eq!(estimate.sample_minutes, 60);
        assert!((estimate.median_m_tokens_per_24h - 1.44).abs() < 0.001);
        assert!((estimate.max_m_tokens_per_24h - 7.2).abs() < 0.001);
    }

    #[test]
    fn event_retention_keeps_configured_recent_rows() {
        let interim = tempfile::tempdir().unwrap();
        let db = StateDb::open_with_retention(interim.path().join("state.sqlite"), 2).unwrap();
        for index in 0..4 {
            db.record_attempt(
                &format!("request-{index}"),
                "fast",
                "provider/model",
                "provider",
                &RouteEventMeta::default(),
                None,
            )
            .unwrap();
        }
        assert_eq!(db.recent_metric_events(10).unwrap().len(), 2);
    }

    #[test]
    fn active_agents_count_tracks_multiple_run_ids() {
        let interim = tempfile::tempdir().unwrap();
        let db = StateDb::open(interim.path().join("state.sqlite")).unwrap();
        db.record_agent_activity(&AgentSource {
            id: "agent-1".to_string(),
            client: Some("opencode-cli".to_string()),
            session_id: Some("session-1".to_string()),
            agent_role: Some("build".to_string()),
            zyal_run_id: Some("run-1".to_string()),
            zyal_lane_id: Some("lane-1".to_string()),
            credential_user_id: None,
            credential_policy: None,
            process_role: Some("main".to_string()),
            pid: Some(111),
            user_agent: Some("opencode/1.0".to_string()),
            version: Some("v1".to_string()),
        })
        .unwrap();
        db.record_agent_activity(&AgentSource {
            id: "agent-2".to_string(),
            client: Some("opencode-cli".to_string()),
            session_id: Some("session-2".to_string()),
            agent_role: Some("critic".to_string()),
            zyal_run_id: Some("run-1".to_string()),
            zyal_lane_id: Some("lane-2".to_string()),
            credential_user_id: None,
            credential_policy: None,
            process_role: Some("worker".to_string()),
            pid: Some(222),
            user_agent: Some("opencode/1.0".to_string()),
            version: Some("v1".to_string()),
        })
        .unwrap();

        assert_eq!(db.active_agents_live(now_unix()).unwrap().len(), 2);
    }

    #[test]
    fn expired_active_agents_are_pruned() {
        let interim = tempfile::tempdir().unwrap();
        let db = StateDb::open(interim.path().join("state.sqlite")).unwrap();
        db.record_agent_activity(&AgentSource {
            id: "agent-expired".to_string(),
            client: None,
            session_id: None,
            agent_role: None,
            zyal_run_id: None,
            zyal_lane_id: None,
            credential_user_id: None,
            credential_policy: None,
            process_role: Some("main".to_string()),
            pid: Some(111),
            user_agent: None,
            version: None,
        })
        .unwrap();

        assert_eq!(db.active_agents_live(now_unix()).unwrap().len(), 1);
        let rows = db
            .prune_expired_agent_activity(1, now_unix() + 120)
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(db.active_agents_live(now_unix() + 120).unwrap().len(), 0);
    }

    #[test]
    fn metric_events_store_agent_fields() {
        let interim = tempfile::tempdir().unwrap();
        let db = StateDb::open(interim.path().join("state.sqlite")).unwrap();
        let agent = AgentSource {
            id: "agent-with-fields".to_string(),
            client: Some("agent-client".to_string()),
            session_id: Some("agent-session".to_string()),
            agent_role: Some("answerer".to_string()),
            zyal_run_id: Some("run-qbank".to_string()),
            zyal_lane_id: Some("lane-qbank".to_string()),
            credential_user_id: None,
            credential_policy: None,
            process_role: Some("main".to_string()),
            pid: Some(100),
            user_agent: Some("opencode/agent".to_string()),
            version: Some("v1".to_string()),
        };
        db.record_attempt(
            "request-1",
            "fast",
            "provider/model",
            "provider",
            &RouteEventMeta::default(),
            Some(&agent),
        )
        .unwrap();
        db.record_success(RecordSuccessInput {
            request_id: "request-1",
            phase: "fast",
            model_id: "provider/model",
            provider: "provider",
            latency_ms: 42,
            winner_model_id: Some("provider/model"),
            usage: None,
            meta: &RouteEventMeta::default(),
            agent: Some(&agent),
        })
        .unwrap();
        db.record_failure(
            "request-1",
            "fusion",
            "provider/model",
            "provider",
            &ErrorKind::Unknown,
            99,
            None,
            Some("failed"),
            &RouteEventMeta::default(),
            Some(&agent),
        )
        .unwrap();

        let events = db.recent_metric_events(3).unwrap();
        assert_eq!(events.len(), 3);
        assert!(
            events
                .iter()
                .all(|event| event.agent_id.as_deref() == Some("agent-with-fields"))
        );
        assert!(
            events
                .iter()
                .all(|event| event.agent_client.as_deref() == Some("agent-client"))
        );
        assert!(
            events
                .iter()
                .all(|event| event.agent_session_id.as_deref() == Some("agent-session"))
        );
    }

    #[test]
    fn recent_metric_events_after_returns_new_rows() {
        let interim = tempfile::tempdir().unwrap();
        let db = StateDb::open(interim.path().join("state.sqlite")).unwrap();
        let first = db
            .record_attempt(
                "request-1",
                "fast",
                "provider/model",
                "provider",
                &RouteEventMeta::default(),
                None,
            )
            .unwrap();
        let second = db
            .record_attempt(
                "request-2",
                "fast",
                "provider/model",
                "provider",
                &RouteEventMeta::default(),
                None,
            )
            .unwrap();
        let tail = db.recent_metric_events_after(first.id, 10).unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].id, second.id);
    }
}
