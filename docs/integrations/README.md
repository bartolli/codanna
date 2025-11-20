[Documentation](../README.md) / **Integrations**

---

# Integrations

Drop codanna in as an MCP server, point the agent at it, and watch it stop hand-waving and start answering with receipts.

## Available Integrations

- **[Claude Code](claude-code.md)** - Claude's official CLI
- **[Claude Desktop](claude-desktop.md)** - Desktop app configuration
- **[Codex CLI](codex-cli.md)** - Alternative CLI client
- **[HTTP/HTTPS Server](http-server.md)** - Persistent server with real-time file watching
- **[Agent Guidance](agent-guidance.md)** - System messages and steering

## Extending with Plugins

- **[Plugin System](../plugins/)** - Add custom commands, agents, and MCP servers to Claude Code

## Quick Setup

### Claude Code
```json
# Add this to your local .mcp.json:
{
  "mcpServers": {
    "codanna": {
      "command": "codanna",
      "args": ["serve", "--watch"]
    }
  }
}
```

### Claude Desktop
For Claude Desktop, you need the `--config` flag since it runs from a different location.

Configure in `~/Library/Application Support/Claude/claude_desktop_config.json` (Mac):
```json
{
  "mcpServers": {
    "codanna": {
      "command": "codanna",
      "args": ["--config", "/absolute/path/to/your/project/.codanna/settings.toml", "serve", "--watch"]
    }
  }
}
```

Replace `/absolute/path/to/your/project/` with your actual project path.

## Agent Workflow

See [Agent Guidance](agent-guidance.md) for recommended tool usage patterns and workflows.

## Next Steps

- Configure [Agent Guidance](agent-guidance.md) for optimal steering
- Learn about [MCP Tools](../user-guide/mcp-tools.md) in detail
- Explore [Advanced](../advanced/) features

[Back to Documentation](../README.md)