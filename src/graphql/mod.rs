//! GraphQL introspection → CLI commands → execute.

mod execute;
mod extract;

pub use execute::{
    build_graphql_document, build_selection_set, execute_graphql, validate_required_graphql_args,
};
pub use extract::{
    extract_graphql_commands, graphql_type_string, graphql_type_to_python, unwrap_type,
};

use serde_json::{json, Value};

use crate::cache::{cache_key_for, load_cached, save_cache};
use crate::error::{Error, Result};
use crate::paths::DEFAULT_CACHE_TTL;

/// Fixed introspection query (parity with Python mcp2cli).
pub const GRAPHQL_INTROSPECTION_QUERY: &str = r#"
query IntrospectionQuery {
  __schema {
    queryType { name }
    mutationType { name }
    types {
      kind
      name
      fields(includeDeprecated: false) {
        name
        description
        args {
          name
          description
          type {
            ...TypeRef
          }
          defaultValue
        }
        type {
          ...TypeRef
        }
      }
      inputFields {
        name
        description
        type {
          ...TypeRef
        }
        defaultValue
      }
      enumValues(includeDeprecated: false) {
        name
        description
      }
    }
  }
}

fragment TypeRef on __Type {
  kind
  name
  ofType {
    kind
    name
    ofType {
      kind
      name
      ofType {
        kind
        name
        ofType {
          kind
          name
        }
      }
    }
  }
}
"#;

/// POST introspection query to a GraphQL endpoint, with caching.
pub fn load_graphql_schema(
    url: &str,
    auth_headers: &[(String, String)],
    cache_key: Option<&str>,
    ttl: Option<u64>,
    refresh: bool,
) -> Result<Value> {
    let ttl = ttl.unwrap_or(DEFAULT_CACHE_TTL);
    let key = cache_key.map(str::to_string).unwrap_or_else(|| {
        cache_key_for(&json!({
            "source": format!("graphql:{url}"),
            "auth_headers": auth_headers,
        }))
    });
    if !refresh {
        if let Some(cached) = load_cached(&key, ttl)? {
            return Ok(cached);
        }
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Error::runtime(e.to_string()))?;
    let mut req = client.post(url).header("Content-Type", "application/json");
    for (k, v) in auth_headers {
        req = req.header(k, v);
    }
    let resp = req
        .json(&json!({ "query": GRAPHQL_INTROSPECTION_QUERY }))
        .send()
        .map_err(|e| Error::runtime(format!("GraphQL introspection request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(Error::runtime(format!(
            "Error {}: {}",
            resp.status().as_u16(),
            resp.text().unwrap_or_default()
        )));
    }
    let result: Value = resp
        .json()
        .map_err(|e| Error::runtime(format!("invalid JSON from GraphQL introspection: {e}")))?;

    if result.get("errors").is_some() && result.get("data").is_none() {
        let msgs = result["errors"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default();
        return Err(Error::runtime(format!(
            "GraphQL introspection failed: {msgs}"
        )));
    }

    let schema = result
        .pointer("/data/__schema")
        .cloned()
        .ok_or_else(|| Error::runtime("introspection returned no schema"))?;
    if !schema.is_object() {
        return Err(Error::runtime("introspection returned no schema"));
    }

    save_cache(&key, &schema)?;
    Ok(schema)
}
