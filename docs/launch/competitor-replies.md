# Competitor-thread reply playbook

Internal guide for **when and how** to reply in Verus, Dafny, Prusti,
Creusot, Aeneas, Kani, or related threads (Reddit, HN, Lobsters, RFMIG,
Zulip). Goal: high-signal comparison, zero spam, no overclaim, no
assura.dev confusion.

Canonical product links: [URLS.md](../URLS.md). Honest limits:
[WHAT-WE-PROVE.md](../WHAT-WE-PROVE.md). Positioning:
[COMPARE.md](../COMPARE.md). SMT detail for experts:
[SMT-NOTE.md](../SMT-NOTE.md).

## When to reply

Reply only when **at least one** is true:

1. Someone is **comparing tools** or asking for alternatives to
   Verus / Dafny / Prusti / Creusot / property tests for AI code.
2. Someone is discussing **AI-written code + proof**, contract-first
   specs, or structured Verified / Counterexample / Unknown loops.
3. You can add a **missing axis** that the thread lacks (not "try Assura"
   as a bare pitch):
   - Separate contract surface vs optional `/// @…` + `check-rust` (modeled
     surface only; not Verus-depth for arbitrary crates)
   - Z3 **and** CVC5 portfolio with shared timeout floor
   - Unknown policy (`not yet encoded in SMT` = warning, not false Verified)
   - Rust as **emit** target, plus annotate-and-check for a subset of bodies
   - MCP / check-rust / auto-implement agent loop

## When **not** to reply

- Pure research-paper or "my thesis" threads with no tool-selection ask
- Hostile, pile-on, or identity-politics threads
- Threads already saturated with tool spam
- Mentions of **assura.dev** (different product; do not "correct" or
  claim affiliation)
- Any urge to post uncited percentages or "proves all security bugs"

Skip is the default. Silence is fine.

## Tone rules

| Do | Do not |
|----|--------|
| Lead with the technical distinction | Lead with install links only |
| Name honest limits early (Unknown, vacuous check) | Claim full Verus-in-place replacement |
| Prefer one tight paragraph + 1–2 links | Wall of marketing |
| Use `assura-lang` / GitHub Pages URLs | Use assura.dev |
| Stay for a few good-faith follow-ups | Hit-and-run drop of README |

## One-paragraph templates

### vs Verus (Rust-in-place)

> Assura is primarily contract-first: dedicated contracts, SMT (Z3/CVC5),
> then **emit** Rust for an AI implement loop. We also support
> `/// @requires` / `@ensures` on existing Rust via `assura check-rust`,
> but only on a **modeled** body surface (fail closed as body_not_modeled
> otherwise). That is not Verus-depth borrow-aware proof of arbitrary
> crates; Verus still wins for that. Compare:
> https://assura-lang.github.io/assura/COMPARE.html
> Surface: https://assura-lang.github.io/assura/CHECK-RUST-SURFACE.html
> Limits: https://assura-lang.github.io/assura/WHAT-WE-PROVE.html

### vs Dafny (mature multi-target)

> If you want a mature multi-target verified language with large
> libraries, Dafny is the safer pick. Assura is narrower: AI-oriented
> contract surface, Rust emit, structured Unknown vs Counterexample for
> agents. Same compare page as above.

### vs unit / property tests only

> Tests still matter. Assura adds SMT obligations on stated contracts;
> green tests do not replace Verified, and Unknown is not Verified.
> Agents should branch on result kind, not treat "check passed" alone as
> coverage.

### AI + proof threads

> We treat AI IR acceptance as **no Counterexample**, not "every clause
> Verified," because unmodeled features often return Unknown with a
> known-limitation marker (CLI warning, exit 0). Details:
> https://assura-lang.github.io/assura/SMT-NOTE.html

## Link budget

Prefer **at most two** of:

1. https://assura-lang.github.io/assura/COMPARE.html
2. https://assura-lang.github.io/assura/WHAT-WE-PROVE.html
3. https://assura-lang.github.io/assura/CHECK-RUST-SURFACE.html (if discussing annotate-on-Rust)
4. https://assura-lang.github.io/assura/SMT-NOTE.html (SMT audience only)
5. https://github.com/assura-lang/assura (source / install)

Avoid dumping every demo URL unless asked.

## Escalation

- Product questions that need a maintainer decision: do not invent policy;
  open a GitHub issue or ask in-repo.
- Public launch posts (Show HN, Lobsters, etc.): use
  [launch README](README.md); do not improvise titles that violate the
  honesty constraints there.
