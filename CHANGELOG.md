# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[v0.1.0]: https://github.com/nizovtsevnv/kibana-mcp-server/releases/tag/v0.1.0
