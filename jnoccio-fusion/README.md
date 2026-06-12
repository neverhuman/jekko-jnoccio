<img src="assets/jnoccio_header.png" alt="Jnoccio" width="100%" />

# Jnoccio Fusion Gateway

Jnoccio is a standalone OpenAI-compatible gateway for routing one visible model, `jnoccio/jnoccio-fusion`, across many upstream providers. It learns provider health, rate capacity, context limits, and winning models from real traffic so future requests avoid unsafe routes.

## Routing

- `config/models.json` declares upstream models, context windows, output caps, roles, scores, and published rate limits.
- Runtime state is stored in SQLite under `state/jnoccio.sqlite`.
- Context overruns are parsed for concrete limits such as context window, request token cap, TPM cap, requested tokens, and prompt/tool/output token splits.
- Learned safe windows are used as hard routing eligibility: estimated prompt tokens plus requested output reserve must fit before a model can be selected.
- Small prompts prefer the smallest safe context band while larger prompts route only to models with learned-safe headroom.

## Dashboard

The dashboard is now served natively in the Jekko TUI. Press `Ctrl+J` to open the Jnoccio Fusion dashboard with real-time model health, wins, latency, token usage, capacity, recent events, and context-run histograms. The legacy `/dashboard/` web UI has been removed.

## Runtime And Scaling

Jnoccio runs as a Rust `axum`/Tokio gateway. The main instance uses up to 10 Tokio worker threads by default based on available CPU parallelism, while spawned instances default to 2 worker threads. Managed gateway scaling is capped at 20 total instances including the main gateway; `jnoccio_spawn_parallel` and `jnoccio_spawn_instance` report the current count, max count, and available slots.

## Setup

```bash
cd /Users/bentaylor/Code/opencode/jnoccio-fusion
cp .env.jnoccio.example .env.jnoccio
$EDITOR .env.jnoccio
rtk cargo run -- --config config/server.json --env-file .env.jnoccio
```

Install the Jekko config fragment when needed:

```bash
./scripts/install-jekko-config.sh
```

The seeded install bundle lives in `~/.config/jekko/jnoccio-fusion/` and is safe to edit directly. `server.jsonc` carries the user-facing defaults and comments, while `models.json` stays as the model registry copy used by the loader. The knobs most users change are `routing.fusion_sample_rate`, `routing.fast_backup_count`, `routing.event_retention_rows`, `routing.minute_bucket_retention_days`, `runtime.spawned_worker_threads`, `scaling.max_instances`, `scaling.spawn_batch_limit`, and the operational `bind`, `database`, `receipts_dir`, and optional `core_token` fields.

Agent MCP snippets live in:

- `agents/codex/AGENTS.md`
- `agents/codex/config.toml.snippet`
- `agents/claude/CLAUDE.md`
- `agents/claude/.mcp.json`

## Validation

```bash
rtk cargo test
rtk cargo check
```

Run `rtk ./scripts/smoke.sh` only when upstream keys are present.

## Safety

Do not commit `.env.jnoccio`, `state/`, `receipts/`, `target/`, or SQLite files. Failure receipts are local runtime artifacts and may include provider error bodies after secret redaction.
