use serde_json::Value;

use crate::kibana::{KibanaClient, SearchQuery};

pub async fn search_logs(client: &KibanaClient, args: &Value) -> Result<String, String> {
    let query_string = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: 'query'")?
        .to_string();

    let index = args.get("index").and_then(|v| v.as_str()).unwrap_or("*");

    let time_from = args
        .get("time_from")
        .and_then(|v| v.as_str())
        .map(String::from);
    let time_to = args
        .get("time_to")
        .and_then(|v| v.as_str())
        .map(String::from);

    let size = args
        .get("size")
        .and_then(|v| v.as_u64())
        .unwrap_or(50)
        .min(10_000) as u32;

    let timestamp_field = args
        .get("timestamp_field")
        .and_then(|v| v.as_str())
        .unwrap_or("@timestamp")
        .to_string();

    let search_after = args.get("cursor").and_then(|v| v.as_array()).cloned();

    let raw = args.get("raw").and_then(|v| v.as_bool()).unwrap_or(false);

    let query = SearchQuery {
        query_string,
        index: index.to_string(),
        time_from,
        time_to,
        size,
        search_after,
        timestamp_field,
    };

    let response = client.search(&query).await?;

    if raw {
        return serde_json::to_string_pretty(&serde_json::json!({
            "total": response.total,
            "took_ms": response.took_ms,
            "hits": response.hits,
            "next_cursor": response.next_cursor,
        }))
        .map_err(|e| format!("JSON serialization error: {e}"));
    }

    Ok(format_search_response(&response))
}

pub async fn get_indices(client: &KibanaClient) -> Result<String, String> {
    let indices = client.get_indices().await?;

    if indices.is_empty() {
        return Ok("No indices found.".to_string());
    }

    let mut output = format!("Found {} indices:\n\n", indices.len());
    for idx in &indices {
        output.push_str(&format!("- {}", idx.name));
        if let Some(ref count) = idx.docs_count {
            output.push_str(&format!(" ({count} docs"));
            if let Some(ref size) = idx.store_size {
                output.push_str(&format!(", {size}"));
            }
            output.push(')');
        }
        output.push('\n');
    }

    Ok(output)
}

pub async fn get_log_context(client: &KibanaClient, args: &Value) -> Result<String, String> {
    let index = args
        .get("index")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: 'index'")?;

    let doc_id = args
        .get("doc_id")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: 'doc_id'")?;

    let size = args.get("size").and_then(|v| v.as_u64()).unwrap_or(5) as u32;

    let timestamp_field = args
        .get("timestamp_field")
        .and_then(|v| v.as_str())
        .unwrap_or("@timestamp");

    let ctx = client
        .get_context(index, doc_id, size, timestamp_field)
        .await?;

    let mut output = String::new();

    if !ctx.before.is_empty() {
        output.push_str("--- Before ---\n");
        for hit in &ctx.before {
            output.push_str(&format_hit(hit));
            output.push('\n');
        }
        output.push('\n');
    }

    output.push_str(">>> Target document <<<\n");
    output.push_str(&format_hit(&ctx.target));
    output.push('\n');

    if !ctx.after.is_empty() {
        output.push_str("\n--- After ---\n");
        for hit in &ctx.after {
            output.push_str(&format_hit(hit));
            output.push('\n');
        }
    }

    Ok(output)
}

fn format_search_response(response: &crate::kibana::SearchResponse) -> String {
    let mut output = format!(
        "Found {} results ({}ms). Showing {}:\n\n",
        response.total,
        response.took_ms,
        response.hits.len()
    );

    for hit in &response.hits {
        output.push_str(&format_hit(hit));
        output.push('\n');
    }

    if let Some(ref cursor) = response.next_cursor {
        let cursor_json = serde_json::to_string(cursor).unwrap_or_default();
        output.push_str(&format!("\nUse cursor {cursor_json} for next page.\n"));
    }

    output
}

fn format_hit(hit: &Value) -> String {
    let source = hit.get("_source").unwrap_or(hit);
    let index = hit.get("_index").and_then(|v| v.as_str()).unwrap_or("");
    let doc_id = hit.get("_id").and_then(|v| v.as_str()).unwrap_or("");

    // Priority fields (ECS compatible)
    let timestamp = get_field(source, "@timestamp");
    let message = get_field(source, "message");
    let level = get_field(source, "level").or_else(|| get_nested_field(source, &["log", "level"]));
    let host =
        get_field(source, "host.name").or_else(|| get_nested_field(source, &["host", "name"]));
    let service = get_field(source, "service.name")
        .or_else(|| get_nested_field(source, &["service", "name"]));

    let mut output = String::new();

    // Main line: [timestamp] [level] message
    if let Some(ref ts) = timestamp {
        output.push_str(&format!("[{ts}]"));
    }
    if let Some(ref lvl) = level {
        output.push_str(&format!(" [{lvl}]"));
    }
    if let Some(ref msg) = message {
        output.push_str(&format!(" {msg}"));
    }
    output.push('\n');

    // Metadata
    if !index.is_empty() {
        output.push_str(&format!("  index: {index}\n"));
    }
    if !doc_id.is_empty() {
        output.push_str(&format!("  _id: {doc_id}\n"));
    }
    if let Some(ref h) = host {
        output.push_str(&format!("  host: {h}\n"));
    }
    if let Some(ref s) = service {
        output.push_str(&format!("  service: {s}\n"));
    }

    // Remaining fields (up to 5)
    let priority_keys: &[&str] = &["@timestamp", "message", "level", "log", "host", "service"];

    if let Some(obj) = source.as_object() {
        let remaining: Vec<(&String, &Value)> = obj
            .iter()
            .filter(|(k, _)| !priority_keys.contains(&k.as_str()))
            .collect();

        let show_count = remaining.len().min(5);
        for (k, v) in &remaining[..show_count] {
            let display = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            // Truncate long values
            let display = if display.len() > 200 {
                format!("{}...", &display[..200])
            } else {
                display
            };
            output.push_str(&format!("  {k}: {display}\n"));
        }
        if remaining.len() > 5 {
            output.push_str(&format!("  ... and {} more fields\n", remaining.len() - 5));
        }
    }

    output
}

fn get_field(source: &Value, field: &str) -> Option<String> {
    source.get(field).and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    })
}

fn get_nested_field(source: &Value, path: &[&str]) -> Option<String> {
    let mut current = source;
    for key in path {
        current = current.get(key)?;
    }
    match current {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_format_hit_full() {
        let hit = json!({
            "_index": "app-logs-2024.01.15",
            "_id": "doc123",
            "_source": {
                "@timestamp": "2024-01-15T10:30:00.000Z",
                "message": "Connection refused to database host",
                "level": "ERROR",
                "host": { "name": "web-server-01" },
                "service": { "name": "auth-service" },
                "trace_id": "abc123"
            }
        });

        let result = format_hit(&hit);
        assert!(result.contains("[2024-01-15T10:30:00.000Z]"));
        assert!(result.contains("[ERROR]"));
        assert!(result.contains("Connection refused"));
        assert!(result.contains("index: app-logs-2024.01.15"));
        assert!(result.contains("host: web-server-01"));
        assert!(result.contains("service: auth-service"));
        assert!(result.contains("trace_id: abc123"));
    }

    #[test]
    fn test_format_hit_minimal() {
        let hit = json!({
            "_index": "logs",
            "_id": "1",
            "_source": {
                "message": "simple log"
            }
        });

        let result = format_hit(&hit);
        assert!(result.contains("simple log"));
        assert!(result.contains("index: logs"));
    }

    #[test]
    fn test_format_hit_ecs_nested_level() {
        let hit = json!({
            "_index": "logs",
            "_id": "1",
            "_source": {
                "@timestamp": "2024-01-15T10:00:00Z",
                "message": "test",
                "log": { "level": "WARN" }
            }
        });

        let result = format_hit(&hit);
        assert!(result.contains("[WARN]"));
    }

    #[test]
    fn test_format_search_response() {
        let response = crate::kibana::SearchResponse {
            hits: vec![json!({
                "_index": "logs",
                "_id": "1",
                "_source": {
                    "@timestamp": "2024-01-15T10:30:00Z",
                    "message": "Error occurred"
                },
                "sort": [1705312200000u64, "1"]
            })],
            total: 100,
            took_ms: 5,
            next_cursor: Some(vec![json!(1705312200000u64), json!("1")]),
        };

        let result = format_search_response(&response);
        assert!(result.contains("Found 100 results (5ms). Showing 1:"));
        assert!(result.contains("Error occurred"));
        assert!(result.contains("Use cursor"));
    }

    #[test]
    fn test_format_search_response_empty() {
        let response = crate::kibana::SearchResponse {
            hits: vec![],
            total: 0,
            took_ms: 1,
            next_cursor: None,
        };

        let result = format_search_response(&response);
        assert!(result.contains("Found 0 results"));
        assert!(!result.contains("Use cursor"));
    }

    #[test]
    fn test_format_hit_many_fields() {
        let hit = json!({
            "_index": "logs",
            "_id": "1",
            "_source": {
                "@timestamp": "2024-01-15T10:00:00Z",
                "message": "test",
                "field1": "v1",
                "field2": "v2",
                "field3": "v3",
                "field4": "v4",
                "field5": "v5",
                "field6": "v6",
                "field7": "v7"
            }
        });

        let result = format_hit(&hit);
        assert!(result.contains("... and"));
        assert!(result.contains("more fields"));
    }
}
