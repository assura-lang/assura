# assura-diagnostics

Diagnostic types for the [Assura](https://github.com/assura-lang/assura)
compiler.

Every pass emits a `Diagnostic` with a code, a span, and a message. The CLI
renders that as text (ariadne) or JSON. `explain` looks up the catalog entry
for a code such as `A05100`.

Depend on this crate when you format or filter compiler diagnostics. You do
not need it to run a check. Use
[assura-pipeline](https://crates.io/crates/assura-pipeline) for that.

## Documentation

- [Error codes](https://github.com/assura-lang/assura/blob/main/docs/error-codes.md)
- [For AI agents](https://github.com/assura-lang/assura/blob/main/docs/AI-AGENTS.md)

## License

MIT OR Apache-2.0.
