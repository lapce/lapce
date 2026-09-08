//! CarpAI Enterprise gRPC client.
//!
//! Connects to a CarpAI Enterprise cluster via gRPC for distributed
//! inference. This is used when `inference_mode = "enterprise"`.
//!
//! ## Protocol
//! - Register as a compute node (worker)
//! - Receive layer assignments for model sharding
//! - Report task completion
//! - Heartbeat with master

use serde::{Deserialize, Serialize};

/// CarpAI Enterprise gRPC client (HTTP REST fallback).
///
/// Uses HTTP REST as primary transport with gRPC-compatible message
/// serialization when the `enterprise` feature is enabled with tonic.
pub struct CarpAiGrpcClient {
    server_url: String,
    auth_token: Option<String>,
    client: reqwest::Client,
}

/// Node registration info.
#[derive(Debug, Clone, Serialize)]
pub struct NodeInfo {
    pub node_name: String,
    pub gpu_memory_mb: u64,
    pub cpu_cores: u32,
    pub system_ram_mb: u64,
    pub os: String,
}

/// Inference task sent from master to worker.
#[derive(Debug, Clone, Deserialize)]
pub struct InferenceTask {
    pub task_id: String,
    pub model_name: String,
    pub prompt: String,
    pub max_tokens: u32,
    pub temperature: f64,
    pub start_layer: u32,
    pub end_layer: u32,
}

/// Task result sent from worker to master.
#[derive(Debug, Clone, Serialize)]
pub struct TaskResult {
    pub task_id: String,
    pub success: bool,
    pub output: Option<String>,
    pub error: Option<String>,
    pub layer_outputs: Vec<f32>,
    pub latency_ms: u64,
}

impl CarpAiGrpcClient {
    /// Create a new CarpAI gRPC client.
    pub fn new(server_url: impl Into<String>, auth_token: Option<String>) -> Self {
        Self {
            server_url: server_url.into(),
            auth_token,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    /// Register this node with the enterprise cluster.
    pub async fn register(&self, info: &NodeInfo) -> Result<String, String> {
        let mut builder = self.client
            .post(format!("{}/parallax/register", self.server_url))
            .json(info);

        if let Some(ref token) = self.auth_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let resp = builder.send().await.map_err(|e| format!("Register error: {}", e))?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Registration failed: {}", body));
        }

        Ok(info.node_name.clone())
    }

    /// Send heartbeat to keep registration alive.
    pub async fn heartbeat(&self, node_id: &str) -> Result<(), String> {
        let mut builder = self.client
            .post(format!("{}/parallax/heartbeat", self.server_url))
            .json(&serde_json::json!({ "worker_id": node_id }));

        if let Some(ref token) = self.auth_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let resp = builder.send().await.map_err(|e| format!("Heartbeat error: {}", e))?;
        if !resp.status().is_success() {
            return Err("Heartbeat failed".into());
        }
        Ok(())
    }

    /// Report a completed task result to the master.
    pub async fn report_task(&self, result: &TaskResult) -> Result<(), String> {
        let mut builder = self.client
            .post(format!("{}/parallax/complete", self.server_url))
            .json(result);

        if let Some(ref token) = self.auth_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let resp = builder.send().await.map_err(|e| format!("Report error: {}", e))?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Task report failed: {}", body));
        }
        Ok(())
    }

    /// Disconnect from the enterprise cluster.
    pub async fn disconnect(&self, node_id: &str) -> Result<(), String> {
        let mut builder = self.client
            .post(format!("{}/parallax/disconnect", self.server_url))
            .json(&serde_json::json!({ "worker_id": node_id }));

        if let Some(ref token) = self.auth_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let _ = builder.send().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_info_serialization() {
        let info = NodeInfo {
            node_name: "test-node".into(),
            gpu_memory_mb: 8192,
            cpu_cores: 8,
            system_ram_mb: 32768,
            os: "linux".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("test-node"));
    }

    #[test]
    fn test_task_result_serialization() {
        let result = TaskResult {
            task_id: "task-1".into(),
            success: true,
            output: Some("Hello".into()),
            error: None,
            layer_outputs: vec![0.1, 0.2],
            latency_ms: 150,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("task-1"));
    }
}
