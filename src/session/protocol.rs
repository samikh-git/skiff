//! NDJSON IPC protocol for session daemons.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMethod {
    ListTools,
    GetTool,
    CallTool,
    ListResources,
    ReadResource,
    ListResourceTemplates,
    ListPrompts,
    GetPrompt,
}

impl SessionMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ListTools => "list_tools",
            Self::GetTool => "get_tool",
            Self::CallTool => "call_tool",
            Self::ListResources => "list_resources",
            Self::ReadResource => "read_resource",
            Self::ListResourceTemplates => "list_resource_templates",
            Self::ListPrompts => "list_prompts",
            Self::GetPrompt => "get_prompt",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "list_tools" => Self::ListTools,
            "get_tool" => Self::GetTool,
            "call_tool" => Self::CallTool,
            "list_resources" => Self::ListResources,
            "read_resource" => Self::ReadResource,
            "list_resource_templates" => Self::ListResourceTemplates,
            "list_prompts" => Self::ListPrompts,
            "get_prompt" => Self::GetPrompt,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRequest {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SessionResponse {
    pub fn ok(id: u64, result: Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: u64, error: impl Into<String>) -> Self {
        Self {
            id,
            result: None,
            error: Some(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn roundtrip() {
        let req = SessionRequest {
            id: 1,
            method: "list_tools".into(),
            params: json!({"refresh": true}),
        };
        let line = serde_json::to_string(&req).unwrap();
        let back: SessionRequest = serde_json::from_str(&line).unwrap();
        assert_eq!(back.method, "list_tools");
        assert_eq!(
            SessionMethod::parse(&back.method),
            Some(SessionMethod::ListTools)
        );
    }
}
