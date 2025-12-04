/**
 * Deadmod VSCode Extension
 *
 * Provides real-time dead module detection for Rust projects
 * by connecting to the deadmod-lsp language server.
 */

import * as vscode from "vscode";
import * as path from "path";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

/**
 * Find the deadmod-lsp executable path.
 */
function getServerPath(): string {
  const config = vscode.workspace.getConfiguration("deadmod");
  const configPath = config.get<string>("serverPath");

  if (configPath && configPath !== "deadmod-lsp") {
    return configPath;
  }

  // Try to find in common locations
  const possiblePaths = [
    // If installed via cargo install
    "deadmod-lsp",
    // Workspace target directory (for development)
    path.join(
      vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || "",
      "target",
      "release",
      "deadmod-lsp"
    ),
    path.join(
      vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || "",
      "target",
      "debug",
      "deadmod-lsp"
    ),
  ];

  // On Windows, add .exe extension
  if (process.platform === "win32") {
    return possiblePaths[0] + ".exe";
  }

  return possiblePaths[0];
}

/**
 * Activate the extension.
 */
export function activate(context: vscode.ExtensionContext): void {
  const serverPath = getServerPath();

  // Server options - run the deadmod-lsp binary
  const serverOptions: ServerOptions = {
    command: serverPath,
    args: [],
    transport: TransportKind.stdio,
  };

  // Client options - configure for Rust files
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "rust" }],
    synchronize: {
      // Watch for changes to Rust files
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.rs"),
    },
    outputChannelName: "Deadmod",
  };

  // Create the language client
  client = new LanguageClient(
    "deadmod-lsp",
    "Deadmod Language Server",
    serverOptions,
    clientOptions
  );

  // Start the client (and server)
  client.start().catch((error) => {
    vscode.window.showErrorMessage(
      `Failed to start Deadmod LSP: ${error.message}. ` +
        `Make sure deadmod-lsp is installed and in PATH.`
    );
  });

  // Register commands
  context.subscriptions.push(
    vscode.commands.registerCommand("deadmod.run", runAnalysis),
    vscode.commands.registerCommand("deadmod.fix", fixDeadModules),
    vscode.commands.registerCommand("deadmod.openGraph", openGraphViewer)
  );

  // Show status message
  vscode.window.setStatusBarMessage("Deadmod: Ready", 3000);
}

/**
 * Deactivate the extension.
 */
export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}

/**
 * Command: Run Deadmod analysis manually.
 */
async function runAnalysis(): Promise<void> {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    vscode.window.showWarningMessage("No active editor");
    return;
  }

  if (editor.document.languageId !== "rust") {
    vscode.window.showWarningMessage("Not a Rust file");
    return;
  }

  // Save the document to trigger analysis
  await editor.document.save();

  vscode.window.showInformationMessage("Deadmod: Analysis triggered");
}

/**
 * Command: Fix dead modules using deadmod CLI.
 */
async function fixDeadModules(): Promise<void> {
  const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
  if (!workspaceFolder) {
    vscode.window.showWarningMessage("No workspace folder open");
    return;
  }

  // First, show what would be fixed (dry run)
  const dryRunResult = await vscode.window.showWarningMessage(
    "This will remove dead modules and their declarations. Run dry-run first?",
    "Dry Run",
    "Fix Now",
    "Cancel"
  );

  if (dryRunResult === "Cancel" || !dryRunResult) {
    return;
  }

  const terminal = vscode.window.createTerminal("Deadmod Fix");
  terminal.show();

  if (dryRunResult === "Dry Run") {
    terminal.sendText(`deadmod "${workspaceFolder.uri.fsPath}" --fix-dry-run`);
  } else {
    terminal.sendText(`deadmod "${workspaceFolder.uri.fsPath}" --fix`);
  }
}

/**
 * Command: Open the HTML graph viewer.
 */
async function openGraphViewer(): Promise<void> {
  const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
  if (!workspaceFolder) {
    vscode.window.showWarningMessage("No workspace folder open");
    return;
  }

  const graphPath = path.join(workspaceFolder.uri.fsPath, "deadmod-graph.html");

  // Generate the graph using deadmod CLI
  const terminal = vscode.window.createTerminal("Deadmod Graph");
  terminal.sendText(
    `deadmod "${workspaceFolder.uri.fsPath}" --html-file "${graphPath}" && echo "Graph saved to ${graphPath}"`
  );
  terminal.show();

  // Wait a moment for the file to be generated, then open it
  setTimeout(async () => {
    try {
      const uri = vscode.Uri.file(graphPath);
      // Try to open in browser
      await vscode.env.openExternal(uri);
    } catch {
      // If external open fails, show in VSCode
      vscode.window.showInformationMessage(
        `Graph saved to: ${graphPath}. Open it in a browser to view.`
      );
    }
  }, 2000);
}
