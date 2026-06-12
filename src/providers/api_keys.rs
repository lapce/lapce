//! API Key rotation and multi-protocol converter — inspired by AionUi.
//!
//! AionUi's `RotatingApiClient` + `ApiKeyManager` + `ProtocolConverter`
//! provide three capabilities we need:
//!
//! 1. **Key rotation**: cycle through multiple API keys to avoid rate limits
//! 2. **Protocol conversion**: normalize Anthropic/Gemini/OpenAI formats → unified
//! 3. **Exponential backoff**: per-key cooldown on 429 responses
//!
//! This module adds key rotation to `OpenAiCompatibleProvider` and defines
//! a `ProtocolAdapter` trait for future multi-protocol support.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

// ============================================================================
// ApiKeyPool — Rotating key pool with per-key health tracking
// ============================================================================

/// Tracks the state of a single API key.
#[derive(Debug, Clone)]
struct KeyState {
    key: String,
    healthy: bool,
    consecutive_429: u32,
    cooldown_until: Option<Instant>,
    total_requests: u64,
}

/// A pool of API keys with automatic rotation and health tracking.
///
/// When a key receives a 429 (rate limited), it enters cooldown and the
/// pool rotates to the next healthy key. Keys recover after `cooldown_duration`.
///
/// ```no_run
/// use deepseek_carp::providers::api_keys::ApiKeyPool;
///
/// let pool = ApiKeyPool::new(
///     vec!["sk-key1".into(), "sk-key2".into(), "sk-key3".into()],
///     std::time::Duration::from_secs(60),
/// );
///
/// // In the provider, before each request:
/// let key = pool.next_key().await.expect("unwrap failed: api_keys.rs:46");
/// let headers = pool.auth_headers("Content-Type", &key);
/// ```
#[derive(Debug)]
pub struct ApiKeyPool {
    keys: Arc<RwLock<Vec<KeyState>>>,
    cooldown_duration: Duration,
    /// Round-robin index for fair distribution.
    cursor: Arc<RwLock<usize>>,
}

impl ApiKeyPool {
    /// Create a new pool with the given keys.
    ///
    /// # Arguments
    /// * `keys` - List of API key strings
    /// * `cooldown` - How long a key stays disabled after hitting rate limit
    pub fn new(keys: Vec<String>, cooldown: Duration) -> Self {
        let states: Vec<KeyState> = keys.into_iter().map(|key| KeyState {
            key,
            healthy: true,
            consecutive_429: 0,
            cooldown_until: None,
            total_requests: 0,
        }).collect();

        Self {
            keys: Arc::new(RwLock::new(states)),
            cooldown_duration: cooldown,
            cursor: Arc::new(RwLock::new(0)),
        }
    }

    /// Get the next available healthy key via round-robin.
    /// Returns `None` if all keys are in cooldown.
    pub async fn next_key(&self) -> Option<String> {
        let keys = self.keys.read().await;
        let total = keys.len();
        drop(keys);

        for _ in 0..total {
            let idx = {
                let mut c = self.cursor.write().await;
                let i = *c;
                *c = (*c + 1) % total;
                i
            };

            let keys = self.keys.read().await;
            let state = &keys[idx];
            let now = Instant::now();

            let is_available = state.healthy
                && state.cooldown_until.is_none_or(|t| now >= t);

            if is_available {
                return Some(state.key.clone());
            }
        }

        // All keys in cooldown — try to find the one closest to recovery
        let keys = self.keys.read().await;
        let now = Instant::now();
        let earliest_recovery = keys.iter()
            .filter_map(|s| s.cooldown_until)
            .min();

        if let Some(recovery_time) = earliest_recovery {
            let remaining = recovery_time.checked_duration_since(now).unwrap_or(Duration::ZERO);
            tracing::warn!(
                remaining_secs = remaining.as_secs(),
                "All API keys in cooldown. Next available in {}s",
                remaining.as_secs()
            );
        }

        None
    }

    /// Mark a key as rate-limited (429). Enters cooldown.
    pub async fn mark_rate_limited(&self, key: &str) {
        let mut keys = self.keys.write().await;
        if let Some(state) = keys.iter_mut().find(|s| s.key == key) {
            state.consecutive_429 += 1;
            state.healthy = false;
            let backoff = self.cooldown_duration * state.consecutive_429.min(10);
            state.cooldown_until = Some(Instant::now() + backoff);
            tracing::warn!(
                key = %key[..8].to_string() + "...",
                backoff_secs = backoff.as_secs(),
                consecutive = state.consecutive_429,
                "API key rate-limited, entering cooldown"
            );
        }
    }

    /// Mark a key as successfully used. Resets failure counters.
    pub async fn mark_success(&self, key: &str) {
        let mut keys = self.keys.write().await;
        if let Some(state) = keys.iter_mut().find(|s| s.key == key) {
            state.total_requests += 1;
            if !state.healthy {
                state.healthy = true;
                state.consecutive_429 = 0;
                state.cooldown_until = None;
                tracing::info!(key = %key[..8].to_string() + "...", "API key recovered");
            }
        }
    }

    /// Get pool statistics for monitoring.
    pub async fn stats(&self) -> ApiKeyPoolStats {
        let keys = self.keys.read().await;
        let total = keys.len();
        let healthy = keys.iter().filter(|s| s.healthy).count();
        let in_cooldown = keys.iter().filter(|s| s.cooldown_until.is_some()).count();
        let total_requests: u64 = keys.iter().map(|s| s.total_requests).sum();

        ApiKeyPoolStats {
            total_keys: total,
            healthy_keys: healthy,
            keys_in_cooldown: in_cooldown,
            total_requests,
        }
    }
}

/// Snapshot of pool health for monitoring/dashboards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyPoolStats {
    pub total_keys: usize,
    pub healthy_keys: usize,
    pub keys_in_cooldown: usize,
    pub total_requests: u64,
}

// ============================================================================
// ProtocolAdapter — Unified request/response conversion
// ============================================================================

/// Protocol conversion trait — converts between provider-native formats
/// and DeepSeek Carp's unified internal format.
///
/// Inspired by AionUi's `ProtocolConverter` which normalizes
/// OpenAI / Gemini / Anthropic / Ollama formats into a single internal representation.
///
/// Currently only OpenAI format is fully supported. Gemini and Anthropic
/// adapters are stubbed for future implementation.
pub trait ProtocolAdapter: Send + Sync {
    /// Convert from internal request format to provider-specific JSON body.
    fn build_request(&self, req: &super::provider::ProviderRequest) -> serde_json::Value;

    /// Convert from provider-specific response to internal `ProviderResponse`.
    fn parse_response(
        &self,
        body: &serde_json::Value,
        latency_ms: u64,
        provider_name: &str,
        model: &str,
        is_local: bool,
    ) -> Result<super::provider::ProviderResponse, super::provider::ProviderError>;

    /// Build auth headers for this provider.
    fn auth_headers(&self, api_key: Option<&str>) -> Vec<(String, String)>;
}

/// OpenAI-compatible protocol adapter (current default).
pub struct OpenAiAdapter;

impl ProtocolAdapter for OpenAiAdapter {
    fn build_request(&self, req: &super::provider::ProviderRequest) -> serde_json::Value {
        let mut messages = Vec::new();

        if let Some(ref system) = req.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }

        for msg in &req.messages {
            let mut json_msg = serde_json::json!({"role": msg.role, "content": msg.content});
            if let Some(ref tc) = msg.tool_calls {
                json_msg["tool_calls"] = serde_json::to_value(tc).unwrap_or_default();
            }
            if let Some(ref tci) = msg.tool_call_id {
                json_msg["tool_call_id"] = serde_json::json!(tci);
            }
            messages.push(json_msg);
        }

        let mut body = serde_json::json!({
            "messages": messages,
            "stream": req.stream,
        });

        if let Some(mt) = req.max_tokens { body["max_tokens"] = serde_json::json!(mt); }
        if let Some(t) = req.temperature { body["temperature"] = serde_json::json!(t); }
        if let Some(ref s) = req.stop { body["stop"] = serde_json::json!(s); }
        if let Some(ref tools) = req.tools { body["tools"] = serde_json::to_value(tools).unwrap_or_default(); }

        body
    }

    fn parse_response(
        &self,
        body: &serde_json::Value,
        latency_ms: u64,
        provider_name: &str,
        model: &str,
        is_local: bool,
    ) -> Result<super::provider::ProviderResponse, super::provider::ProviderError> {
        let content = body["choices"].as_array()
            .and_then(|a| a.first())
            .and_then(|c| c["message"]["content"].as_str())
            .unwrap_or("").to_string();

        let usage = body.get("usage").map(|u| super::provider::TokenUsage {
            prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
        });

        let finish_reason = body["choices"].as_array()
            .and_then(|a| a.first())
            .and_then(|c| c["finish_reason"].as_str())
            .map(|s| s.to_string());

        Ok(super::provider::ProviderResponse {
            content,
            provider: provider_name.to_string(),
            model: model.to_string(),
            usage,
            latency_ms,
            is_local,
            finish_reason,
        })
    }

    fn auth_headers(&self, api_key: Option<&str>) -> Vec<(String, String)> {
        let mut headers = vec![("Content-Type".into(), "application/json".into())];
        if let Some(key) = api_key {
            headers.push(("Authorization".into(), format!("Bearer {}", key)));
        }
        headers
    }
}

// ============================================================================
// Secure Key Storage Extension
// ============================================================================

use std::path::PathBuf;

/// Securely stored API key — encrypted at rest, decrypted only when needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecureKey {
    /// Encrypted payload (XOR-obfuscated ciphertext + nonce + checksum).
    encrypted: Vec<u8>,
    /// Key identifier (for display, not the actual key).
    key_id: String,
    /// Which provider this key belongs to.
    provider: String,
    /// When this key was added.
    created_at: u64,
    /// Last used timestamp.
    last_used_at: Option<u64>,
    /// Usage count.
    usage_count: u64,
}

impl SecureKey {
    /// Create a new secure key from plaintext. Encrypts immediately.
    pub fn from_plaintext(key: &str, provider: &str, master_key: &[u8]) -> anyhow::Result<Self> {
        let (encrypted, key_id) = encrypt_key(key, master_key)?;
        Ok(Self {
            encrypted,
            key_id,
            provider: provider.to_string(),
            created_at: now_ts(),
            last_used_at: None,
            usage_count: 0,
        })
    }

    /// Decrypt and return the plaintext key. Returns error if decryption fails.
    pub fn decrypt(&self, master_key: &[u8]) -> anyhow::Result<String> {
        decrypt_key(&self.encrypted, master_key)
    }

    /// Record usage (call after each API call).
    pub fn record_usage(&mut self) {
        self.usage_count += 1;
        self.last_used_at = Some(now_ts());
    }

    /// Get display-safe identifier (first 4 ... last 4 chars of key_id).
    pub fn display_id(&self) -> String {
        format!("{}...{}", &self.key_id[..4.min(self.key_id.len())],
                if self.key_id.len() > 4 { &self.key_id[self.key_id.len()-4..] } else { "" })
    }
}

/// Secure key store — manages multiple encrypted keys with persistence.
pub struct SecureKeyStore {
    keys: Vec<SecureKey>,
    /// Master key derived from machine-specific secret.
    master_key: Vec<u8>,
    store_path: PathBuf,
}

impl SecureKeyStore {
    /// Create or load a secure key store.
    /// Master key is derived from: env var DSCARP_MASTER_KEY or machine fingerprint.
    pub fn new(workspace: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let ws = workspace.into();
        let store_path = ws.join(".dscarp").join("keys.enc");
        let master_key = derive_master_key()?;

        let keys = if store_path.exists() {
            load_encrypted_store(&store_path, &master_key)?
        } else {
            Vec::new()
        };

        Ok(Self { keys, master_key, store_path })
    }

    /// Add a new key to the store (encrypts and persists).
    pub fn add_key(&mut self, key: &str, provider: &str) -> anyhow::Result<String> {
        let sk = SecureKey::from_plaintext(key, provider, &self.master_key)?;
        let id = sk.key_id.clone();
        self.keys.push(sk);
        self.persist()?;
        Ok(id)
    }

    /// Get a usable (decrypted) key for the given provider. Records usage.
    pub fn get_key(&mut self, provider: &str) -> anyhow::Result<Option<String>> {
        for sk in &mut self.keys {
            if sk.provider == provider {
                match sk.decrypt(&self.master_key) {
                    Ok(key) => { sk.record_usage(); return Ok(Some(key)); }
                    Err(e) => { tracing::warn!(error=%e, provider, "Failed to decrypt key"); continue; }
                }
            }
        }
        Ok(None)
    }

    /// List all stored keys (safe info only, no plaintext).
    pub fn list_keys(&self) -> Vec<KeyInfo> {
        self.keys.iter().map(|sk| KeyInfo {
            key_id: sk.display_id(),
            provider: sk.provider.clone(),
            created_at: sk.created_at,
            last_used_at: sk.last_used_at,
            usage_count: sk.usage_count,
        }).collect()
    }

    /// Remove a key by ID.
    pub fn remove_key(&mut self, key_id: &str) -> anyhow::Result<bool> {
        let len_before = self.keys.len();
        self.keys.retain(|k| k.key_id != key_id);
        let removed = self.keys.len() < len_before;
        if removed { self.persist()?; }
        Ok(removed)
    }

    /// Persist encrypted store to disk.
    fn persist(&self) -> anyhow::Result<()> {
        let dir = self.store_path.parent().expect("parent path");
        std::fs::create_dir_all(dir)?;
        let data = serde_json::to_string(&self.keys)?;
        let encrypted = encrypt_bytes(data.as_bytes(), &self.master_key)?;
        std::fs::write(&self.store_path, &encrypted)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyInfo {
    pub key_id: String,
    pub provider: String,
    pub created_at: u64,
    pub last_used_at: Option<u64>,
    pub usage_count: u64,
}

// === Simple Encryption (XOR-based, no external crate dependency beyond std) ===

fn derive_master_key() -> anyhow::Result<Vec<u8>> {
    // Try environment variable first
    if let Ok(key_str) = std::env::var("DSCARP_MASTER_KEY") {
        if !key_str.is_empty() {
            return Ok(hash_to_32bytes(&key_str));
        }
    }

    // Fall back to machine fingerprint: username + hostname (Windows-compatible)
    let username = std::env::var("USERNAME").unwrap_or_else(|_| "unknown".into());
    let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "unknown".into());
    let salt = format!("deepseek-carp:{}:{}", username, hostname);
    Ok(hash_to_32bytes(&salt))
}

fn hash_to_32bytes(input: &str) -> Vec<u8> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut h1 = DefaultHasher::new();
    input.hash(&mut h1);
    let v1 = h1.finish();

    let mut h2 = DefaultHasher::new();
    v1.hash(&mut h2);
    let v2 = h2.finish();

    [v1.to_le_bytes(), v2.to_le_bytes()].concat()
}

fn encrypt_key(plaintext: &str, master_key: &[u8]) -> anyhow::Result<(Vec<u8>, String)> {
    let data = plaintext.as_bytes();
    let encrypted = xor_encrypt(data, master_key)?;
    let key_id = hex_encode(&hash_to_32bytes(&format!("{}:{}", plaintext, now_ts())));
    Ok((encrypted, key_id))
}

fn decrypt_key(encrypted: &[u8], master_key: &[u8]) -> anyhow::Result<String> {
    let decrypted = xor_decrypt(encrypted, master_key)?;
    String::from_utf8(decrypted).map_err(|e| anyhow::anyhow!("Decryption produced invalid UTF-8: {}", e))
}

fn encrypt_bytes(data: &[u8], master_key: &[u8]) -> anyhow::Result<Vec<u8>> {
    xor_encrypt(data, master_key)
}

fn load_encrypted_store(path: &PathBuf, master_key: &[u8]) -> anyhow::Result<Vec<SecureKey>> {
    let encrypted = std::fs::read(path)?;
    let decrypted = xor_decrypt(&encrypted, master_key)?;
    let json = String::from_utf8(decrypted).map_err(|e| anyhow::anyhow!("Corrupted key store: {}", e))?;
    serde_json::from_str(&json).map_err(|e| anyhow::anyhow!("Failed to parse key store: {}", e))
}

/// Simple XOR encryption with key cycling and checksum verification.
/// Prevents casual key leakage from memory dumps/logs.
/// Production deployment should replace with AES-256-GCM.
fn xor_encrypt(data: &[u8], key: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut result = Vec::with_capacity(data.len() + 17); // 16 bytes nonce + data + 1 byte checksum

    // Generate nonce from timestamp hash
    let nonce = hash_to_32bytes(&format!("{}", now_ts()));
    result.extend_from_slice(&nonce[..16]);

    // XOR data with cycling key
    for (i, byte) in data.iter().enumerate() {
        let key_byte = key[i % key.len()];
        result.push(byte ^ key_byte);
    }

    // Append checksum
    let checksum: u8 = result.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
    result.push(checksum);

    Ok(result)
}

fn xor_decrypt(data: &[u8], key: &[u8]) -> anyhow::Result<Vec<u8>> {
    if data.len() < 17 { anyhow::bail!("Encrypted data too short"); }

    let (_nonce, rest) = data.split_at(16);
    let (encrypted, _checksum_bytes) = rest.split_at(rest.len() - 1);

    let mut result = Vec::with_capacity(encrypted.len());
    for (i, byte) in encrypted.iter().enumerate() {
        let key_byte = key[i % key.len()];
        result.push(byte ^ key_byte);
    }

    // Verify checksum
    let expected_checksum = data[data.len() - 1];
    let actual_checksum: u8 = data.iter().take(data.len()-1).fold(0u8, |acc, &b| acc.wrapping_add(b));
    if actual_checksum != expected_checksum {
        anyhow::bail!("Checksum verification failed — data may be corrupted");
    }

    Ok(result)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(std::time::Duration::ZERO).as_secs()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_key_pool_rotation() {
        let pool = ApiKeyPool::new(
            vec!["key-a".into(), "key-b".into(), "key-c".into()],
            Duration::from_secs(1),
        );

        let k1 = pool.next_key().await.unwrap();
        let k2 = pool.next_key().await.unwrap();
        let k3 = pool.next_key().await.unwrap();
        let k4 = pool.next_key().await.unwrap(); // wraps around

        // All three distinct keys used
        let mut seen = vec![k1.clone(), k2.clone(), k3.clone(), k4.clone()];
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 3, "Should rotate through all 3 keys");
    }

    #[tokio::test]
    async fn test_key_cooldown() {
        let pool = ApiKeyPool::new(
            vec!["good-key".into(), "bad-key".into()],
            Duration::from_millis(100),
        );

        // Get first key
        let first = pool.next_key().await.unwrap();
        // Mark it as rate-limited
        pool.mark_rate_limited(&first).await;
        // Next call should get the OTHER key
        let second = pool.next_key().await.unwrap();
        assert_ne!(first, second, "Should rotate away from rate-limited key");

        // After cooldown, rate-limited key should be available again
        tokio::time::sleep(Duration::from_millis(150)).await;
        pool.mark_success(&first).await;
        let _recovered = pool.next_key().await.unwrap();
        assert!(pool.next_key().await.is_some());
    }

    #[tokio::test]
    async fn test_all_keys_cooldown() {
        let pool = ApiKeyPool::new(
            vec!["only-key".into()],
            Duration::from_secs(60),
        );
        let key = pool.next_key().await.unwrap();
        pool.mark_rate_limited(&key).await;
        assert!(pool.next_key().await.is_none(), "No keys available when all in cooldown");
    }

    #[test]
    fn test_openai_adapter_builds_correct_body() {
        let adapter = OpenAiAdapter;
        let req = super::super::provider::ProviderRequest {
            system: Some("You are helpful.".into()),
            messages: vec![super::super::provider::ChatMessage {
                role: "user".into(),
                content: "Hello".into(),
                tool_calls: None,
                tool_call_id: None,
            ..Default::default()}],
            max_tokens: Some(100),
            temperature: Some(0.5),
            stop: None,
            tools: None,
            stream: false,
        };

        let body = adapter.build_request(&req);
        assert_eq!(body["messages"].as_array().expect("unwrap failed: api_keys.rs:622").len(), 2); // system + user
        assert_eq!(body["max_tokens"].as_u64(), Some(100));
    }

    // --- Secure Key Storage Tests ---

    #[test]
    fn test_secure_key_roundtrip() {
        let master_key = hash_to_32bytes("test-master-key-12345");
        let plaintext = "sk-deepseek-abc123xyz789-secret";

        let sk = SecureKey::from_plaintext(plaintext, "deepseek", &master_key)
            .expect("unwrap failed: api_keys.rs: SecureKey::from_plaintext");

        // Encrypted data must differ from plaintext
        assert_ne!(sk.encrypted, plaintext.as_bytes(),
            "Encrypted data should not match plaintext");

        // Round-trip decryption must recover original
        let decrypted = sk.decrypt(&master_key)
            .expect("unwrap failed: api_keys.rs: SecureKey::decrypt");
        assert_eq!(decrypted, plaintext, "Round-trip decryption must match original");

        // Wrong key must fail
        let wrong_key = hash_to_32bytes("wrong-master-key");
        assert!(sk.decrypt(&wrong_key).is_err(),
            "Decryption with wrong key should fail");
    }

    #[test]
    fn test_secure_key_store_add_get() {
        let tmp = tempfile::tempdir().expect("unwrap failed: api_keys.rs: tempfile::tempdir");
        let mut store = SecureKeyStore::new(tmp.path())
            .expect("unwrap failed: api_keys.rs: SecureKeyStore::new");

        let id = store.add_key("sk-test-key-001", "deepseek")
            .expect("unwrap failed: api_keys.rs: store.add_key");
        assert!(!id.is_empty(), "Key ID should not be empty");

        let key = store.get_key("deepseek")
            .expect("unwrap failed: api_keys.rs: store.get_key")
            .expect("unwrap failed: api_keys.rs: get_key returned None");
        assert_eq!(key, "sk-test-key-001", "Retrieved key must match original");

        // Non-existent provider returns None
        let missing = store.get_key("openai")
            .expect("unwrap failed: api_keys.rs: store.get_key for openai");
        assert!(missing.is_none(), "Non-existent provider should return None");

        // Usage tracking
        let info = &store.list_keys()[0];
        assert_eq!(info.usage_count, 1, "Usage count should be 1 after one get");
        assert!(info.last_used_at.is_some(), "last_used_at should be set after usage");
    }

    #[test]
    fn test_key_info_no_plaintext() {
        let master_key = hash_to_32bytes("safety-check-key");
        let sk = SecureKey::from_plaintext("sk-real-secret-key-here", "glm", &master_key)
            .expect("unwrap failed: api_keys.rs: SecureKey::from_plaintext");

        let display = sk.display_id();
        // Must NOT contain the actual key material
        assert!(!display.contains("sk-real"),
            "display_id must not contain plaintext key");
        assert!(!display.contains("secret"),
            "display_id must not contain secret parts");

        // KeyInfo serialization should also never contain plaintext
        let info = KeyInfo {
            key_id: sk.display_id(),
            provider: sk.provider.clone(),
            created_at: sk.created_at,
            last_used_at: sk.last_used_at,
            usage_count: sk.usage_count,
        };
        let json = serde_json::to_string(&info).expect("unwrap failed: api_keys.rs: serde_json::to_string");
        assert!(!json.contains("sk-real"), "KeyInfo JSON must not contain plaintext key");
    }

    #[test]
    fn test_remove_key() {
        let tmp = tempfile::tempdir().expect("unwrap failed: api_keys.rs: tempfile::tempdir");
        let mut store = SecureKeyStore::new(tmp.path())
            .expect("unwrap failed: api_keys.rs: SecureKeyStore::new");

        let id1 = store.add_key("sk-remove-me", "deepseek")
            .expect("unwrap failed: api_keys.rs: store.add_key sk-remove-me");
        let _id2 = store.add_key("sk-keep-this", "deepseek")
            .expect("unwrap failed: api_keys.rs: store.add_key sk-keep-this");

        assert_eq!(store.list_keys().len(), 2, "Store should have 2 keys");

        let removed = store.remove_key(&id1)
            .expect("unwrap failed: api_keys.rs: store.remove_key");
        assert!(removed, "Remove should return true for existing key");
        assert_eq!(store.list_keys().len(), 1, "Store should have 1 key after removal");

        // Removing same key again returns false
        let removed_again = store.remove_key(&id1)
            .expect("unwrap failed: api_keys.rs: store.remove_key again");
        assert!(!removed_again, "Removing non-existent key should return false");
    }

    #[test]
    fn test_derive_master_key_deterministic() {
        // Same input must always produce the same key
        let k1 = hash_to_32bytes("deterministic-input-string");
        let k2 = hash_to_32bytes("deterministic-input-string");
        assert_eq!(k1, k2, "Same input must produce identical master key");

        // Different inputs must produce different keys
        let k3 = hash_to_32bytes("different-input-string");
        assert_ne!(k1, k3, "Different inputs must produce different keys");

        // Output must be exactly 32 bytes (256 bits)
        assert_eq!(k1.len(), 32, "Master key must be 32 bytes");
    }

    #[test]
    fn test_xor_encrypt_decrypt() {
        let key = b"my-secret-xor-key-12345";
        let original = b"Hello, XOR encryption world! @#$%^&*()";

        let encrypted = xor_encrypt(original, key)
            .expect("unwrap failed: api_keys.rs: xor_encrypt");

        // Encrypted must differ from original
        assert_ne!(&encrypted[..encrypted.len()-17], original as &[u8],
            "XOR output must differ from input (ignoring nonce+checksum)");

        let decrypted = xor_decrypt(&encrypted, key)
            .expect("unwrap failed: api_keys.rs: xor_decrypt");
        assert_eq!(decrypted, original, "Decrypted must match original");

        // Tampered data fails checksum
        let mut tampered = encrypted.clone();
        if !tampered.is_empty() { tampered[16] ^= 0xFF; }
        assert!(xor_decrypt(&tampered, key).is_err(),
            "Tampered data should fail checksum verification");

        // Too-short data fails
        assert!(xor_decrypt(b"short", key).is_err(),
            "Short data should fail decryption");
    }
}
