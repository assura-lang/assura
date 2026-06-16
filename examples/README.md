# Assura Examples

50 example contracts organized by feature category. Each file demonstrates
a specific verification capability of the Assura language.

## How to Run

```bash
# Parse a single example
cargo run --bin assura -- examples/core/ghost-variables.assura

# Parse with AST output
cargo run --bin assura -- --ast examples/core/ghost-variables.assura

# Parse all examples
for f in examples/**/*.assura; do cargo run --bin assura -- "$f"; done
```

## Index

### Core Verification (CORE)

| File | Feature | Description |
|------|---------|-------------|
| `core/ghost-variables.assura` | CORE.1 | Ghost variables in the specification domain |
| `core/lemmas-and-proofs.assura` | CORE.2 | Ghost lemmas for proof steps |
| `core/frame-conditions.assura` | CORE.3 | `modifies()`, `reads()`, `old()` frame specs |
| `core/axiomatic-definitions.assura` | CORE.4 | Axiomatic blocks with `define` and `property` |
| `core/quantifier-triggers.assura` | CORE.5 | `forall` and `exists` in contracts |
| `core/opaque-functions.assura` | CORE.6 | Opaque function abstraction |
| `core/prophecy-variables.assura` | CORE.7 | `ghost prophecy` future value declarations |
| `core/liveness-contracts.assura` | CORE.8 | Liveness and progress properties |

### Memory Safety (MEM)

| File | Feature | Description |
|------|---------|-------------|
| `memory/memory-regions.assura` | MEM.1 | Bounded memory region separation |
| `memory/fixed-width-integers.assura` | MEM.2 | Integer overflow/underflow prevention |
| `memory/allocator-contracts.assura` | MEM.3 | Alloc/dealloc pairing and pool invariants |
| `memory/circular-buffers.assura` | MEM.4 | Ring buffer head/tail wrapping |

### Type System (TYPE)

| File | Feature | Description |
|------|---------|-------------|
| `types/interface-contracts.assura` | TYPE.1 | Interface contract obligations |
| `types/recursive-invariants.assura` | TYPE.2 | BST, balanced tree, linked list invariants |
| `types/error-propagation.assura` | TYPE.3 | Result/Option error handling contracts |

### Security (SEC)

| File | Feature | Description |
|------|---------|-------------|
| `security/taint-tracking.assura` | SEC.1 | Taint annotations for untrusted data |
| `security/ffi-boundaries.assura` | SEC.2 | Extern fn and bind declarations |
| `security/constant-time.assura` | SEC.3 | Constant-time operation annotations |
| `security/secure-erasure.assura` | SEC.4 | Secure zeroization of secret data |
| `security/crypto-conformance.assura` | SEC.5 | Crypto spec conformance checks |

### Concurrency (CONC)

| File | Feature | Description |
|------|---------|-------------|
| `concurrency/shared-memory.assura` | CONC.1 | Atomic CAS, increment, once-flag |
| `concurrency/reentrancy-safety.assura` | CONC.2 | Non-reentrant and guarded callbacks |
| `concurrency/determinism.assura` | CONC.3 | Determinism contracts with `must_be` |
| `concurrency/lock-ordering.assura` | CONC.4 | Lock hierarchy and deadlock prevention |
| `concurrency/temporal-deadlines.assura` | CONC.5 | Bounded wait and heartbeat checks |
| `concurrency/weak-memory-ordering.assura` | CONC.6 | Memory ordering annotations |

### Storage (STOR)

| File | Feature | Description |
|------|---------|-------------|
| `storage/crash-recovery.assura` | STOR.1 | WAL append and fsync ordering |
| `storage/page-cache.assura` | STOR.2 | Dirty/clean page tracking |
| `storage/mvcc-snapshots.assura` | STOR.3 | Snapshot isolation and version chains |
| `storage/transactional-rollback.assura` | STOR.4 | Commit, abort, and savepoint |
| `storage/monotonic-state.assura` | STOR.5 | Version and sequence monotonicity |
| `storage/storage-failure-model.assura` | STOR.6 | Partial writes and torn write detection |

### Format Parsing (FMT)

| File | Feature | Description |
|------|---------|-------------|
| `format/binary-format.assura` | FMT.1 | Fixed and variable field layouts |
| `format/bit-level-format.assura` | FMT.2 | Bitfield extraction and packing |
| `format/string-encoding.assura` | FMT.3 | UTF-8 invariants and safe truncation |
| `format/codec-registry.assura` | FMT.4 | Magic-byte codec dispatch |
| `format/checksum-integrity.assura` | FMT.5 | CRC/hash verification |
| `format/protocol-grammar.assura` | FMT.6 | TLV messages and protocol negotiation |

### Numerical (NUM)

| File | Feature | Description |
|------|---------|-------------|
| `numerical/numerical-precision.assura` | NUM.1 | ULP bounds and fixed-point scaling |
| `numerical/precomputed-tables.assura` | NUM.2 | Lookup table verification |

### Platform (PLAT)

| File | Feature | Description |
|------|---------|-------------|
| `platform/platform-abstraction.assura` | PLAT.1 | Page size, MMIO, endianness |
| `platform/feature-flags.assura` | PLAT.2 | Feature dependencies and constants |
| `platform/resource-limits.assura` | PLAT.3 | FD, memory, and thread budgets |

### Performance (PERF)

| File | Feature | Description |
|------|---------|-------------|
| `performance/unsafe-escape.assura` | PERF.1 | Unsafe blocks with proof obligations |
| `performance/complexity-bounds.assura` | PERF.2 | Big-O complexity contracts |

### Testing (TEST)

| File | Feature | Description |
|------|---------|-------------|
| `testing/test-generation.assura` | TEST.1 | `example` annotations for test gen |
| `testing/behavioral-equivalence.assura` | TEST.2 | Old/new implementation equivalence |
| `testing/multi-pass-refinement.assura` | TEST.3 | Progressive verification passes |

### Miscellaneous (MISC)

| File | Feature | Description |
|------|---------|-------------|
| `misc/incremental-contracts.assura` | MISC.1 | Incremental state machine contracts |
| `misc/scoped-invariant-suspension.assura` | MISC.2 | Temporary invariant breaking |
