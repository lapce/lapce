//! Native GitHub Copilot integration.
//!
//! Copilot ships a standard language server (`copilot-language-server`) that
//! speaks LSP with a few Copilot specific extensions:
//!
//!   * `initialize` must carry `editorInfo` and `editorPluginInfo` inside
//!     `initializationOptions`, and advertise the
//!     `textDocument.inlineCompletion` client capability.
//!   * authentication is driven by the custom `signIn` / `checkStatus` /
//!     `signOut` requests (GitHub device flow).
//!   * suggestions are delivered through the standard
//!     `textDocument/inlineCompletion` request.
//!
//! Rather than reinventing the document syncing and inline completion routing,
//! Copilot is started through the regular [`LspClient`] machinery with a
//! Copilot flavoured `initialize`. Once running it registers as an inline
//! completion provider, so the existing inline completion plumbing (ghost text
//! rendering and the accept keybinding) drives it for free.

use std::path::{Path, PathBuf};

use anyhow::Result;
use lapce_core::meta;
use lapce_rpc::plugin::{PluginId, VoltID};
use lsp_types::{DocumentFilter, DocumentSelector, Url};
use serde_json::{Value, json};

use super::{PluginCatalogRpcHandler, lsp::LspClient};

/// Synthetic plugin author for the built in Copilot client.
pub const COPILOT_PLUGIN_AUTHOR: &str = "lapce";
/// Synthetic plugin name for the built in Copilot client.
pub const COPILOT_PLUGIN_NAME: &str = "copilot";
/// Human readable name shown in logs and messages.
pub const COPILOT_DISPLAY_NAME: &str = "GitHub Copilot";
/// Default command used to launch the Copilot language server.
pub const DEFAULT_SERVER_PATH: &str = "copilot-language-server";

/// Custom Copilot request: begin the GitHub device flow sign in.
pub const SIGN_IN: &str = "signIn";
/// Custom Copilot request: report the current authentication status.
pub const CHECK_STATUS: &str = "checkStatus";
/// Custom Copilot request: sign out of GitHub Copilot.
pub const SIGN_OUT: &str = "signOut";

/// The synthetic [`VoltID`] used to track the Copilot language server amongst
/// the regular plugins in the catalog.
pub fn copilot_volt_id() -> VoltID {
    VoltID {
        author: COPILOT_PLUGIN_AUTHOR.to_string(),
        name: COPILOT_PLUGIN_NAME.to_string(),
    }
}

/// The `initializationOptions` Copilot requires on `initialize`.
pub fn initialization_options() -> Value {
    json!({
        "editorInfo": {
            "name": "Lapce",
            "version": meta::VERSION,
        },
        "editorPluginInfo": {
            "name": "lapce-copilot",
            "version": meta::VERSION,
        },
    })
}

/// A document selector that matches every file so Copilot is offered for all
/// languages, mirroring its behaviour in other editors.
pub fn document_selector() -> DocumentSelector {
    vec![DocumentFilter {
        language: None,
        scheme: None,
        pattern: Some("**".to_string()),
    }]
}

/// Turn a configured server path into a [`Url`] understood by [`LspClient`].
///
/// An absolute path is launched directly (`file:` scheme); a bare command name
/// is resolved through `PATH` (`urn:` scheme).
fn server_uri(server_path: &str) -> Result<Url> {
    let path = Path::new(server_path);
    if path.is_absolute() {
        Url::from_file_path(path).map_err(|_| {
            anyhow::anyhow!("invalid copilot server path: {server_path}")
        })
    } else {
        Ok(Url::parse(&format!("urn:{server_path}"))?)
    }
}

/// Start the Copilot language server and register it with the plugin catalog.
///
/// The returned [`PluginId`] is allocated up front; the server finishes
/// initialising asynchronously and only participates in inline completion once
/// it reports back as loaded.
pub fn start(
    catalog_rpc: PluginCatalogRpcHandler,
    workspace: Option<PathBuf>,
    server_path: String,
    server_args: Vec<String>,
) -> Result<PluginId> {
    let server_path = if server_path.trim().is_empty() {
        DEFAULT_SERVER_PATH.to_string()
    } else {
        server_path
    };
    let args = if server_args.is_empty() {
        vec!["--stdio".to_string()]
    } else {
        server_args
    };

    let server_uri = server_uri(&server_path)?;

    catalog_rpc.core_rpc.log(
        lapce_rpc::core::LogLevel::Info,
        format!("starting {COPILOT_DISPLAY_NAME} language server: {server_path}"),
        Some("lapce_proxy::plugin::copilot::start".to_string()),
    );

    LspClient::start(
        catalog_rpc,
        document_selector(),
        workspace,
        copilot_volt_id(),
        COPILOT_DISPLAY_NAME.to_string(),
        None,
        None,
        None,
        server_uri,
        args,
        Some(initialization_options()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_options_shape() {
        let options = initialization_options();
        assert_eq!(options["editorInfo"]["name"], "Lapce");
        assert_eq!(options["editorPluginInfo"]["name"], "lapce-copilot");
        assert!(options["editorInfo"]["version"].is_string());
        assert!(options["editorPluginInfo"]["version"].is_string());
    }

    #[test]
    fn bare_command_uses_urn_scheme() {
        let uri = server_uri("copilot-language-server").unwrap();
        assert_eq!(uri.scheme(), "urn");
        assert_eq!(uri.path(), "copilot-language-server");
    }

    #[test]
    fn absolute_path_uses_file_scheme() {
        let uri = server_uri("/usr/local/bin/copilot-language-server").unwrap();
        assert_eq!(uri.scheme(), "file");
    }
}
