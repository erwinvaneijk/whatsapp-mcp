//! Tool results in the shape mcp 2.x (the Python SDK) gives them, so clients
//! see the same thing:
//!
//! - **Text content:** a string as it is; an object as pretty-printed JSON
//!   (fields in declared order); a list as one text block per item; `None`
//!   and an empty list as no content at all.
//! - **Structured content:** the result wrapped as `{"result": …}`, except a
//!   plain object type (such as a message context), which is given as is.

use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;
use serde_json::{Value, json};

/// A tool failed; the message says why and goes to the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError(pub String);

/// A successful tool result.
pub struct Output {
    content: Vec<String>,
    structured: Value,
}

impl Output {
    /// A text result (`-> str`).
    pub fn text(text: String) -> Result<Output, ToolError> {
        Ok(Output {
            content: vec![text.clone()],
            structured: json!({ "result": text }),
        })
    }

    /// An optional result (`-> Chat | None`, `-> str | None`).
    pub fn optional<T: Serialize>(value: Option<T>) -> Result<Output, ToolError> {
        match value {
            None => Ok(Output {
                content: Vec::new(),
                structured: json!({ "result": null }),
            }),
            Some(value) => {
                let value = to_value(&value)?;
                Ok(Output {
                    content: vec![content_text(&value)?],
                    structured: json!({ "result": value }),
                })
            }
        }
    }

    /// A list result (`-> list[Chat]`).
    pub fn list<T: Serialize>(items: impl IntoIterator<Item = T>) -> Result<Output, ToolError> {
        let values = items
            .into_iter()
            .map(|item| to_value(&item))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Output {
            content: values.iter().map(content_text).collect::<Result<_, _>>()?,
            structured: json!({ "result": values }),
        })
    }

    /// A plain object result (`-> MessageContext`), not wrapped.
    pub fn object<T: Serialize>(value: &T) -> Result<Output, ToolError> {
        let value = to_value(value)?;
        Ok(Output {
            content: vec![content_text(&value)?],
            structured: value,
        })
    }

    pub fn into_result(self) -> CallToolResult {
        let content = self.content.into_iter().map(ContentBlock::text).collect();
        let mut result = CallToolResult::success(content);
        result.structured_content = Some(self.structured);
        result
    }
}

impl ToolError {
    pub fn into_result(self) -> CallToolResult {
        CallToolResult::error(vec![ContentBlock::text(self.0)])
    }
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, ToolError> {
    serde_json::to_value(value).map_err(|e| ToolError(format!("Internal error: {e}")))
}

/// How one item appears as text content: a string as it is, anything else
/// as JSON with two-space indentation, as pydantic writes it.
fn content_text(value: &Value) -> Result<String, ToolError> {
    match value {
        Value::String(s) => Ok(s.clone()),
        other => serde_json::to_string_pretty(other)
            .map_err(|e| ToolError(format!("Internal error: {e}"))),
    }
}
