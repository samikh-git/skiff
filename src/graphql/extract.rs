//! Introspection schema → `CommandDef` list.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use crate::coerce::to_kebab;
use crate::model::{CommandDef, ParamDef, ParamLocation, ParamType};

/// Unwrap NON_NULL/LIST wrappers. Returns (named_type, is_non_null, is_list).
pub fn unwrap_type(type_ref: &Value) -> (Value, bool, bool) {
    let mut is_non_null = false;
    let mut is_list = false;
    let mut t = type_ref.clone();
    loop {
        let kind = t.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        match kind {
            "NON_NULL" => {
                is_non_null = true;
                t = t.get("ofType").cloned().unwrap_or(Value::Null);
            }
            "LIST" => {
                is_list = true;
                t = t.get("ofType").cloned().unwrap_or(Value::Null);
            }
            _ => return (t, is_non_null, is_list),
        }
        if t.is_null() {
            return (type_ref.clone(), is_non_null, is_list);
        }
    }
}

/// Reconstruct GraphQL type notation, e.g. `[String!]!`.
pub fn graphql_type_string(type_ref: &Value) -> String {
    let kind = type_ref.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    match kind {
        "NON_NULL" => {
            let inner = graphql_type_string(type_ref.get("ofType").unwrap_or(&Value::Null));
            format!("{inner}!")
        }
        "LIST" => {
            let inner = graphql_type_string(type_ref.get("ofType").unwrap_or(&Value::Null));
            format!("[{inner}]")
        }
        _ => type_ref
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("String")
            .to_string(),
    }
}

/// Map GraphQL type → (CLI ParamType, required, choices).
pub fn graphql_type_to_python(
    type_ref: &Value,
    types_by_name: &HashMap<String, Value>,
) -> (ParamType, bool, Option<Vec<String>>) {
    let (named, is_non_null, is_list) = unwrap_type(type_ref);
    let type_name = named.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let type_kind = named.get("kind").and_then(|k| k.as_str()).unwrap_or("");

    if is_list {
        return (ParamType::String, is_non_null, None);
    }

    if type_kind == "ENUM" {
        let choices = types_by_name
            .get(type_name)
            .and_then(|t| t.get("enumValues"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|ev| ev.get("name").and_then(|n| n.as_str()).map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .filter(|c| !c.is_empty());
        return (ParamType::String, is_non_null, choices);
    }

    if type_kind == "INPUT_OBJECT" {
        return (ParamType::String, is_non_null, None);
    }

    let py_type = match type_name {
        "Int" => ParamType::Integer,
        "Float" => ParamType::Float,
        "Boolean" => ParamType::Boolean,
        _ => ParamType::String, // String, ID, custom scalars
    };
    (py_type, is_non_null, None)
}

fn build_graphql_param(arg: &Value, types_by_name: &HashMap<String, Value>) -> ParamDef {
    let type_ref = arg.get("type").unwrap_or(&Value::Null);
    let (py_type, required, choices) = graphql_type_to_python(type_ref, types_by_name);
    let gql_type_str = graphql_type_string(type_ref);
    let (named_t, _, is_list) = unwrap_type(type_ref);

    let mut param_schema = Map::new();
    param_schema.insert("graphql_type".into(), Value::String(gql_type_str));

    if is_list {
        param_schema.insert("type".into(), Value::String("array".into()));
        let item_type_name = named_t.get("name").and_then(|n| n.as_str()).unwrap_or("String");
        let item_json = match item_type_name {
            "Int" => "integer",
            "Float" => "number",
            "Boolean" => "boolean",
            _ => "string",
        };
        param_schema.insert("items".into(), json!({ "type": item_json }));
    } else if named_t.get("kind").and_then(|k| k.as_str()) == Some("INPUT_OBJECT") {
        param_schema.insert("type".into(), Value::String("object".into()));
    } else if named_t.get("kind").and_then(|k| k.as_str()) == Some("ENUM") {
        param_schema.insert("type".into(), Value::String("string".into()));
    } else if named_t.get("name").and_then(|n| n.as_str()) == Some("Boolean") {
        // Enable coerce_value for explicit true/false GraphQL booleans.
        param_schema.insert("type".into(), Value::String("boolean".into()));
    } else if named_t.get("name").and_then(|n| n.as_str()) == Some("Int") {
        param_schema.insert("type".into(), Value::String("integer".into()));
    } else if named_t.get("name").and_then(|n| n.as_str()) == Some("Float") {
        param_schema.insert("type".into(), Value::String("number".into()));
    }

    let arg_name = arg.get("name").and_then(|n| n.as_str()).unwrap_or("arg");
    let mut arg_desc = arg
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or(arg_name)
        .to_string();
    if is_list {
        arg_desc.push_str(" (JSON array)");
    } else if named_t.get("kind").and_then(|k| k.as_str()) == Some("INPUT_OBJECT") {
        arg_desc.push_str(" (JSON object)");
    }

    ParamDef {
        name: to_kebab(arg_name),
        original_name: arg_name.to_string(),
        python_type: py_type,
        required,
        description: arg_desc,
        choices,
        location: ParamLocation::GraphqlArg,
        schema: Value::Object(param_schema),
    }
}

fn detect_field_collisions(query_fields: &[Value], mutation_fields: &[Value]) -> HashSet<String> {
    let mut all_names = HashSet::new();
    let mut collisions = HashSet::new();
    for f in query_fields.iter().chain(mutation_fields.iter()) {
        if let Some(n) = f.get("name").and_then(|n| n.as_str()) {
            if !all_names.insert(n.to_string()) {
                collisions.insert(n.to_string());
            }
        }
    }
    collisions
}

/// Convert introspection `__schema` into CLI commands.
pub fn extract_graphql_commands(schema: &Value) -> Vec<CommandDef> {
    let types_by_name: HashMap<String, Value> = schema
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
        .unwrap_or_default();

    let query_type_name = schema
        .pointer("/queryType/name")
        .and_then(|n| n.as_str());
    let mutation_type_name = schema
        .pointer("/mutationType/name")
        .and_then(|n| n.as_str());

    let query_fields: Vec<Value> = query_type_name
        .and_then(|n| types_by_name.get(n))
        .and_then(|t| t.get("fields"))
        .and_then(|f| f.as_array())
        .cloned()
        .unwrap_or_default();
    let mutation_fields: Vec<Value> = mutation_type_name
        .and_then(|n| types_by_name.get(n))
        .and_then(|t| t.get("fields"))
        .and_then(|f| f.as_array())
        .cloned()
        .unwrap_or_default();

    let collisions = detect_field_collisions(&query_fields, &mutation_fields);
    let mut commands = Vec::new();
    let mut seen_names = HashSet::new();

    for (op_type, fields) in [("query", &query_fields), ("mutation", &mutation_fields)] {
        for field_def in fields {
            let field_name = match field_def.get("name").and_then(|n| n.as_str()) {
                Some(n) if !n.starts_with("__") => n,
                _ => continue,
            };

            let mut cli_name = to_kebab(field_name);
            if collisions.contains(field_name) {
                cli_name = format!("{op_type}-{cli_name}");
            }
            if seen_names.contains(&cli_name) {
                cli_name = format!("{op_type}-{cli_name}");
            }
            seen_names.insert(cli_name.clone());

            let desc = field_def
                .get("description")
                .and_then(|d| d.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{op_type} {field_name}"));

            let params: Vec<ParamDef> = field_def
                .get("args")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|arg| build_graphql_param(arg, &types_by_name))
                        .collect()
                })
                .unwrap_or_default();

            let has_body = !params.is_empty();
            commands.push(CommandDef {
                name: cli_name,
                description: desc,
                params,
                has_body,
                graphql_operation_type: Some(op_type.into()),
                graphql_field_name: Some(field_name.into()),
                graphql_return_type: field_def.get("type").cloned(),
                ..Default::default()
            });
        }
    }

    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_schema() -> Value {
        json!({
            "queryType": {"name": "Query"},
            "mutationType": {"name": "Mutation"},
            "types": [
                {
                    "kind": "OBJECT",
                    "name": "Query",
                    "fields": [
                        {
                            "name": "users",
                            "description": "List all users",
                            "args": [],
                            "type": {
                                "kind": "NON_NULL",
                                "ofType": {
                                    "kind": "LIST",
                                    "ofType": {
                                        "kind": "NON_NULL",
                                        "ofType": {"kind": "OBJECT", "name": "User"}
                                    }
                                }
                            }
                        },
                        {
                            "name": "user",
                            "description": "Get a user by ID",
                            "args": [{
                                "name": "id",
                                "description": "User ID",
                                "type": {
                                    "kind": "NON_NULL",
                                    "ofType": {"kind": "SCALAR", "name": "ID"}
                                }
                            }],
                            "type": {"kind": "OBJECT", "name": "User"}
                        },
                        {
                            "name": "usersByIds",
                            "description": "Get users by a list of IDs",
                            "args": [{
                                "name": "ids",
                                "description": "User IDs",
                                "type": {
                                    "kind": "NON_NULL",
                                    "ofType": {
                                        "kind": "LIST",
                                        "ofType": {
                                            "kind": "NON_NULL",
                                            "ofType": {"kind": "SCALAR", "name": "ID"}
                                        }
                                    }
                                }
                            }],
                            "type": {
                                "kind": "NON_NULL",
                                "ofType": {
                                    "kind": "LIST",
                                    "ofType": {
                                        "kind": "NON_NULL",
                                        "ofType": {"kind": "OBJECT", "name": "User"}
                                    }
                                }
                            }
                        }
                    ]
                },
                {
                    "kind": "OBJECT",
                    "name": "Mutation",
                    "fields": [
                        {
                            "name": "createUser",
                            "description": "Create a new user",
                            "args": [
                                {
                                    "name": "name",
                                    "description": "User name",
                                    "type": {
                                        "kind": "NON_NULL",
                                        "ofType": {"kind": "SCALAR", "name": "String"}
                                    }
                                },
                                {
                                    "name": "email",
                                    "description": "User email",
                                    "type": {
                                        "kind": "NON_NULL",
                                        "ofType": {"kind": "SCALAR", "name": "String"}
                                    }
                                },
                                {
                                    "name": "age",
                                    "description": "User age",
                                    "type": {"kind": "SCALAR", "name": "Int"}
                                }
                            ],
                            "type": {"kind": "OBJECT", "name": "User"}
                        },
                        {
                            "name": "deleteUser",
                            "description": "Delete a user by ID",
                            "args": [{
                                "name": "id",
                                "description": "User ID",
                                "type": {
                                    "kind": "NON_NULL",
                                    "ofType": {"kind": "SCALAR", "name": "ID"}
                                }
                            }],
                            "type": {"kind": "SCALAR", "name": "Boolean"}
                        }
                    ]
                },
                {
                    "kind": "OBJECT",
                    "name": "User",
                    "fields": [
                        {"name": "id", "args": [], "type": {"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "ID"}}},
                        {"name": "name", "args": [], "type": {"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "String"}}},
                        {"name": "email", "args": [], "type": {"kind": "SCALAR", "name": "String"}},
                        {"name": "age", "args": [], "type": {"kind": "SCALAR", "name": "Int"}},
                        {"name": "status", "args": [], "type": {"kind": "ENUM", "name": "Status"}},
                        {"name": "address", "args": [], "type": {"kind": "OBJECT", "name": "Address"}},
                        {"name": "node", "args": [], "type": {"kind": "INTERFACE", "name": "Node"}}
                    ]
                },
                {
                    "kind": "OBJECT",
                    "name": "Address",
                    "fields": [
                        {"name": "city", "args": [], "type": {"kind": "SCALAR", "name": "String"}},
                        {"name": "country", "args": [], "type": {"kind": "SCALAR", "name": "String"}}
                    ]
                },
                {
                    "kind": "INTERFACE",
                    "name": "Node",
                    "fields": [
                        {"name": "id", "args": [], "type": {"kind": "NON_NULL", "ofType": {"kind": "SCALAR", "name": "ID"}}}
                    ]
                },
                {
                    "kind": "ENUM",
                    "name": "Status",
                    "enumValues": [
                        {"name": "ACTIVE"},
                        {"name": "INACTIVE"},
                        {"name": "BANNED"}
                    ]
                },
                {"kind": "SCALAR", "name": "ID"},
                {"kind": "SCALAR", "name": "String"},
                {"kind": "SCALAR", "name": "Int"},
                {"kind": "SCALAR", "name": "Float"},
                {"kind": "SCALAR", "name": "Boolean"},
                {
                    "kind": "INPUT_OBJECT",
                    "name": "UserInput",
                    "inputFields": []
                }
            ]
        })
    }

    #[test]
    fn type_string_basics() {
        assert_eq!(
            graphql_type_string(&json!({"kind": "SCALAR", "name": "String"})),
            "String"
        );
        assert_eq!(
            graphql_type_string(&json!({
                "kind": "NON_NULL",
                "ofType": {"kind": "SCALAR", "name": "String"}
            })),
            "String!"
        );
        assert_eq!(
            graphql_type_string(&json!({
                "kind": "LIST",
                "ofType": {"kind": "SCALAR", "name": "Int"}
            })),
            "[Int]"
        );
        assert_eq!(
            graphql_type_string(&json!({
                "kind": "NON_NULL",
                "ofType": {
                    "kind": "LIST",
                    "ofType": {
                        "kind": "NON_NULL",
                        "ofType": {"kind": "SCALAR", "name": "String"}
                    }
                }
            })),
            "[String!]!"
        );
    }

    #[test]
    fn type_mapping() {
        let types = HashMap::new();
        let (t, req, _) = graphql_type_to_python(
            &json!({"kind": "SCALAR", "name": "String"}),
            &types,
        );
        assert_eq!(t, ParamType::String);
        assert!(!req);

        let (t, req, _) = graphql_type_to_python(
            &json!({
                "kind": "NON_NULL",
                "ofType": {"kind": "SCALAR", "name": "String"}
            }),
            &types,
        );
        assert_eq!(t, ParamType::String);
        assert!(req);

        let (t, _, _) = graphql_type_to_python(
            &json!({"kind": "SCALAR", "name": "Boolean"}),
            &types,
        );
        assert_eq!(t, ParamType::Boolean);

        let (t, req, _) = graphql_type_to_python(
            &json!({
                "kind": "NON_NULL",
                "ofType": {
                    "kind": "LIST",
                    "ofType": {"kind": "SCALAR", "name": "ID"}
                }
            }),
            &types,
        );
        assert_eq!(t, ParamType::String);
        assert!(req);
    }

    #[test]
    fn enum_choices() {
        let schema = sample_schema();
        let types: HashMap<_, _> = schema["types"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| {
                Some((
                    t.get("name")?.as_str()?.to_string(),
                    t.clone(),
                ))
            })
            .collect();
        let (_, _, choices) = graphql_type_to_python(
            &json!({"kind": "ENUM", "name": "Status"}),
            &types,
        );
        assert_eq!(
            choices.unwrap(),
            vec!["ACTIVE".to_string(), "INACTIVE".into(), "BANNED".into()]
        );
    }

    #[test]
    fn extract_commands() {
        let cmds = extract_graphql_commands(&sample_schema());
        let names: Vec<_> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"users"));
        assert!(names.contains(&"user"));
        assert!(names.contains(&"create-user"));
        assert!(names.contains(&"delete-user"));
        assert!(names.contains(&"users-by-ids"));

        let user = cmds.iter().find(|c| c.name == "user").unwrap();
        assert_eq!(user.graphql_operation_type.as_deref(), Some("query"));
        assert_eq!(user.params.len(), 1);
        assert!(user.params[0].required);

        let create = cmds.iter().find(|c| c.name == "create-user").unwrap();
        assert_eq!(create.graphql_operation_type.as_deref(), Some("mutation"));
        assert_eq!(create.params.len(), 3);

        let by_ids = cmds.iter().find(|c| c.name == "users-by-ids").unwrap();
        assert_eq!(
            by_ids.params[0].schema.get("type").and_then(|t| t.as_str()),
            Some("array")
        );
    }

    #[test]
    fn collision_prefixes() {
        let schema = json!({
            "queryType": {"name": "Query"},
            "mutationType": {"name": "Mutation"},
            "types": [
                {
                    "kind": "OBJECT",
                    "name": "Query",
                    "fields": [{
                        "name": "item",
                        "args": [],
                        "type": {"kind": "SCALAR", "name": "String"}
                    }]
                },
                {
                    "kind": "OBJECT",
                    "name": "Mutation",
                    "fields": [{
                        "name": "item",
                        "args": [],
                        "type": {"kind": "SCALAR", "name": "String"}
                    }]
                },
                {"kind": "SCALAR", "name": "String"}
            ]
        });
        let cmds = extract_graphql_commands(&schema);
        let names: Vec<_> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"query-item"));
        assert!(names.contains(&"mutation-item"));
    }
}
