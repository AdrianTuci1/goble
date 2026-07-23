use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::agent::{McpManifest, McpRuntime, McpServer, McpSource};
use crate::mcp_client::McpClient;

/// Installs an MCP server into a local cache directory.
#[derive(Debug, Clone)]
pub struct McpInstaller {
    pub cache_dir: PathBuf,
}

impl McpInstaller {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
        }
    }

    pub fn install_path(&self, id: &str) -> PathBuf {
        self.cache_dir.join("mcp").join(id)
    }

    pub fn is_installed(&self, id: &str) -> bool {
        self.install_path(id).join(".installed").exists()
    }

    pub async fn install(&self, server: &McpServer) -> Result<InstalledMcp> {
        let dest = self.install_path(&server.id);
        std::fs::create_dir_all(&dest)?;

        match &server.source {
            McpSource::Github { repo, rev } => {
                let url = format!("https://github.com/{}/archive/{}.tar.gz", repo, rev);
                let archive = dest.join("source.tar.gz");
                download_file(&url, &archive).await?;
                extract_tarball(&archive, &dest)?;
            }
            McpSource::Npm { package, version } => {
                let mut cmd = tokio::process::Command::new("npm");
                cmd.arg("install")
                    .arg(format!("{}@{}", package, version))
                    .arg("--prefix")
                    .arg(&dest)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                let status: std::process::ExitStatus = cmd.status().await?;
                if !status.success() {
                    anyhow::bail!("npm install failed for {}@{}", package, version);
                }
            }
            McpSource::Local { path } => {
                let src = PathBuf::from(path);
                if !src.exists() {
                    anyhow::bail!("local MCP source does not exist: {}", path);
                }
                copy_dir(&src, &dest)?;
            }
            McpSource::Url { url } => {
                let archive = dest.join("source.tar.gz");
                download_file(url, &archive).await?;
                extract_tarball(&archive, &dest)?;
            }
        }

        std::fs::write(dest.join(".installed"), server.id.clone())?;

        Ok(InstalledMcp {
            id: server.id.clone(),
            path: dest,
            manifest: server.manifest.clone(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledMcp {
    pub id: String,
    pub path: PathBuf,
    pub manifest: McpManifest,
}

impl InstalledMcp {
    /// Build the command + args to run this MCP server natively.
    pub fn runtime_command(&self) -> (String, Vec<String>) {
        match &self.manifest.runtime {
            McpRuntime::Binary { command, args } => (command.clone(), args.clone()),
            McpRuntime::V8Isolate => (
                "node".to_string(),
                vec![self
                    .path
                    .join(&self.manifest.entrypoint)
                    .to_string_lossy()
                    .to_string()],
            ),
        }
    }

    /// Starts the MCP server as a persistent JSON-RPC stdio client.
    pub fn start_client(&self, env: HashMap<String, String>) -> Result<McpClient> {
        let (command, args) = self.runtime_command();
        let mut resolved_args = args;
        if command == "npx" {
            resolved_args.insert(0, "-y".to_string());
        }
        McpClient::spawn(&command, &resolved_args, env).with_context(|| {
            format!(
                "failed to start mcp client for {} from {}",
                self.id,
                self.path.display()
            )
        })
    }

    /// Runs the MCP server in a containerized sandbox if Docker is available,
    /// otherwise falls back to the host runtime described in the manifest.
    pub async fn execute(
        &self,
        args: Vec<String>,
        env: HashMap<String, String>,
    ) -> Result<std::process::Output> {
        let has_docker = docker_available().await;
        if has_docker {
            self.run_in_container(args, env).await
        } else {
            self.run_host(args, env).await
        }
    }

    async fn run_in_container(
        &self,
        args: Vec<String>,
        env: HashMap<String, String>,
    ) -> Result<std::process::Output> {
        let mut cmd = tokio::process::Command::new("docker");
        cmd.arg("run")
            .arg("--rm")
            .arg("-v")
            .arg(format!("{}:/mcp", self.path.display()))
            .arg("-w")
            .arg("/mcp");
        for (k, v) in env {
            cmd.arg("-e").arg(format!("{}={}", k, v));
        }
        cmd.arg("node:20-alpine");
        match &self.manifest.runtime {
            McpRuntime::Binary {
                command,
                args: manifest_args,
            } => {
                cmd.arg(command);
                for a in manifest_args {
                    cmd.arg(a);
                }
            }
            McpRuntime::V8Isolate => {
                cmd.arg("node").arg(&self.manifest.entrypoint);
            }
        }
        for a in args {
            cmd.arg(a);
        }
        cmd.output().await.context("docker run failed")
    }

    async fn run_host(
        &self,
        args: Vec<String>,
        env: HashMap<String, String>,
    ) -> Result<std::process::Output> {
        let (command, manifest_args) = self.runtime_command();
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(manifest_args)
            .current_dir(&self.path)
            .envs(env)
            .args(args);
        cmd.output().await.context("host execution failed")
    }
}

async fn download_file(url: &str, dest: &Path) -> Result<()> {
    let response = reqwest::get(url).await?;
    let bytes = response.bytes().await?;
    std::fs::write(dest, bytes)?;
    Ok(())
}

fn extract_tarball(archive: &Path, dest: &Path) -> Result<()> {
    let output = std::process::Command::new("tar")
        .args([
            "-xzf",
            &archive.to_string_lossy(),
            "-C",
            &dest.to_string_lossy(),
            "--strip-components=1",
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!(
            "tar extraction failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn copy_dir(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = path.file_name().context("missing file name")?;
        let target = dest.join(file_name);
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

async fn docker_available() -> bool {
    match tokio::process::Command::new("docker")
        .arg("version")
        .output()
        .await
    {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn sample_manifest() -> McpManifest {
        McpManifest {
            schema_version: "1".to_string(),
            entrypoint: "index.js".to_string(),
            runtime: McpRuntime::V8Isolate,
            auth_schema: vec![],
            capabilities: vec!["echo".to_string()],
            config_schema: serde_json::json!({}),
        }
    }

    fn make_server(id: &str, source: McpSource) -> McpServer {
        McpServer {
            id: id.to_string(),
            name: id.to_string(),
            source,
            manifest: sample_manifest(),
            credentials_key: None,
            installed_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_installer_local_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("index.js"), "console.log('hello')").unwrap();

        let server = make_server(
            "local-echo",
            McpSource::Local {
                path: src.to_string_lossy().to_string(),
            },
        );

        let installer = McpInstaller::new(tmp.path().join("cache"));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let installed = rt.block_on(installer.install(&server)).unwrap();
        assert!(installed.path.join("index.js").exists());
        assert!(installer.is_installed("local-echo"));

        // Reinstalling the same local source should overwrite the existing cache
        // and still mark the server as installed.
        let reinstalled = rt.block_on(installer.install(&server)).unwrap();
        assert!(reinstalled.path.join("index.js").exists());
        assert!(installer.is_installed("local-echo"));
    }

    #[test]
    fn test_installer_local_path_missing() {
        let tmp = tempfile::TempDir::new().unwrap();
        let server = make_server(
            "missing",
            McpSource::Local {
                path: tmp
                    .path()
                    .join("does-not-exist")
                    .to_string_lossy()
                    .to_string(),
            },
        );

        let installer = McpInstaller::new(tmp.path().join("cache"));
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(installer.install(&server));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("local MCP source does not exist"));
    }

    #[test]
    fn test_runtime_command_binary() {
        let mcp = InstalledMcp {
            id: "binary-tool".to_string(),
            path: PathBuf::from("/tmp/binary-tool"),
            manifest: McpManifest {
                schema_version: "1".to_string(),
                entrypoint: "unused".to_string(),
                runtime: McpRuntime::Binary {
                    command: "my-binary".to_string(),
                    args: vec!["--foo".to_string(), "bar".to_string()],
                },
                auth_schema: vec![],
                capabilities: vec![],
                config_schema: serde_json::json!({}),
            },
        };
        let (cmd, args) = mcp.runtime_command();
        assert_eq!(cmd, "my-binary");
        assert_eq!(args, vec!["--foo", "bar"]);
    }

    #[test]
    fn test_runtime_command_v8_isolate() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("mcp/test");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("index.js"), "").unwrap();
        let mcp = InstalledMcp {
            id: "test".to_string(),
            path,
            manifest: sample_manifest(),
        };
        let (cmd, args) = mcp.runtime_command();
        assert_eq!(cmd, "node");
        assert_eq!(args.len(), 1);
        assert!(args[0].ends_with("index.js"));
    }

    #[test]
    fn test_execute_falls_back_to_host_when_docker_unavailable() {
        // Uses a host shell that exists everywhere and echoes its args. The
        // manifest describes a Binary runtime so the host path is used directly.
        let tmp = tempfile::TempDir::new().unwrap();
        let dest = tmp.path().join("mcp/fallback");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join(".installed"), "fallback").unwrap();

        let mcp = InstalledMcp {
            id: "fallback".to_string(),
            path: dest,
            manifest: McpManifest {
                schema_version: "1".to_string(),
                entrypoint: "unused".to_string(),
                runtime: McpRuntime::Binary {
                    command: "echo".to_string(),
                    args: vec!["host-fallback".to_string()],
                },
                auth_schema: vec![],
                capabilities: vec![],
                config_schema: serde_json::json!({}),
            },
        };

        // Docker is unavailable in the test environment, so execute should
        // fall back to host execution and still return the expected output.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let output = rt
            .block_on(mcp.execute(vec!["extra".to_string()], HashMap::new()))
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("host-fallback"));
        assert!(stdout.contains("extra"));
    }

    #[test]
    fn test_installed_mcp_serde() {
        let mcp = InstalledMcp {
            id: "echo".to_string(),
            path: PathBuf::from("/tmp/echo"),
            manifest: sample_manifest(),
        };
        let json = serde_json::to_string(&mcp).unwrap();
        assert!(json.contains("echo"));
    }
}
