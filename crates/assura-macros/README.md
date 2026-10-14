# assura-macros

Proc macros for inline [Assura](https://github.com/assura-lang/assura)
contracts on Rust functions.

`#[contract]` reads `@requires` and `@ensures` from doc comments and, in
debug builds, turns them into `debug_assert!`. Release builds keep the
attribute and emit no checks. `#[trust("reason")]` marks a function as
trusted and keeps the reason in the docs.

```rust
use assura_macros::contract;

/// @requires x > 0
/// @ensures result > x
#[contract]
fn inc(x: i64) -> i64 {
    x + 1
}
```

## Documentation

- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)

## License

MIT OR Apache-2.0.
