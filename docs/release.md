# Release

jekko-jnoccio releases follow the split-family release gate:

1. Update the version source in `Cargo.toml`.
2. Run `bash scripts/ci-local.sh`.
3. Preserve `agent/repo-score.json` and `agent/repo-score.md` from the
   declared Jankurai command.
4. Attach checksums or provenance for any produced artifact.
5. Roll back by reverting the release commit and re-running the same local gate.
