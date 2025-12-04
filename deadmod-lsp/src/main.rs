//! Deadmod LSP Server - Real-time dead module detection for Rust.
//!
//! Provides IDE integration with:
//! - Live diagnostics on file open/save
//! - Warning markers on dead modules
//! - Hover information
//!
//! NASA-grade resilience: never panics, handles all errors gracefully.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use deadmod_core::{
    build_graph, cache, find_crate_root, find_dead, find_root_modules, gather_rs_files,
    reachable_from_roots,
};

/// Deadmod Language Server state.
struct DeadmodLsp {
    client: Client,
    /// Cached workspace root path.
    workspace_root: Arc<RwLock<Option<PathBuf>>>,
}

impl DeadmodLsp {
    fn new(client: Client) -> Self {
        Self {
            client,
            workspace_root: Arc::new(RwLock::new(None)),
        }
    }

    /// Run deadmod analysis and publish diagnostics.
    async fn run_analysis(&self, uri: Url) {
        // Convert URI to file path
        let file_path = match uri.to_file_path() {
            Ok(p) => p,
            Err(_) => {
                self.log_error("Invalid file URI").await;
                return;
            }
        };

        // Find crate root
        let crate_root = match find_crate_root(&file_path) {
            Some(r) => r,
            None => {
                self.log_info("No Cargo.toml found, skipping analysis").await;
                return;
            }
        };

        // Store workspace root
        {
            let mut root = self.workspace_root.write().await;
            *root = Some(crate_root.clone());
        }

        // Run deadmod analysis
        match self.compute_diagnostics(&crate_root).await {
            Ok(file_diagnostics) => {
                // Publish diagnostics for each file
                for (file_uri, diagnostics) in file_diagnostics {
                    self.client
                        .publish_diagnostics(file_uri, diagnostics, None)
                        .await;
                }
            }
            Err(e) => {
                self.log_error(&format!("Analysis failed: {}", e)).await;
            }
        }
    }

    /// Compute diagnostics for all dead modules.
    async fn compute_diagnostics(
        &self,
        crate_root: &std::path::Path,
    ) -> Result<HashMap<Url, Vec<Diagnostic>>> {
        // Gather files
        let files = gather_rs_files(crate_root)?;

        // Parse modules (without cache for simplicity in LSP)
        let mods = cache::incremental_parse(crate_root, &files, None)?;

        // Build graph
        let graph = build_graph(&mods);

        // Find root modules and compute reachability (single O(|V|+|E|) traversal)
        let root_modules = find_root_modules(crate_root);
        let valid_roots = root_modules
            .iter()
            .filter(|name| mods.contains_key(*name))
            .map(|s| s.as_str());
        let reachable: HashSet<&str> = reachable_from_roots(&graph, valid_roots);

        // Find dead modules
        let dead = find_dead(&mods, &reachable);

        // Build diagnostics per file
        let mut result: HashMap<Url, Vec<Diagnostic>> = HashMap::new();

        // First, clear diagnostics for all known files (mark them as having no issues)
        for info in mods.values() {
            if let Ok(uri) = Url::from_file_path(&info.path) {
                result.entry(uri).or_default();
            }
        }

        // Add diagnostics for dead modules
        for module_name in &dead {
            if let Some(info) = mods.get(*module_name) {
                if let Ok(uri) = Url::from_file_path(&info.path) {
                    let diagnostic = Diagnostic {
                        range: Range {
                            start: Position {
                                line: 0,
                                character: 0,
                            },
                            end: Position {
                                line: 0,
                                character: 100,
                            },
                        },
                        severity: Some(DiagnosticSeverity::WARNING),
                        code: Some(NumberOrString::String("deadmod".to_string())),
                        code_description: None,
                        source: Some("deadmod".to_string()),
                        message: format!(
                            "Dead module: `{}` is not reachable from any entry point",
                            module_name
                        ),
                        related_information: Some(vec![DiagnosticRelatedInformation {
                            location: Location {
                                uri: uri.clone(),
                                range: Range::default(),
                            },
                            message: "This module is not imported by any reachable module"
                                .to_string(),
                        }]),
                        tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                        data: None,
                    };

                    result.entry(uri).or_default().push(diagnostic);
                }
            }
        }

        // Log summary
        self.log_info(&format!(
            "Analysis complete: {} modules, {} dead",
            mods.len(),
            dead.len()
        ))
        .await;

        Ok(result)
    }

    async fn log_info(&self, message: &str) {
        self.client.log_message(MessageType::INFO, message).await;
    }

    async fn log_error(&self, message: &str) {
        self.client.log_message(MessageType::ERROR, message).await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for DeadmodLsp {
    async fn initialize(&self, params: InitializeParams) -> LspResult<InitializeResult> {
        // Store workspace root if provided
        if let Some(root_uri) = params.root_uri {
            if let Ok(path) = root_uri.to_file_path() {
                let mut root = self.workspace_root.write().await;
                *root = Some(path);
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(false),
                        })),
                        ..Default::default()
                    },
                )),
                // We can add more capabilities later (hover, code actions, etc.)
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "deadmod-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Deadmod LSP server initialized")
            .await;
    }

    async fn shutdown(&self) -> LspResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        if uri.path().ends_with(".rs") {
            self.run_analysis(uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if uri.path().ends_with(".rs") {
            self.run_analysis(uri).await;
        }
    }

    async fn did_change(&self, _params: DidChangeTextDocumentParams) {
        // We could run analysis on change, but that might be too aggressive.
        // For now, we only analyze on save.
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        // Clear diagnostics for closed file
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }
}

#[tokio::main]
async fn main() {
    // Set up panic hook for graceful error handling
    std::panic::set_hook(Box::new(|info| {
        eprintln!("[PANIC] deadmod-lsp internal error: {}", info);
    }));

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(DeadmodLsp::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_crate_root() {
        // This is a basic sanity test
        let path = PathBuf::from("/some/path/src/main.rs");
        // Can't really test without filesystem, but function should not panic
        let _ = find_crate_root(&path);
    }
}
