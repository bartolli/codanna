# MCP HTTPS Server with Self-Signed Certificates

This guide explains how to use Codanna's HTTPS MCP server with self-signed certificates, particularly for secure local development and production deployments.

## Overview

The HTTPS MCP server provides:
- **TLS/SSL encryption** for secure communication
- **Streamable HTTP** transport compatible with Claude Code
- **OAuth2 authentication flow** for secure access control
- **Self-signed certificate generation** with proper X.509 attributes
- **Bearer token validation** for API security

## The Certificate Trust Challenge

Claude Code uses Node.js internally, which maintains its own certificate store separate from your system's certificate store. This means that even if you trust a certificate in your operating system (macOS Keychain, Windows Certificate Store, etc.), Node.js won't recognize it.

When connecting to an HTTPS server with a self-signed certificate, you'll encounter:
- `fetch failed` errors in Claude Code
- `unable to verify the first certificate` errors
- Connection failures despite the certificate being trusted in your browser

## Solution: NODE_EXTRA_CA_CERTS

The solution is to explicitly tell Node.js about your certificate using the `NODE_EXTRA_CA_CERTS` environment variable.

## Step-by-Step Setup

### 1. Start the HTTPS Server

```bash
cargo run --all-features -- serve --https --watch
```

Or if installed:
```bash
codanna serve --https --watch
```

On first run, this will:
- Generate a self-signed certificate
- Save it to `~/Library/Application Support/codanna/certs/server.pem` (macOS)
- Display certificate details and fingerprint

### 2. Copy Certificate to a Standard Location

Create a directory for your certificates and copy the generated certificate:

```bash
# Create SSL directory if it doesn't exist
mkdir -p ~/.ssl

# Copy the certificate
cp ~/Library/Application\ Support/codanna/certs/server.pem ~/.ssl/codanna-ca.pem
```

### 3. Configure MCP in Your Project

Add to `.mcp.json` in your project root:

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

### 4. Launch Claude Code with Certificate Trust

```bash
NODE_EXTRA_CA_CERTS=~/.ssl/codanna-ca.pem claude
```

### 5. Verify Connection

In Claude Code, use the `/mcp` command to check the connection status. You should see:

```
codanna-https  ✔ connected
```

## Alternative Setup Methods

### Method 1: Shell Alias

Add to your `~/.bashrc` or `~/.zshrc`:

```bash
alias claude-secure='NODE_EXTRA_CA_CERTS=~/.ssl/codanna-ca.pem claude'
```

Then use:
```bash
claude-secure
```

### Method 2: System-wide Trust (macOS)

For system-wide trust (though Node.js still requires NODE_EXTRA_CA_CERTS):

```bash
sudo security add-trusted-cert -d -r trustRoot \
  -k /Library/Keychains/System.keychain \
  ~/Library/Application\ Support/codanna/certs/server.pem
```

## OAuth Authentication Flow

The HTTPS server includes a complete OAuth2 implementation:

1. **Discovery**: `/.well-known/oauth-authorization-server`
2. **Registration**: `/oauth/register` 
3. **Authorization**: `/oauth/authorize`
4. **Token Exchange**: `/oauth/token`

This flow is handled automatically by Claude Code when connecting to the server.

## Troubleshooting

### "fetch failed" Error

**Problem**: Claude Code shows "fetch failed" when trying to connect.

**Solution**: Ensure you're running Claude Code with `NODE_EXTRA_CA_CERTS`:
```bash
NODE_EXTRA_CA_CERTS=~/.ssl/codanna-ca.pem claude
```

### Certificate Already Exists

**Problem**: Server says certificate already exists but you want to regenerate.

**Solution**: Delete the existing certificates:
```bash
rm -rf ~/Library/Application\ Support/codanna/certs/
```

Then restart the server to generate new ones.

### 401 Unauthorized

**Problem**: Server returns 401 errors.

**Solution**: The OAuth flow should handle authentication automatically. If you see 401 errors:
1. Check server logs for Bearer token validation messages
2. Ensure you're using `"type": "http"` in `.mcp.json`
3. Try reconnecting with `/mcp` command in Claude Code

### Browser Works But Claude Code Doesn't

**Problem**: You can access `https://127.0.0.1:8443/health` in browser but Claude Code fails.

**Solution**: Browsers use the system certificate store, but Node.js doesn't. You must use `NODE_EXTRA_CA_CERTS`.

## Security Considerations

### For Development

Self-signed certificates are acceptable for local development. The `NODE_EXTRA_CA_CERTS` approach is secure as it only trusts your specific certificate.

### For Production

Consider these alternatives for production:

1. **Let's Encrypt**: Use certbot to get free, valid certificates
2. **Reverse Proxy**: Place nginx/caddy with valid certs in front of your server
3. **Cloud Provider**: Use managed certificates from AWS, GCP, Azure
4. **Corporate CA**: Use your organization's internal certificate authority

### Never Do This

**DO NOT** use `NODE_TLS_REJECT_UNAUTHORIZED=0` in production. This disables ALL certificate validation and is a serious security risk.

## Implementation Details

The HTTPS server (`src/mcp/https_server.rs`) provides:

- **Certificate Generation**: Using `rcgen` crate with proper X.509 attributes
- **TLS Configuration**: Via `rustls` and `axum-server`
- **Local IP Detection**: Automatically includes local network IP in certificate SANs
- **Certificate Persistence**: Reuses certificates across server restarts
- **Bearer Token Validation**: Middleware for secure API access
- **OAuth2 Endpoints**: Complete authorization code flow implementation

## Platform-Specific Notes

### macOS
Certificates stored in: `~/Library/Application Support/codanna/certs/`

### Linux
Certificates stored in: `~/.config/codanna/certs/`

### Windows
Certificates stored in: `%APPDATA%\codanna\certs\`

Note: On Windows, use forward slashes in paths for NODE_EXTRA_CA_CERTS:
```cmd
set NODE_EXTRA_CA_CERTS=C:/Users/username/.ssl/codanna-ca.pem
claude
```

## Future Improvements

We're investigating ways to make certificate trust easier:

1. **Automatic Trust Setup**: A `codanna trust-cert` command that handles all setup
2. **Certificate Bundle**: Including the CA cert in a format Claude Code can auto-detect
3. **Platform Integration**: Better integration with system certificate stores
4. **Documentation**: In-app guidance when certificate issues are detected

## References

- [Node.js TLS Documentation](https://nodejs.org/api/tls.html#tlscreatesecurecontextoptions)
- [Claude Code MCP Documentation](https://docs.anthropic.com/en/docs/claude-code/mcp)
- [Model Context Protocol Specification](https://modelcontextprotocol.io)
- [GitHub Issue #2899](https://github.com/anthropics/claude-code/issues/2899) - Self-signed certificate support

## Summary

While self-signed certificates require an extra setup step with `NODE_EXTRA_CA_CERTS`, they provide a secure way to run HTTPS MCP servers locally or in controlled environments. The key is understanding that Node.js needs explicit trust configuration separate from your operating system's certificate store.

For the best developer experience, we recommend creating a shell alias or wrapper script that automatically sets the `NODE_EXTRA_CA_CERTS` environment variable when launching Claude Code.