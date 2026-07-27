# SMT and portfolio design notes

Technical note for readers who care about **solver encoding and result
policy**, not AI marketing. Companion to [What we prove](WHAT-WE-PROVE.md)
and [Compiler Internals](INTERNALS.md).

## Dual solvers, one policy brain

Assura can run **Z3** (default) and **CVC5** (portfolio / optional
native). Encoding and clause gates live in shared policy modules under
`assura-smt` (`portfolio_policy`, `clause_gate_policy`,
`encode_timeout_policy`, …). Backends build solver terms; they should not
redefine merge order or limitation strings.

### Portfolio merge priority

When both solvers report a result for the same clause, merge prefers:

1. **Verified**
2. **Counterexample**
3. **Unknown**
4. **Timeout**

On equal priority, **Z3 wins** (richer models / unsat cores in practice).
See `merge_portfolio_results` / `portfolio_result_priority` in
`crates/assura-smt/src/policy/portfolio_policy.rs`.

## Shared timeout floor

Config defaults for verify can be short (historically ~1s at the options
layer). Per-clause solving **floors at 10_000 ms** so multi-clause demos
are not starved:

| Backend | Mechanism |
|---------|-----------|
| Z3 | Soft/hard timeout from `clause_timeout_ms(requested)` |
| CVC5 shell | `--tlimit` string from `clause_timeout_tlimit` |
| CVC5 native | solver option `tlimit` with the same resolved ms |

Raising `[verify] timeout` (or CLI equivalent) above the floor passes
through. Source: `crates/assura-smt/src/policy/encode_timeout_policy.rs`
(issues #1383 / #1384).

## Unknown is not one thing

| Kind | Marker / reason | CLI default | Interpretation |
|------|-----------------|-------------|----------------|
| Known limitation | reason contains **`not yet encoded in SMT`** | Warning, exit 0 | Compiler skipped modeling; **not proved** |
| Other Unknown | non-linear, solver unknown, etc. | Error (exit 1) | Inconclusive; treat as failure for gate scripts |
| Timeout | dedicated result variant | Error | Budget exhausted |

Helpers: `VerificationResult::unknown_not_encoded`,
`KNOWN_SMT_LIMITATION_MARKER`, `is_known_smt_limitation`. Do not invent
alternate marker wording in new code paths.

## IR bodies and unconstrained results

Without an implementation body (IR sidecar, synthesis, or check-rust
encode), **result** (and other outputs) may be free variables. Ensures
that only constrain unconstrained outputs can produce **Counterexample**
even when the requires look fine. Mitigations:

- Co-located `{ContractName}.ir`
- In-memory synthesis for common `result == …` shapes
- `assura check-rust` body encoding where modeled
- Writing ensures as consequences of requires on **inputs** only (demo style)

Vacuous files (no SMT obligations) can still "pass" type-check; JSON
exposes `vacuous` / "no SMT proof obligations". Agents must not treat
that as proof coverage.

## AI-generated IR acceptance

For auto-implement / LLM IR injection, acceptance is typically **no
Counterexample**, not "every clause Verified." Unknown from unmodeled
features is common; codegen still emits runtime assertions for
requires/ensures where applicable.

## What this is not

- A claim that Z3/CVC5 themselves are proved correct
- A claim that every Assura feature is fully encoded
- A Verus-style proof of arbitrary existing Rust

## Further reading

- [WHAT-WE-PROVE.md](WHAT-WE-PROVE.md) — user-facing result kinds and layers
- [COMPARE.md](COMPARE.md) — Assura vs Dafny / Verus / tests
- [INTERNALS.md](INTERNALS.md) — crate and module map
- [SPECIFICATION.md](SPECIFICATION.md) — language and verification layers
- Launch / outreach drafts under [launch/](launch/README.md)
