# assura-resolve

Name resolution for the [Assura](https://github.com/assura-lang/assura)
compiler. It builds the symbol table, checks imports, and reports unknown
names.

Depend on this crate when you already have a parsed `SourceFile` and need a
`ResolvedFile`. The usual entry is `assura_resolve::resolve`.

Most embedders want
[assura-pipeline](https://crates.io/crates/assura-pipeline) instead of chaining
this pass themselves.

```rust
let file = assura_parser::parse_unwrap("contract C { requires { true } }");
let resolved = assura_resolve::resolve(&file);
assert!(resolved.is_ok());
```

## Documentation

- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)

## License

MIT OR Apache-2.0.
