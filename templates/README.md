# AI Prompt Templates

Templates used by `assura build --auto-implement` to guide AI code generation.
The compiler sends these prompts (filled with contract details) to the
configured LLM provider, which returns an implementation in Assura IR.

## Structure

- `single-function.md` -- single contract with one function
- `module-level.md` -- multi-contract module
- `service-typestate.md` -- stateful service with typestate
- `concurrency.md` -- concurrent data structures (CONC features)
- `error-propagation.md` -- Result/Option error handling
- `cve-patterns.md` -- CVE prevention patterns
- `ir/README.md` -- pointer only; **canonical IR prompts live in**
  [`crates/assura-smt/templates/ir/`](../crates/assura-smt/templates/ir/)
  (required for `include_str!` / crates.io packaging; do not duplicate
  markdown bodies under monorepo `templates/ir/`)

## Configuration

The LLM provider is configured in `assura.toml`:

```toml
[ai]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
```

See `docs/SPECIFICATION.md` Section 10 for all `[ai]` configuration options.
