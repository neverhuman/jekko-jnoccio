# Jnoccio MCP Usage For Codex Agents

Jnoccio exposes a local MCP server that Codex agents can use for focused delegation, routing/status checks, and compact second-pass analysis. It is not an Jekko instruction file; it is guidance for agents that have this repository checked out.

## Required Behavior

- Use `jnoccio_delegate` for isolated questions with a clear expected output: findings, summaries, review notes, patch guidance, or decision support.
- Keep delegated prompts small and self-contained. Include only the goal, relevant paths or short snippets, constraints, and expected output.
- Do not send secrets, API keys, private headers, full prompt logs, full tool arguments, or unnecessary raw files.
- Use `jnoccio_status` before expensive delegation when provider health, context limits, or remaining capacity matter.
- Use `jnoccio_spawn_parallel` to spin up multiple gateway instances for concurrent workloads (e.g. parallel research, multi-file edits, batch delegation). It respects a 20-total-instance hard cap including the main gateway, and all instances share the same database and model pool.
- Use `jnoccio_spawn_instance` to add a single extra gateway instance when incremental scaling is needed.
- Use `jnoccio_instances` to check how many instances are currently running before spawning more.
- Treat Jnoccio output as advice unless the prompt requested deterministic data from an explicit resource or tool.

## Bootstrap

Build the local MCP launcher from this repo:

```bash
cd /Users/bentaylor/Code/opencode/jnoccio-fusion
rtk cargo build --bin jnoccio-mcp
```

Add this to `~/.codex/config.toml` or merge the snippet in `agents/codex/config.toml.snippet`:

```toml
[mcp_servers.jnoccio]
command = "/Users/bentaylor/Code/opencode/jnoccio-fusion/target/debug/jnoccio-mcp"
args = [
  "--config",
  "/Users/bentaylor/Code/opencode/jnoccio-fusion/config/server.json",
  "--ensure-server"
]
enabled = true
startup_timeout_sec = 30
tool_timeout_sec = 180
```

Restart Codex after changing MCP config and verify the `jnoccio` tools are listed.
