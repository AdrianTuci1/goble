//! JSON-RPC 2.0 envelopes for MCP over stdio.
//!
//! The server writes exactly one JSON object per line. A request has an `id`
//! (any JSON scalar) and a `method`; a notification has no `id` and gets no
//! reply. Errors use the standard codes below.

use serde_json::{json, Value};

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// The JSON-RPC version string every message carries.
pub const VERSION: &str = "2.0";

/// A successful reply for `id`.
pub fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": VERSION, "id": id, "result": result })
}

/// An error reply for `id`.
pub fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": VERSION,
        "id": id,
        "error": { "code": code, "message": message.into() },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelopes_carry_the_version_and_their_id() {
        let ok = result(json!(3), json!({ "ok": true }));
        assert_eq!(ok["jsonrpc"], "2.0");
        assert_eq!(ok["id"], 3);
        assert_eq!(ok["result"]["ok"], true);

        let failed = error(json!("a"), INVALID_PARAMS, "bad arguments");
        assert_eq!(failed["jsonrpc"], "2.0");
        assert_eq!(failed["id"], "a");
        assert_eq!(failed["error"]["code"], INVALID_PARAMS);
        assert_eq!(failed["error"]["message"], "bad arguments");
    }
}
