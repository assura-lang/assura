# assura-runtime

Runtime contract checks for Rust generated from
[Assura](https://github.com/assura-lang/assura).

When codegen is asked for runtime checks, `requires`, `ensures`, and
`invariant` stay in release builds. A violation goes to a handler you can
replace. The handler can log, panic, or record the failure.

Depend on this crate from generated code, or when you want the same handler
API. You do not need it to prove a contract. Use
[assura-pipeline](https://crates.io/crates/assura-pipeline) for that.

## Documentation

- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)

## License

MIT OR Apache-2.0.
