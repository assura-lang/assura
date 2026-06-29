# Assura for Visual Studio Code

Syntax highlighting and language server support for the
[Assura](https://github.com/assura-lang/assura) contract-first
verification language.

## Features

- Syntax highlighting for `.assura` files (keywords, types, operators,
  comments, strings, numbers)
- Diagnostics (errors and warnings) from the Assura compiler
- Go to definition
- Hover information
- Code completions
- Document symbols

All IDE features beyond syntax highlighting are provided by the
`assura-lsp` language server.

## Requirements

Install the `assura-lsp` binary and make sure it is available in your
`PATH`. You can build it from the Assura repository:

```bash
cargo install --path crates/assura-lsp
```

Alternatively, set the path manually in VS Code settings:

```json
{
  "assura.serverPath": "/path/to/assura-lsp"
}
```

## Extension Settings

| Setting             | Default | Description                                    |
|---------------------|---------|------------------------------------------------|
| `assura.serverPath` | `""`    | Path to the `assura-lsp` binary. If empty, the extension searches `PATH`. |

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
