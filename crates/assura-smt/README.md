# assura-smt

SMT verification for [Assura](https://github.com/assura-lang/assura) contracts.
Z3 is the default solver. CVC5 is the optional shell or native backend.
Portfolio runs both and keeps the stronger result.

Depend on this crate only when you drive the verifier yourself. Pass options
through `assura_config::VerifyOptions`. Clause timeouts below 10 seconds are
raised to 10 seconds.

Most embedders want
[assura-pipeline](https://crates.io/crates/assura-pipeline), which calls
`verify_typed` for you.

A `Verified` result means the clause held. A `Counterexample` is a failing
input. `Unknown` with the text "not yet encoded in SMT" is a compiler limit,
not a proof.

## Documentation

- [What we prove](https://github.com/assura-lang/assura/blob/main/docs/WHAT-WE-PROVE.md)
- [Internals](https://github.com/assura-lang/assura/blob/main/docs/INTERNALS.md)

## License

MIT OR Apache-2.0.
