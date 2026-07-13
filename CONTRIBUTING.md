# Contributing to Assura

Thank you for your interest in Assura! This guide covers everything you
need to set up, build, test, and submit changes.

## Prerequisites

- **Rust 1.85+** (edition 2024)

The Z3 SMT solver is downloaded automatically during `cargo build` (via
the `z3` crate's `gh-release` feature), so no manual Z3 installation is
needed.

CVC5 is optional (portfolio solver mode). It is not in Homebrew; use the
setup script instead:

```bash
bash scripts/setup-cvc5.sh
# paste the printed export lines (CVC5_LIB_DIR, CVC5_INCLUDE_DIR)
```

## Getting Started

**Try the released CLI** (optional, no clone required):

```bash
cargo install assura --locked
assura doctor
assura check demos/libwebp-huffman.assura   # needs a clone or your own .assura file
```

Prebuilt installers: [GitHub Releases](https://github.com/assura-lang/assura/releases).
Co-publish details: [docs/CRATES-IO.md](docs/CRATES-IO.md).

**Develop from a clone:**

```bash
git clone https://github.com/assura-lang/assura.git
cd assura
cargo test --workspace
# Local binary without publishing:
cargo run --bin assura -- check demos/libwebp-huffman.assura
```

If all tests pass, you are ready to contribute.

### Fuzzing (optional, needs nightly)

The weekly **Fuzz** workflow and local fuzzing require a **nightly** toolchain
(sanitizer / coverage flags). Targets live under `fuzz/`:

```bash
rustup toolchain install nightly --component rust-src
cargo install cargo-fuzz --locked
cargo +nightly fuzz run fuzz_lex -- -max_total_time=30
cargo +nightly fuzz run fuzz_parse -- -max_total_time=30
cargo +nightly fuzz run fuzz_typecheck -- -max_total_time=30
```

See `fuzz/README.md` for details. Crash artifacts land under `fuzz/artifacts/`.

### Where to start

Look for issues labeled
[`good first issue`](https://github.com/assura-lang/assura/labels/good%20first%20issue)
for newcomer-friendly tasks. Issues labeled
[`help wanted`](https://github.com/assura-lang/assura/labels/help%20wanted)
are open for contribution.

Good first areas: adding test fixtures in `tests/fixtures/`, improving
error messages in `assura-diagnostics`, and expanding demo contracts in
`demos/`.

**Not part of the default workspace:** `crates/assura-driver` is an
exploratory rustc driver (`publish = false`, listed in root
`Cargo.toml` `exclude`). It needs nightly + `rustc-dev`. Do not expect
`cargo test --workspace` to build it.

## Project Structure

The compiler is a Cargo workspace with one crate per pipeline stage:

```
Source (.assura)
  --> assura-parser     Lexer (logos) + recursive-descent parser (rowan CST)
  --> assura-resolve    Name resolution, symbol table, scope analysis
  --> assura-types      Type checking, 50+ domain-specific checkers
  --> assura-smt        Z3 SMT solver integration, verification
  --> assura-codegen    Rust code generation via prettyplease
  --> assura-cli        CLI binary (check, build, init, fmt, explain, ...)
  --> assura-lsp        Language Server Protocol (tower-lsp)
```

Supporting crates: `assura-diagnostics` (error types), `assura-config`
(project configuration), `assura-fmt` (formatter), `assura-pipeline`
(multi-file compilation orchestration), `assura-macros` (`#[contract]`
and `#[trust]` proc macros), `assura-stdlib` (standard library
definitions), `assura-mcp` (MCP server), `assura-rust-analyzer` (Rust
source analysis), `assura-bench` (benchmarks), `assura-server`
(gRPC/HTTP API).

## Issues and triage

New issues from outside maintainers are labeled `needs-triage`
automatically (see `.github/workflows/issue-triage.yml`). They stay in
that inbox until a maintainer accepts them by adding `ready` and
removing `needs-triage`. Issues opened by repository owners, org
members, or collaborators skip the inbox and receive `ready`
immediately.

| Label | Meaning |
|-------|---------|
| `needs-triage` | New external report; not yet accepted for implementation |
| `ready` | Accepted; automation and contributors may pick it up |
| `needs-info` | Waiting on the reporter for more detail |

Once an issue is accepted (`ready`), reopening it does not put it back
in `needs-triage` automatically. Remove `ready` (or add `needs-info`) if
work should pause.

Please include reproduction steps (or a clear feature request), expected
vs actual behavior, and your Assura version when filing bugs.

**Agent / maintainer automation** follows the canonical owned-repo
policy in `github-interaction` (issue triage + comment scope): only
implement `ready` (or legacy unlabeled) issues; never auto-implement
pure `needs-triage` or `needs-info`; when filing as owner, use
`--label "…,ready"`. External comments on a ready issue do not expand
implementation scope (title/body + creator/maintainer comments only).

## Development Workflow

### 1. Make your change

Edit the relevant crate. Every compiler pass lives in its own crate
under `crates/`.

### 2. Run the pre-commit gate

Full pre-commit gate (matches CI). Use `--locked` so `Cargo.lock` is not
rewritten accidentally:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --locked -- -D warnings
cargo deny check
bash scripts/guards.sh
bash scripts/check-publish-plan.sh
bash scripts/check-cargo-package.sh   # every publishable crate; see docs/CRATES-IO.md
cargo clippy -p assura-smt --features cvc5-verify -- -D warnings
cargo test --workspace --locked
cargo check --no-default-features -p assura-smt
```

Faster local gate (fmt + guards + clippy on key crates + one demo check):

```bash
bash scripts/preflight.sh
bash scripts/preflight.sh assura-types assura-smt   # subset
```

While iterating on a single crate (faster, agent-friendly):

```bash
cargo fmt -- <changed files>
cargo clippy -p <crate> --locked -- -D warnings
cargo check -p <crate> --locked
cargo test -p <crate> --locked --lib
```

### `check-rust` body proof

`assura check-rust` proves `/// @ensures` against either a co-located
`{Name}.ir` sidecar or a **encoded** Rust body. Encoded surface includes
int/bool arith, if/else/match, multi-let (incl. `let y = if/match …; y + n`), if/match-over-binary (both sides),
unary-neg, cast-of-if, method-on-if receivers, single if method-arg via distribute; peel `&`/`*` outer layers; `checked_add`/`checked_sub`(const).`unwrap_or`, abs/min/max/clamp/signum/saturating (incl. u64 via synthetic max)/
abs_diff, &&/||, is_multiple_of, into/as, PartialOrd/borrow/deref/pow/default,
fixed-width wrapping_* (incl. nested width fallback and `wrapping_pow` with
const exp ≤4), variable wrapping_shl/shr
and rotate through 64 bits, BitAnd/Or/Xor (const mask ≤64; both-var signed/
unsigned ≤64), variable bitwise `!x` ≤64, pot `is_power_of_two` through u64,
variable `ilog2`/`ilog10`/`next_power_of_two` for unsigned path params ≤64
(and signed `ilog2`/`ilog10` ≤64 with `a>0` math log; `a<=0` modeled as 0),
variable `isqrt` for unsigned path params ≤64, `u64`/`usize` `MAX`/`MIN` associated consts, signed/unsigned path-param
`count_ones`/`count_zeros` (≤64; signed via bit-pattern map),
`trailing_zeros`/`leading_zeros`/`trailing_ones`/`leading_ones`/`reverse_bits`/`swap_bytes` (≤64; signed fixed-width via bit-pattern map;
ones via NOT+zeros), and `rem_euclid`/`div_euclid`/`div_ceil`/`next_multiple_of` with a positive
const or `NonZeroU*` path-param divisor (`.get()` peels; `div_ceil` needs a
non-neg receiver). `signum` is nestable in arith (clamp to [-1, 1]). Top-level
`wrapping_neg` expands to multi-block if (MIN stays MIN).

Residual `body_not_modeled` (still intentional): panic paths (`/0`, `%0`, `/`/`%` with zero-including path divisors,
`is_multiple_of(0)` / zero-including path divisors, literal `0.ilog2()`);
`rem_euclid`/`div_euclid`/`div_ceil`/`next_multiple_of` with non-positive or
zero-including divisors (use a positive const or `NonZeroU*` param). Bodies that
cannot be modeled report `body_not_modeled` and exit **1** (including SMT
skipped/checked soft passes). Do not treat empty/skipped SMT as proof.

Implementation: `crates/assura-cli/src/check/rust_body_ir/` (`mod` +
`bitops` + `width` + tests) and `should_mark_body_not_modeled` in
`check_rust.rs`. Multi-block IR temps must use unique slots across sibling
blocks (see module docs).

### Agent / global `--json` purity

Subcommands that accept global `--json` must emit **only** parseable JSON on
stdout for both success and error paths (exit codes still signal failure).
Do not print bare human `Error: …` lines when `--json` is set. Prefer a
stable shape such as `{"ok":false,"error":"…","message":"…"}`.

Quick check after changing a CLI error path:

```bash
cargo run -q --bin assura -- <cmd> … --json | python3 -m json.tool
```

`cargo deny check` enforces license, advisory, and source policies from
`deny.toml` (same step as the CI Fast lint job). Install with
`cargo install cargo-deny` if needed.

`check-publish-plan.sh` asserts the 13-crate library publish set and
topological order (including path **dev**-dependencies).
`check-cargo-package.sh` runs `cargo package -p <crate> --locked` for each
publishable crate so monorepo-only `include_str!` paths fail before a
release (CI job **Cargo package (publishable)**).
Refresh `MASTER-PLAN.md` crate LOC/test counts with
`bash scripts/count-crates.sh`.

When editing `.github/workflows/**` or `.github/actions/**`, also run:

```bash
actionlint -color
zizmor --config .github/zizmor.yml .github/workflows/*.yml
```

The `cvc5-verify` clippy pass mirrors the CI `cvc5` job and catches
cfg-gate mistakes in native CVC5 modules that default workspace clippy
skips. The final `cargo check --no-default-features` verifies the no-Z3
build: any code in `assura-smt` that imports Z3 must be behind
`#[cfg(feature = "z3-verify")]` with a fallback.

For local CVC5 on macOS ARM, run `bash scripts/setup-cvc5.sh` and export
the printed `CVC5_LIB_DIR` / `CVC5_INCLUDE_DIR` before the cvc5 clippy/test
commands (source builds often fail under AppleClang).

### 3. Verify demo files still parse

```bash
cargo run --bin assura -- check demos/libwebp-huffman.assura
cargo run --bin assura -- check demos/zlib-inflate.assura
cargo run --bin assura -- check demos/mbedtls-x509.assura
cargo run --bin assura -- check demos/taint-tracking.assura
cargo run --bin assura -- check demos/heartbleed.assura
```

### 4. Commit

Use scoped commit messages:

```
<scope>: <description>
```

| Scope | When to use |
|-------|-------------|
| `parser` | Lexer or parser changes |
| `resolve` | Name resolution |
| `types` | Type checker |
| `smt` | SMT verification |
| `codegen` | Rust code generation |
| `cli` | CLI commands |
| `lsp` | Language server |
| `docs` | Documentation |
| `tests` | Test infrastructure |
| `ci` | CI/CD workflows |
| `deps` | Dependency updates |

## Testing

### Unit tests

Write `#[test]` functions in the same file as the code they test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_contract() {
        let (ast, errors) = assura_parser::parse("contract Foo { }");
        assert!(errors.is_empty());
        assert!(ast.is_some());
    }
}
```

### Fixture tests

Test `.assura` files live in `tests/fixtures/`:

- `must_compile/` -- valid contracts that must parse and type-check
- `must_reject/` -- invalid contracts annotated with `// MUST REJECT Axxxxx`
- `errors/` -- files with specific parse errors

### End-to-end tests

Full pipeline tests in `tests/e2e/` exercise parsing through verification.

### Demo files

Files under `demos/` are regression guards and teaching examples. See
[`demos/README.md`](demos/README.md) for the SHOWCASE vs EXPECT FAIL
taxonomy. Prefer showcase demos for first-time checks; `*-audit.assura`
files are intentional red. CI runs non-audit demos on every PR.

## Adding a New Compiler Pass

When adding a new crate or major feature:

1. Create `crates/assura-{name}/` with workspace-inherited metadata
2. Wire it through `assura_pipeline` (and thin CLI wrappers in
   `crates/assura-cli/src/shared.rs` / `check/` / `build.rs`). Do not
   re-chain parse/resolve/type_check in frontends.
3. Add at least one integration test that feeds output from the
   previous pass
4. Verify end-to-end: `cargo run --bin assura -- check demos/libwebp-huffman.assura`

Every new pass must be called from the pipeline. Orphan code (compiles
but is never invoked) is a bug.

## Error Codes

Error codes follow the spec (Appendix D):

| Range | Category |
|-------|----------|
| A01xxx | Syntax errors (parser) |
| A02xxx | Name resolution errors |
| A03xxx | Type errors |
| A05xxx | Linearity errors |
| A06xxx | Typestate errors |
| A07xxx | Effect errors |
| A08xxx | Information flow errors |

Use `assura explain <code>` to look up any error code.

## Code Style

- `cargo fmt` is the formatter; do not deviate
- `cargo clippy -- -D warnings` must pass with zero warnings
- Use `pub(crate)` for internal visibility; `pub` only for cross-crate API
- No `unwrap()` in library code (OK in tests and CLI)
- Every AST node carries a `Span` for error reporting

## Documentation

- [Tutorial](docs/TUTORIAL.md) -- getting started
- [Specification](docs/SPECIFICATION.md) -- full language spec (11,800 lines)
- [Internals](docs/INTERNALS.md) -- architecture and crate details
- [Cookbook](docs/COOKBOOK.md) -- 25 ready-to-copy contract patterns
- [Scenario Guides](docs/SCENARIOS.md) -- practical walkthroughs
- [Roadmap](docs/ROADMAP.md) -- phased development plan

## License

Dual-licensed under [MIT](LICENSE) or [Apache-2.0](LICENSE-APACHE),
at your option. By contributing, you agree to license your contribution
under the same terms.