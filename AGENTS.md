# jekko-jnoccio Agent Instructions

- Read `agent/JANKURAI_STANDARD.md` first.
- Keep product behavior in Rust under `src/` with integration proof in `tests/`.
- Keep CI entrypoints thin and shell-driven under `ops/ci/*.sh`.
- Keep the remotes wired to the canonical Jeryu and GitHub URLs.
- Pin Jankurai to `1.6.1` and keep `audit-clean` evidence explicit.
- Generated audit artifacts are `agent/repo-score.json` and `agent/repo-score.md`; regenerate them with `bash ops/ci/jankurai.sh`.
