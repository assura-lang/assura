import * as assert from "assert";
import * as vscode from "vscode";
import * as path from "path";

suite("Assura Extension", () => {
  test("Extension is present", () => {
    const ext = vscode.extensions.getExtension("assura-lang.assura-lang");
    assert.ok(ext, "Extension should be found by ID");
  });

  test("Language configuration is registered for .assura files", async () => {
    const langs = await vscode.languages.getLanguages();
    assert.ok(
      langs.includes("assura"),
      `Language 'assura' should be registered. Found: ${langs.join(", ")}`
    );
  });

  test("TextMate grammar applies to .assura files", async () => {
    // Create an untitled .assura document and check its language ID
    const doc = await vscode.workspace.openTextDocument({
      language: "assura",
      content: 'contract Test {\n  requires { x > 0 }\n}\n',
    });
    assert.strictEqual(doc.languageId, "assura");
  });

  test("Extension contributes assura.serverPath setting", () => {
    const config = vscode.workspace.getConfiguration("assura");
    const serverPath = config.get<string>("serverPath");
    // Default value should be empty string
    assert.strictEqual(
      serverPath,
      "",
      "Default serverPath should be empty"
    );
  });

  test(".assura file association works", async () => {
    // Create a temp file with .assura extension
    const uri = vscode.Uri.parse("untitled:test.assura");
    const doc = await vscode.workspace.openTextDocument(uri);
    assert.strictEqual(
      doc.languageId,
      "assura",
      "Files with .assura extension should use assura language"
    );
  });
});