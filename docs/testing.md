# Testing

Run the local proof lanes before changing this repo:

```bash
just fast
just check
just test
just typecheck
just build
bash ops/ci/jankurai.sh
```

`tests/identity.rs` and `crates/domain/tests/domain_error.rs` are integration
proofs for the split-family manifest identity and typed repair surface. The CI
wrappers call the same shell lanes as local development.

## Publish Proof Artifacts

Publishing is allowed only with these proof artifacts:

- CI transcript from `bash scripts/ci-local.sh`
- Jankurai JSON and Markdown score artifacts from `bash ops/ci/jankurai.sh`
- security evidence from `bash ops/ci/security.sh`
- checksum/provenance/SBOM evidence for produced artifacts
- changelog entry and rollback note in `docs/release.md`

## Cost Budget

This child repo has no paid runtime by default. If future work adds paid or
unbounded operations, the change must declare a quota, spend cap, stop
condition, and kill switch before publishing.
