# Assura 0.2.0

Second public release after v0.1.0. Libraries publish to crates.io; the CLI continues to ship via GitHub Release installers (cargo-dist), not as a crates.io binary.

## Highlights

### Named `feature_max` bounds in resolve and demos

Named `feature_max` constants resolve correctly so contracts can use them in specs and demos without spurious unresolved-name errors. Several CVE demos (including zlib inflate) use named bounds and input-only ensures that the SMT path can verify without IR sidecars.

### Incremental contracts (MISC.1) in SMT

Top-level `incremental Name { ... }` blocks parse as real declarations (no longer mis-attached as a clause on the previous function). The verifier checks a documented subset: block invariants and re-parseable `step` / `resume` / `on` boolean requires/ensures under Z3. Complex typestate fragments may still be skipped without treating them as hard failures. `demos/zlib-inflate.assura` verifies its InflateDecoder invariant and related ensures without an `incremental_contract` known-limitation warning.

### Clearer `assura check` for sparse contracts

Requires-only and other vacuous cases no longer look like a full success with no verifiable work. Human output and JSON report vacuous flags and messaging more accurately, including truncated long verification display names.

### More reliable crates.io co-publish

Publish order accounts for path dependencies (including dev-deps), retries rate limits, skips versions already on crates.io, ships IR prompt templates inside `assura-smt`, and CI runs `check-cargo-package` so monorepo-only assets do not break tarballs again.

## Install

**CLI (recommended):** download installers from the [GitHub Release](https://github.com/assura-lang/assura/releases/tag/v0.2.0) (cargo-dist). Do not `cargo install assura` from crates.io; the CLI is not published as a crate.

**Libraries (embed / pipeline):** use crates.io versions at **0.2.0**, for example `assura-pipeline` as the public compile entry surface. See [docs/CRATES-IO.md](https://github.com/assura-lang/assura/blob/main/docs/CRATES-IO.md).

## Verification notes

- Prefer `assura check` on demos after upgrade; known-limitation skips (`A05102`) are warnings, not hard errors, when they use the standard “not yet encoded in SMT” marker.
- Full typestate state-machine encoding for every incremental `on` body is still a subset; arithmetic and simple boolean step/resume paths and block invariants are covered.

## Full changelog

See [CHANGELOG.md](https://github.com/assura-lang/assura/blob/main/CHANGELOG.md) for the machine-generated list of commits between v0.1.0 and v0.2.0.

