# Release

jekko-jnoccio release readiness is intentionally conservative for the split-port branch. Version source is the root `Cargo.toml` and any package manifest copied with the owned surface. Release history is tracked in `CHANGELOG.md`.

## Release Gate

The release gate is:

```bash
just fast
bash scripts/ci-local.sh
bash ops/ci/jankurai.sh
```

The gate records audit artifacts under `.jankurai/`, `target/jankurai/`, and `agent/repo-score.{json,md}`.

## Integrity

Before publishing, record checksum or sha256 evidence for release archives, attach provenance or SBOM evidence when artifacts are distributed, and keep the generated lockfile with the release commit.

## Rollback

Rollback starts by preserving the current commit and score artifacts, restoring the last known-good release commit, rerunning the release gate, and recording monitoring status. Backup or preservation behavior, rollback instructions, monitoring evidence, and rate limit or abuse controls are required launch gate inputs.
