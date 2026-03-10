use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, warn};

use crate::kibana::KibanaClient;

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Deserialize)]
pub(crate) struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    pub(crate) id: Option<Value>,
    pub(crate) method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Serialize)]
pub(crate) struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

/// Process a JSON-RPC request and return a response.
/// Returns None for notifications that require no response.
pub async fn dispatch_request(request_json: &str, client: &Arc<KibanaClient>) -> Option<String> {
    let request: JsonRpcRequest = match serde_json::from_str(request_json) {
        Ok(r) => r,
        Err(e) => {
            warn!("Invalid JSON-RPC request: {e}");
            let resp = JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: None,
                result: None,
                error: Some(json!({"code": -32700, "message": "Parse error"})),
            };
            return serde_json::to_string(&resp).ok();
        }
    };

    debug!("Received method: {}", request.method);

    if request.method.starts_with("notifications/") {
        return None;
    }

    let response = match request.method.as_str() {
        "initialize" => handle_initialize(&request),
        "tools/list" => handle_tools_list(&request),
        "tools/call" => handle_tools_call(&request, client).await,
        _ => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(json!({"code": -32601, "message": "Method not found"})),
        },
    };

    match serde_json::to_string(&response) {
        Ok(json) => Some(json),
        Err(e) => {
            error!("Failed to serialize response: {e}");
            None
        }
    }
}

pub async fn run_stdio_loop(client: Arc<KibanaClient>) {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(e) => {
                error!("stdin read error: {e}");
                break;
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(response) = dispatch_request(trimmed, &client).await {
            let mut out = response.into_bytes();
            out.push(b'\n');
            let _ = stdout.write_all(&out).await;
            let _ = stdout.flush().await;
        }
    }
}

fn handle_initialize(request: &JsonRpcRequest) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone(),
        result: Some(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "kibana-mcp-server",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        error: None,
    }
}

fn handle_tools_list(request: &JsonRpcRequest) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone(),
        result: Some(json!({
            "tools": [
                {
                    "name": "search_logs",
                    "description": "Search logs in Elasticsearch/Kibana using query string syntax. Returns matching log entries with timestamps, levels, and messages.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["query"],
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Elasticsearch query string (e.g. 'error AND service:auth')"
                            },
                            "index": {
                                "type": "string",
                                "description": "Index pattern to search (default: '*')",
                                "default": "*"
                            },
                            "time_from": {
                                "type": "string",
                                "description": "Start of time range (default: 'now-1h'). Supports ES date math.",
                                "default": "now-1h"
                            },
                            "time_to": {
                                "type": "string",
                                "description": "End of time range (default: 'now'). Supports ES date math.",
                                "default": "now"
                            },
                            "size": {
                                "type": "integer",
                                "description": "Number of results to return (default: 50, max: 10000)",
                                "default": 50
                            },
                            "timestamp_field": {
                                "type": "string",
                                "description": "Name of the timestamp field (default: '@timestamp')",
                                "default": "@timestamp"
                            },
                            "cursor": {
                                "type": "array",
                                "description": "Pagination cursor from previous search response"
                            },
                            "raw": {
                                "type": "boolean",
                                "description": "Return raw JSON instead of formatted text (default: false)",
                                "default": false
                            }
                        }
                    }
                },
                {
                    "name": "get_indices",
                    "description": "List available indices (Elasticsearch) or index patterns (Kibana).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "get_log_context",
                    "description": "Get surrounding log entries for a specific document. Shows entries before and after the target document in chronological order.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["index", "doc_id"],
                        "properties": {
                            "index": {
                                "type": "string",
                                "description": "Index name containing the document"
                            },
                            "doc_id": {
                                "type": "string",
                                "description": "Document ID to get context for"
                            },
                            "size": {
                                "type": "integer",
                                "description": "Number of entries before and after (default: 5)",
                                "default": 5
                            },
                            "timestamp_field": {
                                "type": "string",
                                "description": "Name of the timestamp field (default: '@timestamp')",
                                "default": "@timestamp"
                            }
                        }
                    }
                }
            ]
        })),
        error: None,
    }
}

async fn handle_tools_call(
    request: &JsonRpcRequest,
    client: &Arc<KibanaClient>,
) -> JsonRpcResponse {
    let tool_name = request
        .params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let arguments = request
        .params
        .get("arguments")
        .cloned()
        .unwrap_or(json!({}));

    let result = match tool_name {
        "search_logs" => crate::tools::search_logs(client, &arguments).await,
        "get_indices" => crate::tools::get_indices(client).await,
        "get_log_context" => crate::tools::get_log_context(client, &arguments).await,
        _ => {
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result: Some(mcp_error(&format!("Unknown tool: {tool_name}"))),
                error: None,
            };
        }
    };

    match result {
        Ok(text) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(json!({
                "content": [{"type": "text", "text": text}]
            })),
            error: None,
        },
        Err(e) => {
            error!("Tool '{tool_name}' failed: {e}");
            JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id.clone(),
                result: Some(mcp_error(&e)),
                error: None,
            }
        }
    }
}

pub(crate) fn mcp_error(message: &str) -> Value {
    json!({
        "content": [{"type": "text", "text": message}],
        "isError": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_dispatch(request_json: &str) -> Option<String> {
        let request: JsonRpcRequest = match serde_json::from_str(request_json) {
            Ok(r) => r,
            Err(e) => {
                warn!("Invalid JSON-RPC request: {e}");
                let resp = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(json!({"code": -32700, "message": "Parse error"})),
                };
                return serde_json::to_string(&resp).ok();
            }
        };

        if request.method.starts_with("notifications/") {
            return None;
        }

        let response = match request.method.as_str() {
            "initialize" => handle_initialize(&request),
            "tools/list" => handle_tools_list(&request),
            _ => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(json!({"code": -32601, "message": "Method not found"})),
            },
        };

        serde_json::to_string(&response).ok()
    }

    #[test]
    fn test_dispatch_parse_error() {
        let result = parse_and_dispatch("not valid json {{{");
        assert!(result.is_some());
        let resp: Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(resp["error"]["code"], -32700);
    }

    #[test]
    fn test_dispatch_method_not_found() {
        let input = r#"{"jsonrpc":"2.0","id":1,"method":"unknown/method"}"#;
        let result = parse_and_dispatch(input);
        assert!(result.is_some());
        let resp: Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
        assert_eq!(resp["id"], 1);
    }

    #[test]
    fn test_dispatch_notification_returns_none() {
        let input = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let result = parse_and_dispatch(input);
        assert!(result.is_none());
    }

    #[test]
    fn test_dispatch_initialize() {
        let input = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let result = parse_and_dispatch(input);
        assert!(result.is_some());
        let resp: Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(resp["id"], 1);
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(resp["result"]["serverInfo"]["name"], "kibana-mcp-server");
    }

    #[test]
    fn test_dispatch_tools_list() {
        let input = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
        let result = parse_and_dispatch(input);
        assert!(result.is_some());
        let resp: Value = serde_json::from_str(&result.unwrap()).unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);

        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"search_logs"));
        assert!(names.contains(&"get_indices"));
        assert!(names.contains(&"get_log_context"));
    }
}
