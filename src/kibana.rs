use std::sync::OnceLock;
use std::time::Duration;

use axum::http::HeaderMap;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    Kibana,
    Elasticsearch,
}

pub enum AuthMethod {
    None,
    Basic { username: String, password: String },
    ApiKey(String),
}

pub struct KibanaClient {
    client: reqwest::Client,
    base_url: String,
    auth: AuthMethod,
    backend: OnceLock<BackendType>,
}

pub struct SearchQuery {
    pub query_string: String,
    pub index: String,
    pub time_from: Option<String>,
    pub time_to: Option<String>,
    pub size: u32,
    pub search_after: Option<Vec<Value>>,
    pub timestamp_field: String,
}

pub struct SearchResponse {
    pub hits: Vec<Value>,
    pub total: u64,
    pub took_ms: u64,
    pub next_cursor: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
pub struct IndexInfo {
    pub name: String,
    pub docs_count: Option<String>,
    pub store_size: Option<String>,
}

pub struct ContextResponse {
    pub before: Vec<Value>,
    pub target: Value,
    pub after: Vec<Value>,
}

impl AuthMethod {
    /// Extract Kibana credentials from HTTP headers.
    ///
    /// Supported schemes (mutually exclusive):
    /// - `X-Kibana-API-Key` header → `AuthMethod::ApiKey`
    /// - `X-Kibana-Username` + `X-Kibana-Password` headers → `AuthMethod::Basic`
    pub fn from_headers(headers: &HeaderMap) -> Result<Self, String> {
        let api_key = headers
            .get("x-kibana-api-key")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty());
        let username = headers
            .get("x-kibana-username")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty());
        let password = headers
            .get("x-kibana-password")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty());

        match (api_key, username, password) {
            (Some(_), Some(_), _) | (Some(_), _, Some(_)) => Err(
                "Cannot use both X-Kibana-API-Key and X-Kibana-Username/X-Kibana-Password"
                    .to_string(),
            ),
            (Some(key), None, None) => Ok(AuthMethod::ApiKey(key.to_string())),
            (None, Some(u), Some(p)) => Ok(AuthMethod::Basic {
                username: u.to_string(),
                password: p.to_string(),
            }),
            (None, Some(_), None) | (None, None, Some(_)) => Err(
                "Both X-Kibana-Username and X-Kibana-Password must be provided together"
                    .to_string(),
            ),
            (None, None, None) => {
                Err("Missing Kibana credentials: provide X-Kibana-API-Key or X-Kibana-Username + X-Kibana-Password headers".to_string())
            }
        }
    }
}

impl KibanaClient {
    pub fn new(
        base_url: &str,
        username: Option<&str>,
        password: Option<&str>,
        api_key: Option<&str>,
        insecure: bool,
    ) -> Self {
        let auth = if let Some(key) = api_key {
            AuthMethod::ApiKey(key.to_string())
        } else if let (Some(u), Some(p)) = (username, password) {
            AuthMethod::Basic {
                username: u.to_string(),
                password: p.to_string(),
            }
        } else {
            AuthMethod::None
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .danger_accept_invalid_certs(insecure)
            .build()
            .expect("failed to create HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth,
            backend: OnceLock::new(),
        }
    }

    /// Create a client that reuses an existing connection pool.
    pub fn with_shared_client(client: reqwest::Client, base_url: &str, auth: AuthMethod) -> Self {
        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth,
            backend: OnceLock::new(),
        }
    }

    fn apply_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.auth {
            AuthMethod::None => req,
            AuthMethod::Basic { username, password } => req.basic_auth(username, Some(password)),
            AuthMethod::ApiKey(key) => req.header("Authorization", format!("ApiKey {key}")),
        }
    }

    /// Send a request to Elasticsearch, routing through Kibana proxy if needed.
    async fn es_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, String> {
        let backend = self.detect_backend().await;

        let req = match backend {
            BackendType::Elasticsearch => {
                let url = format!("{}{}", self.base_url, path);
                let mut r = match method {
                    "POST" => self.client.post(&url),
                    "GET" => self.client.get(&url),
                    _ => {
                        let m = method
                            .parse()
                            .map_err(|e| format!("Invalid HTTP method '{method}': {e}"))?;
                        self.client.request(m, &url)
                    }
                };
                if let Some(b) = body {
                    r = r.json(b);
                }
                r
            }
            BackendType::Kibana => {
                let url = format!("{}/api/console/proxy", self.base_url);
                let mut r = self
                    .client
                    .post(&url)
                    .header("kbn-xsrf", "true")
                    .query(&[("path", path), ("method", method)]);
                if let Some(b) = body {
                    r = r.json(b);
                }
                r
            }
        };

        let req = self.apply_auth(req);
        let resp = req.send().await.map_err(|e| format!("HTTP error: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Elasticsearch returned {status}: {text}"));
        }

        resp.json()
            .await
            .map_err(|e| format!("JSON parse error: {e}"))
    }

    pub async fn detect_backend(&self) -> BackendType {
        if let Some(b) = self.backend.get() {
            return *b;
        }

        let url = format!("{}/api/status", self.base_url);
        let req = self.client.get(&url);
        let req = self.apply_auth(req);

        let backend = match req.send().await {
            Ok(resp) if resp.status().is_success() => BackendType::Kibana,
            _ => BackendType::Elasticsearch,
        };

        info!("Detected backend: {:?}", backend);
        let _ = self.backend.set(backend);
        backend
    }

    pub async fn search(&self, query: &SearchQuery) -> Result<SearchResponse, String> {
        let mut must = vec![json!({
            "query_string": { "query": query.query_string }
        })];

        let time_from = query.time_from.as_deref().unwrap_or("now-1h");
        let time_to = query.time_to.as_deref().unwrap_or("now");
        must.push(json!({
            "range": {
                query.timestamp_field.clone(): {
                    "gte": time_from,
                    "lte": time_to
                }
            }
        }));

        let mut body = json!({
            "query": { "bool": { "must": must } },
            "size": query.size,
            "sort": [
                { query.timestamp_field.clone(): { "order": "desc" } },
                { "_doc": { "order": "asc" } }
            ]
        });

        if let Some(ref cursor) = query.search_after {
            body["search_after"] = json!(cursor);
        }

        let data = self
            .es_request("POST", &format!("/{}/_search", query.index), Some(&body))
            .await?;

        parse_search_response(&data)
    }

    pub async fn get_indices(&self) -> Result<Vec<IndexInfo>, String> {
        let backend = self.detect_backend().await;

        match backend {
            BackendType::Kibana => {
                let url = format!(
                    "{}/api/saved_objects/_find?type=index-pattern&per_page=1000",
                    self.base_url
                );
                let req = self.client.get(&url).header("kbn-xsrf", "true");
                let req = self.apply_auth(req);

                let resp = req.send().await.map_err(|e| format!("HTTP error: {e}"))?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(format!("Kibana returned {status}: {text}"));
                }

                let data: Value = resp
                    .json()
                    .await
                    .map_err(|e| format!("JSON parse error: {e}"))?;

                let saved_objects = data["saved_objects"]
                    .as_array()
                    .ok_or("Invalid Kibana response: missing saved_objects")?;

                let indices = saved_objects
                    .iter()
                    .filter_map(|obj| {
                        let name = obj["attributes"]["title"].as_str()?.to_string();
                        Some(IndexInfo {
                            name,
                            docs_count: None,
                            store_size: None,
                        })
                    })
                    .collect();

                Ok(indices)
            }
            BackendType::Elasticsearch => {
                let url = format!(
                    "{}/_cat/indices?format=json&h=index,docs.count,store.size",
                    self.base_url
                );
                let req = self.client.get(&url);
                let req = self.apply_auth(req);

                let resp = req.send().await.map_err(|e| format!("HTTP error: {e}"))?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    return Err(format!("Elasticsearch returned {status}: {text}"));
                }

                let data: Value = resp
                    .json()
                    .await
                    .map_err(|e| format!("JSON parse error: {e}"))?;

                let arr = data
                    .as_array()
                    .ok_or("Invalid ES response: expected array")?;

                let indices = arr
                    .iter()
                    .filter_map(|item| {
                        let name = item["index"].as_str()?.to_string();
                        // Skip system indices
                        if name.starts_with('.') {
                            return None;
                        }
                        Some(IndexInfo {
                            name,
                            docs_count: item["docs.count"].as_str().map(|s| s.to_string()),
                            store_size: item["store.size"].as_str().map(|s| s.to_string()),
                        })
                    })
                    .collect();

                Ok(indices)
            }
        }
    }

    pub async fn get_document(&self, index: &str, doc_id: &str) -> Result<Value, String> {
        self.es_request("GET", &format!("/{}/_doc/{}", index, doc_id), None)
            .await
    }

    pub async fn get_context(
        &self,
        index: &str,
        doc_id: &str,
        size: u32,
        timestamp_field: &str,
    ) -> Result<ContextResponse, String> {
        // Get the target document
        let doc = self.get_document(index, doc_id).await?;
        let source = doc
            .get("_source")
            .ok_or("Document has no _source field")?
            .clone();
        let timestamp = source
            .get(timestamp_field)
            .ok_or(format!("Document has no '{timestamp_field}' field"))?;

        // Get documents before (older)
        let before_body = json!({
            "query": {
                "range": {
                    timestamp_field: { "lt": timestamp }
                }
            },
            "size": size,
            "sort": [{ timestamp_field: { "order": "desc" } }]
        });

        let search_path = format!("/{}/_search", index);
        let before_data = self
            .es_request("POST", &search_path, Some(&before_body))
            .await?;
        let mut before_hits = extract_hits(&before_data);
        before_hits.reverse(); // Chronological order

        // Get documents after (newer)
        let after_body = json!({
            "query": {
                "range": {
                    timestamp_field: { "gt": timestamp }
                }
            },
            "size": size,
            "sort": [{ timestamp_field: { "order": "asc" } }]
        });

        let after_data = self
            .es_request("POST", &search_path, Some(&after_body))
            .await?;
        let after_hits = extract_hits(&after_data);

        Ok(ContextResponse {
            before: before_hits,
            target: doc,
            after: after_hits,
        })
    }
}

fn extract_hits(data: &Value) -> Vec<Value> {
    data["hits"]["hits"].as_array().cloned().unwrap_or_default()
}

fn parse_search_response(data: &Value) -> Result<SearchResponse, String> {
    let hits_array = data["hits"]["hits"]
        .as_array()
        .ok_or("Invalid response: missing hits.hits")?;

    let total = data["hits"]["total"]["value"]
        .as_u64()
        .or_else(|| data["hits"]["total"].as_u64())
        .unwrap_or(0);

    let took_ms = data["took"].as_u64().unwrap_or(0);

    // Extract cursor from the last hit's sort values
    let next_cursor = hits_array
        .last()
        .and_then(|hit| hit.get("sort").and_then(|s| s.as_array()).cloned());

    Ok(SearchResponse {
        hits: hits_array.clone(),
        total,
        took_ms,
        next_cursor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_search_response_valid() {
        let data = json!({
            "took": 23,
            "hits": {
                "total": { "value": 1234, "relation": "eq" },
                "hits": [
                    {
                        "_index": "app-logs-2024.01.15",
                        "_id": "doc1",
                        "_source": {
                            "@timestamp": "2024-01-15T10:30:00.000Z",
                            "message": "Connection refused",
                            "level": "ERROR"
                        },
                        "sort": [1705312200000u64, "doc1"]
                    }
                ]
            }
        });

        let result = parse_search_response(&data).unwrap();
        assert_eq!(result.total, 1234);
        assert_eq!(result.took_ms, 23);
        assert_eq!(result.hits.len(), 1);
        assert!(result.next_cursor.is_some());
        let cursor = result.next_cursor.unwrap();
        assert_eq!(cursor.len(), 2);
    }

    #[test]
    fn test_parse_search_response_empty() {
        let data = json!({
            "took": 1,
            "hits": {
                "total": { "value": 0 },
                "hits": []
            }
        });

        let result = parse_search_response(&data).unwrap();
        assert_eq!(result.total, 0);
        assert_eq!(result.hits.len(), 0);
        assert!(result.next_cursor.is_none());
    }

    #[test]
    fn test_extract_hits() {
        let data = json!({
            "hits": {
                "hits": [
                    { "_id": "1", "_source": { "message": "test" } },
                    { "_id": "2", "_source": { "message": "test2" } }
                ]
            }
        });
        let hits = extract_hits(&data);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn test_extract_hits_missing() {
        let data = json!({});
        let hits = extract_hits(&data);
        assert!(hits.is_empty());
    }

    #[test]
    fn test_auth_method_from_headers_api_key() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-kibana-api-key", "my-api-key".parse().unwrap());
        let auth = AuthMethod::from_headers(&headers).unwrap();
        assert!(matches!(auth, AuthMethod::ApiKey(k) if k == "my-api-key"));
    }

    #[test]
    fn test_auth_method_from_headers_basic() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-kibana-username", "user".parse().unwrap());
        headers.insert("x-kibana-password", "pass".parse().unwrap());
        let auth = AuthMethod::from_headers(&headers).unwrap();
        assert!(
            matches!(auth, AuthMethod::Basic { username, password } if username == "user" && password == "pass")
        );
    }

    #[test]
    fn test_auth_method_from_headers_conflict() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-kibana-api-key", "key".parse().unwrap());
        headers.insert("x-kibana-username", "user".parse().unwrap());
        headers.insert("x-kibana-password", "pass".parse().unwrap());
        assert!(AuthMethod::from_headers(&headers).is_err());
    }

    #[test]
    fn test_auth_method_from_headers_missing() {
        let headers = axum::http::HeaderMap::new();
        assert!(AuthMethod::from_headers(&headers).is_err());
    }

    #[test]
    fn test_auth_method_from_headers_partial_basic() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-kibana-username", "user".parse().unwrap());
        assert!(AuthMethod::from_headers(&headers).is_err());
    }

    #[test]
    fn test_build_search_query_body() {
        let query = SearchQuery {
            query_string: "error".to_string(),
            index: "*".to_string(),
            time_from: Some("now-1h".to_string()),
            time_to: Some("now".to_string()),
            size: 50,
            search_after: None,
            timestamp_field: "@timestamp".to_string(),
        };

        let mut must = vec![json!({
            "query_string": { "query": query.query_string }
        })];
        must.push(json!({
            "range": {
                query.timestamp_field.clone(): {
                    "gte": query.time_from.as_deref().unwrap_or("now-1h"),
                    "lte": query.time_to.as_deref().unwrap_or("now")
                }
            }
        }));

        let body = json!({
            "query": { "bool": { "must": must } },
            "size": query.size,
            "sort": [
                { query.timestamp_field.clone(): { "order": "desc" } },
                { "_doc": { "order": "asc" } }
            ]
        });

        assert_eq!(body["size"], 50);
        assert!(body["query"]["bool"]["must"].is_array());
        assert_eq!(body["query"]["bool"]["must"].as_array().unwrap().len(), 2);
    }
}
