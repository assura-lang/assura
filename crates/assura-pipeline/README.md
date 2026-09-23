# assura-pipeline

The canonical compile/verify facade for
[Assura](https://github.com/assura-lang/assura), a contract-first language whose
contracts are proved by an SMT solver (Z3/CVC5) and compiled to Rust.

This is the crate to depend on when you want to **embed** Assura — in a build
script, an editor plugin, an agent tool, or CI. It wraps the whole chain
(parse → resolve → type-check → SMT → codegen) behind a few functions, so you
do not re-chain the compiler passes yourself.

```toml
[dependencies]
assura-pipeline = "0.4"
```

## API

| Function | Does |
|----------|------|
| `compile` | parse → resolve → type-check (no SMT) |
| `compile_full` | the above, plus SMT verification and Rust codegen |
| `verify_typed` | run SMT over an already type-checked file |
| `verify_ir` | verify generated IR against a contract |

Verification options (solver, timeout, layer, parallelism) come from
`assura_config::VerifyOptions` on `CompilerConfig`.

## Reading the result

`CompilationOutput.has_errors` reflects **parse, resolve, and type** errors only.
SMT outcomes live in `output.verification`, so check them explicitly:

```rust
use assura_pipeline::{compile_full, verification_succeeded};

let output = compile_full(source, path, &config);
if output.has_errors {
    // syntax / name / type errors
}
if !verification_succeeded(&output.verification) {
    // counterexamples or timeouts
}
```

Use `verification_succeeded` (lenient) or `verification_strict_succeeded`
(strict). An empty result set means "no proof obligations", which is success —
pair it with the vacuity fields if empty coverage would be misleading.

## Documentation

- [For AI agents](https://github.com/assura-lang/assura/blob/main/docs/AI-AGENTS.md) — JSON output, MCP server
- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md) — architecture and crate map
- [What we prove](https://github.com/assura-lang/assura/blob/main/docs/WHAT-WE-PROVE.md) — the honest limits

## License

MIT OR Apache-2.0.
