# assura-parser

Lexer, concrete syntax tree, and lowering for
[Assura](https://github.com/assura-lang/assura).

Depend on this crate to turn source text into an AST. `parse` keeps going
after a syntax error and returns the partial tree plus diagnostics.

Most embedders want
[assura-pipeline](https://crates.io/crates/assura-pipeline) instead of calling
the parser directly.

```rust
let (file, errors) = assura_parser::parse("contract C { }");
assert!(file.is_some() || !errors.is_empty());
```

## Documentation

- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)
- [Specification](https://github.com/assura-lang/assura/blob/main/docs/SPECIFICATION.md)

## License

MIT OR Apache-2.0.
