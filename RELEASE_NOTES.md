# Assura 0.2.0

Assura is a contract-first language: you write what code should do, the compiler and SMT solvers check it, and the toolchain generates Rust.

This release improves what you can name and prove in contracts, how `assura check` reports partial work, and how reliable the published library crates are for embeds and downstream tools.

## What's new

### Named limits that resolve and verify

`feature_max` constants are registered in name resolution, so contracts and demos can use named bounds (for example buffer caps and length limits) without unresolved-name noise. CVE-style demos, including zlib inflate, use those bounds with input-only ensures that Z3 can prove without an IR implementation body.

### Incremental contracts you can actually parse and check

Top-level `incremental Name { ... }` blocks are real declarations (state machines, steps, transitions). The SMT path verifies a practical subset:

- block **invariants**
- **step** / **resume** / **on** bodies when requires and ensures re-parse as ordinary boolean predicates

The zlib inflate demo verifies the InflateDecoder invariant and related ensures end-to-end. Rich typestate in some `on` clauses may still be skipped as a known limitation warning rather than a false hard failure.

### Honest feedback from `assura check`

Sparse contracts (for example requires-only) no longer look like a full green verify with nothing useful reported. Human output and JSON expose vacuous / no-obligation cases more clearly, and long verification names are truncated so terminals stay readable.

### Stronger library packaging

crates.io co-publish is more robust for the public library stack (dependency order including dev path deps, rate-limit retries, skip already-uploaded versions, IR prompt templates inside `assura-smt`, and a CI `cargo package` gate). That reduces “works in the monorepo, fails as a tarball” regressions for anyone depending on `assura-pipeline` and friends at **0.2.0**.

## Get Assura

**Compiler CLI:** installers on the [v0.2.0 GitHub Release](https://github.com/assura-lang/assura/releases/tag/v0.2.0) (cargo-dist shell installer and platform builds).

**Embed the compiler in Rust:** depend on **0.2.0** crates from crates.io. Prefer **`assura-pipeline`** as the public entry point (`compile` / `compile_full` / `verify_typed`). Details: [docs/CRATES-IO.md](https://github.com/assura-lang/assura/blob/main/docs/CRATES-IO.md).

From a workspace checkout, contributors can still build the full tool with `cargo build -p assura` or `cargo install --path crates/assura-cli`.

## After you upgrade

- Run `assura check` on your contracts and demos. SMT known-limitation skips use `A05102` and the marker `not yet encoded in SMT`; those are warnings when that marker is present, not a broken install.
- Incremental state machines with heavy typestate may still show gaps; simple boolean step/resume and invariants are the supported subset in 0.2.0.

## Full commit list

Machine-generated history: [CHANGELOG.md](https://github.com/assura-lang/assura/blob/main/CHANGELOG.md) (v0.1.0 → v0.2.0).

