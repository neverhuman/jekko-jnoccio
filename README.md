# jekko-jnoccio

[![Jankurai score](https://img.shields.io/badge/Jankurai-score-brightgreen)](agent/repo-score.md)

Read [AGENTS.md](AGENTS.md) first.

`jekko-jnoccio` is the router split-family repository in the Jekko baseline.

- Target stack: Rust split-family child repo with local CI, audit, and
  Jankurai metadata.
- Score artifacts: `agent/repo-score.json` and `agent/repo-score.md`.
- Remotes: wired to the canonical Jeryu and GitHub URLs.

## Quick start

1. `bash scripts/ci-doctor.sh`
2. `just fast`
3. `just score`
