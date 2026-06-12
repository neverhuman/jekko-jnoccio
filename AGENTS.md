# jekko-jnoccio Agent Instructions

- Read `agent/JANKURAI_STANDARD.md` first.
- Treat this as an independent repository, not as part of a parent Cargo workspace.
- Keep all Cargo path dependencies repo-local; do not point at sibling split repos or the portal checkout.
- Keep CI entrypoints thin and shell-driven under `ops/ci/*.sh`.
- Keep the remotes wired to the canonical Jeryu and GitHub URLs.
- Pin Jankurai to `1.6.1`; regenerate `agent/repo-score.json` and `agent/repo-score.md` with `bash ops/ci/jankurai.sh`.
- Do not commit secrets, runtime state, `.jankurai/`, `target/`, browser profiles, downloads, logs, local env files, or tokens.
