# Claude Code Integration

Set up Codanna with Claude's official CLI.

## Configuration

Add this to your local `.mcp.json`:

```json
{
  "mcpServers": {
    "codanna": {
      "command": "codanna",
      "args": ["serve", "--watch"]
    }
  }
}
```

## Features

- File watching with `--watch` flag
- Auto-reload on index changes
- stdio transport (default)

## Verification

After configuration, verify the connection:

```bash
codanna mcp-test
```

This will confirm Claude can connect and list available tools.

## Agent Workflow

See [Agent Guidance](agent-guidance.md) for recommended tool usage patterns and workflows.

## Troubleshooting

- Ensure Codanna is in your PATH
- Check `.codanna/settings.toml` exists in your project
- Run `codanna index` before starting the server

## See Also

- [MCP Tools Reference](../user-guide/mcp-tools.md)
- [Agent Guidance](agent-guidance.md)
- [Configuration](../user-guide/configuration.md)