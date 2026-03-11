# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [v0.1.2] - 2026-03-11

### Added
- GET /health endpoint returning {"status": "ok"} for load balancer and monitoring health checks

## [v0.1.1] - 2026-03-10

### Changed
- Replace clap CLI arguments with environment variable configuration
- Configuration via KIBANA_URL, KIBANA_USERNAME, KIBANA_PASSWORD, KIBANA_API_KEY, KIBANA_INSECURE
- HTTP mode configured via MCP_HOST, MCP_PORT, MCP_AUTH_TOKEN
- Simple CLI commands: --stdio (default), --http, --version, --help
- Remove clap dependency

### Fixed
- Build fully static Linux binary (musl) instead of dynamically linked

## [v0.1.0] - 2026-03-10

### Added
- MCP server for Kibana/Elasticsearch log access
- Tools: search_logs, get_document, get_context
- Streamable HTTP transport with SSE
- Auto-detection of backend type (Elasticsearch / Kibana)
- ES request routing through Kibana /api/console/proxy
- Bearer token authentication
- Session management
- Cursor-based pagination with search_after
- KQL query support

### Fixed
- Route ES requests through Kibana proxy for search/get_document operations
- Use _doc sort instead of _id to avoid fielddata errors
- Fallback from log to message field in hit formatting

[v0.1.2]: https://github.com/nizovtsevnv/kibana-mcp-server/releases/tag/v0.1.2
[v0.1.1]: https://github.com/nizovtsevnv/kibana-mcp-server/releases/tag/v0.1.1
[v0.1.0]: https://github.com/nizovtsevnv/kibana-mcp-server/releases/tag/v0.1.0
