# Checker layer naming (`checks` / `checkers` / `domain`)

The `assura-types` crate splits type-checking into three layers. Each layer has a distinct responsibility; names are intentional, not historical duplication.

## `domain/` — feature checkers (the *what*)

**Purpose:** Self-contained checker **structs** that validate contracts against a specific verification feature domain (memory safety, concurrency, binary formats, crypto conformance, etc.).

**Pattern:**
- One struct per feature area (e.g. `MemoryChecker`, `LockOrderChecker`)
- Methods like `check_*()` return `Vec<TypeError>` or internal `CheckerError`
- No knowledge of pipeline order or how other checkers run

**When to add code here:** Implementing or extending one of the 50 verification features from the spec (MEM.*, SEC.*, CONC.*, …).

## `checkers/` — structural AST analysis (the *how* on syntax)

**Purpose:** Cross-cutting **structural** checkers that operate on AST shape, symbol tables, or type environments—not tied to a single spec feature category.

**Examples:** `FrameChecker` (modifies clauses), `TaintLabel` propagation helpers, match exhaustiveness, generic instantiation, quantifier triggers, prophecy resolution.

**Pattern:**
- Stateful structs used by both `checks/` wiring and SMT encoding (e.g. `FrameChecker` is reused in `assura-smt`)
- Some items are `pub` because downstream crates need them

**When to add code here:** Analysis that is structural (scopes, symbols, frames, generics) rather than domain-specific (buffers, locks, codecs).

## `checks/` — pipeline wiring (the *when*)

**Purpose:** Thin **`run_*_checks` functions** that instantiate domain/checker structs and collect errors. Grouped by category (`memory.rs`, `concurrency.rs`, …) for maintainability.

**Pattern:**
```rust
pub(crate) fn run_memory_checks(source: &SourceFile) -> Vec<TypeError> {
    MemoryChecker::new().check_all(source)
}
```

**Registration:** `pipeline.rs` defines `CHECKER_PIPELINE`, an ordered `&[CheckerDispatch]` array. `run_all_checks()` is the single dispatch point—all `type_check_*` entry points call it after clause-body checking.

**When to add code here:** Wiring a new checker into the pipeline (new `run_*_checks` + one `CheckerDispatch` entry). The checker logic itself belongs in `domain/` or `checkers/`.

## Decision guide

| Question | Put it in |
|----------|-----------|
| Validates a spec feature (CVE pattern, buffer bound, lock order)? | `domain/` |
| Inspects AST/symbols/types structurally (frame, taint, match exhaustiveness)? | `checkers/` |
| Calls a checker from the type-check pipeline? | `checks/` |
| Defines pipeline order or shared dispatch? | `pipeline.rs` |

## Anti-patterns

- **Domain logic in `checks/`** — keep wiring thin; no duplicated validation rules.
- **Pipeline wiring in `domain/`** — domain structs should not know their order relative to other checkers.
- **New checker not in `CHECKER_PIPELINE`** — dead code; every `run_*_checks` must appear in the registry.