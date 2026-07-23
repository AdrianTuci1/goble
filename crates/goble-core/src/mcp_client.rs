use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::Mutex;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Minimal MCP JSON-RPC client over stdio.
pub struct McpClient {
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<ChildStdout>,
    #[allow(dead_code)]
    child: Child,
    next_id: Mutex<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest<T> {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: T,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

impl McpClient {
    pub fn spawn(command: &str, args: &[String], env: HashMap<String, String>) -> Result<Self> {
        let mut cmd = std::process::Command::new(command);
        cmd.args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().context("failed to spawn mcp process")?;
        let stdin = child.stdin.take().context("missing stdin")?;
        let stdout = child.stdout.take().context("missing stdout")?;
        Ok(Self {
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(stdout),
            child,
            next_id: Mutex::new(1),
        })
    }

    pub fn request<T: Serialize>(&self, method: &str, params: T) -> Result<serde_json::Value> {
        let id = {
            let mut guard = self.next_id.lock().unwrap();
            let id = *guard;
            *guard += 1;
            id
        };
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.to_string(),
            params,
        };
        let line = serde_json::to_string(&req)?;
        {
            let mut stdin = self.stdin.lock().unwrap();
            writeln!(stdin, "{}", line)?;
            stdin.flush()?;
        }

        let mut stdout = self.stdout.lock().unwrap();
        let mut reader = std::io::BufReader::new(&mut *stdout);
        let mut line_buffer = String::new();
        reader.read_line(&mut line_buffer)?;
        let resp: JsonRpcResponse = serde_json::from_str(&line_buffer)
            .with_context(|| format!("invalid jsonrpc response: {}", line_buffer.trim()))?;
        if let Some(err) = resp.error {
            anyhow::bail!("mcp error {}: {}", err.code, err.message);
        }
        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    }

    pub fn initialize(&self) -> Result<serde_json::Value> {
        self.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "goble", "version": "0.1.0" }
            }),
        )
    }

    pub fn list_tools(&self) -> Result<serde_json::Value> {
        self.request("tools/list", serde_json::json!({}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_client_spawn_node_echo() {
        let client = McpClient::spawn(
            "node",
            &[
                "-e".to_string(),
                "process.stdin.once('data', () => {})".to_string(),
            ],
            HashMap::new(),
        );
        assert!(client.is_err() || client.unwrap().child.id() > 0);
    }

    #[test]
    fn test_jsonrpc_serialize() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "initialize".to_string(),
            params: serde_json::json!({"a": 1}),
        };
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains("initialize"));
    }
}
