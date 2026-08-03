# `check-rust` demos

Inline Rust contracts (`/// @ensures`) proved with `assura check-rust`.
Surface map: [docs/CHECK-RUST-SURFACE.md](../../docs/CHECK-RUST-SURFACE.md).

## Prove (expect exit 0)

```bash
assura check-rust demos/check-rust/ok
# or per file:
assura check-rust demos/check-rust/ok/clamp.rs
assura check-rust demos/check-rust/ok/inc.rs
assura check-rust demos/check-rust/ok/abs.rs
assura check-rust demos/check-rust/ok/inc_mut.rs
```

| File | Point |
|------|--------|
| `ok/clamp.rs` | if/else + `result >= 0` |
| `ok/inc.rs` | pure `let` + `result == x + 1` |
| `ok/abs.rs` | `x.abs()` method body |
| `ok/inc_mut.rs` | linear `let mut` + `+=` |

## Fail intentionally (expect exit non-zero)

```bash
assura check-rust demos/check-rust/fail/clamp_wrong.rs
# JSON: errors >= 1, body_not_modeled == 0
assura check-rust demos/check-rust/fail/clamp_wrong.rs --json
```

This is the agent/debug loop: wrong ensures or body → fix → re-check.

## Gate

```bash
bash scripts/check-rust-demos.sh
```

Also covered by `cargo test -p assura --test check_rust_demos --locked`.
