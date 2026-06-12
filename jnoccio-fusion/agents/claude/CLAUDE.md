# Jnoccio MCP Usage For Claude Agents

Jnoccio exposes a local MCP server that Claude agents can use to offload focused work, inspect routing/capacity, and request compact second-pass analysis. These notes are for agents using this repository; they are not Jekko setup instructions.

## Required Behavior

- Use `jnoccio_delegate` for isolated implementation planning, review, summarization, alternate-model reasoning, or compact research within provided context.
- Keep MCP inputs concise. Provide the goal, relevant local paths or snippets, constraints, and expected output.
- Do not send secrets, API keys, private headers, full transcript logs, complete tool arguments, or unnecessary file contents.
- Use `jnoccio_status` before expensive delegation or when routing health, context limits, or capacity matter.
- Use `jnoccio_spawn_parallel` to spin up multiple gateway instances for concurrent workloads (e.g. parallel research, multi-file edits, batch delegation). It respects a 20-total-instance hard cap including the main gateway, and all instances share the same database and model pool.
- Use `jnoccio_spawn_instance` to add a single extra gateway instance when incremental scaling is needed.
- Use `jnoccio_instances` to check how many instances are currently running before spawning more.
- Prefer direct Jnoccio MCP calls over routing through another agent or application.

## Bootstrap

Build the local MCP launcher from this repo:

```bash
cd /Users/bentaylor/Code/opencode/jnoccio-fusion
rtk cargo build --bin jnoccio-mcp
```

Use `agents/claude/.mcp.json` as the project MCP config:

```json
{
  "mcpServers": {
    "jnoccio": {
      "command": "/Users/bentaylor/Code/opencode/jnoccio-fusion/target/debug/jnoccio-mcp",
      "args": [
        "--config",
        "/Users/bentaylor/Code/opencode/jnoccio-fusion/config/server.json",
        "--ensure-server"
      ],
      "env": {
        "MCP_TIMEOUT": "30000",
        "MAX_MCP_OUTPUT_TOKENS": "50000"
      }
    }
  }
}
```

Restart Claude Code after changing MCP config and verify the `jnoccio` tools are listed.
