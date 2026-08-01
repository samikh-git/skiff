//! Extract CLI commands from an OpenAPI document.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::coerce::to_kebab;
use crate::model::{schema_type_to_python, CommandDef, ParamDef, ParamLocation, ParamType};

const HTTP_METHODS: &[&str] = &["get", "post", "put", "delete", "patch"];

pub fn extract_openapi_commands(spec: &Value) -> Vec<CommandDef> {
    let mut commands = Vec::new();
    let mut seen_names: HashMap<String, u32> = HashMap::new();

    let Some(paths) = spec.get("paths").and_then(|p| p.as_object()) else {
        return commands;
    };

    for (path, methods) in paths {
        let Some(methods) = methods.as_object() else {
            continue;
        };
        for (method, details) in methods {
            if !HTTP_METHODS.contains(&method.as_str()) {
                continue;
            }
            let Some(details) = details.as_object() else {
                continue;
            };

            let mut name = if let Some(op_id) = details.get("operationId").and_then(|v| v.as_str())
            {
                to_kebab(op_id)
            } else {
                let slug = path
                    .trim_matches('/')
                    .replace('/', "-")
                    .replace(['{', '}'], "");
                if slug.is_empty() {
                    method.clone()
                } else {
                    format!("{method}-{slug}")
                }
            };

            if seen_names.contains_key(&name) {
                name = format!("{name}-{method}");
            }
            seen_names.insert(name.clone(), 1);

            let desc = details
                .get("summary")
                .and_then(|v| v.as_str())
                .or_else(|| details.get("description").and_then(|v| v.as_str()))
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("{} {path}", method.to_uppercase()));

            let mut params = Vec::new();

            if let Some(param_list) = details.get("parameters").and_then(|v| v.as_array()) {
                for param in param_list {
                    let Some(pname) = param.get("name").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let schema = param
                        .get("schema")
                        .cloned()
                        .unwrap_or_else(|| Value::Object(Default::default()));
                    let (py_type, suffix) = schema_type_to_python(&schema);
                    let location = match param.get("in").and_then(|v| v.as_str()).unwrap_or("query")
                    {
                        "path" => ParamLocation::Path,
                        "header" => ParamLocation::Header,
                        "body" => ParamLocation::Body,
                        _ => ParamLocation::Query,
                    };
                    let description = format!(
                        "{}{suffix}",
                        param
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or(pname)
                    );
                    let choices = enum_choices(&schema);
                    params.push(ParamDef {
                        name: to_kebab(pname),
                        original_name: pname.to_string(),
                        python_type: py_type,
                        required: param
                            .get("required")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        description,
                        choices,
                        location,
                        schema,
                    });
                }
            }

            let rb_content = details
                .get("requestBody")
                .and_then(|v| v.get("content"))
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default()));

            let multipart_schema = rb_content
                .get("multipart/form-data")
                .and_then(|v| v.get("schema"))
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default()));
            let json_schema = rb_content
                .get("application/json")
                .and_then(|v| v.get("schema"))
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default()));

            let mp_props = multipart_schema
                .get("properties")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let has_binary = mp_props
                .values()
                .any(|p| p.get("format").and_then(|v| v.as_str()) == Some("binary"));

            let (rb_schema, cmd_content_type) = if has_binary {
                (multipart_schema, Some("multipart/form-data".to_string()))
            } else if json_schema.get("properties").is_some() {
                (json_schema, None)
            } else if !mp_props.is_empty() {
                (multipart_schema, Some("multipart/form-data".to_string()))
            } else {
                (Value::Object(Default::default()), None)
            };

            let required_fields: HashSet<String> = rb_schema
                .get("required")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let properties = rb_schema
                .get("properties")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            let has_body = !properties.is_empty();

            for (prop_name, prop_schema) in properties {
                let is_binary = cmd_content_type.as_deref() == Some("multipart/form-data")
                    && prop_schema.get("format").and_then(|v| v.as_str()) == Some("binary");
                let (loc, py_type, suffix) = if is_binary {
                    (ParamLocation::File, ParamType::String, " (file path)")
                } else {
                    let (t, s) = schema_type_to_python(&prop_schema);
                    (ParamLocation::Body, t, s)
                };
                let description = format!(
                    "{}{suffix}",
                    prop_schema
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&prop_name)
                );
                params.push(ParamDef {
                    name: to_kebab(&prop_name),
                    original_name: prop_name.clone(),
                    python_type: py_type,
                    required: required_fields.contains(&prop_name),
                    description,
                    choices: enum_choices(&prop_schema),
                    location: loc,
                    schema: prop_schema,
                });
            }

            commands.push(CommandDef {
                name,
                description: desc,
                params,
                has_body,
                method: Some(method.clone()),
                path: Some(path.clone()),
                content_type: cmd_content_type,
                ..Default::default()
            });
        }
    }

    commands
}

fn enum_choices(schema: &Value) -> Option<Vec<String>> {
    schema.get("enum").and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn petstore() -> Value {
        let raw = include_str!("../../tests/fixtures/petstore.json");
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn extracts_petstore_commands() {
        let cmds = extract_openapi_commands(&petstore());
        assert_eq!(cmds.len(), 5);
        let names: HashSet<_> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains("list-pets"));
        assert!(names.contains("create-pet"));
        assert!(names.contains("get-pet"));
        assert!(names.contains("delete-pet"));
        assert!(names.contains("update-pet"));
    }
}
