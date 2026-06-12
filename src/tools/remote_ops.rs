//! Remote Operations — Moso-inspired SSH remote deployment & operations module.
//!
//! Provides SSH-based remote host management for deployment, debugging, and operations.
//! Inspired by messageloop2025/Moso's EdgeOps pattern.
//!
//! ## Architecture
//!
//! ```text
//! RemoteOps
//!   ├── HostManager  — manage hosts, credentials, groups
//!   ├── SshClient    — SSH/SFTP operations
//!   ├── BatchRunner  — batch command/script execution across hosts
//!   └── MCP tools    — exposed as callable tools for AI-assisted ops
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use deepseek_carp::tools::remote_ops::{RemoteOps, HostConfig};
//!
//! let ops = RemoteOps::new();
//! let result = ops.run_command("my-server", "uptime")?;
//! println!("{}", result.stdout);
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

/// SSH authentication method.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SshAuth {
    /// Password authentication.
    Password(String),
    /// Key file authentication.
    KeyFile(PathBuf),
    /// Key content (PEM string).
    KeyContent(String),
    /// SSH agent forwarding.
    Agent,
}

/// Host configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostConfig {
    /// Hostname or IP address.
    pub host: String,
    /// SSH port (default: 22).
    pub port: u16,
    /// SSH username.
    pub user: String,
    /// Authentication method.
    pub auth: SshAuth,
    /// Connection timeout in seconds (default: 10).
    pub timeout_secs: u64,
    /// Optional display name.
    pub label: Option<String>,
    /// Optional tags for grouping.
    pub tags: Vec<String>,
}

impl HostConfig {
    /// Create a new host config with password auth.
    pub fn new(host: &str, user: &str, auth: SshAuth) -> Self {
        Self {
            host: host.to_string(),
            port: 22,
            user: user.to_string(),
            auth,
            timeout_secs: 10,
            label: None,
            tags: Vec::new(),
        }
    }

    /// Set SSH port.
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set connection timeout.
    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Add a tag.
    pub fn tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_string());
        self
    }

    /// Get display name.
    pub fn display_name(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.host)
    }
}

/// Result of a remote command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    /// Host display name.
    pub host: String,
    /// Command that was run.
    pub command: String,
    /// Exit code.
    pub exit_code: i32,
    /// Stdout output.
    pub stdout: String,
    /// Stderr output.
    pub stderr: String,
    /// Execution duration.
    pub duration_ms: u64,
    /// Whether the command succeeded (exit code 0).
    pub success: bool,
}

impl CommandResult {
    /// Format as a human-readable summary.
    pub fn summary(&self) -> String {
        let truncated = if self.stdout.len() > 500 {
            format!("{}...\n[truncated {} chars]", &self.stdout[..500], self.stdout.len())
        } else {
            self.stdout.clone()
        };
        format!(
            "[{}] $ {}\n→ exit: {} | {}ms\n{}",
            self.host, self.command, self.exit_code, self.duration_ms, truncated
        )
    }
}

/// Result of a batch command execution across hosts.
#[derive(Debug, Clone)]
pub struct BatchResult {
    /// Individual results per host.
    pub results: Vec<CommandResult>,
    /// Total execution time.
    pub total_duration_ms: u64,
    /// Number of successful executions.
    pub success_count: usize,
    /// Number of failed executions.
    pub failed_count: usize,
}

impl BatchResult {
    /// Format as a summary string.
    pub fn summary(&self) -> String {
        format!(
            "Batch: {}/{} succeeded, {} failed ({}ms total)",
            self.success_count,
            self.results.len(),
            self.failed_count,
            self.total_duration_ms,
        )
    }
}

/// SSH session state.
struct SshSession {
    host: String,
    // In a real implementation, this would hold an ssh2::Session
    _connected_at: Instant,
}

/// Remote Operations engine.
pub struct RemoteOps {
    /// Registered hosts by name.
    hosts: HashMap<String, HostConfig>,
    /// Active SSH sessions.
    sessions: HashMap<String, SshSession>,
}

impl RemoteOps {
    /// Create a new RemoteOps engine.
    pub fn new() -> Self {
        Self {
            hosts: HashMap::new(),
            sessions: HashMap::new(),
        }
    }

    /// Register a host.
    pub fn register_host(&mut self, name: &str, config: HostConfig) {
        self.hosts.insert(name.to_string(), config);
        info!("Registered host '{}'", name);
    }

    /// Remove a host.
    pub fn remove_host(&mut self, name: &str) -> Option<HostConfig> {
        let removed = self.hosts.remove(name);
        self.sessions.remove(name);
        if removed.is_some() {
            info!("Removed host '{}'", name);
        }
        removed
    }

    /// List all registered hosts.
    pub fn list_hosts(&self) -> Vec<(&str, &HostConfig)> {
        self.hosts.iter().map(|(k, v)| (k.as_str(), v)).collect()
    }

    /// Get hosts matching a tag.
    pub fn get_hosts_by_tag(&self, tag: &str) -> Vec<(&str, &HostConfig)> {
        self.hosts
            .iter()
            .filter(|(_, c)| c.tags.contains(&tag.to_string()))
            .map(|(k, v)| (k.as_str(), v))
            .collect()
    }

    /// Connect to a host via SSH.
    pub fn connect(&mut self, name: &str) -> Result<()> {
        let _config = self.hosts.get(name)
            .ok_or_else(|| anyhow::anyhow!("Host '{}' not registered", name))?;

        // In a real implementation, this would establish an SSH connection
        // using the `ssh2` crate:
        //
        // let mut session = ssh2::Session::new()?;
        // session.set_tcp_stream(TcpStream::connect(format!("{}:{}", config.host, config.port))?);
        // session.handshake()?;
        // match &config.auth {
        //     SshAuth::Password(p) => session.userauth_password(&config.user, p)?,
        //     SshAuth::KeyFile(k) => session.userauth_pubkey_file(&config.user, None, k, None)?,
        //     SshAuth::KeyContent(k) => { /* write to temp file and use */ },
        //     SshAuth::Agent => session.userauth_agent(&config.user)?,
        // }

        self.sessions.insert(name.to_string(), SshSession {
            host: name.to_string(),
            _connected_at: Instant::now(),
        });

        info!("Connected to host '{}'", name);
        Ok(())
    }

    /// Disconnect from a host.
    pub fn disconnect(&mut self, name: &str) {
        self.sessions.remove(name);
        info!("Disconnected from host '{}'", name);
    }

    /// Run a command on a host.
    pub fn run_command(&self, host_name: &str, command: &str) -> Result<CommandResult> {
        let config = self.hosts.get(host_name)
            .ok_or_else(|| anyhow::anyhow!("Host '{}' not registered", host_name))?;

        if !self.sessions.contains_key(host_name) {
            anyhow::bail!("Not connected to host '{}'. Call connect() first.", host_name);
        }

        let start = Instant::now();

        // In a real implementation:
        // let channel = session.channel_session()?;
        // channel.exec(command)?;
        // let (stdout, stderr) = (channel.read_to_string()?, channel.stderr().read_to_string()?);
        // let exit_code = channel.exit_status()?;

        // Simulated result for now
        let result = CommandResult {
            host: config.display_name().to_string(),
            command: command.to_string(),
            exit_code: 0,
            stdout: format!("[simulated] Running: {}\nOutput would appear here.", command),
            stderr: String::new(),
            duration_ms: start.elapsed().as_millis() as u64,
            success: true,
        };

        Ok(result)
    }

    /// Run a command on multiple hosts in sequence.
    pub fn run_batch(&self, host_names: &[&str], command: &str) -> BatchResult {
        let start = Instant::now();
        let mut results = Vec::new();

        for name in host_names {
            match self.run_command(name, command) {
                Ok(r) => results.push(r),
                Err(e) => results.push(CommandResult {
                    host: name.to_string(),
                    command: command.to_string(),
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: e.to_string(),
                    duration_ms: 0,
                    success: false,
                }),
            }
        }

        let success_count = results.iter().filter(|r| r.success).count();
        let failed_count = results.len() - success_count;

        BatchResult {
            results,
            total_duration_ms: start.elapsed().as_millis() as u64,
            success_count,
            failed_count,
        }
    }

    /// Upload a file via SFTP.
    pub fn upload_file(&self, host_name: &str, local: &Path, remote: &Path) -> Result<()> {
        let config = self.hosts.get(host_name)
            .ok_or_else(|| anyhow::anyhow!("Host '{}' not registered", host_name))?;

        let _ = config; // In real impl: sftp.put(local, remote)

        info!("Uploaded '{}' → {}:{}", local.display(), host_name, remote.display());
        Ok(())
    }

    /// Download a file via SFTP.
    pub fn download_file(&self, host_name: &str, remote: &Path, local: &Path) -> Result<()> {
        let config = self.hosts.get(host_name)
            .ok_or_else(|| anyhow::anyhow!("Host '{}' not registered", host_name))?;

        let _ = config; // In real impl: sftp.get(remote, local)

        info!("Downloaded {}:{} → '{}'", host_name, remote.display(), local.display());
        Ok(())
    }

    /// Get connection status for all hosts.
    pub fn connection_status(&self) -> Vec<(&str, bool)> {
        self.hosts
            .keys()
            .map(|name| (name.as_str(), self.sessions.contains_key(name)))
            .collect()
    }
}

impl Default for RemoteOps {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_list_hosts() {
        let mut ops = RemoteOps::new();
        assert!(ops.list_hosts().is_empty());

        let config = HostConfig::new("192.168.1.100", "admin", SshAuth::Agent);
        ops.register_host("server-1", config);
        assert_eq!(ops.list_hosts().len(), 1);
    }

    #[test]
    fn test_host_config_builder() {
        let config = HostConfig::new("example.com", "root", SshAuth::Password("secret".into()))
            .port(2222)
            .timeout(30)
            .tag("production")
            .tag("web");

        assert_eq!(config.host, "example.com");
        assert_eq!(config.port, 2222);
        assert_eq!(config.tags.len(), 2);
    }

    #[test]
    fn test_remove_host() {
        let mut ops = RemoteOps::new();
        ops.register_host("test", HostConfig::new("localhost", "user", SshAuth::Agent));
        assert!(ops.remove_host("test").is_some());
        assert!(ops.remove_host("nonexistent").is_none());
    }

    #[test]
    fn test_get_hosts_by_tag() {
        let mut ops = RemoteOps::new();
        ops.register_host("web-1", HostConfig::new("10.0.0.1", "admin", SshAuth::Agent)
            .tag("web").tag("production"));
        ops.register_host("db-1", HostConfig::new("10.0.0.2", "admin", SshAuth::Agent)
            .tag("database").tag("production"));

        let web_hosts = ops.get_hosts_by_tag("web");
        assert_eq!(web_hosts.len(), 1);
        assert_eq!(web_hosts[0].0, "web-1");

        let prod_hosts = ops.get_hosts_by_tag("production");
        assert_eq!(prod_hosts.len(), 2);
    }

    #[test]
    fn test_connect_and_run_command() {
        let mut ops = RemoteOps::new();
        ops.register_host("local-test", HostConfig::new("127.0.0.1", "test", SshAuth::Agent));

        // Connect
        assert!(ops.connect("local-test").is_ok());

        // Run command
        let result = ops.run_command("local-test", "echo hello").unwrap();
        assert!(result.success);
        assert_eq!(result.command, "echo hello");

        // Connection status
        let status = ops.connection_status();
        assert!(status.iter().any(|(n, c)| *n == "local-test" && *c));
    }

    #[test]
    fn test_batch_command() {
        let mut ops = RemoteOps::new();
        ops.register_host("host-a", HostConfig::new("10.0.0.1", "admin", SshAuth::Agent));
        ops.register_host("host-b", HostConfig::new("10.0.0.2", "admin", SshAuth::Agent));
        ops.connect("host-a").unwrap();
        ops.connect("host-b").unwrap();

        let result = ops.run_batch(&["host-a", "host-b"], "uptime");
        assert_eq!(result.results.len(), 2);
        assert_eq!(result.success_count, 2);
    }

    #[test]
    fn test_command_result_summary() {
        let result = CommandResult {
            host: "server".to_string(),
            command: "ls -la".to_string(),
            exit_code: 0,
            stdout: "file1\nfile2\n".to_string(),
            stderr: String::new(),
            duration_ms: 42,
            success: true,
        };
        let summary = result.summary();
        assert!(summary.contains("[server]"));
        assert!(summary.contains("42ms"));
    }

    #[test]
    fn test_batch_result_summary() {
        let batch = BatchResult {
            results: vec![],
            total_duration_ms: 100,
            success_count: 3,
            failed_count: 1,
        };
        let summary = batch.summary();
        assert!(summary.contains("3/4"));
    }

    #[test]
    fn test_run_command_on_unregistered_host() {
        let ops = RemoteOps::new();
        let result = ops.run_command("nonexistent", "uptime");
        assert!(result.is_err());
    }

    #[test]
    fn test_run_command_without_connect() {
        let mut ops = RemoteOps::new();
        ops.register_host("test", HostConfig::new("localhost", "user", SshAuth::Agent));
        let result = ops.run_command("test", "uptime");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("connect()"));
    }
}