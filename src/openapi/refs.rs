//! OpenAPI `$ref` resolution (Python `resolve_refs`).

use serde_json::{Map, Value};
use std::collections::HashSet;

pub fn resolve_refs(spec: Value) -> Value {
    let root = spec.clone();
    resolve_node(spec, &root, &HashSet::new())
}

fn resolve_node(node: Value, root: &Value, seen: &HashSet<String>) -> Value {
    match node {
        Value::Object(map) => {
            if let Some(Value::String(ref_path)) = map.get("$ref") {
                if seen.contains(ref_path) {
                    return Value::Object(map);
                }
                if let Some(rest) = ref_path.strip_prefix("#/") {
                    let mut target = root;
                    for part in rest.split('/') {
                        match target.get(part) {
                            Some(next) => target = next,
                            None => return Value::Object(map),
                        }
                    }
                    let mut next_seen = seen.clone();
                    next_seen.insert(ref_path.clone());
                    return resolve_node(target.clone(), root, &next_seen);
                }
                return Value::Object(map);
            }
            let mut out = Map::new();
            for (k, v) in map {
                out.insert(k, resolve_node(v, root, seen));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(|item| resolve_node(item, root, seen))
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn simple_ref() {
        let spec = json!({
            "components": {
                "parameters": {
                    "LimitParam": {
                        "name": "limit",
                        "in": "query",
                        "schema": {"type": "integer"}
                    }
                }
            },
            "paths": {
                "/pets": {
                    "get": {
                        "parameters": [{"$ref": "#/components/parameters/LimitParam"}]
                    }
                }
            }
        });
        let resolved = resolve_refs(spec);
        let params = &resolved["paths"]["/pets"]["get"]["parameters"];
        assert_eq!(params.as_array().unwrap().len(), 1);
        assert_eq!(params[0]["name"], "limit");
        assert!(params[0].get("$ref").is_none());
    }

    #[test]
    fn circular_ref_safe() {
        let spec = json!({
            "a": {"$ref": "#/b"},
            "b": {"$ref": "#/a"},
        });
        let resolved = resolve_refs(spec);
        assert!(resolved["a"].get("$ref").is_some() || resolved["b"].get("$ref").is_some());
    }
}
