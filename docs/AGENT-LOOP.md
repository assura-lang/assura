# Agent verification loop

How agents (and humans) should iterate on Assura without treating every
green check as full mathematical coverage.

## Loop

```text
1. Write / edit contract surface
   - Preferred: .assura contracts
   - Optional: /// @requires / @ensures on Rust + assura check-rust
2. Propose an implementation
   - IR sidecar, auto-implement, or ordinary Rust body
3. Run a machine check
   - assura check path.assura --json
   - assura check-rust src/ --json
4. Branch on structured results (below)
5. Fix contract or body; repeat
```

Demo that shows CE then success: `demos/check-rust/fail/clamp_wrong.rs`
(expect fail) vs `demos/check-rust/ok/clamp.rs` (expect pass).

```bash
assura check-rust demos/check-rust/fail/clamp_wrong.rs --json   # errors >= 1
assura check-rust demos/check-rust/ok/clamp.rs --json           # verified >= 1
```

## JSON field contract

### `assura check path.assura --json`

Top-level keys commonly used by agents:

| Field | Meaning |
|-------|---------|
| `file_info.success` | Parse/resolve/type path succeeded (not the same as SMT proof) |
| `file_info.vacuous` / vacuous reasons | No SMT obligations; do not treat as full coverage |
| `diagnostics` | Structural / type diagnostics |
| `verification[]` | Per-clause SMT outcomes |
| `verification[].clause` | e.g. `Name::ensures` |
| `verification[].status` | `verified` / `counterexample` / `timeout` / `unknown` (and related) |

**Agent policy (LLM IR / auto-implement):**

| Outcome | Action |
|---------|--------|
| Any **counterexample** | Reject; fix body or contract |
| **unknown** with reason containing `not yet encoded in SMT` | Warning / non-proof; do not claim Verified (CLI often exit 0) |
| Other **unknown** / **timeout** | Treat as inconclusive; tighten, raise timeout, or simplify |
| All modeled clauses verified, not vacuous | Accept for those clauses |
| Vacuous success | Not coverage of ensures |

See [What we prove](WHAT-WE-PROVE.md) and [SMT portfolio note](SMT-NOTE.md).

### `assura check-rust path --json`

| Field | Meaning |
|-------|---------|
| `verified` | Count of proved annotation clauses / items (see CLI version) |
| `errors` | Failures including counterexamples on annotated items |
| `body_not_modeled` | Ensures present but body not encoded and no `.ir` (fail closed; exit 1) |
| `files` / `items` / `clauses` | Counts |
| `results[]` | Per-item: `status` (`verified`, `error`, `body_not_modeled`, …), `item`, `file` |
| `policy` | Human-readable body-proof policy string |

**Agent policy (`check-rust`):**

| Outcome | Action |
|---------|--------|
| `errors` > 0 | Reject; inspect CE / fix |
| `body_not_modeled` > 0 | Not proved; simplify body, add `.ir`, or see [CHECK-RUST-SURFACE](CHECK-RUST-SURFACE.md) |
| `verified` > 0 and errors == 0 and BNM == 0 | Accept for those annotations |

Do **not** require every pipeline clause to be `verified` when using `.assura`
auto-implement (Unknown from unmodeled features is common). Do **require**
no Counterexample for IR acceptance.

## Related

- [For AI agents](AI-AGENTS.md)
- [CHECK-RUST-SURFACE](CHECK-RUST-SURFACE.md)
- [COMPARE](COMPARE.md)
- Design notes: [DESIGN-AI-VERIFICATION-LOOP](DESIGN-AI-VERIFICATION-LOOP.md)
