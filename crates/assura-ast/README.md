# assura-ast

Canonical AST for the [Assura](https://github.com/assura-lang/assura) compiler.
`Expr`, `Decl`, spans, and the visitors live here.

Depend on this crate when you walk or build Assura syntax. The parser
produces these types. Downstream crates should not define a second AST.

Most embedders want
[assura-pipeline](https://crates.io/crates/assura-pipeline) instead of this crate.

```rust
use assura_ast::Expr;

fn is_result(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(name) if name == "result")
}
```

## Documentation

- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)
- [Specification](https://github.com/assura-lang/assura/blob/main/docs/SPECIFICATION.md)

## License

MIT OR Apache-2.0.
