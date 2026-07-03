# crates.io packaging

## What ships on crates.io

Library crates in dependency order (computed by `scripts/publish-crates.sh`):

`assura-ast` → `assura-config` → `assura-diagnostics` → `assura-macros` →
`assura-runtime` → `assura-parser` → `assura-fmt` → `assura-stdlib` →
`assura-resolve` → `assura-types` → `assura-codegen` → `assura-smt` →
**`assura-pipeline`** (preferred public embed API).

## What does not (yet)

| Package | Reason |
|---------|--------|
| `assura` (CLI binary package) | Depends on unpublished frontends (`assura-lsp`, `assura-mcp`, `assura-llm`, …). Install via **GitHub Releases / cargo-dist**, not `cargo install assura`. |
| `assura-test-support` | Internal test helpers only (`publish = false`). |
| `assura-lsp` / `mcp` / `llm` / `server` / … | Product frontends; not required for the library stack. |

## Local dry-run

```bash
bash scripts/publish-crates.sh --dry-run
```

Fail-closed real publish (CI release job):

```bash
CARGO_REGISTRY_TOKEN=… bash scripts/publish-crates.sh
```
