# assura-types

Type checker for the [Assura](https://github.com/assura-lang/assura) compiler.
Layer 0 lives here: base types, refinements, and the checker pipeline.

Depend on this crate when you have a resolved file and need a `TypedFile`.
New checkers are registered in `CHECKER_PIPELINE`.

Most embedders want
[assura-pipeline](https://crates.io/crates/assura-pipeline) instead of calling
the type checker directly.

## Documentation

- [Checker layers](https://github.com/assura-lang/assura/blob/main/crates/assura-types/src/CHECKER-LAYERS.md)
- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)
- [Error codes](https://github.com/assura-lang/assura/blob/main/docs/error-codes.md)

## License

MIT OR Apache-2.0.
