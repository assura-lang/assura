# Assura for AI agents

Assura is built so agents can **propose implementations** and the compiler
can **accept or reject** them with structured evidence.

## Install

```bash
cargo install assura --locked
assura --help
```

Docs site: https://assura-lang.github.io/assura/

## Core loop

1. Human (or agent) writes or edits a `.assura` contract.
2. Agent produces an implementation path (synthesized IR, co-located `.ir`,
   or Rust under `check-rust`).
3. Agent runs `assura check` / `assura check-rust` / `assura build`.
4. Agent reads machine-readable results and iterates on **counterexamples**,
   not on vibes.

```bash
assura check path/to/file.assura --json
assura check-rust src/ --json
```

## Acceptance policy (LLM IR / auto-implement)

Do **not** require every clause `status == "verified"`. Many clauses return
**Unknown** because a feature is not yet encoded in SMT. For agent IR:

- **Reject** if any **Counterexample** appears.
- **Accept** when there is no counterexample (Unknown is allowed for
  unmodeled features). Generated Rust may still carry runtime assertions
  from requires/ensures codegen.

See AGENTS.md pipeline notes for `verify_ir` and multi-contract source
pitfalls (first contract only unless single-contract source is synthesized).

## Vacuous success

JSON success can still be **vacuous** (no contracts, or no SMT proof
obligations). Inspect `file_info.vacuous` / `vacuous_reason` (and human
summaries that say "no SMT proof obligations"). Do not report that as
full coverage.

## MCP

The `assura-mcp` crate exposes tools for agent hosts (rmcp). From a
workspace build:

```bash
cargo run -p assura-mcp -- --help
```

Exact tool names evolve; list tools from your MCP client after connecting.
Prefer `assura check --json` when MCP is unavailable.

## Suggested agent checklist

1. Prefer showcase demos over `*-audit.assura` (many audits are EXPECT FAIL).
2. On failure, paste the clause description and model into the next edit.
3. Use `--layer 0` for fast structural iteration when SMT is noisy.
4. Read [What we prove](WHAT-WE-PROVE.md) before claiming "proved secure".

## Related

- [Getting started](GETTING-STARTED.md)
- [Compared to other tools](COMPARE.md)
- [Case studies](CASE-STUDIES.md)
- [Preferred URLs](URLS.md)
