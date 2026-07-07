# Getting Started with Assura

One boring path from install to a verified, runnable implementation.

This guide works on a clean machine (no monorepo required). It uses a
tiny result-bearing showcase contract plus a co-located Implementation IR
sidecar so `assura check` can **verify** (not only type-check), and
`assura build` can emit real Rust instead of `todo!()`.

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

Write the contract (`ShowcaseEcho.assura`) and co-located IR sidecar
(`ShowcaseEcho.ir`). The IR file name must match the **contract name**
(not the `.assura` file stem):

```bash
cat > ShowcaseEcho.assura << 'EOF'
// SHOWCASE: result-bearing identity with co-located IR.
contract ShowcaseEcho {
  input(x: Int)
  output(result: Int)
  ensures { result == x }
}
EOF

cat > ShowcaseEcho.ir << 'EOF'
module ShowcaseEcho {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $result = load $0 : Int
  }
}
EOF
```

In the Assura monorepo you can use the same files already under
`demos/showcase-echo.assura` and `demos/ShowcaseEcho.ir`.

## 3. Check (prefer all clauses Verified)

```bash
assura check ShowcaseEcho.assura
```

Expected: `ShowcaseEcho: ensures ... verified` and `check passed (no errors)`.

Without the `.ir` sidecar, an ensures on unconstrained `result` is
skipped or reported as unknown, with a tip to add IR or run
`assura build --auto-implement`. That is intentional: contract-only
postconditions on free outputs are not silently proved.

## 4. Build (IR becomes the implementation)

```bash
assura build ShowcaseEcho.assura --output generated
```

Assura loads co-located IR for verification and injects it into the
generated Rust body (you should see a log line about injected IR bodies).
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
| Check | `assura check ...` | SMT proved `result == x` under the IR body |
| Build | `assura build ...` | Rust body implements the same IR (not `todo!()`) |
| Test | `cargo test` | Runtime asserts + proptest agree with the postcondition |

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
