# Compared to other tools

Assura is a **contract-first language** aimed at AI-assisted development:
humans write behavioral contracts; the compiler proves implementations
(or returns a counterexample) with SMT (Z3/CVC5) and emits Rust.

This page answers the first question PL and Rust audiences ask: *is this
just Dafny / Verus / Liquid Haskell / better unit tests?*

## Snapshot

| | Assura | Dafny | Verus | Liquid Haskell | Unit / property tests |
|--|--------|-------|-------|----------------|------------------------|
| **Primary surface** | `.assura` contracts; optional `/// @requires` / `@ensures` on Rust via `check-rust` | Dafny language | Specs and proofs as annotations on Rust | Liquid types / refinements on Haskell | Tests in host language |
| **Implementation author** | Often AI (IR / auto-implement / check-rust) | Human (or AI as ordinary code) | Human-written Rust | Human-written Haskell | Human or AI |
| **Proof backend** | Z3 / CVC5 via Assura pipeline | Boogie / Z3 | VIR / Z3 | Liquid Fixpoint / SMT | None (sampling) |
| **Default emit** | Rust source (`rustc` / WASM) | C#, Go, JS, Java, Python, … | Stays Rust | Stays Haskell | N/A |
| **AI agent loop** | First-class (MCP, check-rust, auto-implement) | Possible but not the product shape | Possible | Possible | Common, no proof |
| **What "success" means** | No counterexample for modeled clauses; layers 0–2; unmodeled Rust bodies are `body_not_modeled`, not silent success | Verified method / module | Verified function under Verus's Rust model | Type-checked refinements | Tests green |

## Assura on existing Rust vs Verus

Assura can annotate **existing Rust** without a separate `.assura` file per
function: put contracts in doc comments and run `assura check-rust`
(human or LLM can add the annotations). Example shape:

```rust
/// @requires x >= 0
/// @ensures result >= 0
pub fn abs_i64(x: i64) -> i64 { /* ... */ }
```

That is real, but it is **not** the same product as Verus:

| | Assura `check-rust` | Verus |
|--|---------------------|-------|
| **How you attach specs** | `/// @requires` / `@ensures` (and related) on Rust items | Verus attributes / proof blocks in Rust |
| **What is modeled** | Growing but intentional subset of bodies (arith, control flow, wrapping/bitops, …) or a co-located `.ir` sidecar | Deep model of Rust (including ownership/borrow patterns Verus supports) |
| **Unmodeled code** | Reports `body_not_modeled` (not treated as verified) | Outside Verus's supported surface, or unfinished proof, as Verus defines |
| **Primary story** | Contracts first; AI loop; also annotate-and-check | Prove the Rust you keep writing in place |

**Prefer Verus** when the goal is fine-grained, borrow-aware proofs of
**existing Rust crates** as the long-term source of truth.

**Prefer Assura** when you want a separate contract language and/or an
agent-friendly check loop, including optional inline annotations on Rust
with honest body modeling limits (see [What we prove](WHAT-WE-PROVE.md)
and CONTRIBUTING "check-rust body proof").

## When Assura is a better fit

- You want **specs separate from host-language syntax** so agents and humans
  share a stable contract surface (`.assura`), or light `/// @…` contracts
  on Rust via `check-rust`.
- You care about an **AI write → SMT check → fix** loop with structured
  results (counterexample vs unknown vs verified vs body_not_modeled).
- You want **Rust as the ship format** without requiring Verus-style
  verified Rust-in-place as the only workflow.

## When another tool is a better fit

| Need | Prefer |
|------|--------|
| Deep **borrow-aware** proofs of existing Rust as the main workflow | [Verus](https://github.com/verus-lang/verus) |
| Mature multi-target verified language with large libraries | [Dafny](https://dafny.org/) |
| Refinement types inside Haskell | [Liquid Haskell](https://ucsd-progsys.github.io/liquidhaskell/) |
| Fast feedback without SMT, or non-modeled effects | Property tests / fuzzing (still useful *with* Assura) |

## Honesty constraints

Assura does **not** claim:

- That every clause is always decided (see [What we prove](WHAT-WE-PROVE.md)).
- That it replaces human review for product requirements.
- That `check-rust` is a drop-in Verus substitute for verifying arbitrary
  Rust crates (partial body model; unmodeled paths fail closed as
  `body_not_modeled`).
- That every green check means full mathematical coverage of all features.

For competitive research notes (internal depth), see
[INVESTIGATION.md](INVESTIGATION.md). For a short public pitch, start with
the [docs site introduction](https://assura-lang.github.io/assura/).
