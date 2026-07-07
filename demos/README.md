# Assura Demo Contracts

Example `.assura` contracts. **Do not run every file as a must-pass test.**
Demos fall into three buckets (see headers in each file).

## Taxonomy (#864)

| Kind | Meaning | Exit / result | When to use |
|------|---------|---------------|-------------|
| **SHOWCASE (must-pass)** | Happy-path product demos | `assura check` → **no errors** (Unknown *warnings* discouraged) | Install docs, README, first-time users |
| **FEATURE** | Language feature samples | Prefer clean pass; may warn on partial SMT | Learning a feature |
| **EXPECT FAIL** | Adversarial / audit **attack models** | **Errors or counterexamples are intentional** | Teaching what *bad* code looks like under proof |

Header markers (first lines of the file):

```text
// SHOWCASE (must-pass): ...
// EXPECT FAIL: adversarial / audit model — counterexamples or errors are intentional.
```

## Quick start (boring path)

End-to-end install → check → build → `cargo test` (no monorepo required):
see [docs/GETTING-STARTED.md](../docs/GETTING-STARTED.md).

## Quick start (demos in this repo)

```bash
# After: cargo install assura --locked
assura check demos/heartbleed.assura

# Result-bearing postconditions need an implementation body (IR).
# This showcase verifies with a co-located .ir sidecar:
assura check demos/showcase-echo.assura
```

Do **not** start with `demos/*-audit.assura` or `defi-audit.assura` unless you
are studying expected failures.

## SHOWCASE (must-pass)

Preferred for docs and CI smoke:

| File | Notes |
|------|--------|
| `heartbleed.assura` | CVE-style buffer safety; input-only ensures |
| `showcase-echo.assura` + `ShowcaseEcho.ir` | `result == x` with co-located IR (result-bearing path) |
| `zlib-inflate.assura` | Real inflate contracts |
| `integer-overflow.assura` | Overflow-safe arithmetic shape |
| `mbedtls-x509.assura` | X.509 / TLS-shaped contracts |

Also usually clean on current main (feature demos):  
`null-deref`, `double-free`, `use-after-free`, `path-traversal`, `sql-injection`,
`deserialization`, `race-condition`, `crypto-weakness`, `stack-overflow`,
`effect-handler`, `linear-resource`, `typestate-protocol`, `refinement-banking`,
`concurrent-lock`, `mbedtls-audit`.

### Pass with SMT Unknown *warnings* (not errors)

These may print `check passed (N warning)` when some clauses are not yet
encoded. That is a **limitation warning**, not a failed proof of the rest:

| File | Notes |
|------|--------|
| `libwebp-huffman.assura` | Often used in docs; may warn |
| `taint-tracking.assura` | SEC.1 demo; may warn |

Prefer `heartbleed` or `showcase-echo` for a first green check.

## EXPECT FAIL (intentional red)

| File | Intent |
|------|--------|
| `defi-audit.assura` | DeFi exploit patterns (CE expected) |
| `boring-vault-audit.assura` / `boring-vault-audit-deep.assura` | Vault attack models |
| `concurrency-audit.assura` | Concurrency attack models |
| `image-crate-audit.assura` | Image decoding attack models |
| `libssh2-audit.assura` | SSH buffer / window attacks |
| `nghttp2-audit.assura` | HTTP/2 attack models |
| `stb-image-audit.assura` | Image loader attack models |
| `zip-crate-audit.assura` | ZIP parsing attack models |

If `assura check` fails on these, that is **by design** for teaching.

## CVE prevention demos (mixed kinds)

| File | CVE | Class | Kind |
|------|-----|-------|------|
| `heartbleed.assura` | CVE-2014-0160 | Buffer over-read | SHOWCASE |
| `libwebp-huffman.assura` | CVE-2023-4863 | Heap overflow | FEATURE (may warn) |
| `zlib-inflate.assura` | CVE-2022-37434 | Heap overflow | SHOWCASE |
| `mbedtls-x509.assura` | CVE-2023-45199 cluster | TLS/X.509 | SHOWCASE |
| `double-free.assura` | CVE-2014-0195 | Double-free | FEATURE |
| `use-after-free.assura` | CVE-2023-4911 | UAF | FEATURE |
| `null-deref.assura` | CVE-2023-25136 | Null deref | FEATURE |
| `integer-overflow.assura` | CVE-2021-3156 | Integer overflow | SHOWCASE |
| `stack-overflow.assura` | CVE-2022-35737 | Stack overflow | FEATURE |
| `sql-injection.assura` | CVE-2019-9193 | SQLi / RCE | FEATURE |
| `deserialization.assura` | CVE-2021-44228 | Unsafe deser | FEATURE |
| `path-traversal.assura` | CVE-2021-41773 | Path traversal | FEATURE |
| `race-condition.assura` | CVE-2016-5195 | TOCTOU | FEATURE |
| `crypto-weakness.assura` | CVE-2014-3566 | Crypto weakness | FEATURE |

## Language feature demos

| File | Features | Scenario |
|------|----------|----------|
| `taint-tracking.assura` | SEC.1 | Taint / input validation |
| `linear-resource.assura` | MEM.* | Linear file handles |
| `typestate-protocol.assura` | TYPE.* | TLS handshake states |
| `refinement-banking.assura` | Refinement | Banking transfer |
| `effect-handler.assura` | Effects | I/O containment |
| `concurrent-lock.assura` | CONC.* | Lock ordering |
| `showcase-echo.assura` | IR + result | Identity IR verifies `result == x` |

## Result postconditions and IR (#865)

`ensures { result == ... }` needs an **implementation body**. Without a
co-located `.ir` file, `assura check` **auto-synthesizes** analyzable shapes
in memory when it can (identity, simple arithmetic, known call/if/match
patterns) so you often get **Verified** with no sidecar.

If the ensures shape is **not** synthesizable (e.g. bare `result > 0`), those
clauses are **skipped with Unknown** (not a silent counterexample). Write a
`{ContractName}.ir`, run `assura build --write-ir`, or use `--auto-implement`.

See `showcase-echo.assura` (+ optional co-located `ShowcaseEcho.ir`) for the
happy path.

## Running demos

```bash
# Happy path
assura check demos/heartbleed.assura
assura check demos/showcase-echo.assura

# Verbose
assura check demos/heartbleed.assura --verbose

# From a monorepo clone without installing:
cargo run --bin assura -- check demos/heartbleed.assura
```

## Generated files

`demos/generated/` holds local build artifacts (often gitignored). Committed
`.ir` next to a showcase `.assura` is intentional (e.g. `showcase-echo.ir`).

Internal IR fixtures also live under `tests/fixtures/`.
