//! Build GraphQL documents and execute against an endpoint.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{json, Map, Value};

use crate::cli::dynamic::ParsedToolArgs;
use crate::coerce::coerce_value;
use crate::error::{Error, Result};
use crate::graphql::extract::unwrap_type;
use crate::model::{CommandDef, ParamLocation};
use crate::output::{output_result, OutputOptions};

/// Auto-generate a selection set (depth 2 = scalars + one nested object level).
/// INTERFACE/UNION fields emit `__typename` (+ interface scalar fields when present).
pub fn build_selection_set(
    type_ref: &Value,
    types_by_name: &HashMap<String, Value>,
    depth: i32,
    seen: &mut HashSet<String>,
) -> String {
    let (named, _, _) = unwrap_type(type_ref);
    let type_name = named.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let type_kind = named.get("kind").and_then(|k| k.as_str()).unwrap_or("");

    if matches!(type_kind, "SCALAR" | "ENUM") {
        return String::new();
    }

    if type_name.is_empty() || seen.contains(type_name) || depth <= 0 {
        return String::new();
    }

    if matches!(type_kind, "INTERFACE" | "UNION") {
        seen.insert(type_name.to_string());
        let mut parts = vec!["__typename".to_string()];
        if type_kind == "INTERFACE" {
            if let Some(type_def) = types_by_name.get(type_name) {
                if let Some(fields) = type_def.get("fields").and_then(|f| f.as_array()) {
                    for f in fields {
                        let fname = f.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        let (f_named, _, _) = unwrap_type(f.get("type").unwrap_or(&Value::Null));
                        let f_kind = f_named.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                        if matches!(f_kind, "SCALAR" | "ENUM") && !fname.is_empty() {
                            parts.push(fname.to_string());
                        }
                    }
                }
            }
        }
        return format!("{{ {} }}", parts.join(" "));
    }

    let Some(type_def) = types_by_name.get(type_name) else {
        return String::new();
    };
    let Some(fields) = type_def.get("fields").and_then(|f| f.as_array()) else {
        return String::new();
    };
    if fields.is_empty() {
        return String::new();
    }

    seen.insert(type_name.to_string());
    let mut parts = Vec::new();
    for f in fields {
        let fname = f.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if fname.is_empty() {
            continue;
        }
        let f_type = f.get("type").unwrap_or(&Value::Null);
        let (f_named, _, _) = unwrap_type(f_type);
        let f_kind = f_named.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if matches!(f_kind, "SCALAR" | "ENUM") {
            parts.push(fname.to_string());
        } else if f_kind == "OBJECT" && depth > 1 {
            let nested = build_selection_set(f_type, types_by_name, depth - 1, seen);
            if !nested.is_empty() {
                parts.push(format!("{fname} {nested}"));
            }
        } else if matches!(f_kind, "INTERFACE" | "UNION") && depth > 1 {
            let nested = build_selection_set(f_type, types_by_name, depth - 1, seen);
            if !nested.is_empty() {
                parts.push(format!("{fname} {nested}"));
            }
        }
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("{{ {} }}", parts.join(" "))
}

fn types_index(schema: &Value) -> HashMap<String, Value> {
    schema
        .get("types")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let name = t.get("name").and_then(|n| n.as_str())?;
                    Some((name.to_string(), t.clone()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// After flags + stdin merge, ensure NON_NULL GraphQL args are present.
pub fn validate_required_graphql_args(
    cmd: &CommandDef,
    variables: &Map<String, Value>,
) -> Result<()> {
    for p in &cmd.params {
        if !p.required || p.location != ParamLocation::GraphqlArg {
            continue;
        }
        if !variables.contains_key(&p.original_name) {
            return Err(Error::usage(format!(
                "missing required GraphQL argument: --{}",
                p.name
            )));
        }
    }
    Ok(())
}

/// Build document + variables from parsed CLI args.
pub fn build_graphql_document(
    cmd: &CommandDef,
    values: &BTreeMap<String, Value>,
    stdin_vars: Option<Map<String, Value>>,
    schema: &Value,
    fields_override: Option<&str>,
) -> Result<(String, Map<String, Value>, String)> {
    let types_by_name = types_index(schema);

    let mut variables = Map::new();
    if let Some(stdin) = stdin_vars {
        variables = stdin;
    } else {
        for p in &cmd.params {
            if let Some(val) = values.get(&p.original_name) {
                let coerced = coerce_value(Some(val.clone()), &p.schema).unwrap_or_else(|| val.clone());
                variables.insert(p.original_name.clone(), coerced);
            }
        }
    }

    validate_required_graphql_args(cmd, &variables)?;

    let mut var_decls = Vec::new();
    for p in &cmd.params {
        if variables.contains_key(&p.original_name) {
            let gql_type = p
                .schema
                .get("graphql_type")
                .and_then(|t| t.as_str())
                .unwrap_or("String");
            var_decls.push(format!("${}: {gql_type}", p.original_name));
        }
    }

    let selection = if let Some(fields) = fields_override {
        format!("{{ {fields} }}")
    } else if let Some(ret) = &cmd.graphql_return_type {
        let mut seen = HashSet::new();
        build_selection_set(ret, &types_by_name, 2, &mut seen)
    } else {
        String::new()
    };

    let mut field_args = Vec::new();
    for p in &cmd.params {
        if variables.contains_key(&p.original_name) {
            field_args.push(format!("{}: ${}", p.original_name, p.original_name));
        }
    }

    let field_name = cmd
        .graphql_field_name
        .clone()
        .unwrap_or_else(|| cmd.name.clone());
    let args_str = if field_args.is_empty() {
        String::new()
    } else {
        format!("({})", field_args.join(", "))
    };
    let op_type = cmd
        .graphql_operation_type
        .as_deref()
        .unwrap_or("query");
    let var_decls_str = if var_decls.is_empty() {
        String::new()
    } else {
        format!("({})", var_decls.join(", "))
    };

    let selection_part = if selection.is_empty() {
        String::new()
    } else {
        format!(" {selection}")
    };
    let document = format!("{op_type}{var_decls_str} {{ {field_name}{args_str}{selection_part} }}");
    Ok((document, variables, field_name))
}

/// Execute a GraphQL operation and print the result.
pub fn execute_graphql(
    parsed: &ParsedToolArgs,
    url: &str,
    schema: &Value,
    auth_headers: &[(String, String)],
    fields_override: Option<&str>,
    output: &OutputOptions,
) -> Result<()> {
    let stdin_vars = if parsed.stdin {
        let v = crate::cli::dynamic::read_stdin_json("GraphQL variables")?;
        Some(
            v.as_object()
                .cloned()
                .ok_or_else(|| Error::usage("--stdin for GraphQL expects a JSON object"))?,
        )
    } else {
        None
    };

    let (document, variables, field_name) = build_graphql_document(
        &parsed.command,
        &parsed.values,
        stdin_vars,
        schema,
        fields_override,
    )?;

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| Error::runtime(e.to_string()))?;

    let mut req = client.post(url).header("Content-Type", "application/json");
    for (k, v) in auth_headers {
        req = req.header(k, v);
    }

    let body = json!({
        "query": document,
        "variables": if variables.is_empty() { Value::Null } else { Value::Object(variables) },
    });
    let resp = req
        .json(&body)
        .send()
        .map_err(|e| Error::runtime(format!("GraphQL request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(Error::runtime(format!(
            "Error {}: {}",
            resp.status().as_u16(),
            resp.text().unwrap_or_default()
        )));
    }

    let result: Value = resp
        .json()
        .map_err(|e| Error::runtime(format!("invalid JSON from GraphQL: {e}")))?;

    if result.get("errors").is_some() {
        if result.get("data").is_none() || result.get("data").map(|d| d.is_null()).unwrap_or(false)
        {
            let msgs = result["errors"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|e| e.get("message").and_then(|m| m.as_str()))
                        .collect::<Vec<_>>()
                        .join("; ")
                })
                .unwrap_or_default();
            return Err(Error::runtime(format!("GraphQL error: {msgs}")));
        }
        // Partial errors — include full envelope.
        output_result(result, output).map_err(Error::from)?;
        return Ok(());
    }

    let data = result.get("data").cloned().unwrap_or(Value::Null);
    let field_data = data
        .get(&field_name)
        .cloned()
        .unwrap_or(data);
    output_result(field_data, output).map_err(Error::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphql::extract::extract_graphql_commands;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "queryType": {"name": "Query"},
            "mutationType": {"name": "Mutation"},
            "types": [
                {
                    "kind": "OBJECT",
                    "name": "Query",
                    "fields": [{
                        "name": "user",
                        "args": [{
                            "name": "id",
                            "type": {
                                "kind": "NON_NULL",
                                "ofType": {"kind": "SCALAR", "name": "ID"}
                            }
                        }],
                        "type": {"kind": "OBJECT", "name": "User"}
                    }]
                },
                {
                    "kind": "OBJECT",
                    "name": "Mutation",
                    "fields": []
                },
                {
                    "kind": "OBJECT",
                    "name": "User",
                    "fields": [
                        {"name": "id", "args": [], "type": {"kind": "SCALAR", "name": "ID"}},
                        {"name": "name", "args": [], "type": {"kind": "SCALAR", "name": "String"}},
                        {"name": "address", "args": [], "type": {"kind": "OBJECT", "name": "Address"}},
                        {"name": "node", "args": [], "type": {"kind": "INTERFACE", "name": "Node"}}
                    ]
                },
                {
                    "kind": "OBJECT",
                    "name": "Address",
                    "fields": [
                        {"name": "city", "args": [], "type": {"kind": "SCALAR", "name": "String"}}
                    ]
                },
                {
                    "kind": "INTERFACE",
                    "name": "Node",
                    "fields": [
                        {"name": "id", "args": [], "type": {"kind": "SCALAR", "name": "ID"}}
                    ]
                },
                {"kind": "SCALAR", "name": "ID"},
                {"kind": "SCALAR", "name": "String"}
            ]
        })
    }

    #[test]
    fn selection_depth_2_includes_nested() {
        let types = types_index(&schema());
        let mut seen = HashSet::new();
        let sel = build_selection_set(
            &json!({"kind": "OBJECT", "name": "User"}),
            &types,
            2,
            &mut seen,
        );
        assert!(sel.contains("id"));
        assert!(sel.contains("name"));
        assert!(sel.contains("address"));
        assert!(sel.contains("city"));
        assert!(sel.contains("__typename"));
        assert!(sel.contains("node"));
    }

    #[test]
    fn selection_depth_1_no_nested() {
        let types = types_index(&schema());
        let mut seen = HashSet::new();
        let sel = build_selection_set(
            &json!({"kind": "OBJECT", "name": "User"}),
            &types,
            1,
            &mut seen,
        );
        assert!(sel.contains("id"));
        assert!(!sel.contains("city"));
    }

    #[test]
    fn scalar_selection_empty() {
        let types = types_index(&schema());
        let mut seen = HashSet::new();
        let sel = build_selection_set(
            &json!({"kind": "SCALAR", "name": "String"}),
            &types,
            2,
            &mut seen,
        );
        assert!(sel.is_empty());
    }

    #[test]
    fn document_and_required() {
        let cmds = extract_graphql_commands(&schema());
        let cmd = cmds.iter().find(|c| c.name == "user").unwrap();
        let mut values = BTreeMap::new();
        values.insert("id".into(), json!("1"));
        let (doc, vars, field) =
            build_graphql_document(cmd, &values, None, &schema(), None).unwrap();
        assert_eq!(field, "user");
        assert!(doc.starts_with("query("));
        assert!(doc.contains("$id: ID!"));
        assert_eq!(vars.get("id"), Some(&json!("1")));

        let empty = BTreeMap::new();
        let err = build_graphql_document(cmd, &empty, None, &schema(), None).unwrap_err();
        assert!(err.to_string().contains("missing required"));
    }

    #[test]
    fn fields_override() {
        let cmds = extract_graphql_commands(&schema());
        let cmd = cmds.iter().find(|c| c.name == "user").unwrap();
        let mut values = BTreeMap::new();
        values.insert("id".into(), json!("1"));
        let (doc, _, _) =
            build_graphql_document(cmd, &values, None, &schema(), Some("id name")).unwrap();
        assert!(doc.contains("{ id name }"));
    }
}
