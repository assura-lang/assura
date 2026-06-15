import * as vscode from "vscode";

/** A single clause for overlay display. */
interface OverlayClause {
  kind: string;
  body: string;
}

/** A contract overlay item for a function/struct/impl. */
interface ContractOverlayItem {
  name: string;
  line: number;
  kind: string;
  clauses: OverlayClause[];
}

/** Response from the contract overlay extraction. */
interface ContractOverlayResponse {
  items: ContractOverlayItem[];
}

/** Regex pattern for @-clause annotations in doc comments. */
const CLAUSE_PATTERN =
  /^\/\/\/\s*@(requires|ensures|invariant|effects|decreases)\s+(.*)/;

/**
 * Parse contract annotations directly from Rust source text.
 *
 * Extracts `/// @requires`, `/// @ensures`, etc. from doc comments
 * and groups them by the function/struct they precede.
 */
function parseContractAnnotations(text: string): ContractOverlayResponse {
  const lines = text.split("\n");
  const items: ContractOverlayItem[] = [];
  let currentClauses: OverlayClause[] = [];

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    const match = CLAUSE_PATTERN.exec(line);

    if (match) {
      currentClauses.push({ kind: match[1], body: match[2].trim() });
    } else if (currentClauses.length > 0) {
      // Check if this line is the function/struct/impl declaration
      const fnMatch = line.match(
        /^(?:pub\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+(\w+)/
      );
      const structMatch = line.match(/^(?:pub\s+)?struct\s+(\w+)/);
      const implMatch = line.match(
        /^impl(?:<[^>]*>)?\s+(?:(\w+)\s+for\s+)?(\w+)/
      );

      if (fnMatch) {
        items.push({
          name: fnMatch[1],
          line: i,
          kind: "function",
          clauses: [...currentClauses],
        });
        currentClauses = [];
      } else if (structMatch) {
        items.push({
          name: structMatch[1],
          line: i,
          kind: "struct",
          clauses: [...currentClauses],
        });
        currentClauses = [];
      } else if (implMatch) {
        const name = implMatch[1]
          ? `${implMatch[1]} for ${implMatch[2]}`
          : implMatch[2];
        items.push({
          name,
          line: i,
          kind: "impl",
          clauses: [...currentClauses],
        });
        currentClauses = [];
      } else if (!line.startsWith("///") && !line.startsWith("//")) {
        // Non-doc, non-declaration line: discard accumulated clauses
        currentClauses = [];
      }
    }
  }

  return { items };
}

/**
 * Provides contract overlay decorations for Rust files.
 *
 * Renders inline contract annotations (`/// @requires`, `/// @ensures`, etc.)
 * as virtual text decorations above the annotated items, with color-coded
 * clause kinds and a toggle command.
 */
export class ContractOverlayProvider implements vscode.Disposable {
  private enabled: boolean;
  private decorationTypes: Map<string, vscode.TextEditorDecorationType>;
  private disposables: vscode.Disposable[] = [];

  constructor() {
    const config = vscode.workspace.getConfiguration("assura");
    this.enabled = config.get<boolean>("contractOverlay.enabled", true);

    this.decorationTypes = new Map();
    this.decorationTypes.set(
      "requires",
      vscode.window.createTextEditorDecorationType({
        before: {
          color: new vscode.ThemeColor(
            "editorInfo.foreground"
          ),
          fontStyle: "italic",
        },
        isWholeLine: true,
      })
    );
    this.decorationTypes.set(
      "ensures",
      vscode.window.createTextEditorDecorationType({
        before: {
          color: new vscode.ThemeColor(
            "editorHint.foreground"
          ),
          fontStyle: "italic",
        },
        isWholeLine: true,
      })
    );
    this.decorationTypes.set(
      "invariant",
      vscode.window.createTextEditorDecorationType({
        before: {
          color: new vscode.ThemeColor(
            "editorWarning.foreground"
          ),
          fontStyle: "italic",
        },
        isWholeLine: true,
      })
    );
    this.decorationTypes.set(
      "effects",
      vscode.window.createTextEditorDecorationType({
        before: {
          color: new vscode.ThemeColor(
            "editorInfo.foreground"
          ),
          fontStyle: "italic",
        },
        isWholeLine: true,
      })
    );
    this.decorationTypes.set(
      "decreases",
      vscode.window.createTextEditorDecorationType({
        before: {
          color: new vscode.ThemeColor(
            "editorInfo.foreground"
          ),
          fontStyle: "italic",
        },
        isWholeLine: true,
      })
    );

    // Update decorations when the active editor changes
    this.disposables.push(
      vscode.window.onDidChangeActiveTextEditor((editor) => {
        if (editor) {
          this.updateDecorations(editor);
        }
      })
    );

    // Update decorations when the document changes
    this.disposables.push(
      vscode.workspace.onDidChangeTextDocument((event) => {
        const editor = vscode.window.activeTextEditor;
        if (editor && editor.document === event.document) {
          this.updateDecorations(editor);
        }
      })
    );

    // Update decorations when configuration changes
    this.disposables.push(
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration("assura.contractOverlay.enabled")) {
          const config = vscode.workspace.getConfiguration("assura");
          this.enabled = config.get<boolean>(
            "contractOverlay.enabled",
            true
          );
          const editor = vscode.window.activeTextEditor;
          if (editor) {
            this.updateDecorations(editor);
          }
        }
      })
    );

    // Apply to the current active editor
    const editor = vscode.window.activeTextEditor;
    if (editor) {
      this.updateDecorations(editor);
    }
  }

  /** Toggle overlay visibility on/off. */
  toggle(): void {
    this.enabled = !this.enabled;
    const editor = vscode.window.activeTextEditor;
    if (editor) {
      this.updateDecorations(editor);
    }
    vscode.window.showInformationMessage(
      `Assura contract overlay ${this.enabled ? "enabled" : "disabled"}`
    );
  }

  /** Update decorations for the given editor. */
  private updateDecorations(editor: vscode.TextEditor): void {
    // Only apply to Rust files
    if (editor.document.languageId !== "rust") {
      this.clearDecorations(editor);
      return;
    }

    if (!this.enabled) {
      this.clearDecorations(editor);
      return;
    }

    const source = editor.document.getText();
    const response = parseContractAnnotations(source);

    // Group decorations by clause kind
    const decorationsByKind = new Map<string, vscode.DecorationOptions[]>();
    for (const kind of this.decorationTypes.keys()) {
      decorationsByKind.set(kind, []);
    }

    for (const item of response.items) {
      for (const clause of item.clauses) {
        const decos = decorationsByKind.get(clause.kind);
        if (decos) {
          const line = Math.max(0, item.line);
          const range = new vscode.Range(line, 0, line, 0);
          decos.push({
            range,
            renderOptions: {
              before: {
                contentText: `  @${clause.kind} ${clause.body}`,
              },
            },
          });
        }
      }
    }

    // Apply decorations
    for (const [kind, type] of this.decorationTypes) {
      const decos = decorationsByKind.get(kind) || [];
      editor.setDecorations(type, decos);
    }
  }

  /** Clear all contract overlay decorations. */
  private clearDecorations(editor: vscode.TextEditor): void {
    for (const type of this.decorationTypes.values()) {
      editor.setDecorations(type, []);
    }
  }

  dispose(): void {
    for (const type of this.decorationTypes.values()) {
      type.dispose();
    }
    for (const d of this.disposables) {
      d.dispose();
    }
  }
}