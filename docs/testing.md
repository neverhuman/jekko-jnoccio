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

`tests/identity.rs` is the integration contract for the split-family manifest
identity. The CI wrappers call the same shell lanes as local development.
