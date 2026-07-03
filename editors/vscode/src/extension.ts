import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";
import { ContractOverlayProvider } from "./contractOverlay";

let client: LanguageClient | undefined;
let overlayProvider: ContractOverlayProvider | undefined;

export async function activate(
  context: vscode.ExtensionContext
): Promise<void> {
  const serverPath = getServerPath();
  if (!serverPath) {
    vscode.window.showErrorMessage(
      "Assura LSP server not found. Install the assura-lsp binary or set assura.serverPath in settings."
    );
    return;
  }

  const serverOptions: ServerOptions = {
    run: { command: serverPath },
    debug: { command: serverPath },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "assura" }],
  };

  client = new LanguageClient(
    "assura",
    "Assura Language Server",
    serverOptions,
    clientOptions
  );

  await client.start();
  context.subscriptions.push({
    dispose: () => {
      if (client) {
        client.stop();
      }
    },
  });

  // Set up contract overlay for Rust files
  overlayProvider = new ContractOverlayProvider();
  context.subscriptions.push(overlayProvider);

  context.subscriptions.push(
    vscode.commands.registerCommand("assura.toggleContractOverlay", () => {
      if (overlayProvider) {
        overlayProvider.toggle();
      }
    })
  );
}

export async function deactivate(): Promise<void> {
  if (overlayProvider) {
    overlayProvider.dispose();
    overlayProvider = undefined;
  }
  if (client) {
    await client.stop();
    client = undefined;
  }
}

function getServerPath(): string | undefined {
  const config = vscode.workspace.getConfiguration("assura");
  const configPath = config.get<string>("serverPath");
  if (configPath && configPath.length > 0) {
    return configPath;
  }
  // Fall back to looking for the binary in PATH.
  // The LanguageClient will resolve it from PATH automatically
  // when given just the binary name.
  return "assura-lsp";
}
