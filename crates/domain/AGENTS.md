# crates/domain

- Domain code owns typed errors and repair hints.
- Keep every error variant paired with `repair_hint`, `common_fixes`, and `docs_url` helpers.
- Keep `observability.md` in sync with the local repair receipt for this crate.
- Route proof through `just test` and the root integration test lane.
