# Getting Started with Assura

From install to a verified, runnable implementation.

This guide works on a clean machine (no monorepo required). It uses a
tiny result-bearing showcase contract. For synthesizable ensures shapes
(including `result == x` and multi-clause bounds), `assura check`
synthesizes an in-memory implementation body so you get **Verified**
without writing IR by hand. Residual shapes use
`assura build --write-ir` (offline) or `--auto-implement` (LLM).

For broader language coverage, see [TUTORIAL.md](TUTORIAL.md) and
[demos/README.md](../demos/README.md) (showcase vs EXPECT FAIL taxonomy).

## 1. Install

Requires Rust 1.85+ (edition 2024). Z3 is pulled in automatically for
verification builds.

```bash
cargo install assura --locked
assura --help
```

From a git checkout instead of crates.io:

```bash
cargo install --path crates/assura-cli --locked
```

## 2. Create a tiny project

```bash
mkdir hello-assura && cd hello-assura
```

Write the contract only (`ShowcaseEcho.assura`). No sidecar required for
identity:

```bash
cat > ShowcaseEcho.assura << 'EOF'
// SHOWCASE: result-bearing identity (auto-synthesized IR).
contract ShowcaseEcho {
  input(x: Int)
  output(result: Int)
  ensures { result == x }
}
EOF
```

In the Assura monorepo you can use `demos/showcase-echo.assura` (and
optional `demos/ShowcaseEcho.ir` if you want an explicit sidecar).

## 3. Check (prefer all clauses Verified)

```bash
assura check ShowcaseEcho.assura
```

Expected: `ShowcaseEcho: ensures ... verified` and `check passed (no errors)`.

**Synthesizable** ensures shapes (no hand IR):

| Family | Examples |
|--------|----------|
| Equality | `result == x`, `result == x + 1`, nested arith, `-x` |
| Builtins | free or method: `abs`/`min`/`max`/`clamp`/`signum` (e.g. `x.abs()`) |
| Bounds | `result >= e`, `>`, `<=`, `<`; And chains; **multi-clause** bounds prefer a lower-bound witness |
| Multi-ensures | Prefer `result == e` when present; combine pure bound ensures otherwise |
| Structure | fields, tuples, length, Bool logic/cmp, if/match/let, same-file pure calls |

Shapes the planner cannot synthesize still report **Unknown** (not a fake
pass). Ladder when that happens:

1. Simplify ensures toward the table above, or
2. `assura build file.assura --write-ir` (offline heuristic IR next to source), or
3. `assura build file.assura --auto-implement` (offline first, then LLM for residuals).

Inequality synthesis picks one **witness** body (e.g. `result >= x` uses
`result = x`); multi-bound clauses share one witness (prefer lower bound).
That is not a full specification of every satisfying implementation.

Use `assura check -v` to see `synthesized in-memory: ContractName`.

### Optional: co-located IR sidecar

When you want an explicit body (or synthesis cannot cover the shape),
add `{ContractName}.ir` next to the source (name matches the **contract**,
not the file stem):

```bash
cat > ShowcaseEcho.ir << 'EOF'
module ShowcaseEcho {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $result = load $0 : Int
  }
}
EOF
```

## 4. Build (IR becomes the implementation)

```bash
assura build ShowcaseEcho.assura --output generated
```

Assura loads co-located IR for verification and injects it into the
generated Rust body (you should see a log line about injected IR bodies).
`assura build --write-ir` also writes that IR next to the source so a later
`assura build` reuses it. Field and abs/min/max/clamp/signum IR lower to real
Rust (`.y`, `.abs()`, `.min()`, `.clamp()`, `.signum()`) that `cargo test`
exercises via proptest.
The generated crate is a library with a property test, not a binary.

## 5. Run tests on the generated artifact

```bash
cd generated
cargo test
```

Expected: proptest test `test_showcaseecho` passes (identity on random `i64`
inputs, with `debug_assert!` on the ensures).

Call the generated API from your own code:

```rust
use generated::contract_showcaseecho::check;

fn main() {
    assert_eq!(check(42), 42);
}
```

A minimal extra smoke test inside the generated crate (optional; do not
commit edits under `generated/`, re-run `assura build` after contract changes):

```bash
cd generated
cat >> src/lib.rs << 'EOF'

#[cfg(test)]
mod smoke {
    #[test]
    fn echo_forty_two() {
        assert_eq!(super::contract_showcaseecho::check(42), 42);
    }
}
EOF
cargo test smoke::echo_forty_two
```

## What you just proved

| Step | Command | What green means |
|------|---------|------------------|
| Check | `assura check ...` | SMT proved the ensures under the IR body (sidecar or synthesized) |
| Build | `assura build ...` | Rust body implements the same IR (not `todo!()`) |
| Test | `cargo test` | Runtime asserts + proptest agree with the postcondition |


## Offline IR (no LLM) and runnable binary

Generate co-located IR from ensures heuristics (no API key), inject it into
Rust, and emit a binary you can `cargo run`:

```bash
assura build ShowcaseEcho.assura --write-ir --bin --output generated
cd generated
cargo run -- 42
# prints: 42
```

### Auto-implement residual shapes

`--auto-implement` fills bodies for contracts that still lack IR:

1. Co-located `.ir` on disk (if any)
2. Offline ensures heuristics (same as `--write-ir`, no API key)
3. LLM only for remaining unanalyzable ensures (needs AI config)

```bash
assura build richer.assura --auto-implement --output generated
```

Strict verification (Unknown limitations fail the check):

```bash
assura check ShowcaseEcho.assura --strict
```

Directory mode for SHOWCASE demos only:

```bash
assura check demos --showcase-only
```

## Next steps

- **More demos:** [demos/README.md](../demos/README.md) (prefer SHOWCASE files)
- **Language tutorial:** [TUTORIAL.md](TUTORIAL.md)
- **Call-shaped helpers:** same-file pure callees get non-identity IR siblings
  when their ensures are analyzable

## Optional monorepo smoke

From a full clone (CI or local):

```bash
bash scripts/smoke-getting-started.sh
# or manually:
assura check demos/showcase-echo.assura
assura build demos/showcase-echo.assura --output /tmp/assura-gs-out
(cd /tmp/assura-gs-out && cargo test)
```
