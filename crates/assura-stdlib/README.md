# assura-stdlib

Standard library contracts for [Assura](https://github.com/assura-lang/assura).

The crate ships the `.assura` files for math, strings, and collections, plus
the prelude that every Assura file can use. Call the query API when a tool
needs those definitions without reading the files itself.

Most programs should `import` the library from Assura source. Depend on this
crate from Rust when you need the file bytes or the prelude list.

## Documentation

- [Specification, standard library](https://github.com/assura-lang/assura/blob/main/docs/SPECIFICATION.md)
- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)

## License

MIT OR Apache-2.0.
