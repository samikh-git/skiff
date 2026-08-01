//! Value coercion and naming (`to_kebab`, JSON Schema → CLI values, `env:`/`file:` secrets).

use regex::Regex;
use serde_json::{json, Value};
use std::sync::LazyLock;

use crate::error::{Error, Result};

static KEBAB_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([a-z0-9])([A-Z])").expect("kebab regex"));

/// Convert camelCase / snake_case names to kebab-case (Python `to_kebab`).
pub fn to_kebab(name: &str) -> String {
    KEBAB_RE
        .replace_all(name, "$1-$2")
        .replace('_', "-")
        .to_lowercase()
}

fn coerce_item(value: &str, item_type: Option<&str>) -> Value {
    match item_type {
        Some("integer") => value
            .parse::<i64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(value.to_string())),
        Some("number") => value
            .parse::<f64>()
            .map(Value::from)
            .unwrap_or_else(|_| Value::String(value.to_string())),
        Some("boolean") => {
            let v = value.to_lowercase();
            Value::Bool(matches!(v.as_str(), "true" | "1" | "yes"))
        }
        _ => Value::String(value.to_string()),
    }
}

/// Coerce a CLI string (or JSON value) according to a JSON Schema fragment.
pub fn coerce_value(value: Option<Value>, schema: &Value) -> Option<Value> {
    let value = value?;
    let t = schema.get("type").and_then(|x| x.as_str());

    match t {
        Some("array") => {
            if value.is_array() {
                return Some(value);
            }
            if let Some(s) = value.as_str() {
                if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                    if parsed.is_array() {
                        return Some(parsed);
                    }
                }
                let item_type = schema
                    .get("items")
                    .and_then(|i| i.get("type"))
                    .and_then(|x| x.as_str());
                if s.contains(',') {
                    let items: Vec<Value> = s
                        .split(',')
                        .map(|part| coerce_item(part.trim(), item_type))
                        .collect();
                    return Some(Value::Array(items));
                }
                return Some(Value::Array(vec![coerce_item(s, item_type)]));
            }
            Some(value)
        }
        Some("object") => {
            if let Some(s) = value.as_str() {
                if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                    return Some(parsed);
                }
            }
            Some(value)
        }
        Some("boolean") => Some(Value::Bool(match &value {
            Value::Bool(b) => *b,
            Value::String(s) => {
                let v = s.to_lowercase();
                matches!(v.as_str(), "true" | "1" | "yes")
            }
            Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
            _ => !value.is_null(),
        })),
        Some("integer") => {
            if let Some(n) = value.as_i64() {
                return Some(json!(n));
            }
            if let Some(s) = value.as_str() {
                if let Ok(n) = s.parse::<i64>() {
                    return Some(json!(n));
                }
            }
            Some(value)
        }
        Some("number") => {
            if let Some(n) = value.as_f64() {
                return Some(json!(n));
            }
            if let Some(s) = value.as_str() {
                if let Ok(n) = s.parse::<f64>() {
                    return Some(json!(n));
                }
            }
            Some(value)
        }
        None => {
            // Schema-less fallback: try to parse JSON objects/arrays from strings
            if let Some(s) = value.as_str() {
                let stripped = s.trim();
                if stripped.starts_with('{') || stripped.starts_with('[') {
                    if let Ok(parsed) = serde_json::from_str::<Value>(stripped) {
                        if parsed.is_object() || parsed.is_array() {
                            return Some(parsed);
                        }
                    }
                }
            }
            Some(value)
        }
        _ => Some(value),
    }
}

/// Resolve `env:VAR` or `file:/path` secret values.
///
/// Literal values (no recognized prefix) are rejected: secrets must never be
/// passed directly on argv, where they leak via `ps`, `/proc/<pid>/cmdline`,
/// and shell history.
pub fn resolve_secret(value: &str) -> Result<String> {
    if let Some(var) = value.strip_prefix("env:") {
        return std::env::var(var)
            .map_err(|_| Error::runtime(format!("environment variable {var:?} is not set")));
    }
    if let Some(path) = value.strip_prefix("file:") {
        let path = std::path::Path::new(path);
        if !path.exists() {
            return Err(Error::runtime(format!(
                "secret file not found: {}",
                path.display()
            )));
        }
        let text = std::fs::read_to_string(path)?;
        return Ok(text.trim_end_matches('\n').to_string());
    }
    Err(Error::runtime(format!(
        "refusing literal secret value {value:?}: use env:VAR or file:/path instead of putting secrets directly on the command line"
    )))
}

/// Truncate JSON arrays to first N elements; pass through other values.
pub fn apply_head(data: Value, n: usize) -> Value {
    match data {
        Value::Array(mut arr) => {
            arr.truncate(n);
            Value::Array(arr)
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn schema_types() {
        use crate::model::{schema_type_to_python, ParamType};
        assert_eq!(
            schema_type_to_python(&json!({"type": "integer"})),
            (ParamType::Integer, "")
        );
        assert_eq!(
            schema_type_to_python(&json!({"type": "number"})),
            (ParamType::Float, "")
        );
        assert_eq!(
            schema_type_to_python(&json!({"type": "boolean"})),
            (ParamType::Boolean, "")
        );
        assert_eq!(
            schema_type_to_python(&json!({"type": "string"})),
            (ParamType::String, "")
        );
        let (t, s) = schema_type_to_python(&json!({"type": "array"}));
        assert_eq!(t, ParamType::String);
        assert!(s.contains("JSON array"));
        let (t, s) = schema_type_to_python(&json!({"type": "object"}));
        assert_eq!(t, ParamType::String);
        assert!(s.contains("JSON object"));
        assert_eq!(schema_type_to_python(&json!({})), (ParamType::String, ""));
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn coerce_basics() {
        assert!(coerce_value(None, &json!({"type": "string"})).is_none());
        assert_eq!(
            coerce_value(Some(json!("42")), &json!({"type": "integer"})),
            Some(json!(42))
        );
        assert_eq!(
            coerce_value(Some(json!("3.14")), &json!({"type": "number"})),
            Some(json!(3.14))
        );
        assert_eq!(
            coerce_value(Some(json!(true)), &json!({"type": "boolean"})),
            Some(json!(true))
        );
        assert_eq!(
            coerce_value(Some(json!("[1, 2, 3]")), &json!({"type": "array"})),
            Some(json!([1, 2, 3]))
        );
        assert_eq!(
            coerce_value(Some(json!("{\"a\": 1}")), &json!({"type": "object"})),
            Some(json!({"a": 1}))
        );
        assert_eq!(
            coerce_value(Some(json!("not json")), &json!({"type": "array"})),
            Some(json!(["not json"]))
        );
        assert_eq!(
            coerce_value(Some(json!("hello")), &json!({"type": "string"})),
            Some(json!("hello"))
        );
    }

    #[test]
    fn coerce_arrays() {
        assert_eq!(
            coerce_value(Some(json!("TO_DO,IN_PROGRESS")), &json!({"type": "array"})),
            Some(json!(["TO_DO", "IN_PROGRESS"]))
        );
        assert_eq!(
            coerce_value(Some(json!("TO_DO")), &json!({"type": "array"})),
            Some(json!(["TO_DO"]))
        );
        assert_eq!(
            coerce_value(
                Some(json!("[\"TO_DO\",\"IN_PROGRESS\"]")),
                &json!({"type": "array"})
            ),
            Some(json!(["TO_DO", "IN_PROGRESS"]))
        );
        assert_eq!(
            coerce_value(Some(json!(["a", "b"])), &json!({"type": "array"})),
            Some(json!(["a", "b"]))
        );
        assert_eq!(
            coerce_value(
                Some(json!("1,2,3")),
                &json!({"type": "array", "items": {"type": "number"}})
            ),
            Some(json!([1.0, 2.0, 3.0]))
        );
        assert_eq!(
            coerce_value(
                Some(json!("1,2,3")),
                &json!({"type": "array", "items": {"type": "integer"}})
            ),
            Some(json!([1, 2, 3]))
        );
        assert_eq!(
            coerce_value(
                Some(json!("true,false")),
                &json!({"type": "array", "items": {"type": "boolean"}})
            ),
            Some(json!([true, false]))
        );
    }

    #[test]
    fn coerce_schemaless() {
        assert_eq!(
            coerce_value(Some(json!("{\"key\": \"val\"}")), &json!({})),
            Some(json!({"key": "val"}))
        );
        assert_eq!(
            coerce_value(Some(json!("[1, 2, 3]")), &json!({})),
            Some(json!([1, 2, 3]))
        );
        assert_eq!(
            coerce_value(Some(json!("hello")), &json!({})),
            Some(json!("hello"))
        );
        assert_eq!(
            coerce_value(Some(json!("{not valid json")), &json!({})),
            Some(json!("{not valid json"))
        );
    }

    #[test]
    fn kebab_cases() {
        assert_eq!(to_kebab("findPetsByStatus"), "find-pets-by-status");
        assert_eq!(to_kebab("list_items"), "list-items");
        assert_eq!(to_kebab("list-items"), "list-items");
        assert_eq!(to_kebab("getHTTPResponse"), "get-httpresponse");
    }

    #[test]
    fn resolve_secret_env_prefix() {
        std::env::set_var("SKIFF_TEST_RESOLVE_SECRET_ENV", "shh");
        assert_eq!(
            resolve_secret("env:SKIFF_TEST_RESOLVE_SECRET_ENV").unwrap(),
            "shh"
        );
        std::env::remove_var("SKIFF_TEST_RESOLVE_SECRET_ENV");
    }

    #[test]
    fn resolve_secret_env_missing() {
        assert!(resolve_secret("env:SKIFF_TEST_RESOLVE_SECRET_MISSING").is_err());
    }

    #[test]
    fn resolve_secret_file_prefix() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "skiff-resolve-secret-test-{}",
            std::process::id()
        ));
        std::fs::write(&path, "topsecret\n").unwrap();
        let resolved = resolve_secret(&format!("file:{}", path.display())).unwrap();
        assert_eq!(resolved, "topsecret");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn resolve_secret_rejects_literal() {
        let err = resolve_secret("literal-token-on-argv").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("env:VAR"));
        assert!(msg.contains("file:/path"));
    }

    #[test]
    fn head_truncation() {
        assert_eq!(apply_head(json!([1, 2, 3, 4, 5]), 3), json!([1, 2, 3]));
        assert_eq!(apply_head(json!({"a": 1}), 2), json!({"a": 1}));
        assert_eq!(apply_head(json!([]), 5), json!([]));
        assert_eq!(apply_head(json!([1, 2]), 10), json!([1, 2]));
    }
}
