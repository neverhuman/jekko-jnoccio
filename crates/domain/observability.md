# Domain Observability Contract

```yaml
repair_ticket:
  domain: observability
  code: OBS-001
  boundary: repair
  retryable: true
  purpose: typed agent-friendly repair surface
  repair_hint: rerun `just score`
  common_fixes:
    - inspect `docs/testing.md`
    - keep the fix scoped to `crates/domain`
    - rerun the narrow lane before widening the change
```

```json
{
  "task_id": "JK-OBS-001",
  "lane": "observability",
  "result_code": "pass",
  "proof_command": "just score",
  "evidence_path": "crates/domain/observability.md",
  "purpose": "keep reruns local",
  "repair_hint": "rerun just score after the scoped domain change"
}
```
