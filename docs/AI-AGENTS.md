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

Full field table and branch policy: [Agent verification loop](AGENT-LOOP.md).
Inline Rust surface: [CHECK-RUST-SURFACE](CHECK-RUST-SURFACE.md).
Demos: `demos/check-rust/` (prove + intentional fail).

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

The `assura-mcp` crate exposes tools for agent hosts (rmcp). There is no
standalone `assura-mcp` binary. Start the server from the CLI:

```bash
assura mcp
```

Tools: `assura_check`, `assura_infer`, `assura_explain`, `assura_type_map`,
`assura_ir_prompt`, `assura_ir_verify`. List them from your MCP client after
connecting. Prefer `assura check --json` when MCP is unavailable.

### JSON envelopes (`assura_check`, `assura_infer`)

Both tools return a JSON object, not raw diagnostic text or raw contract
source. Shared fields:

| Field | Meaning |
|-------|---------|
| `success` | Work completed without a hard error. Vacuous work is still `true`. |
| `vacuous` | No contracts / no SMT obligations / nothing inferred. Not coverage. |
| `vacuous_reason` | Why the run was empty (present when `vacuous` is true). |
| `text` | (`assura_infer` only) inferred `.assura` source. |

`assura_infer` is **not** a raw contract string. Write `result.text` to a
`.assura` file, never the whole envelope. When infer is vacuous, `success`
is true, `vacuous` is true, and `text` is empty or a "nothing found"
placeholder. Branch on `vacuous` (and `vacuous_reason`), not on `success`.

`success: false` means a real error (unknown `--function` on the CLI,
parse/IO/LLM failure, or a jail reject below).

### File jail

`file` / `ir_file` arguments must be **relative to the MCP process cwd**.
Allowed extensions: `.assura`, `.rs`, `.ir`. The same extension check
applies after `canonicalize` (a `leak.rs` symlink to `.env` is rejected).
Absolute paths outside cwd, `../` escapes, missing files, and other
extensions are rejected.

Rejected paths return JSON (same shape as other JSON-tool errors):

```json
{"success": false, "error": "path not allowed", "error_kind": "PATH_NOT_ALLOWED"}
```

The error string does not include the requested filesystem path. Inline
`source` / `ir` text is not jailed. File reads take at most 16 MiB of
actual bytes (not a metadata-only check). Invalid UTF-8 is reported as
`source is not valid UTF-8`, not as a jail reject.

## Suggested agent checklist

1. Prefer showcase demos over `*-audit.assura` (many audits are EXPECT FAIL).
2. On failure, paste the clause description and model into the next edit.
3. Use `--layer 0` for fast structural iteration when SMT is noisy.
4. Read [What we prove](WHAT-WE-PROVE.md) before claiming "proved secure".

## Related

- [Agent verification loop](AGENT-LOOP.md)
- [Getting started](GETTING-STARTED.md)
- [Compared to other tools](COMPARE.md)
- [`check-rust` supported surface](CHECK-RUST-SURFACE.md)
- [Case studies](CASE-STUDIES.md)
- [Preferred URLs](URLS.md)
