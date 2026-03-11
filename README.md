# kibana-mcp-server

[![CI](https://github.com/nizovtsevnv/kibana-mcp-server/actions/workflows/ci.yml/badge.svg)](https://github.com/nizovtsevnv/kibana-mcp-server/actions/workflows/ci.yml)
[![Release](https://github.com/nizovtsevnv/kibana-mcp-server/actions/workflows/release.yml/badge.svg)](https://github.com/nizovtsevnv/kibana-mcp-server/actions/workflows/release.yml)
[![crates.io](https://img.shields.io/crates/v/kibana-mcp-server)](https://crates.io/crates/kibana-mcp-server)

MCP server for accessing logs in Kibana/Elasticsearch.

Standalone binary that exposes log search tools over MCP (Model Context Protocol) via stdio or HTTP transport using JSON-RPC 2.0. Supports both Elasticsearch and Kibana backends with automatic detection.

## Features

- **Tool `search_logs`** — search logs using Elasticsearch query string syntax with time range filtering and pagination
- **Tool `get_indices`** — list available indices (ES) or index patterns (Kibana)
- **Tool `get_log_context`** — get surrounding log entries for a specific document
- **Auto-detection** — automatically detects Kibana vs Elasticsearch backend
- **Authentication** — supports Basic auth, API key, and no-auth modes; per-client credentials in HTTP mode
- **ECS-aware formatting** — formats log entries using Elastic Common Schema fields
- **HTTP transport** — MCP Streamable HTTP with Bearer token authentication and session management
- **Dual transport** — stdio (default) or HTTP mode via `--http` flag

## Architecture

Single crate, six source modules:

| Module | Responsibility |
|---|---|
| `src/main.rs` | Entry point, transport selection |
| `src/cli.rs` | CLI argument parsing |
| `src/config.rs` | Configuration from environment variables |
| `src/mcp.rs` | JSON-RPC 2.0 dispatch, MCP tool definitions, async stdio read/write loop |
| `src/http.rs` | HTTP transport: axum server, Bearer auth, session management |
| `src/kibana.rs` | HTTP client for Elasticsearch/Kibana REST API |
| `src/tools.rs` | MCP tool implementations, log entry formatting |

## CLI Commands

```
kibana-mcp-server [COMMAND]

Commands:
  --stdio      Run in stdio mode (default)
  --http       Run in HTTP mode
  --version    Print version and exit
  --help       Print this help and exit
```

## Environment Variables

| Variable | Description | Mode | Required |
|---|---|---|---|
| `KIBANA_URL` | Kibana or Elasticsearch base URL | Both | Yes |
| `KIBANA_INSECURE` | Skip TLS verification (`"true"` or `"1"`) | Both | No |
| `KIBANA_USERNAME` | Username for basic authentication | Stdio only | No |
| `KIBANA_PASSWORD` | Password for basic authentication | Stdio only | No |
| `KIBANA_API_KEY` | API key for Elasticsearch authentication | Stdio only | No |
| `MCP_HOST` | Host to bind HTTP server [default: 127.0.0.1] | HTTP only | No |
| `MCP_PORT` | Port for HTTP server [default: 8080] | HTTP only | No |
| `MCP_AUTH_TOKEN` | Bearer token for HTTP authentication | HTTP only | No |

### Per-client credentials (HTTP mode)

In HTTP mode, Kibana credentials are not configured via environment variables. Instead, each client provides its own credentials through HTTP headers on the `initialize` request:

| Header | Description |
|---|---|
| `X-Kibana-Username` + `X-Kibana-Password` | Basic authentication |
| `X-Kibana-API-Key` | API key authentication |

The two schemes are mutually exclusive. Credentials are stored per-session and used for all subsequent requests within that session.

## Build

### Prerequisites

- Rust toolchain (stable)

### Build

```bash
cargo build --release
```

## Rust Dependencies

| Crate | Purpose |
|---|---|
| `reqwest` | HTTP client for Elasticsearch/Kibana API |
| `serde`, `serde_json` | JSON serialization for MCP protocol and ES queries |
| `tracing`, `tracing-subscriber` | Structured logging to stderr |
| `axum` | HTTP server framework for MCP HTTP transport |
| `tokio` | Async runtime |
| `uuid` | Session ID generation (UUID v4) |

## MCP Protocol

The server supports two transport modes:

- **stdio** (default) — communicates over stdin/stdout, one JSON object per line
- **HTTP** — MCP Streamable HTTP on `POST /mcp` and `DELETE /mcp`

### HTTP Transport

Start the server in HTTP mode:

```bash
KIBANA_URL=http://localhost:9200 MCP_PORT=8080 MCP_AUTH_TOKEN=secret123 kibana-mcp-server --http
```

**Authentication**: when `MCP_AUTH_TOKEN` is set, all requests must include `Authorization: Bearer <token>`. Without `MCP_AUTH_TOKEN`, authentication is disabled.

**Per-client Kibana credentials**: the `initialize` request must include Kibana credentials via `X-Kibana-Username`/`X-Kibana-Password` or `X-Kibana-API-Key` headers. Each session gets its own Kibana client with these credentials.

**Sessions**: the `initialize` request returns an `Mcp-Session-Id` header. All subsequent requests must include this header. Sessions are terminated via `DELETE /mcp`.

### Stdio Transport

The server communicates over stdin/stdout using JSON-RPC 2.0, one JSON object per line.

### Initialize

Request:
```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
```

Response:
```json
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"kibana-mcp-server","version":"<version>"}}}
```

### List tools

Request:
```json
{"jsonrpc":"2.0","id":2,"method":"tools/list"}
```

### Search logs

Request:
```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_logs","arguments":{"query":"error","index":"app-logs-*","size":10}}}
```

### Get indices

Request:
```json
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_indices","arguments":{}}}
```

### Get log context

Request:
```json
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"get_log_context","arguments":{"index":"app-logs-2024.01.15","doc_id":"abc123"}}}
```

## CI/CD

GitHub Actions workflows:

- **CI** (`ci.yml`) — runs `cargo fmt`, `cargo clippy`, `cargo test` on every push/PR to `main`/`develop`
- **Release** (`release.yml`) — builds binaries for 5 targets on tag push (`v*`), uploads as release assets

Release targets:

| Artifact | Build method | Notes |
|---|---|---|
| `linux-x86_64` | nix (musl) | Static binary |
| `windows-x86_64` | cargo (native) | Windows runner |
| `macos-x86_64` | nix (default) | Intel Mac |
| `macos-arm64` | nix (default) | Apple Silicon |

Release process:
1. Create a git tag: `git tag vX.Y.Z && git push --tags`
2. CI builds binaries for all targets
3. Create a GitHub release from the tag — CI attaches build artifacts automatically

To update `cargoHash` in `flake.nix` after changing dependencies:
```bash
./scripts/update-cargo-hash.sh
```

## Usage

### Claude Desktop (stdio)

Add to `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "kibana": {
      "command": "/path/to/kibana-mcp-server",
      "env": {
        "KIBANA_URL": "http://localhost:9200"
      }
    }
  }
}
```

### With authentication

```json
{
  "mcpServers": {
    "kibana": {
      "command": "/path/to/kibana-mcp-server",
      "env": {
        "KIBANA_URL": "https://my-kibana.example.com",
        "KIBANA_USERNAME": "elastic",
        "KIBANA_PASSWORD": "changeme"
      }
    }
  }
}
```

### HTTP mode

```bash
KIBANA_URL=http://localhost:9200 MCP_AUTH_TOKEN=mytoken kibana-mcp-server --http
```

Connect any HTTP-capable MCP client to `http://127.0.0.1:8080/mcp`.

#### Claude Code (HTTP with per-client credentials)

```bash
claude mcp add --transport http kibana https://mcp.example.com/mcp \
  --header "Authorization: Bearer <mcp-token>" \
  --header "X-Kibana-Username: myuser" \
  --header "X-Kibana-Password: mypass"
```

Or with an API key:

```bash
claude mcp add --transport http kibana https://mcp.example.com/mcp \
  --header "Authorization: Bearer <mcp-token>" \
  --header "X-Kibana-API-Key: <base64-encoded-api-key>"
```
