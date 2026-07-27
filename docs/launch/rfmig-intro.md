# RFMIG outreach outline

Rust Formal Methods Interest Group materials for a short intro or
meeting proposal. Public site: https://rust-formal-methods.github.io/

## Channel facts (as of 2026)

| Item | Detail |
|------|--------|
| Forum | [Zulip `wg-formal-methods`](https://rust-lang.zulipchat.com/#narrow/stream/183875-wg-formal-methods) |
| Meetings | Last Monday of each month, typically 19:00 CET (adjust for speakers) |
| Format | Invited talk + discussion; past talks include Creusot, Verus, Flux, Aeneas, Kani, Prusti, … |
| Contact | Propose via Zulip stream; follow meeting cadence and group norms |

Do **not** cold-email a marketing pitch. Frame as **comparison + limits**
for people already evaluating Verus / Prusti / Creusot / Aeneas.

## 5–10 minute talk outline

### 1. Problem shape (1 min)

AI produces a lot of code. Unit tests often encode the same wrong
assumption twice. We want **structured** Verified / Counterexample /
Unknown results agents can branch on, not only "tests green."

### 2. Surface choice (2 min)

- Assura is **contract-first**: dedicated `.assura` contracts, not
  attributes on arbitrary existing Rust.
- Implementation may come from humans, heuristic IR, check-rust body
  encoding, or LLM auto-implement.
- **Emit** is Rust source; proof is over the contract + modeled body,
  not "Verus for any crate."

### 3. Verification stack (3 min)

- Layer 0: parse / resolve / types / domain checkers
- Layer 1: Z3 and optional CVC5; **portfolio merge** prefers Verified and
  Counterexample over Timeout/Unknown; ties prefer Z3
- Shared **10s floor** on per-clause timeout (`tlimit` / Z3); raise via
  `verify.timeout` when needed
- Known-limitation Unknowns use marker `not yet encoded in SMT` (CLI
  warning, not false success)

Pointer: [SMT-NOTE.md](../SMT-NOTE.md), [WHAT-WE-PROVE.md](../WHAT-WE-PROVE.md)

### 4. AI loop (2 min)

- MCP tools, `assura check --json`, check-rust body IR honesty (BNM when
  unmodeled)
- Acceptance for LLM IR: **no Counterexample**, not "all Verified"
- Runtime `debug_assert!` from codegen still applies under Unknown

### 5. Explicit non-goals (1–2 min)

- Not a Verus substitute for verifying arbitrary Rust-in-place
- Not a claim of "all security bugs gone"
- Vacuous success: empty obligations can still "pass"; JSON marks
  `vacuous`
- CVC5 native macOS source build still blocked on upstream cvc5-rs
  (prebuilt path via `setup-cvc5.sh`)

### 6. Ask (30 s)

Feedback on: portfolio policy, Unknown severity for agents, and whether
a contract-first surface belongs in RFMIG tool interop discussions.

## One-page intro blurb (Zulip / meeting proposal)

```
Assura is a contract-first language aimed at AI-assisted development:
humans write behavioral contracts; Z3/CVC5 check them; the toolchain
emits Rust. It is not Verus-in-place. Differentiators for this group:
dual-solver portfolio with shared timeout floors, structured Unknown
with a known-limitation marker, and an agent loop (MCP / check-rust /
auto-implement) that treats Counterexample as fail and limitation
Unknown as non-proof.

Docs: https://assura-lang.github.io/assura/
Compare: …/COMPARE.html
What we prove: …/WHAT-WE-PROVE.html
SMT note: …/SMT-NOTE.html
Source: https://github.com/assura-lang/assura

Happy to give a short RFMIG talk focused on encoding/portfolio design
and honest agent UX rather than marketing.
```

## Proposed next steps (human)

**Tracked in open issue [#1411](https://github.com/assura-lang/assura/issues/1411).**
Do not treat this markdown section as complete tracking; close #1411 only
after the send/record steps there are done.

1. Post a short, technical note in Zulip `wg-formal-methods` (not a
   product dump).
2. Offer a 10–15 minute slot for a later month if maintainers invite.
3. Stay available for Q&A on SMT encoding and limits.
4. Comment on #1411 with Zulip link or decline/no-reply outcome.

## Related launch docs

- [Launch post pack](README.md) (HN / Lobsters / Reddit; separate from RFMIG)
- [Competitor reply playbook](competitor-replies.md)
