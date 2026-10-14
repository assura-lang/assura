# assura-codegen

Rust source generation for [Assura](https://github.com/assura-lang/assura).

`codegen` takes a `TypedFile` and returns a `GeneratedProject` (a `Cargo.toml`
plus Rust sources). Contract clauses become runtime checks. The Rust text is
formatted with `prettyplease`.

Depend on this crate when you already have a typed file and only need the
Rust output. Most embedders want
[assura-pipeline](https://crates.io/crates/assura-pipeline) `compile_full`
instead.

## Documentation

- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)
- [Specification, codegen](https://github.com/assura-lang/assura/blob/main/docs/SPECIFICATION.md)

## License

MIT OR Apache-2.0.
