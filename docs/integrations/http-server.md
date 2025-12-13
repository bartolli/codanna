# HTTP/HTTPS Server

For persistent server with real-time file watching.

## HTTP Server

Run with OAuth authentication:

```bash
# HTTP server with OAuth authentication (requires http-server feature)
codanna serve --http --watch
```

## HTTPS Server

Run with TLS encryption:

```bash
# HTTPS server with TLS encryption (requires https-server feature)
codanna serve --https --watch
```

## Configuration

### MCP Client Configuration

Configure in `.mcp.json`:
```json
{
  "mcpServers": {
    "codanna-http": {
      "type": "http",
      "url": "http://127.0.0.1:8080/mcp"
    }
  }
}
```

For HTTPS, use:
```json
{
  "mcpServers": {
    "codanna-https": {
      "type": "http",
      "url": "https://127.0.0.1:8443/mcp"
    }
  }
}
```

### Custom Bind Address

```bash
# Bind to custom address and port
codanna serve --http --bind 0.0.0.0:3000

# Bind to all interfaces on port 8080
codanna serve --http --bind 0.0.0.0:8080
```

## Features

- Persistent server process
- Multiple client support
- Real-time file watching with `--watch`
- OAuth authentication (HTTP)
- TLS encryption (HTTPS)

## Advanced Setup

For detailed HTTPS setup with self-signed certificates, see the [HTTPS Setup Guide](https-setup.md).

## Advantages

- Multiple clients can connect to the same server
- Server persists between client sessions
- Centralized index management
- Network-accessible (with proper configuration)

## Security Considerations

- HTTP mode includes OAuth for authentication
- HTTPS mode provides TLS encryption
- Default bind is localhost only (127.0.0.1)
- Use caution when binding to 0.0.0.0 (all interfaces)

## See Also

- [Serve Command](../user-guide/cli-reference.md#codanna-serve)
- [MCP Tools](../user-guide/mcp-tools.md)
- [Agent Guidance](agent-guidance.md)