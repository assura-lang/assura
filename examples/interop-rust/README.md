# Interop sample: normal Rust + `check-rust`

Shows how Assura fits next to ordinary Rust development:

1. Write (or generate) Rust as usual.
2. Add `/// @requires` / `@ensures` on functions you care about.
3. Run `assura check-rust` for SMT proof of the **modeled** surface.
4. Keep `cargo test` for runtime behavior.

This is **not** Verus-in-place for arbitrary crates. Unmodeled bodies report
`body_not_modeled` (fail closed). Map: [CHECK-RUST-SURFACE.md](../../docs/CHECK-RUST-SURFACE.md).

## Commands

```bash
# From repo root
cargo test --manifest-path examples/interop-rust/Cargo.toml
assura check-rust examples/interop-rust/src
assura check-rust examples/interop-rust/src --json
```

## What is proved vs not

| Layer | What |
|-------|------|
| `assura check-rust` | Ensures on annotated functions when body encodes (or `.ir` sidecar) |
| `cargo test` | Runtime behavior of the same functions |
| Not claimed | Borrow-aware proofs of the whole crate; unannotated code |

## Related product path

- Full contract language: `*.assura` + `assura check` / `assura build` (emit Rust)
- Compare vs Verus: [COMPARE.md](../../docs/COMPARE.md)
- Agent loop: [AI-AGENTS.md](../../docs/AI-AGENTS.md) and [AGENT-LOOP.md](../../docs/AGENT-LOOP.md)
