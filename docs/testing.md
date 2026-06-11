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
