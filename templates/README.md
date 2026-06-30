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
- `ir/` -- IR-specific templates
  - `base.md` -- base IR generation prompt
  - `patterns/` -- common IR patterns (arithmetic, bounds checks, etc.)

## Configuration

The LLM provider is configured in `assura.toml`:

```toml
[ai]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
```

See the root README for all `[ai]` configuration options.
