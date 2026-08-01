//! Execute OpenAPI operations via HTTP.

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use reqwest::blocking::{multipart, Client};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

use crate::cli::dynamic::{read_stdin_json, ParsedToolArgs};
use crate::error::{Error, Result};
use crate::model::{CommandDef, ParamLocation};
use crate::output::{output_result, OutputOptions};

pub struct OpenApiRequest {
    pub path: String,
    pub query: HashMap<String, Value>,
    pub headers: HashMap<String, String>,
    pub body: Option<Value>,
    pub files: Vec<(String, String)>, // field name -> file path
}

pub fn collect_openapi_params(cmd: &CommandDef, parsed: &ParsedToolArgs) -> Result<OpenApiRequest> {
    let mut path = cmd.path.clone().unwrap_or_default();
    let mut query = HashMap::new();
    let mut headers = HashMap::new();
    let mut body: Option<Value> = None;
    let mut files = Vec::new();

    for p in &cmd.params {
        if p.location == ParamLocation::Path {
            if let Some(val) = parsed.values.get(&p.original_name) {
                let s = value_to_string(val);
                path = path.replace(&format!("{{{}}}", p.original_name), &s);
            }
        }
    }

    let method = cmd.method.as_deref().unwrap_or("get").to_lowercase();

    if method == "get" {
        for p in &cmd.params {
            let Some(val) = parsed.values.get(&p.original_name) else {
                continue;
            };
            match p.location {
                ParamLocation::Query => {
                    query.insert(p.original_name.clone(), val.clone());
                }
                ParamLocation::Header => {
                    headers.insert(p.original_name.clone(), value_to_string(val));
                }
                _ => {}
            }
        }
    } else if parsed.stdin {
        body = Some(read_stdin_json("OpenAPI request body")?);
        for p in &cmd.params {
            if p.location == ParamLocation::Query {
                if let Some(val) = parsed.values.get(&p.original_name) {
                    query.insert(p.original_name.clone(), val.clone());
                }
            }
        }
    } else {
        let mut body_obj = serde_json::Map::new();
        for p in &cmd.params {
            match p.location {
                ParamLocation::Header => {
                    if let Some(val) = parsed.values.get(&p.original_name) {
                        headers.insert(p.original_name.clone(), value_to_string(val));
                    }
                }
                ParamLocation::Path => {}
                ParamLocation::File => {
                    if let Some(val) = parsed.values.get(&p.original_name) {
                        let fp = value_to_string(val);
                        if !Path::new(&fp).is_file() {
                            return Err(Error::runtime(format!("file not found: {fp}")));
                        }
                        files.push((p.original_name.clone(), fp));
                    }
                }
                ParamLocation::Query => {
                    if let Some(val) = parsed.values.get(&p.original_name) {
                        query.insert(p.original_name.clone(), val.clone());
                    }
                }
                ParamLocation::Body | ParamLocation::ToolInput | ParamLocation::GraphqlArg => {
                    if let Some(val) = parsed.values.get(&p.original_name) {
                        body_obj.insert(p.original_name.clone(), val.clone());
                    }
                }
            }
        }
        if !body_obj.is_empty() {
            body = Some(Value::Object(body_obj));
        }
    }

    Ok(OpenApiRequest {
        path,
        query,
        headers,
        body,
        files,
    })
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub fn execute_openapi(
    parsed: &ParsedToolArgs,
    base_url: &str,
    auth_headers: &[(String, String)],
    opts: &OutputOptions,
) -> Result<()> {
    let cmd = &parsed.command;
    let req_parts = collect_openapi_params(cmd, parsed)?;
    let url = format!("{}{}", base_url.trim_end_matches('/'), req_parts.path);
    let method = cmd.method.as_deref().unwrap_or("get").to_uppercase();

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| Error::runtime(e.to_string()))?;

    let mut header_map = HeaderMap::new();
    let is_multipart =
        !req_parts.files.is_empty() || cmd.content_type.as_deref() == Some("multipart/form-data");
    if !is_multipart {
        header_map.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
    }
    for (k, v) in auth_headers {
        header_map.insert(
            HeaderName::from_bytes(k.as_bytes()).map_err(|e| Error::runtime(e.to_string()))?,
            HeaderValue::from_str(v).map_err(|e| Error::runtime(e.to_string()))?,
        );
    }
    for (k, v) in &req_parts.headers {
        header_map.insert(
            HeaderName::from_bytes(k.as_bytes()).map_err(|e| Error::runtime(e.to_string()))?,
            HeaderValue::from_str(v).map_err(|e| Error::runtime(e.to_string()))?,
        );
    }

    let mut request = client.request(
        method
            .parse()
            .map_err(|_| Error::runtime(format!("invalid method: {method}")))?,
        &url,
    );
    request = request.headers(header_map);

    // Query params: stringify values
    let mut pairs: Vec<(String, String)> = Vec::new();
    for (k, v) in &req_parts.query {
        pairs.push((k.clone(), value_to_query(v)));
    }
    if !pairs.is_empty() {
        request = request.query(&pairs);
    }

    let response = if !req_parts.files.is_empty() {
        let mut form = multipart::Form::new();
        if let Some(Value::Object(map)) = &req_parts.body {
            for (k, v) in map {
                form = form.text(k.clone(), value_to_string(v));
            }
        }
        for (field, path) in &req_parts.files {
            let file = File::open(path).map_err(Error::from)?;
            let filename = Path::new(path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("upload")
                .to_string();
            let part = multipart::Part::reader(file)
                .file_name(filename)
                .mime_str("application/octet-stream")
                .map_err(|e| Error::runtime(e.to_string()))?;
            form = form.part(field.clone(), part);
        }
        request.multipart(form).send()
    } else if cmd.content_type.as_deref() == Some("multipart/form-data") {
        // form fields without files
        let mut form = multipart::Form::new();
        if let Some(Value::Object(map)) = &req_parts.body {
            for (k, v) in map {
                form = form.text(k.clone(), value_to_string(v));
            }
        }
        request.multipart(form).send()
    } else if let Some(body) = &req_parts.body {
        request.json(body).send()
    } else {
        request.send()
    };

    let resp = response.map_err(|e| Error::runtime(e.to_string()))?;
    let status = resp.status();
    let bytes = resp.bytes().map_err(|e| Error::runtime(e.to_string()))?;

    if !status.is_success() {
        let text = String::from_utf8_lossy(&bytes);
        return Err(Error::runtime(format!("Error {}: {text}", status.as_u16())));
    }

    if opts.json_output {
        let data = serde_json::from_slice::<Value>(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        output_result(data, opts)?;
        return Ok(());
    }

    if opts.raw {
        use std::io::Write;
        std::io::stdout().write_all(&bytes).map_err(Error::from)?;
        return Ok(());
    }

    match serde_json::from_slice::<Value>(&bytes) {
        Ok(data) => output_result(data, opts)?,
        Err(_) => {
            println!("{}", String::from_utf8_lossy(&bytes));
        }
    }
    Ok(())
}

fn value_to_query(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(v).unwrap_or_else(|_| v.to_string())
        }
        Value::Null => String::new(),
    }
}

/// Resolve base URL from --base-url, servers[], or spec URL origin.
pub fn resolve_base_url(explicit: Option<&str>, spec: &Value, spec_source: &str) -> Result<String> {
    if let Some(u) = explicit {
        return Ok(u.to_string());
    }
    let mut base = spec
        .get("servers")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .and_then(|s| s.get("url"))
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();

    if base.is_empty() || !base.starts_with("http") {
        if spec_source.starts_with("http://") || spec_source.starts_with("https://") {
            let url =
                reqwest::Url::parse(spec_source).map_err(|e| Error::runtime(e.to_string()))?;
            let origin = format!(
                "{}://{}",
                url.scheme(),
                url.host_str().unwrap_or("localhost")
            );
            let origin = if let Some(port) = url.port() {
                format!("{origin}:{port}")
            } else {
                origin
            };
            if !base.is_empty() && !base.starts_with("http") {
                base = format!("{origin}{base}");
            } else {
                base = origin;
            }
        } else if base.is_empty() {
            return Err(Error::runtime("cannot determine base URL. Use --base-url."));
        }
    }
    Ok(base)
}
