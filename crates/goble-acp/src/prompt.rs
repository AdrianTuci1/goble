//! Prompt content-block handling.
//!
//! ACP prompts are arrays of content blocks. The gateway's run API takes a
//! single text task, so the bridge renders every supported block to text and
//! rejects the ones it cannot honestly carry. The sandbox is the only place
//! code and files live, so there is no local attachment upload.

use serde_json::Value;

/// Why a prompt could not be rendered to text. Every variant becomes a
/// JSON-RPC `-32602` (invalid params) with a client-readable message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PromptError {
    #[error("prompt is empty: send at least one text content block")]
    Empty,
    #[error("prompt block {index} is not an object")]
    NotAnObject { index: usize },
    #[error("prompt block {index} has unsupported type {kind:?}")]
    UnsupportedType { index: usize, kind: String },
    #[error("prompt block {index} is missing text")]
    MissingText { index: usize },
    #[error(
        "prompt block {index} is an embedded resource without inline text; \
         blob resources and image/audio attachments are not carried to the gateway"
    )]
    UnsupportedResource { index: usize },
    #[error("prompt must be an array of content blocks")]
    NotAnArray,
}

/// Render ACP prompt content blocks into the single text task the gateway
/// receives.
///
/// Supported:
/// - `text` — passed through verbatim;
/// - `resource_link` — rendered as `[resource_link name="…" uri="…"]`;
/// - `resource` with inline `text` — its text is embedded, prefixed with the
///   resource URI so the agent knows where it came from.
///
/// Refused with [`PromptError::UnsupportedType`]: `image`, `audio`, and any
/// unknown block, because the gateway prompt API is text-only. This is
/// advertised honestly in `initialize` (`promptCapabilities.image/audio:
/// false`).
pub fn render_prompt(blocks: &Value) -> Result<String, PromptError> {
    let blocks = blocks.as_array().ok_or(PromptError::NotAnArray)?;
    let mut rendered = Vec::with_capacity(blocks.len());
    for (index, block) in blocks.iter().enumerate() {
        let object = block
            .as_object()
            .ok_or(PromptError::NotAnObject { index })?;
        let kind =
            object
                .get("type")
                .and_then(Value::as_str)
                .ok_or(PromptError::UnsupportedType {
                    index,
                    kind: "<missing>".to_string(),
                })?;
        match kind {
            "text" => {
                let text = object
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or(PromptError::MissingText { index })?;
                rendered.push(text.to_string());
            }
            "resource_link" => {
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| object.get("title").and_then(Value::as_str))
                    .unwrap_or("resource");
                let uri = object
                    .get("uri")
                    .and_then(Value::as_str)
                    .ok_or(PromptError::MissingText { index })?;
                rendered.push(format!("[resource_link name={name:?} uri={uri:?}]"));
            }
            "resource" => {
                let resource = object
                    .get("resource")
                    .ok_or(PromptError::UnsupportedResource { index })?;
                let uri = resource
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or("resource");
                let text = resource
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or(PromptError::UnsupportedResource { index })?;
                rendered.push(format!("[resource uri={uri:?}]\n{text}"));
            }
            other => {
                return Err(PromptError::UnsupportedType {
                    index,
                    kind: other.to_string(),
                })
            }
        }
    }
    rendered.retain(|part| !part.trim().is_empty());
    if rendered.is_empty() {
        return Err(PromptError::Empty);
    }
    Ok(rendered.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plain_text_is_passed_through() {
        let blocks = json!([{"type": "text", "text": "run the tests"}]);
        assert_eq!(render_prompt(&blocks).unwrap(), "run the tests");
    }

    #[test]
    fn multiple_text_blocks_are_joined() {
        let blocks = json!([
            {"type": "text", "text": "first"},
            {"type": "text", "text": "second"},
        ]);
        assert_eq!(render_prompt(&blocks).unwrap(), "first\n\nsecond");
    }

    #[test]
    fn resource_links_become_text() {
        let blocks = json!([{
            "type": "resource_link",
            "name": "notes.md",
            "uri": "file:///workspace/notes.md",
        }]);
        assert_eq!(
            render_prompt(&blocks).unwrap(),
            "[resource_link name=\"notes.md\" uri=\"file:///workspace/notes.md\"]"
        );
    }

    #[test]
    fn embedded_text_resources_are_inlined() {
        let blocks = json!([{
            "type": "resource",
            "resource": {"uri": "goble://context/1", "text": "hello from a resource"},
        }]);
        assert_eq!(
            render_prompt(&blocks).unwrap(),
            "[resource uri=\"goble://context/1\"]\nhello from a resource"
        );
    }

    #[test]
    fn image_and_audio_blocks_are_refused() {
        for kind in ["image", "audio"] {
            let blocks = json!([{"type": kind, "data": "...", "mimeType": "image/png"}]);
            let error = render_prompt(&blocks).expect_err("attachments are not carried");
            assert_eq!(
                error,
                PromptError::UnsupportedType {
                    index: 0,
                    kind: kind.to_string()
                }
            );
        }
    }

    #[test]
    fn blob_resources_are_refused() {
        let blocks = json!([{
            "type": "resource",
            "resource": {"uri": "file:///x.bin", "blob": "AAAA"},
        }]);
        assert_eq!(
            render_prompt(&blocks).expect_err("blob is unsupported"),
            PromptError::UnsupportedResource { index: 0 }
        );
    }

    #[test]
    fn empty_prompts_are_refused() {
        assert_eq!(render_prompt(&json!([])).unwrap_err(), PromptError::Empty);
        assert_eq!(
            render_prompt(&json!([{"type": "text", "text": "   "}])).unwrap_err(),
            PromptError::Empty
        );
    }

    #[test]
    fn malformed_blocks_are_refused() {
        assert_eq!(
            render_prompt(&json!("not an array")).unwrap_err(),
            PromptError::NotAnArray
        );
        assert_eq!(
            render_prompt(&json!(["nope"])).unwrap_err(),
            PromptError::NotAnObject { index: 0 }
        );
        assert_eq!(
            render_prompt(&json!([{"type": "text"}])).unwrap_err(),
            PromptError::MissingText { index: 0 }
        );
    }
}
