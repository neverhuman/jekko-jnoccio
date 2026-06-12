# Testing and Proof Lanes

Use `agent/test-map.json` for the narrow proof route for a changed path. The local parity entry point is:

```bash
bash scripts/ci-local.sh
```

## Lanes

- Fast Rust proof: `bash ops/ci/fast.sh`
- Security and dependency proof: `bash ops/ci/security.sh`
- Release readiness proof: `bash ops/ci/jankurai.sh` plus `docs/release.md` evidence
- Audit proof: `bash ops/ci/jankurai.sh`
- Doctor proof: `bash scripts/ci-doctor.sh`

Rust invariant coverage is tracked as property-style proof. New invariants should use proptest, quickcheck, or rstest where the crate already has that harness; otherwise add focused unit or integration coverage and route it through `bash ops/ci/fast.sh`.

## Repair Errors

Agent-readable errors include `purpose`, `reason`, common fixes, `docs_url`, and `repair_hint`. The purpose names the boundary that failed, the reason gives the stable machine-readable cause, common fixes list the smallest likely local repairs, `docs_url` points to this file or a more specific owner document, and `repair_hint` names the next rerun command.

## Budgets and Stop Conditions

CI lanes must not require real credentials, browser profiles, SSH, paid APIs, or private runtime state. Paid or unbounded work has a zero-default budget in CI; it requires an explicit budget, quota, spend cap, kill switch, and stop condition before any remote or account-backed run.

## Launch Gates

Release evidence must cover security scans, backup or preservation behavior, monitoring and audit artifacts, rollback instructions, and abuse controls for prompt or account agency. The minimum local launch gate is `bash ops/ci/jankurai.sh`; failures stop release work until the named artifact is repaired.
