# Testing

jekko-jnoccio keeps release proof local and reproducible. The required proof
lane for source changes is:

- `just fast`
- `just check`
- `just test`
- `just typecheck`
- `just build`
- `bash ops/ci/security.sh`
- `bash ops/ci/jankurai.sh`

Launch gate evidence is recorded before any public release:

- Security: `bash ops/ci/security.sh` runs the local security receipt lane, and
  the workflow records the gitleaks, cargo-audit, zizmor, and SBOM commands.
- Backups: this repository has no production datastore; rollback is the backup
  control for source releases, and published artifacts must keep their release
  tag, checksum, and provenance record.
- Monitoring: maintainers watch CI, downstream portal sync, and issue reports
  after publication.
- Rollback: revert the release commit or move the release tag back to the last
  passing commit, then rerun the full proof lane.
- Abuse controls: this child repository exposes no network service by default;
  abuse handling is limited to dependency intake, workflow permissions, and
  release artifact integrity checks.

Repair receipts are the command outputs and refreshed `agent/repo-score.json`
and `agent/repo-score.md` artifacts produced by `bash ops/ci/jankurai.sh`.
Structured Rust errors stay in the crate API so the next agent can tie a failing
test or audit finding back to a typed repair path.

## Observability and Repair Evidence

The typed repair surface is implemented in `crates/domain/src/lib.rs` and
recorded locally in `crates/domain/observability.md`. The next agent should be
able to read the error, inspect the receipt, and rerun the narrow lane without
guessing.

### Exception Surface

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
  telemetry_fields:
    - trace_id
    - lane
    - result_code
    - repair_hint
    - receipt_path
```

### Trace Contract

```json
{
  "task_id": "JK-OBS-001",
  "lane": "observability",
  "result_code": "pass",
  "proof_command": "just score",
  "evidence_path": "crates/domain/observability.md",
  "purpose": "keep reruns local",
  "repair_hint": "rerun just score after the scoped domain change",
  "telemetry_path": "target/jankurai/observability/telemetry.jsonl"
}
```

`crates/domain/observability.md` is the local repair receipt for domain
failures. When a domain test or audit fails, record the failure in the
telemetry path, keep the change scoped to `crates/domain`, and rerun
`just score` before widening the fix.

### Repair Receipt Index

- Error contract: `DomainError::IdentityDrift` exposes `purpose()`, `reason()`,
  `repair_hint()`, `common_fixes()`, and `docs_url()` for agent routing.
- Telemetry contract: write `trace_id`, `lane`, `result_code`, and
  `repair_hint` into the local repair receipt before rerunning proof.
- Receipt contract: keep `crates/domain/observability.md` as the canonical
  local repair receipt and refresh `agent/repo-score.json` after a scoped fix.
- Rerun contract: use `just score` for the narrow domain lane and only widen
  scope if the receipt still points at the same failing boundary.
