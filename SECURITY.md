# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.x     | Yes       |

## Reporting a Vulnerability

If you discover a security vulnerability in Assura, please report it
responsibly through [GitHub Security Advisories](https://github.com/assura-lang/assura/security/advisories/new).

Do **not** open a public issue for security vulnerabilities.

### What to include

- Description of the vulnerability
- Steps to reproduce (a minimal `.assura` file if applicable)
- Impact assessment (what an attacker could achieve)
- Affected component (parser, codegen, SMT encoding, CLI, etc.)

### Response timeline

- **Acknowledgment**: within 48 hours
- **Initial assessment**: within 1 week
- **Fix or mitigation**: depends on severity, targeting 30 days for critical issues

### Scope

The following are in scope for security reports:

- Parser crashes or panics on malformed input
- Code generation that produces unsafe Rust from safe contracts
- SMT encoding errors that cause unsound verification (compiler says
  "verified" but the contract is actually violated)
- Command injection or path traversal in CLI commands
- Dependency vulnerabilities in the supply chain

### Out of scope

- Denial of service via expensive SMT queries (expected; use `--timeout`)
- Issues in generated Rust code that `rustc` would independently catch
