# Getting Started with Assura

From install to a verified, runnable implementation.

This guide works on a clean machine (no monorepo required). It uses a
tiny result-bearing showcase contract. For synthesizable ensures shapes
(including `result == x`), `assura check` synthesizes an in-memory
implementation body so you get **Verified** without writing IR by hand.
Optional co-located IR and `assura build --write-ir` cover shapes
synthesis cannot prove.

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

**Synthesizable** ensures shapes (no hand IR): `result == x`, arithmetic
including nested/`-x`/`abs`/`min`/`max`/`clamp`/`signum` and nested calls
like `abs(min(x,y))`, inequality witnesses `result >= e` / `result > e` /
`result <= e` / `result < e` and conjuncts `result >= lo && result <= hi`
(weakest equality or ±1 body), `let` bindings, field loads `p.x` / `p.y`
and nested `o.inner.v` on multi-field structs (newline-separated fields
are fine), tuple projections `t.0` / `t.1` (and nested chains like
`t.1.0`; use a trailing comma for 1-tuples: `(Int,)`; empty `(,)` is
rejected), collection length `result == xs.length()` (also `.len()` /
`.size()` on List/Bytes/String), Bool `!`/`&&`/`||`/`=>` and comparisons,
same-file pure call chains, nested if, match arms the planner knows.

Shapes the planner cannot synthesize still report **Unknown** (not a fake
pass), with a tip to write co-located IR, `assura build --write-ir`, or
`--auto-implement`. Inequality synthesis picks one witness body (e.g.
`result >= x` uses `result = x`); it is not a full specification of every
satisfying implementation.

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
- **AI / auto-implement:** `assura build --auto-implement` (LLM + IR loop)
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
