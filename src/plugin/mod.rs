//! Plugin system — extensible architecture for custom tools, commands, and hooks.
//!
//! Plugins are shared libraries (.dll/.so) or embedded Rust modules that can:
//! - Register custom CLI subcommands
//! - Add tool definitions for the Agent
//! - Hook into the request lifecycle (before/after prompt, before/after response)
//! - Provide custom completion engines
//! - Extend the TUI

use std::collections::HashMap;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use serde::{Serialize, Deserialize};

/// Plugin metadata from manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub dscarp_min_version: String,
    pub permissions: Vec<PluginPermission>,
    pub entry_point: String,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginPermission {
    FileAccess,
    NetworkAccess,
    ShellExecute,
    EnvironmentRead,
    EnvironmentWrite,
    ConfigModify,
}

/// Holds the loaded library handle from libloading.
pub struct DynamicLibraryHandle {
    pub path: PathBuf,
    #[allow(dead_code)]
    library: Option<libloading::Library>, // Keep alive while plugin is loaded
}

/// A loaded plugin instance.
pub struct PluginInstance {
    pub manifest: PluginManifest,
    pub state: PluginState,
    pub loaded_at: u64,
    hooks: Vec<PluginHook>,
    tools: Vec<ToolDefinition>,
    pub dyn_handle: Option<DynamicLibraryHandle>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginState {
    Loaded,
    Active,
    Suspended,
    #[serde(skip)]
    Error(String),
    Unloaded,
}

/// A lifecycle hook point.
#[derive(Debug, Clone)]
pub struct PluginHook {
    pub hook_type: HookType,
    pub priority: i32,
    pub handler_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum HookType {
    PrePrompt,
    PostPrompt,
    PreResponse,
    PostResponse,
    PreFileWrite,
    PostFileWrite,
    OnConfigChange,
    OnShutdown,
}

/// Custom tool definition for the Agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
    pub handler: String,
}

/// Plugin manager — loads, manages, and coordinates all plugins.
pub struct PluginManager {
    plugins: HashMap<String, PluginInstance>,
    plugin_dirs: Vec<PathBuf>,
    hook_registry: HashMap<HookType, Vec<PluginHook>>,
    global_state: Arc<std::sync::RwLock<serde_json::Value>>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            plugin_dirs: Vec::new(),
            hook_registry: HashMap::new(),
            global_state: Arc::new(std::sync::RwLock::new(serde_json::Value::Object(serde_json::Map::new()))),
        }
    }

    /// Add a directory to scan for plugins.
    pub fn add_plugin_dir(&mut self, dir: impl Into<PathBuf>) {
        self.plugin_dirs.push(dir.into());
    }

    /// Discover and load all plugins from configured directories.
    ///
    /// Returns the names of successfully loaded plugins.
    pub fn discover_and_load(&mut self) -> anyhow::Result<Vec<String>> {
        let mut loaded = Vec::new();

        // Clone dirs to avoid borrow conflict with &mut self.load_plugin
        let dirs: Vec<PathBuf> = self.plugin_dirs.clone();

        for dir in &dirs {
            if !dir.is_dir() {
                continue;
            }

            let read_result = std::fs::read_dir(dir);
            let entries = match read_result {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                // Look for manifest files or known plugin extensions
                if path.file_name()
                    .map(|n| n.to_string_lossy().ends_with(".json"))
                    .unwrap_or(false)
                    || path.extension().map(|e| e == "so" || e == "dll").unwrap_or(false)
                {
                    match self.load_plugin(&path) {
                        Ok(name) => loaded.push(name),
                        Err(e) => tracing::warn!("Failed to load plugin at {:?}: {}", path, e),
                    }
                }
            }
        }

        Ok(loaded)
    }

    /// Load a specific plugin from a path.
    ///
    /// Parses the manifest (if JSON) or uses filename as plugin name.
    /// Returns the plugin name on success.
    pub fn load_plugin(&mut self, path: &Path) -> anyhow::Result<String> {
        let manifest = if path.extension().is_some_and(|e| e == "json") {
            // Parse manifest file
            let content = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read plugin manifest: {}", e))?;
            let parsed: PluginManifest = serde_json::from_str(&content)
                .map_err(|e| anyhow::anyhow!("Invalid plugin manifest JSON: {}", e))?;

            // Validate required fields
            if parsed.name.is_empty() {
                anyhow::bail!("Plugin manifest missing 'name' field");
            }
            if parsed.entry_point.is_empty() {
                anyhow::bail!("Plugin manifest missing 'entry_point' field");
            }
            parsed
        } else {
            // Create a synthetic manifest from path for non-JSON plugins
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "unknown".to_string());

            PluginManifest {
                name: name.clone(),
                version: "0.1.0".to_string(),
                description: format!("Auto-discovered plugin from {:?}", path),
                author: "unknown".to_string(),
                license: "unknown".to_string(),
                dscarp_min_version: "0.1.0".to_string(),
                permissions: vec![PluginPermission::FileAccess],
                entry_point: format!("{}_init", name),
                dependencies: vec![],
            }
        };

        let name = manifest.name.clone();

        // Check if already loaded
        if self.plugins.contains_key(&name) {
            anyhow::bail!("Plugin '{}' is already loaded", name);
        }

        // Check dependencies
        for dep in &manifest.dependencies {
            if !self.plugins.contains_key(dep) {
                anyhow::bail!(
                    "Plugin '{}' depends on '{}', which is not loaded",
                    name,
                    dep
                );
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let instance = PluginInstance {
            state: PluginState::Loaded,
            loaded_at: now,
            hooks: vec![],
            tools: vec![],
            manifest,
            dyn_handle: None,
        };

        self.plugins.insert(name.clone(), instance);

        tracing::info!("Plugin '{}' loaded from {:?}", name, path);
        Ok(name)
    }

    /// Unload a plugin by name.
    ///
    /// Returns `true` if the plugin was found and unloaded, `false` otherwise.
    pub fn unload_plugin(&mut self, name: &str) -> anyhow::Result<bool> {
        let Some(instance) = self.plugins.get_mut(name) else {
            return Ok(false); // Not found — not an error
        };

        // Already unloaded
        if matches!(instance.state, PluginState::Unloaded) {
            return Ok(false);
        }

        // Remove all hooks registered by this plugin
        for hook_type in HookType::all_variants() {
            if let Some(hooks) = self.hook_registry.get_mut(&hook_type) {
                hooks.retain(|h| h.handler_name != format!("{}::", name));
            }
        }

        // Explicitly drop the dynamic library handle if present
        if instance.dyn_handle.is_some() {
            instance.dyn_handle = None; // Drop Library here
        }

        instance.state = PluginState::Unloaded;
        tracing::info!("Plugin '{}' unloaded", name);
        Ok(true)
    }

    /// Suspend a plugin (keep loaded but don't execute hooks).
    ///
    /// Returns `true` if the plugin was found and suspended.
    pub fn suspend_plugin(&mut self, name: &str) -> anyhow::Result<bool> {
        let Some(instance) = self.plugins.get_mut(name) else {
            return Ok(false);
        };
        instance.state = PluginState::Suspended;
        tracing::info!("Plugin '{}' suspended", name);
        Ok(true)
    }

    /// Resume a suspended plugin.
    ///
    /// Returns `true` if the plugin was found and resumed.
    pub fn resume_plugin(&mut self, name: &str) -> anyhow::Result<bool> {
        let Some(instance) = self.plugins.get_mut(name) else {
            return Ok(false);
        };
        if matches!(instance.state, PluginState::Suspended) {
            instance.state = PluginState::Active;
            tracing::info!("Plugin '{}' resumed", name);
            Ok(true)
        } else {
            Ok(false) // Wasn't suspended
        }
    }

    /// Execute all hooks for a given type (in priority order).
    ///
    /// Only active and loaded plugins participate; suspended/unloaded ones are skipped.
    pub fn fire_hooks(
        &self,
        hook_type: HookType,
        context: &serde_json::Value,
    ) -> Vec<HookResult> {
        let start = std::time::Instant::now();
        let mut results = Vec::new();

        let hooks_for_type = match self.hook_registry.get(&hook_type) {
            Some(hooks) => hooks,
            None => return results,
        };

        // Sort by priority (lower runs first)
        let mut sorted_hooks: Vec<&PluginHook> = hooks_for_type.iter().collect();
        sorted_hooks.sort_by_key(|h| h.priority);

        for hook in sorted_hooks {
            // Find which plugin owns this hook
            let owner_plugin = self.plugins.values().find(|p| {
                p.hooks.iter().any(|ph| ph.handler_name == hook.handler_name)
            });

            let plugin_name = owner_plugin
                .map(|p| p.manifest.name.clone())
                .unwrap_or_else(|| "unknown".to_string());

            // Skip if plugin is not active/loaded
            let should_execute = owner_plugin.is_none_or(|p| {
                matches!(p.state, PluginState::Loaded | PluginState::Active)
            });

            if !should_execute {
                continue;
            }

            // Simulate hook execution — in production this would call into plugin code
            let elapsed_us = start.elapsed().as_micros() as u64;

            // Simulate occasional failures for testing purposes
            let success = true; // Always succeed in skeleton

            results.push(HookResult {
                plugin_name,
                success,
                data: Some(context.clone()),
                error: None,
                duration_us: elapsed_us,
            });
        }

        results
    }

    /// List all available tools from plugins.
    pub fn list_tools(&self) -> Vec<&ToolDefinition> {
        self.plugins
            .values()
            .filter(|p| matches!(p.state, PluginState::Loaded | PluginState::Active))
            .flat_map(|p| p.tools.iter())
            .collect()
    }

    /// List all loaded plugins.
    pub fn list_plugins(&self) -> Vec<&PluginInstance> {
        self.plugins.values().collect()
    }

    /// Get plugin state snapshot.
    pub fn status(&self) -> PluginStatusReport {
        let total_plugins = self.plugins.len();
        let active_plugins = self
            .plugins
            .values()
            .filter(|p| matches!(p.state, PluginState::Active))
            .count();
        let total_hooks: usize = self.hook_registry.values().map(|v| v.len()).sum();
        let total_tools: usize =
            self.plugins.values().map(|p| p.tools.len()).sum();
        let plugins: Vec<String> = self.plugins.keys().cloned().collect();

        PluginStatusReport {
            total_plugins,
            active_plugins,
            total_hooks,
            total_tools,
            plugins,
        }
    }

    /// Register a hook for a given plugin.
    pub(crate) fn register_hook(&mut self, plugin_name: &str, hook: PluginHook) {
        let hooks = self.hook_registry.entry(hook.hook_type).or_default();
        // Ensure handler name is namespaced
        let namespaced_hook = PluginHook {
            handler_name: format!("{}::{}", plugin_name, hook.handler_name),
            ..hook
        };
        hooks.push(namespaced_hook.clone());

        // Also add to the plugin's own hook list
        if let Some(plugin) = self.plugins.get_mut(plugin_name) {
            plugin.hooks.push(namespaced_hook);
        }
    }

    /// Register a tool for a given plugin.
    pub(crate) fn register_tool(&mut self, plugin_name: &str, tool: ToolDefinition) {
        if let Some(plugin) = self.plugins.get_mut(plugin_name) {
            plugin.tools.push(tool);
        }
    }

    /// Load a plugin directly from a manifest (for programmatic / test use).
    pub fn load_plugin_from_manifest(
        &mut self,
        manifest: PluginManifest,
    ) -> anyhow::Result<String> {
        let name = manifest.name.clone();

        if name.is_empty() {
            anyhow::bail!("Plugin name must not be empty");
        }

        if self.plugins.contains_key(&name) {
            anyhow::bail!("Plugin '{}' is already loaded", name);
        }

        // Check dependencies
        for dep in &manifest.dependencies {
            if !self.plugins.contains_key(dep.as_str()) {
                anyhow::bail!(
                    "Plugin '{}' depends on '{}', which is not loaded",
                    name,
                    dep
                );
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let instance = PluginInstance {
            state: PluginState::Loaded,
            loaded_at: now,
            hooks: vec![],
            tools: vec![],
            manifest,
            dyn_handle: None,
        };

        self.plugins.insert(name.clone(), instance);
        tracing::info!("Plugin '{}' loaded from manifest", name);
        Ok(name)
    }

    /// Load a plugin from a dynamic library (.dll / .so).
    ///
    /// Attempts to load the shared library via `libloading`, then looks for
    /// the `dscarp_plugin_register` symbol to obtain a JSON manifest.
    /// Falls back to a synthetic manifest based on filename if the symbol is absent.
    pub fn load_dynamic_plugin(&mut self, dll_path: &Path) -> anyhow::Result<String> {
        let library = unsafe {
            libloading::Library::new(dll_path)
                .map_err(|e| anyhow::anyhow!("Failed to load dynamic library {:?}: {}", dll_path, e))?
        };

        // Try to find the registration symbol
        let manifest = unsafe {
            let register_fn: Result<
                libloading::Symbol<extern "C" fn() -> *const c_char>,
                _,
            > = library.get(b"dscarp_plugin_register");

            match register_fn {
                Ok(func) => {
                    let raw_ptr = func();
                    if raw_ptr.is_null() {
                        anyhow::bail!(
                            "Plugin at {:?} returned null from dscarp_plugin_register",
                            dll_path
                        );
                    }
                    let c_str = std::ffi::CStr::from_ptr(raw_ptr);
                    let json_str = c_str.to_string_lossy();
                    let parsed: PluginManifest = serde_json::from_str(&json_str)
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "Invalid manifest JSON from dscarp_plugin_register: {}",
                                e
                            )
                        })?;
                    parsed
                }
                Err(_) => {
                    // Symbol not found — fall back to synthetic manifest
                    let name = dll_path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "unknown".to_string());

                    PluginManifest {
                        name: name.clone(),
                        version: "0.1.0".to_string(),
                        description: format!("Dynamic plugin from {:?}", dll_path),
                        author: "unknown".to_string(),
                        license: "unknown".to_string(),
                        dscarp_min_version: "0.1.0".to_string(),
                        permissions: vec![PluginPermission::FileAccess],
                        entry_point: format!("{}_init", name),
                        dependencies: vec![],
                    }
                }
            }
        };

        let name = manifest.name.clone();

        if self.plugins.contains_key(&name) {
            anyhow::bail!("Plugin '{}' is already loaded", name);
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let dyn_handle = DynamicLibraryHandle {
            path: dll_path.to_path_buf(),
            library: Some(library),
        };

        let instance = PluginInstance {
            state: PluginState::Active,
            loaded_at: now,
            hooks: vec![],
            tools: vec![],
            manifest,
            dyn_handle: Some(dyn_handle),
        };

        self.plugins.insert(name.clone(), instance);
        tracing::info!("Dynamic plugin '{}' loaded from {:?}", name, dll_path);
        Ok(name)
    }

    /// Call a tool provided by a loaded plugin.
    ///
    /// Looks up which plugin owns the given tool, then invokes the
    /// `dscarp_plugin_call` symbol from its dynamic library handle.
    pub fn call_plugin_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        // Find the plugin that owns this tool
        let owner = self.plugins.values().find(|p| {
            p.tools.iter().any(|t| t.name == tool_name)
        });

        let instance = owner.ok_or_else(|| {
            format!("No plugin provides tool '{}'", tool_name)
        })?;

        let handle = instance.dyn_handle.as_ref().ok_or_else(|| {
            format!(
                "Plugin '{}' has no dynamic library handle (not loaded as .dll/.so)",
                instance.manifest.name
            )
        })?;

        let lib = handle.library.as_ref().ok_or_else(|| {
            format!(
                "Dynamic library for plugin '{}' has been dropped",
                instance.manifest.name
            )
        })?;

        // Look up the call symbol
        unsafe {
            let call_fn: libloading::Symbol<
                extern "C" fn(*const c_char, *const c_char) -> *const c_char,
            > = lib.get(b"dscarp_plugin_call").map_err(|e| {
                format!(
                    "Symbol 'dscarp_plugin_call' not found in plugin '{}': {}",
                    instance.manifest.name, e
                )
            })?;

            let tool_cstr =
                std::ffi::CString::new(tool_name).map_err(|e| format!("Invalid tool name: {}", e))?;
            let args_json = serde_json::to_string(&args)
                .map_err(|e| format!("Failed to serialize args: {}", e))?;
            let args_cstr =
                std::ffi::CString::new(args_json).map_err(|e| format!("Invalid args JSON: {}", e))?;

            let result_ptr = call_fn(tool_cstr.as_ptr(), args_cstr.as_ptr());

            if result_ptr.is_null() {
                return Err(format!(
                    "Plugin '{}' returned null for tool '{}'",
                    instance.manifest.name, tool_name
                ));
            }

            let result_cstr = std::ffi::CStr::from_ptr(result_ptr);
            let result_str = result_cstr.to_string_lossy();
            serde_json::from_str(&result_str)
                .map_err(|e| format!("Invalid result JSON from plugin: {}", e))
        }
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HookType {
    /// All hook variants for iteration.
    pub fn all_variants() -> [HookType; 8] {
        [
            HookType::PrePrompt,
            HookType::PostPrompt,
            HookType::PreResponse,
            HookType::PostResponse,
            HookType::PreFileWrite,
            HookType::PostFileWrite,
            HookType::OnConfigChange,
            HookType::OnShutdown,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct HookResult {
    pub plugin_name: String,
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<String>,
    pub duration_us: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginStatusReport {
    pub total_plugins: usize,
    pub active_plugins: usize,
    pub total_hooks: usize,
    pub total_tools: usize,
    pub plugins: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_manager_creation() {
        let mgr = PluginManager::new();
        assert!(mgr.list_plugins().is_empty());
        assert_eq!(mgr.status().total_plugins, 0);
    }

    #[test]
    fn test_plugin_discovery() {
        let mut mgr = PluginManager::new();
        mgr.add_plugin_dir("/nonexistent/path");

        // Should not panic on nonexistent directory
        let result = mgr.discover_and_load();
        assert!(result.is_ok());
        assert!(result.expect("ok").is_empty());
    }

    #[test]
    fn test_hook_firing_order() {
        let mut mgr = PluginManager::new();

        // Load two plugins
        let _ = mgr.load_plugin_from_manifest(PluginManifest {
            name: "alpha".to_string(),
            version: "0.1.0".to_string(),
            description: "Test alpha".to_string(),
            author: "test".to_string(),
            license: "MIT".to_string(),
            dscarp_min_version: "0.1.0".to_string(),
            permissions: vec![],
            entry_point: "alpha_init".to_string(),
            dependencies: vec![],
        });

        let _ = mgr.load_plugin_from_manifest(PluginManifest {
            name: "beta".to_string(),
            version: "0.1.0".to_string(),
            description: "Test beta".to_string(),
            author: "test".to_string(),
            license: "MIT".to_string(),
            dscarp_min_version: "0.1.0".to_string(),
            permissions: vec![],
            entry_point: "beta_init".to_string(),
            dependencies: vec![],
        });

        // Register hooks with different priorities
        mgr.register_hook("alpha", PluginHook {
            hook_type: HookType::PrePrompt,
            priority: 10,
            handler_name: "on_pre_prompt".to_string(),
        });
        mgr.register_hook("beta", PluginHook {
            hook_type: HookType::PrePrompt,
            priority: 1, // Lower = first
            handler_name: "on_pre_prompt".to_string(),
        });

        let ctx = serde_json::json!({"prompt": "hello"});
        let results = mgr.fire_hooks(HookType::PrePrompt, &ctx);

        // Beta has lower priority so it should fire first
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].plugin_name, "beta"); // Priority 1 fires first
        assert_eq!(results[1].plugin_name, "alpha"); // Priority 10 fires second
    }

    #[test]
    fn test_tool_listing() {
        let mut mgr = PluginManager::new();
        let _ = mgr.load_plugin_from_manifest(PluginManifest {
            name: "tool_plugin".to_string(),
            version: "0.1.0".to_string(),
            description: "Tools".to_string(),
            author: "test".to_string(),
            license: "MIT".to_string(),
            dscarp_min_version: "0.1.0".to_string(),
            permissions: vec![],
            entry_point: "init".to_string(),
            dependencies: vec![],
        });

        mgr.register_tool("tool_plugin", ToolDefinition {
            name: "custom_search".to_string(),
            description: "Search files".to_string(),
            parameters: serde_json::json!({"type": "object"}),
            handler: "search_handler".to_string(),
        });

        let tools = mgr.list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "custom_search");
    }

    #[test]
    fn test_plugin_lifecycle() {
        let mut mgr = PluginManager::new();
        let _ = mgr.load_plugin_from_manifest(PluginManifest {
            name: "lifecycle_test".to_string(),
            version: "0.1.0".to_string(),
            description: "Lifecycle".to_string(),
            author: "test".to_string(),
            license: "MIT".to_string(),
            dscarp_min_version: "0.1.0".to_string(),
            permissions: vec![],
            entry_point: "init".to_string(),
            dependencies: vec![],
        });

        // Suspend
        let suspended = mgr.suspend_plugin("lifecycle_test")
            .expect("suspend ok");
        assert!(suspended);

        // Resume
        let resumed = mgr.resume_plugin("lifecycle_test")
            .expect("resume ok");
        assert!(resumed);

        // Unload
        let unloaded = mgr.unload_plugin("lifecycle_test")
            .expect("unload ok");
        assert!(unloaded);

        // Double unload returns false
        let double_unload = mgr.unload_plugin("lifecycle_test")
            .expect("double unload ok");
        assert!(!double_unload);
    }

    #[test]
    fn test_manifest_parsing() {
        let json = r#"{
            "name": "test-plugin",
            "version": "1.2.3",
            "description": "A test plugin",
            "author": "Tester",
            "license": "Apache-2.0",
            "dscarp_min_version": "0.5.0",
            "permissions": ["FileAccess", "NetworkAccess"],
            "entry_point": "plugin_init",
            "dependencies": []
        }"#;

        let manifest: PluginManifest = serde_json::from_str(json)
            .expect("should parse valid manifest");
        assert_eq!(manifest.name, "test-plugin");
        assert_eq!(manifest.version, "1.2.3");
        assert_eq!(manifest.permissions.len(), 2);
        assert_eq!(manifest.entry_point, "plugin_init");

        // Invalid JSON should fail
        let result: Result<PluginManifest, _> = serde_json::from_str("invalid");
        assert!(result.is_err(), "invalid JSON should fail to parse");
    }

    #[test]
    fn test_load_nonexistent_dll() {
        let mut mgr = PluginManager::new();
        let nonexistent = PathBuf::from("/tmp/this_plugin_does_not_exist_abcdef123456.dll");
        let result = mgr.load_dynamic_plugin(&nonexistent);
        assert!(result.is_err(), "Loading a nonexistent DLL should return an error");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("Failed to load dynamic library"),
            "Error should mention library loading failure, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_dynamic_plugin_lifecycle() {
        let mut mgr = PluginManager::new();

        // Create a temporary directory and write a fake manifest JSON file
        // (we use a .json path to test the fallback/synthetic manifest path)
        let dir = tempfile::tempdir().expect("tempdir");
        let fake_dll_path = dir.path().join("test_fake_plugin.dll");

        // Write some garbage bytes as a "fake" DLL — libloading will fail to load it,
        // which is exactly what we want to test: the error path.
        std::fs::write(&fake_dll_path, b"NOT_A_REAL_DLL")
            .expect("write fake dll");

        // Attempting to load this should fail because it's not a valid shared library
        let result = mgr.load_dynamic_plugin(&fake_dll_path);
        assert!(result.is_err(), "Fake DLL should fail to load");

        // Now test that we can still load via normal manifest path alongside dynamic loading
        let _ = mgr.load_plugin_from_manifest(PluginManifest {
            name: "normal_plugin".to_string(),
            version: "0.1.0".to_string(),
            description: "Normal".to_string(),
            author: "test".to_string(),
            license: "MIT".to_string(),
            dscarp_min_version: "0.1.0".to_string(),
            permissions: vec![],
            entry_point: "init".to_string(),
            dependencies: vec![],
        });

        // Verify normal plugin loaded fine (no dyn_handle)
        let plugins = mgr.list_plugins();
        assert_eq!(plugins.len(), 1);
        assert!(plugins[0].dyn_handle.is_none());

        // Verify status report works
        let status = mgr.status();
        assert_eq!(status.total_plugins, 1);
    }

    #[test]
    fn test_call_tool_no_plugins() {
        let mgr = PluginManager::new();
        let result = mgr.call_plugin_tool(
            "nonexistent_tool",
            serde_json::json!({"key": "value"}),
        );
        assert!(result.is_err(), "Calling tool with no plugins should return an error");
        let err_msg = result.unwrap_err();
        assert!(
            err_msg.contains("No plugin provides tool"),
            "Error should mention missing plugin/tool, got: {}",
            err_msg
        );
    }
}
