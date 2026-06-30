# Assura Demo Contracts

Example `.assura` contracts demonstrating how Assura prevents real-world vulnerabilities.

## CVE Prevention Demos

Each demo models a real CVE with its CVSS score, root cause analysis, and
the Assura contracts that would have prevented the vulnerability.

| File | CVE | Vulnerability Class | CVSS |
|------|-----|---------------------|------|
| `heartbleed.assura` | CVE-2014-0160 | Buffer Over-Read | 7.5 |
| `libwebp-huffman.assura` | CVE-2023-4863 | Heap Buffer Overflow | 9.8 |
| `zlib-inflate.assura` | CVE-2022-37434 | Heap Buffer Overflow | 9.8 |
| `mbedtls-x509.assura` | CVE-2023-45199 + cluster | TLS/X.509 Buffer Overflow | 9.8 |
| `double-free.assura` | CVE-2014-0195 | Double-Free | 6.8 |
| `use-after-free.assura` | CVE-2023-4911 | Use-After-Free | 7.8 |
| `null-deref.assura` | CVE-2023-25136 | Null Dereference | 6.5 |
| `integer-overflow.assura` | CVE-2021-3156 | Integer Overflow | 7.8 |
| `stack-overflow.assura` | CVE-2022-35737 | Stack Buffer Overflow | 7.5 |
| `sql-injection.assura` | CVE-2019-9193 | SQL Injection / RCE | 9.0 |
| `deserialization.assura` | CVE-2021-44228 | Unsafe Deserialization | 10.0 |
| `path-traversal.assura` | CVE-2021-41773 | Path Traversal | 7.5 |
| `race-condition.assura` | CVE-2016-5195 | Race Condition (TOCTOU) | 7.8 |
| `crypto-weakness.assura` | CVE-2014-3566 | Cryptographic Weakness | 3.4 |

## Language Feature Demos

Each demo showcases a specific Assura language feature with a real-world
motivation.

| File | Features | Scenario |
|------|----------|----------|
| `taint-tracking.assura` | SEC.1 | Taint tracking for SQLite-class input validation |
| `linear-resource.assura` | MEM.1-MEM.3 | File handle safety via linear types |
| `typestate-protocol.assura` | TYPE.1-TYPE.3 | TLS handshake state machine |
| `refinement-banking.assura` | TYPE.4 | Banking transfer safety with refinement types |
| `effect-handler.assura` | TYPE.7-TYPE.8 | I/O effect containment |
| `concurrent-lock.assura` | CONC.1-CONC.3 | Deadlock prevention via lock ordering |

## Running Demos

```bash
# Check a single demo
cargo run --bin assura -- check demos/heartbleed.assura

# Verbose output (shows pipeline phases and timing)
cargo run --bin assura -- check demos/heartbleed.assura --verbose

# Check all demos
for f in demos/*.assura; do cargo run --bin assura -- check "$f"; done
```

## Generated Files

The `generated/` subdirectory contains IR sidecar files (`.ir`) produced
by `assura build`. These are gitignored artifacts, not source files.

Internal compiler test contracts and IR fixtures live in
`tests/fixtures/ir-demos/`.
