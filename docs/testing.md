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
and property proofs for the split-family manifest identity and typed repair
surface. The CI wrappers call the same shell lanes as local development.

## Resource Budget

This child repo has no paid runtime by default. If future work adds paid or
unbounded operations, the change must declare a quota, spend cap, stop
condition, and kill switch in the same patch.
