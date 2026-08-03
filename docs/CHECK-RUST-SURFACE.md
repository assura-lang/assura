# `check-rust` supported surface

This page answers: **what can `assura check-rust` prove on annotated Rust
today?** It is the user-facing map for inline contracts
(`/// @requires` / `@ensures` / related). Contributor detail and residual
tables live in [CONTRIBUTING.md](../CONTRIBUTING.md) ("check-rust body proof").

**Not Verus.** Assura models an intentional subset of function bodies (or a
co-located IR sidecar). It does **not** offer Verus-depth borrow-aware proofs
of arbitrary crates. Prefer [Verus](https://github.com/verus-lang/verus) when
that is the goal. See [Compared to other tools](COMPARE.md).

## How body proof works

For each annotated item, `assura check-rust` tries body proof in order:

1. **Co-located IR sidecar** — `{FunctionName}.ir` next to the source (or as
   documented for your layout), used as the modeled body.
2. **Encoded Rust body** — a pure-ish subset of Rust is lowered to IR and
   checked against `@ensures` (under `@requires`).
3. Otherwise — **`body_not_modeled`**: not treated as verified; process exits
   **1**. Do not treat skipped/empty SMT as proof.

Also see [What we prove](WHAT-WE-PROVE.md) for Verified / Counterexample /
Timeout / Unknown on the `.assura` pipeline (related but not identical CLI).

```bash
assura check-rust src/
assura check-rust src/ --json
assura check-rust src/ --suggest   # suggest contracts for unannotated items
```

## Annotations

Typical doc-comment contracts on Rust items:

```rust
/// @requires x >= 0
/// @ensures result >= 0
pub fn abs_i64(x: i64) -> i64 {
    if x < 0 { -x } else { x }
}
```

Related annotations (see explainer / features) may include trust/taint tags
such as `@trust`. Structural extraction is separate from full body proof.

## Bucket A: Modeled (encoded Rust body)

When the body stays in this surface, ensures can be proved or refuted with a
counterexample. Width limits are typically **through 64 bits** unless noted.

### Control and pure binding

| Area | Examples |
|------|----------|
| Control | `if` / `else`, `match` |
| Binding | Multi-`let`, pure `let mut` (no reassignment), `let y = if/match …; y + n` |
| Composition | if/match over binary ops (both sides), method-on-if receivers, cast-of-if |
| References | Peel outer `&` / `*` layers |

### Arithmetic and comparisons

| Area | Examples |
|------|----------|
| Int / bool ops | `+ - * / %`, unary `-`, logical and/or, comparisons / `PartialOrd` |
| Casts / convert | `as`, `into` (where encoded) |
| Defaults | `default()`, associated `MIN` / `MAX` (e.g. `u64` / `usize`) |

### Wrapping, saturating, and checked peels

| Area | Notes |
|------|--------|
| `wrapping_*` | Fixed-width wrapping add/sub/mul/…; nested width fallback; `wrapping_pow` const exp ≤ 4; `wrapping_div` / `wrapping_rem` with nonzero const or positive path-param divisor; `wrapping_neg` (MIN stays MIN) |
| `wrapping_shl` / `shr` / rotates | Variable shifts/rotates through 64 bits |
| Saturating / abs family | `abs`, `min`, `max`, `clamp`, `signum`, saturating ops, `abs_diff` |
| `checked_*` peels | After `.unwrap_or` / `.unwrap_or_default` / `.is_some()` / `.is_none()`; specific forms (`checked_add`/`sub`/`mul` with small const, `checked_div`/`rem` const, `checked_neg`/`abs`, ilog/pow/next_power_of_two/shl/shr as listed in CONTRIBUTING) |
| `overflowing_*` peels | `.0` as wrapping; `.1` as overflow flag (dual of checked is_none patterns); div/rem refuse zero |

### Bit and integer helpers

| Area | Notes |
|------|--------|
| Bitwise | `BitAnd`/`Or`/`Xor` (const mask ≤ 64; both-var ≤ 64), variable `!x` ≤ 64 |
| Power of two | `is_power_of_two` through u64; `next_power_of_two` for unsigned path params ≤ 64 |
| Logs / sqrt | `ilog2` / `ilog10` (unsigned path params ≤ 64; signed with `a>0`, else modeled 0); `isqrt` unsigned ≤ 64 |
| Bit counts | `count_ones`/`zeros`, leading/trailing ones/zeros, `reverse_bits`, `swap_bytes` (≤ 64; signed via bit-pattern map) |
| Euclidean / ceil | `rem_euclid` / `div_euclid` / `div_ceil` / `next_multiple_of` with **positive** const or `NonZeroU*` path-param divisor (`.get()` peels; `div_ceil` needs non-neg receiver) |
| Other | `is_multiple_of` (nonzero), `pow` (small const where required), `borrow` / `deref` where encoded |

Exact operator lists evolve with the encoder. When in doubt, run
`assura check-rust` on a minimal function: success means that body shape is
in the modeled set for your version; `body_not_modeled` means it is not.

## Bucket B: `body_not_modeled` (fail closed)

These shapes are **intentionally residual** or not yet SSA-modeled. The CLI
reports `body_not_modeled` and exits **1**. They are not silent Verified.

| Shape | Why / what to do |
|-------|------------------|
| `let mut y = x; y += 1; y` (reassignment) | Pure `let mut` fold only; mutation/SSA not modeled. Prefer pure expressions or immutable lets. |
| Bare `checked_*` / `overflowing_*` as the **return type** (full `Option` / `(T, bool)`) | Peel: `.unwrap_or` / `.unwrap_or_default` / `.is_some()` / `.is_none()` / `.0` / `.1`. Full Option/tuple values are not IR result types. |
| Bodies outside Bucket A (I/O, arbitrary methods, complex ADTs, …) | Supply a co-located `{Name}.ir`, simplify the body, or keep contracts on `.assura` + generated code. |

Tracking work to shrink first-contact residuals: see epic
[check-rust competitiveness](https://github.com/assura-lang/assura/issues/1456)
and body-encode issues under that epic.

## Bucket C: Never (soundness refusals)

Assura **refuses to model** shapes that would turn panic or undefined
behavior into free SMT success. Do not expect these to become "Verified"
by encoding alone.

| Shape | Reason |
|-------|--------|
| Panic div/mod (`/0`, `%0`, path divisors that may be zero) | Panic is not free SMT division |
| `is_multiple_of(0)`, literal `0.ilog2()` | Same class of unsound free math |
| `rem_euclid` / `div_euclid` / `div_ceil` / `next_multiple_of` with non-positive or zero-including divisors | Use a positive const or `NonZeroU*` parameter |

## What this page does **not** claim

- Verus-level ownership / borrow proofs of existing crates
- That every green check covers all security properties
- That `body_not_modeled` or SMT Unknown means the contract holds
- That co-located IR is automatically generated for every function

## Related

- [Compared to other tools](COMPARE.md) — Assura vs Verus snapshot
- [What we prove](WHAT-WE-PROVE.md) — pipeline result kinds and layers
- [For AI agents](AI-AGENTS.md) — agent-oriented commands
- [CONTRIBUTING.md](../CONTRIBUTING.md) — full body-proof residual table for contributors
- Implementation: `crates/assura-cli/src/check/rust_body_ir/`, `check_rust.rs`
