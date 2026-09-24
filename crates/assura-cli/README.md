# assura

**Write what it should do. AI proves it does.**

The command-line compiler for [Assura](https://github.com/assura-lang/assura), a
contract-first language. You write behavioral contracts; an SMT solver (Z3/CVC5)
checks an implementation against them, or, when there is no body, checks
whether the ensures follows from the requires. It returns the exact input
that breaks the claim. Verified contracts compile to Rust.

```bash
cargo install assura --locked
```

## What it looks like

A real invariant from the [`zip`](https://github.com/zip-rs/zip2) crate's
central-directory parser, where the archive offset is an unchecked `u64`
subtraction:

```assura
contract FindCdSubtractSafe {
    input(cd_offset: Nat, relative_cd_offset: Nat, eocd_offset: Nat)

    requires { cd_offset <= 18446744073709551615 }
    requires { relative_cd_offset <= 18446744073709551615 }
    requires { eocd_offset <= 18446744073709551615 }
    requires { cd_offset <= eocd_offset }

    ensures { cd_offset >= relative_cd_offset }
}
```

```console
$ assura check demos/zip-crate-audit.assura

  FindCdSubtractSafe:
    ensures              ... COUNTEREXAMPLE
      | cd_offset = 0, eocd_offset = 0, relative_cd_offset = 1
```

This demo has no implementation. The counterexample is an input allowed by
the preconditions that makes the ensures false. You get **Verified**, a
**Counterexample**, or an honest **Unknown**. An incomplete encoding is
**Unknown**, not a green check.

## Commands

```bash
assura init my-project          # scaffold a project
assura check contract.assura    # verify (--json for agents, --stats for timings)
assura build contract.assura    # emit Rust
assura explain A05100           # explain any error code
assura check-rust src/          # verify inline contracts in Rust source
assura mcp                      # run the MCP server for agent hosts
```

## Requirements

A [Rust toolchain](https://rustup.rs/) (edition 2024 / rustc 1.87+). Z3 ships
prebuilt via the `z3` crate. No manual Z3 install for a normal build.

## Documentation

- [Getting started](https://github.com/assura-lang/assura/blob/main/docs/GETTING-STARTED.md)
- [Tutorial](https://github.com/assura-lang/assura/blob/main/docs/TUTORIAL.md)
- [What we prove](https://github.com/assura-lang/assura/blob/main/docs/WHAT-WE-PROVE.md): the honest limits
- [Docs site](https://assura-lang.github.io/assura/)

Embedding Assura in your own tool? Use
[`assura-pipeline`](https://crates.io/crates/assura-pipeline) instead of shelling
out to this binary.

## License

MIT OR Apache-2.0.
