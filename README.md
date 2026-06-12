# jekko-jnoccio

Status badge marker: jankurai-badge.

jekko-jnoccio-boot, jnoccio-fusion split out of the Jekko portal as an independently buildable Rust repository.

This repository is a standalone split-family checkout. It contains repo-local workspace members only; support crates copied from the portal are present under `crates/` so CI does not depend on sibling split repositories. Read [AGENTS.md](AGENTS.md) before editing.

## Quick Start

```bash
just fast
just score
just score-fast
bash ops/ci/jankurai.sh
bash scripts/ci-doctor.sh
bash scripts/ci-local.sh
```

## Target Stack

The target stack is Rust workspace code with shell-based CI parity and Jankurai audit artifacts.

## Primary Owned Surfaces

- jekko-jnoccio-boot
- jnoccio-fusion

## Workspace Members

- `crates/agent-search`
- `crates/jekko-jnoccio-boot`
- `crates/zyal-core`
- `crates/zyal-key-pool`

## Jankurai Score Flow

Jankurai writes `.jankurai/repo-score.{json,md}` first, then mirrors the same score files into `target/jankurai/` and tracked `agent/repo-score.{json,md}`. Score history remains in `.jankurai/` and is mirrored into `target/jankurai/`.
