# crates/domain

- Domain code owns typed errors and repair hints.
- Keep every error variant paired with `repair_hint`, `common_fixes`, and `docs_url` helpers.
- Route proof through `just test` and the root integration test lane.
