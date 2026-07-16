# Compared to other tools

Assura is a **contract-first language** aimed at AI-assisted development:
humans write behavioral contracts; the compiler proves implementations
(or returns a counterexample) with SMT (Z3/CVC5) and emits Rust.

This page answers the first question PL and Rust audiences ask: *is this
just Dafny / Verus / Liquid Haskell / better unit tests?*

## Snapshot

| | Assura | Dafny | Verus | Liquid Haskell | Unit / property tests |
|--|--------|-------|-------|----------------|------------------------|
| **Primary surface** | Dedicated `.assura` contracts | Dafny language | Annotations on Rust | Liquid types / refinements on Haskell | Tests in host language |
| **Implementation author** | Often AI (IR / auto-implement / check-rust) | Human (or AI as ordinary code) | Human-written Rust | Human-written Haskell | Human or AI |
| **Proof backend** | Z3 / CVC5 via Assura pipeline | Boogie / Z3 | VIR / Z3 | Liquid Fixpoint / SMT | None (sampling) |
| **Default emit** | Rust source (`rustc` / WASM) | C#, Go, JS, Java, Python, … | Stays Rust | Stays Haskell | N/A |
| **AI agent loop** | First-class (MCP, check-rust, auto-implement) | Possible but not the product shape | Possible | Possible | Common, no proof |
| **What "success" means** | No counterexample for modeled clauses; layers 0–2 | Verified method / module | Verified function | Type-checked refinements | Tests green |

## When Assura is a better fit

- You want **specs separate from host-language syntax** so agents and humans
  share a stable contract surface.
- You care about an **AI write → SMT check → fix** loop with structured
  results (counterexample vs unknown vs verified).
- You want **Rust as the ship format** without requiring the implementation
  to be authored as verified Rust-in-place first.

## When another tool is a better fit

| Need | Prefer |
|------|--------|
| Verify **existing Rust** in place with fine-grained borrow-aware proofs | [Verus](https://github.com/verus-lang/verus) |
| Mature multi-target verified language with large libraries | [Dafny](https://dafny.org/) |
| Refinement types inside Haskell | [Liquid Haskell](https://ucsd-progsys.github.io/liquidhaskell/) |
| Fast feedback without SMT, or non-modeled effects | Property tests / fuzzing (still useful *with* Assura) |

## Honesty constraints

Assura does **not** claim:

- That every clause is always decided (see [What we prove](WHAT-WE-PROVE.md)).
- That it replaces human review for product requirements.
- That it is a drop-in Verus substitute for verifying arbitrary Rust crates.

For competitive research notes (internal depth), see
[INVESTIGATION.md](INVESTIGATION.md). For a short public pitch, start with
the [docs site introduction](https://assura-lang.github.io/assura/).
