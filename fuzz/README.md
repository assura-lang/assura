# Assura fuzz targets

LibFuzzer targets for parser and type-check entry points. **Nightly Rust is
required** (`cargo-fuzz` uses `-Zsanitizer=address` and coverage passes).

## Setup

```bash
rustup toolchain install nightly --component rust-src
cargo install cargo-fuzz --locked
```

## Run (examples)

```bash
# From the repository root
cargo +nightly fuzz run fuzz_lex -- -max_total_time=60 -max_len=4096
cargo +nightly fuzz run fuzz_parse -- -max_total_time=60 -max_len=4096
cargo +nightly fuzz run fuzz_typecheck -- -max_total_time=60 -max_len=4096
```

Corpus seeds live under `fuzz/corpus/<target>/`. Crashes are written to
`fuzz/artifacts/<target>/`.

## CI

`.github/workflows/fuzz.yml` runs weekly (Monday 06:00 UTC) and on
`workflow_dispatch`. It installs **nightly** + `rust-src` before
`cargo fuzz run`. Using stable alone fails with
`the option Z is only accepted on the nightly compiler`.
