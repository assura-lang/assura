# Assura for Visual Studio Code

Syntax highlighting and language server support for the
[Assura](https://github.com/assura-lang/assura) contract-first
verification language.

## Marketplace status

**Not published** to the VS Code Marketplace or Open VSX yet. Install from
this monorepo (developer build) until a public listing exists:

```bash
cd editors/vscode
npm ci
npm run compile
# Package (optional):
npx vsce package
# Install the generated .vsix via VS Code: Extensions → … → Install from VSIX
```

## Features

- Syntax highlighting for `.assura` files (keywords, types, operators,
  comments, strings, numbers)
- Diagnostics (errors and warnings) from the Assura compiler
- Go to definition
- Hover information
- Code completions
- Document symbols

All IDE features beyond syntax highlighting are provided by
`assura lsp` (from the `assura` CLI).

## Requirements

Install the CLI so `assura lsp` is on your `PATH`:

```bash
cargo install assura --locked
```

Alternatively, set a custom server command in VS Code settings (no extra
args are added):

```json
{
  "assura.serverPath": "/path/to/assura"
}
```

A standalone `assura-lsp` binary (`cargo install --path crates/assura-lsp`)
works if you point `assura.serverPath` at that path.

## Extension Settings

| Setting             | Default | Description                                    |
|---------------------|---------|------------------------------------------------|
| `assura.serverPath` | `""`    | Path to the language server. If empty, the extension runs `assura lsp`. |

## Development

```bash
cd editors/vscode
npm install
npm run compile
```

To run the extension in a development host:

1. Open this folder in VS Code
2. Press F5 to launch the Extension Development Host
3. Open any `.assura` file
