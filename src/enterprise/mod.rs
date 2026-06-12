//! CarpAI Enterprise compute node connector.
//!
//! When enterprise mode is enabled, DeepSeek Carp registers as a compute node
//! in a CarpAI Enterprise cluster via HTTP REST (same protocol as carpai-worker).
//!
//! ## Protocol (compatible with carpai-server Parallax Master)
//!
//! ```text
//! POST /parallax/register    → RegisterWorker RPC
//! POST /parallax/heartbeat   → Heartbeat RPC (every 30s)
//! GET  /parallax/layers      → GetLayerAssignment RPC
//! POST /parallax/complete    → ReportTaskComplete RPC
//! ```
//!
//! Based on `proto/parallax.proto` definitions.

use crate::config::EnterpriseNodeConfig;
use std::sync::Arc;
use tokio::sync::RwLock;

// ============================================================================
// Types (matching parallax.proto message definitions)
// ============================================================================

#[derive(Debug, Clone, serde::Serialize)]
pub struct RegisterRequest {
    pub worker_id: String,
    pub memory_gb: f64,
    pub cpu_cores: u32,
    pub port: u32,
    pub os: String,
    pub mode: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RegisterResponse {
    pub accepted: bool,
    pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HeartbeatRequest {
    pub worker_id: String,
    pub layers_loaded: u64,
    pub tasks_completed: u64,
    pub memory_used_gb: f64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct HeartbeatResponse {
    pub alive: bool,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LayerAssignment {
    pub model_name: String,
    pub total_layers: u32,
    pub assigned_layers: Vec<LayerRange>,
    pub pipeline: Vec<WorkerEndpoint>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct LayerRange {
    pub start_layer: u32,
    pub end_layer: u32,
    pub layer_type: String,
    pub estimated_size_mb: u64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct WorkerEndpoint {
    pub worker_id: String,
    pub grpc_addr: String,
    pub start_layer: u32,
    pub end_layer: u32,
}

// ============================================================================
// Hardware Info
// ============================================================================

#[derive(Debug, Clone)]
pub struct NodeHardwareInfo {
    pub gpu_memory_mb: u64,
    pub gpu_memory_available_mb: u64,
    pub gpu_count: u32,
    pub cpu_cores: u32,
    pub system_ram_mb: u64,
    pub system_ram_available_mb: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeState {
    Disconnected,
    Connecting,
    Connected,
    Draining,
    Error(String),
}

// ============================================================================
// Enterprise Connector
// ============================================================================

pub struct EnterpriseConnector {
    config: EnterpriseNodeConfig,
    state: Arc<RwLock<NodeState>>,
    worker_id: Option<String>,
    hardware: NodeHardwareInfo,
    http_client: reqwest::Client,
    /// Active heartbeat task handle
    heartbeat_handle: Option<tokio::task::JoinHandle<()>>,
}

impl EnterpriseConnector {
    pub fn new(config: EnterpriseNodeConfig) -> anyhow::Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {}", e))?;

        let hardware = Self::detect_hardware(&config);

        Ok(Self {
            config,
            state: Arc::new(RwLock::new(NodeState::Disconnected)),
            worker_id: None,
            hardware,
            http_client,
            heartbeat_handle: None,
        })
    }

    fn detect_hardware(config: &EnterpriseNodeConfig) -> NodeHardwareInfo {
        let sys = sysinfo::System::new_all();
        let total_ram = sys.total_memory() / (1024 * 1024);
        let available_ram = sys.available_memory() / (1024 * 1024);
        let cpu_cores = config.max_cpu_cores.max(sys.cpus().len() as u32);

        // GPU detection simplified
        let (gpu_count, gpu_mem, gpu_avail) = (0u32, 0u64, 0u64);

        NodeHardwareInfo {
            gpu_memory_mb: gpu_mem,
            gpu_memory_available_mb: gpu_avail,
            gpu_count,
            cpu_cores: if cpu_cores == 0 { sys.cpus().len() as u32 } else { cpu_cores },
            system_ram_mb: total_ram,
            system_ram_available_mb: available_ram,
        }
    }

    /// Connect to the enterprise cluster via HTTP REST registration.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let server_url = self.config.server_url.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Enterprise server URL not configured"))?
            .trim_end_matches('/')
            .to_string();

        *self.state.write().await = NodeState::Connecting;

        let worker_id = self.config.node_name.clone()
            .unwrap_or_else(|| format!("deepseek-carp-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("node")));

        let reg_req = RegisterRequest {
            worker_id: worker_id.clone(),
            memory_gb: self.hardware.system_ram_available_mb as f64 / 1024.0,
            cpu_cores: self.hardware.cpu_cores,
            port: 0, // No gRPC server yet
            os: std::env::consts::OS.to_string(),
            mode: "opportunistic".to_string(),
        };

        tracing::info!(
            server=%server_url,
            worker_id=%worker_id,
            mem_gb=reg_req.memory_gb,
            cpu=reg_req.cpu_cores,
            "Registering with enterprise cluster"
        );

        // POST /parallax/register
        let resp: RegisterResponse = self.http_client
            .post(format!("{}/parallax/register", server_url))
            .json(&reg_req)
            .send()
            .await?
            .json()
            .await?;

        if !resp.accepted {
            *self.state.write().await = NodeState::Error(resp.message.clone());
            return Err(anyhow::anyhow!("Registration rejected: {}", resp.message));
        }

        self.worker_id = Some(worker_id.clone());
        *self.state.write().await = NodeState::Connected;

        // Start heartbeat loop
        let state = self.state.clone();
        let client = self.http_client.clone();
        let server_clone = server_url.clone();
        let wid = worker_id.clone();

        self.heartbeat_handle = Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                if *state.read().await != NodeState::Connected {
                    break;
                }

                let hb = HeartbeatRequest {
                    worker_id: wid.clone(),
                    layers_loaded: 0,
                    tasks_completed: 0,
                    memory_used_gb: 0.0,
                };

                match client
                    .post(format!("{}/parallax/heartbeat", server_clone))
                    .json(&hb)
                    .send()
                    .await
                {
                    Ok(resp) => {
                        if let Ok(hb_resp) = resp.json::<HeartbeatResponse>().await {
                            if !hb_resp.alive {
                                tracing::warn!("Server reports node as dead, reconnecting...");
                                // Would trigger reconnect logic here
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error=%e, "Heartbeat failed");
                    }
                }
            }
        }));

        tracing::info!(worker_id=%worker_id, server=%server_url, "Connected to enterprise cluster");
        Ok(())
    }

    /// Disconnect from the enterprise cluster.
    pub async fn disconnect(&mut self) -> anyhow::Result<()> {
        *self.state.write().await = NodeState::Draining;

        if let Some(handle) = self.heartbeat_handle.take() {
            handle.abort();
        }

        self.worker_id = None;
        *self.state.write().await = NodeState::Disconnected;
        tracing::info!("Disconnected from enterprise cluster");
        Ok(())
    }

    /// Get current node state.
    pub async fn state(&self) -> NodeState {
        self.state.read().await.clone()
    }

    /// Get hardware info.
    pub fn hardware(&self) -> &NodeHardwareInfo {
        &self.hardware
    }

    /// Get worker ID (if connected).
    pub fn worker_id(&self) -> Option<&str> {
        self.worker_id.as_deref()
    }

    /// Check if enterprise mode is active.
    pub fn is_enabled(&self) -> bool {
        true // Called only if feature enabled and mode=enterprise
    }

    /// Check if connected.
    pub async fn is_connected(&self) -> bool {
        matches!(*self.state.read().await, NodeState::Connected)
    }

    /// Attempt to reconnect after connection loss.
    /// Uses exponential backoff: 1s → 2s → 4s → 8s → 16s (max).
    pub async fn reconnect_with_backoff(&mut self, max_attempts: u32) -> anyhow::Result<()> {
        let mut delay = std::time::Duration::from_secs(1);

        for attempt in 1..=max_attempts {
            tracing::info!(attempt, "Enterprise: reconnection attempt");
            match self.connect().await {
                Ok(()) => {
                    tracing::info!("Enterprise: reconnected successfully");
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!(attempt, error=%e, delay_ms=delay.as_millis(), "Enterprise: reconnect failed, retrying");
                    if attempt < max_attempts {
                        tokio::time::sleep(delay).await;
                        delay = (delay * 2).min(std::time::Duration::from_secs(30));
                    }
                }
            }
        }
        Err(anyhow::anyhow!("Failed to reconnect after {} attempts", max_attempts))
    }
}
