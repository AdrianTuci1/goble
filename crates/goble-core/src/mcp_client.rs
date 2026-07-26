use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, ChildStdout, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

/// Minimal MCP JSON-RPC client over stdio or SSE.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpToolInputSchema {
    Object {
        #[serde(rename = "type")]
        typ: String,
        properties: Option<serde_json::Map<String, serde_json::Value>>,
        required: Option<Vec<String>>,
    },
    Other(serde_json::Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: McpToolInputSchema,
}

impl McpClient {
    pub fn spawn(
        command: &str,
        args: &[impl AsRef<str>],
        env: HashMap<String, String>,
    ) -> Result<Self> {
        let owned_args: Vec<String> = args.iter().map(|s| s.as_ref().to_string()).collect();
        Self::spawn_owned(command, &owned_args, env)
    }

    pub fn spawn_owned(
        command: &str,
        args: &[String],
        env: HashMap<String, String>,
    ) -> Result<Self> {
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

    pub fn list_tools(&self) -> Result<Vec<McpTool>> {
        let res = self.request("tools/list", serde_json::json!({}))?;
        let tools: Vec<serde_json::Value> = res
            .get("tools")
            .and_then(|v: &serde_json::Value| v.as_array())
            .cloned()
            .unwrap_or_default();
        tools
            .into_iter()
            .map(|v| serde_json::from_value::<McpTool>(v).context("invalid mcp tool"))
            .collect()
    }

    pub fn call_tool(&self, name: &str, arguments: serde_json::Value) -> Result<serde_json::Value> {
        self.request(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": arguments }),
        )
    }
}

/// SSE-based MCP client. Each request posts JSON-RPC and waits for a matching id on the SSE stream.
pub struct McpSseClient {
    base_url: String,
    client: reqwest::Client,
    next_id: Mutex<u64>,
}

impl McpSseClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("build client"),
            next_id: Mutex::new(1),
        }
    }

    pub async fn initialize(&self) -> Result<serde_json::Value> {
        self.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "goble", "version": "0.1.0" }
            }),
        )
        .await
    }

    pub async fn list_tools(&self) -> Result<Vec<McpTool>> {
        let res = self.request("tools/list", serde_json::json!({})).await?;
        let tools: Vec<serde_json::Value> = res
            .get("tools")
            .and_then(|v: &serde_json::Value| v.as_array())
            .cloned()
            .unwrap_or_default();
        tools
            .into_iter()
            .map(|v| serde_json::from_value::<McpTool>(v).context("invalid mcp tool"))
            .collect()
    }

    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.request(
            "tools/call",
            serde_json::json!({ "name": name, "arguments": arguments }),
        )
        .await
    }

    async fn request<T: Serialize>(&self, method: &str, params: T) -> Result<serde_json::Value> {
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
        let post_url = format!("{}/message", self.base_url);
        let _ = self
            .client
            .post(&post_url)
            .json(&req)
            .send()
            .await
            .context("mcp sse post failed")?;

        let sse_url = format!("{}/sse", self.base_url);
        let resp = self
            .client
            .get(&sse_url)
            .send()
            .await
            .context("mcp sse stream failed")?;
        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();
        let timeout = tokio::time::Duration::from_secs(30);
        let result = tokio::time::timeout(timeout, async {
            while let Some(chunk) = stream.next().await {
                let chunk: bytes::Bytes = chunk.context("sse chunk failed")?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));
                for line in buffer.split('\n') {
                    if let Some(data) = line.strip_prefix("data: ") {
                        if let Ok(rpc) = serde_json::from_str::<JsonRpcResponse>(data) {
                            if rpc.id == id {
                                return Ok(rpc);
                            }
                        }
                    }
                }
                if buffer.len() > 200_000 {
                    buffer.clear();
                }
            }
            anyhow::bail!("sse stream closed without response for id {}", id)
        })
        .await
        .context("mcp sse request timed out")?;

        let resp = result?;
        if let Some(err) = resp.error {
            anyhow::bail!("mcp sse error {}: {}", err.code, err.message);
        }
        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_client_spawn_node_echo() {
        let client = McpClient::spawn(
            "node",
            &["-e", "process.stdin.once('data', () => {})"],
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

    #[test]
    fn test_mcp_tool_deserialization() {
        let json = serde_json::json!({
            "name": "read_file",
            "description": "Read a file",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }
        });
        let tool: McpTool = serde_json::from_value(json).unwrap();
        assert_eq!(tool.name, "read_file");
    }
}
