# Testing

Run these local checks before changing this repo:

```bash
just fast
just check
just test
just typecheck
just build
bash ops/ci/jankurai.sh
```

`tests/identity.rs` checks the split-family manifest identity. The domain crate
has its own error metadata test under `crates/domain/tests/`.
