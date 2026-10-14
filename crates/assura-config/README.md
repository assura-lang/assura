# assura-config

Project configuration for the [Assura](https://github.com/assura-lang/assura)
compiler.

This crate parses `assura.toml` into `ProjectConfig` and defines
`CompilerConfig`, `VerifyOptions`, `OutputMode`, and `Verbosity`. The CLI,
pipeline, and SMT verifier share these types so they do not drift.

Depend on this crate when you need to build a `CompilerConfig` for
[assura-pipeline](https://crates.io/crates/assura-pipeline).

```rust
let options = assura_config::VerifyOptions::for_tests();
assert!(options.timeout_ms > 0);
```

## Documentation

- [Specification, CLI and config](https://github.com/assura-lang/assura/blob/main/docs/SPECIFICATION.md)

## License

MIT OR Apache-2.0.
