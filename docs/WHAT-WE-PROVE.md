# What we prove

Assura uses layered checking. Not every green `assura check` means
"mathematically proved forever for every feature." This page is the
honest map for security and formal-methods readers.

## Result kinds (SMT)

| Result | Meaning | Typical user action |
|--------|---------|---------------------|
| **Verified** | Solver showed the obligation holds under the model (e.g. validity of ensures under requires) | None; keep the contract |
| **Counterexample** | Solver found values that break a clause | Fix the contract or the implementation / IR |
| **Timeout** | Solver hit the time budget | Simplify; raise timeout; try portfolio (Z3+CVC5) |
| **Unknown** | Inconclusive for other reasons | Read the reason string |

### Known SMT limitation (warning, not hard fail)

When the reason includes the marker **`not yet encoded in SMT`**, the CLI
treats the result as a **warning** (exit 0 by default). That means the
compiler intentionally skipped modeling a feature, not that the clause
was proved.

Other Unknown reasons (non-linear arithmetic, genuine solver unknown)
are treated more severely. See FAQ: "Unknown" verification result.

## Layers

| Layer | What runs | Time scale | Proves? |
|-------|-----------|------------|---------|
| **0 Structural** | Parse, resolve, type, domain checkers | ms | Types, names, many security *structure* rules (taint, lock order shape, …) |
| **1 Decidable / SMT** | Clause encoding to Z3/CVC5 | sub-second to seconds | Modeled boolean obligations (requires/ensures/invariants in the supported fragment) |
| **2+ Heavy / advanced** | Extra passes (liveness-style, BMC-related, …) | seconds+ | Additional obligations when enabled |

Default config values are short (often 1s at the options layer). **Layer 1
clause solvers floor at 10 seconds** so multi-clause demos are not
starved; set `[verify] timeout = 30000` (or higher) when you need more.
See FAQ: Z3 timeout on a contract.

## What is *not* automatically proved

- Features that still return Unknown with the known-limitation marker
- Host code outside Assura contracts / IR / check-rust body modeling
  (for inline Rust, see
  [`check-rust` supported surface](CHECK-RUST-SURFACE.md): modeled vs
  `body_not_modeled` vs soundness refusals)
- Absolute absence of all security bugs (only the properties you state
  and that the solver models)
- Correctness of the SMT solvers themselves or of `rustc`
- Overflow and wraparound for `Int` and `Nat`. Those types encode as
  unbounded SMT integers. Codegen maps them to `i64` / `u64`. A clause
  that holds for every mathematical integer can still fail at 64-bit
  width. Use `U8`–`I64` when wraparound is part of what you want
  proved. Generated `debug_assert!` widens comparisons to `i128`, so
  it checks the unbounded model, not machine wrap.

## `Int` / `Nat` vs fixed-width integers

| Source type | SMT sort | Generated Rust |
|-------------|----------|----------------|
| `Int` | unbounded `Int` | `i64` |
| `Nat` | unbounded `Int` (`>= 0`) | `u64` |
| `U8`–`U64`, `I8`–`I64` | bitvector of that width | matching primitive |

`MAX_INT` / `MIN_INT` overflow stories (for example `I32::MIN / -1`)
apply only to the fixed-width row. They do not apply to `Int` / `Nat`.

## Vacuous success

An empty file or a contract with **no SMT proof obligations** can still
report check success with no type errors. Human and JSON output call this
out (`vacuous` / "no SMT proof obligations"). Agents must not treat that
as coverage of ensures/invariants.

## AI-generated implementations

For auto-implement / IR injection, acceptance is typically **no
Counterexample**, not "every clause is Verified". Unknown from unmodeled
features is common; runtime assertions from codegen still apply.

## Related

- [FAQ](FAQ.md) — timeouts, counterexamples, Unknown
- [Compared to other tools](COMPARE.md)
- [`check-rust` supported surface](CHECK-RUST-SURFACE.md) — inline Rust body model
- [Preferred URLs](URLS.md)
