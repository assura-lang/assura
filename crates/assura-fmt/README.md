# assura-fmt

Formatter for the [Assura](https://github.com/assura-lang/assura) language.

`format_source` walks the concrete syntax tree, so comments and clause text
stay put. It normalizes indentation to 4 spaces, strips trailing whitespace,
and ends the file with one newline.

Depend on this crate to format Assura source inside an editor or a build
step. The `assura fmt` command uses it.

```rust
let formatted = assura_fmt::format_source("contract C {\nrequires { true }\n}\n");
assert!(formatted.contains("contract C"));
```

## Documentation

- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)

## License

MIT OR Apache-2.0.
