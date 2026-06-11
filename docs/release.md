# Release

jekko-jnoccio releases use the split-family launch gate and keep a reproducible
release record.

Required release evidence:

- changelog entry in `CHANGELOG.md`
- CI transcript from `bash scripts/ci-local.sh`
- Jankurai score artifacts from `bash ops/ci/jankurai.sh`
- checksum/provenance/SBOM evidence for any produced artifact
- rollback plan: revert the release commit and rerun `bash scripts/ci-local.sh`
- monitoring note: watch CI, issue reports, and downstream portal sync after publish
- abuse and rate-limit note: this child repo publishes no network service by default

Release steps:

1. Update the version source in `Cargo.toml`.
2. Update `CHANGELOG.md`.
3. Run `bash scripts/ci-local.sh`.
4. Commit refreshed `agent/repo-score.json` and `agent/repo-score.md`.
5. Attach checksums, provenance, and SBOM evidence to the release record.
