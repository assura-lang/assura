# assura-rust-analyzer

Reads inline [Assura](https://github.com/assura-lang/assura) contracts from
Rust source.

Doc comments such as `/// @requires` and `/// @ensures` are parsed and
attached to the function, struct, or impl they document. `RustAdapter` is the
adapter for `.rs` files.

Depend on this crate when you analyze Rust that already contains Assura
annotations. The `assura check-rust` command uses it.

```rust
let adapter = assura_rust_analyzer::adapter_for_extension("rs");
assert!(adapter.is_some());
```

## Documentation

- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)

## License

MIT OR Apache-2.0.
