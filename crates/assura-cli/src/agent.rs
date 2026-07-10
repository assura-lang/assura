// `assura agent-instructions` -- print compact agent reference
// ---------------------------------------------------------------------------

pub(crate) fn run_agent_instructions(output_mode: assura_config::OutputMode) {
    let markdown = agent_instructions_markdown();
    if output_mode == assura_config::OutputMode::Json {
        let report = serde_json::json!({
            "title": "Assura: AI Agent Quick Reference",
            "markdown": markdown,
            "install": [
                "cargo install assura --locked",
                "assura doctor",
                "assura check path/to/file.assura",
            ],
            "commands": [
                "check", "check-rust", "suggest-from-crash", "build", "init",
                "explain", "fmt", "infer", "test-gen", "agent-instructions",
                "doctor", "lsp", "completions", "coverage", "audit", "repl",
                "diff", "ir-prompt", "ir", "doc", "mcp",
            ],
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        print!("{markdown}");
    }
}

/// Build the human-readable agent quick reference markdown.
pub(crate) fn agent_instructions_markdown() -> String {
    use assura_codegen::type_map::rust_type_to_assura;
    use std::fmt::Write;

    // Build the type mapping table dynamically from the codegen module
    let type_pairs: Vec<(&str, &str)> = vec![
        ("i8, i16, i32, i64, i128, isize", "Int"),
        ("u8, u16, u32, u64, u128, usize", "Nat"),
        ("f32, f64", "Float"),
        ("bool", "Bool"),
        ("String, &str", "String"),
        ("Vec<u8>, &[u8]", "Bytes"),
        ("()", "Unit"),
    ];
    // Dynamic mappings verified against the codegen module
    let dynamic_pairs: Vec<(String, String)> = vec![
        (
            "Vec<T>".to_string(),
            format!(
                "List<T> (e.g., Vec<i64> -> {})",
                rust_type_to_assura("Vec<i64>")
            ),
        ),
        (
            "Option<T>".to_string(),
            format!(
                "T? (e.g., Option<i64> -> {})",
                rust_type_to_assura("Option<i64>")
            ),
        ),
        (
            "HashMap<K,V>, BTreeMap<K,V>".to_string(),
            format!(
                "Map<K,V> (e.g., HashMap<String, i64> -> {})",
                rust_type_to_assura("HashMap<String, i64>")
            ),
        ),
        (
            "HashSet<T>, BTreeSet<T>".to_string(),
            format!(
                "Set<T> (e.g., HashSet<i64> -> {})",
                rust_type_to_assura("HashSet<i64>")
            ),
        ),
        (
            "Box<T>, Arc<T>, Rc<T>".to_string(),
            format!(
                "T (wrapper erasure, e.g., Arc<String> -> {})",
                rust_type_to_assura("Arc<String>")
            ),
        ),
        (
            "&T, &mut T".to_string(),
            format!(
                "T (reference erasure, e.g., &i64 -> {})",
                rust_type_to_assura("&i64")
            ),
        ),
    ];

    let mut out = String::new();
    let _ = write!(
        out,
        r#"# Assura: AI Agent Quick Reference

## What is Assura?

A contract-first language that compiles to Rust. You write behavioral
contracts (preconditions, postconditions, invariants). The compiler
proves correctness via Z3 SMT solver, then generates Rust source code.

## Install the CLI

```bash
cargo install assura --locked
# or prebuilt: https://github.com/assura-lang/assura/releases
assura doctor
assura check path/to/file.assura
```

Prefer the monorepo `cargo run --bin assura -- …` only when developing the
compiler itself.

## Contract Syntax

```assura
contract ContractName {{
    input(param1: Type, param2: Type)
    output(result: ReturnType)

    requires {{ precondition_expression }}
    ensures  {{ postcondition_expression }}
    effects  {{ effect_list }}
}}
```

### Clause Kinds

| Clause | Purpose |
|--------|---------|
| `input(...)` | Parameters the function accepts |
| `output(...)` | Return value |
| `requires {{ ... }}` | Preconditions (caller must satisfy) |
| `ensures {{ ... }}` | Postconditions (implementation must satisfy) |
| `effects {{ ... }}` | Side effects the function may perform |
| `invariant {{ ... }}` | Properties that hold throughout execution |
| `decreases {{ ... }}` | Termination measure for recursive functions |
| `states {{ ... }}` | Typestate declarations (for services) |
| `ghost {{ ... }}` | Ghost variables (verification only, erased at runtime) |
| `data_flow {{ ... }}` | Information flow / taint tracking constraints |

### Expression Features

- `old(expr)` in ensures: value of expr before the call
- `result` in ensures: the return value
- `forall x in collection : predicate`: universal quantifier
- `exists x in collection : predicate`: existential quantifier
- `if cond then a else b`: conditional expression
- `length(collection)`: collection length
- `abs(x)`: absolute value

## Type Mapping (Rust to Assura)

| Rust Type | Assura Type |
|-----------|-------------|
"#
    );

    for (rust, assura) in &type_pairs {
        let _ = writeln!(out, "| `{rust}` | `{assura}` |");
    }
    for (rust, assura) in &dynamic_pairs {
        let _ = writeln!(out, "| `{rust}` | `{assura}` |");
    }

    let _ = write!(
        out,
        r#"
## Binding Contracts to Existing Rust Functions

Use `bind` to attach a contract to a Rust function without rewriting it:

```assura
bind "crate::module::function_name" as function_name_checked {{
    input(x: Int, data: Bytes)
    output(result: Nat)
    requires {{ length(data) > 0 }}
    requires {{ x >= 0 }}
    ensures  {{ result <= length(data) }}
    effects  {{ io }}
}}
```

## Valid Effect Names

`io`, `database`, `logging`, `mem`, `net`, `fs`, `rng`, `time`,
`alloc`, `diverge`, `random`, `pure`

Dotted sub-effects: `console.read`, `console.write`, `filesystem.read`,
`filesystem.write`, `network.connect`, `network.listen`, `database.read`,
`database.write`, `log.info`, `log.error`

## CLI Commands

```bash
# Check contracts (parse + resolve + typecheck + verify)
assura check file.assura
assura check dir/                          # project mode (multi-file + SMT)
assura check file.assura --layer 0         # structural only
assura check file.assura --json            # pure JSON for agents
assura check file.assura --watch           # re-check on changes
assura check file.assura --stats           # verification statistics
assura check-rust src/lib.rs               # /// @requires / @ensures on Rust
assura check-rust src/lib.rs --json        # pure JSON; body proof: .ir or encoded int/bool/if/match/abs/min/max; else body_not_modeled

# Build: verify + generate Rust
assura build file.assura                   # default output: generated/ beside source
assura build file.assura --json            # pure JSON status for agents
assura build file.assura --write-ir        # heuristic IR sidecars + inject bodies
assura build file.assura --output out/     # CWD-relative unless bare "generated"
assura build file.assura --target wasm

# IR
assura ir-prompt file.assura --list
assura ir-prompt file.assura --decl Foo
assura ir Foo.ir --contract file.assura --verify --json

# Infer / audit / coverage (existing Rust)
assura infer src/lib.rs -o contracts.assura
assura audit . --unsafe-only
assura coverage . --min-coverage 80
assura suggest-from-crash --target src/lib.rs --crash-dir artifacts/

# Docs / diff / playground
assura doc file.assura --verify
assura diff old.assura new.assura --verify --json
assura repl                                # interactive contract playground
assura repl --json                         # NDJSON one object per command

# Other
assura init my-project                     # scaffold assura.toml + starter contract
assura init my-project --json              # pure JSON status for agents
assura fmt file.assura
assura explain A03001
assura test-gen file.assura -o tests.rs
assura doctor
assura completions zsh                     # shell script on stdout
assura completions bash --json             # JSON: {{command,shell,script}}
assura lsp
assura mcp                                 # MCP server for agent tools
assura agent-instructions --json           # this reference
```

## Error Code Ranges

| Range | Category |
|-------|----------|
| A01xxx | Syntax errors (lexer/parser) |
| A02xxx | Name resolution errors |
| A03xxx | Type errors |
| A05xxx | Linearity / verification errors |
| A06xxx | Typestate errors |
| A07xxx | Effect system errors |
| A08xxx | Information flow errors |

Use `assura explain <code>` for details on any error code.

## Development Workflow

1. Write a contract defining what the function should do
2. Run `assura check contract.assura` to verify the contract is well-formed
3. Generate Rust with `assura build contract.assura`
4. If verification fails, read the counterexample and fix the contract
5. The generated Rust includes `debug_assert!` from requires/ensures clauses

For existing Rust code:
1. Run `assura infer src/lib.rs -o contracts.assura` to generate skeletons
2. Refine the skeleton contracts with real invariants
3. Run `assura check contracts.assura` to verify
4. Counterexamples reveal bugs in the original code

## Example Contracts

### Simple arithmetic safety
```assura
contract SafeDivision {{
    input(a: Int, b: Int)
    output(result: Int)
    requires {{ b != 0 }}
    ensures  {{ result * b + (a mod b) == a }}
}}
```

### Bounds checking
```assura
contract BoundedAccess {{
    input(data: List<Int>, index: Nat)
    output(result: Int)
    requires {{ index < length(data) }}
    requires {{ length(data) > 0 }}
    ensures  {{ result == data[index] }}
}}
```

### Side effects declaration
```assura
contract WriteLog {{
    input(message: String)
    output(result: Bool)
    requires {{ length(message) > 0 }}
    ensures  {{ result == true }}
    effects  {{ io, fs }}
}}
```
"#
    );

    out
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[test]
    fn agent_instructions_include_cargo_install() {
        // Install section must stay in the printed reference after co-publish.
        let md = super::agent_instructions_markdown();
        assert!(
            md.contains("cargo install assura --locked"),
            "agent-instructions must document crates.io install"
        );
        assert!(
            md.contains("## Install the CLI"),
            "agent-instructions must have an Install the CLI section"
        );
    }

    #[test]
    fn agent_instructions_json_is_valid_object() {
        let md = super::agent_instructions_markdown();
        let report = serde_json::json!({
            "title": "Assura: AI Agent Quick Reference",
            "markdown": md,
        });
        let text = serde_json::to_string(&report).expect("serialize agent instructions JSON");
        let parsed: serde_json::Value =
            serde_json::from_str(&text).expect("parse agent instructions JSON");
        assert!(
            parsed["markdown"]
                .as_str()
                .unwrap()
                .contains("Contract Syntax")
        );
        assert_eq!(
            parsed["title"].as_str().unwrap(),
            "Assura: AI Agent Quick Reference"
        );
    }

    #[test]
    fn type_mapping_pairs_are_correct() {
        // The agent instructions rely on rust_type_to_assura; verify key mappings.
        use assura_codegen::type_map::rust_type_to_assura;
        assert_eq!(rust_type_to_assura("i64"), "Int");
        assert_eq!(rust_type_to_assura("u32"), "Nat");
        assert_eq!(rust_type_to_assura("f64"), "Float");
        assert_eq!(rust_type_to_assura("bool"), "Bool");
        assert_eq!(rust_type_to_assura("String"), "String");
        assert_eq!(rust_type_to_assura("Vec<u8>"), "Bytes");
        assert_eq!(rust_type_to_assura("()"), "Unit");
    }

    #[test]
    fn error_catalog_is_nonempty() {
        // The agent instructions build error code groups from the catalog.
        let catalog = assura_diagnostics::error_catalog();
        assert!(!catalog.is_empty(), "error catalog should not be empty");
        // Every entry should have a non-empty code starting with 'A'.
        for info in &catalog {
            assert!(
                info.code.starts_with('A'),
                "error code should start with 'A': {}",
                info.code
            );
        }
    }

    #[test]
    fn dynamic_type_mappings_produce_nonempty_strings() {
        use assura_codegen::type_map::rust_type_to_assura;
        // Generic / wrapper type mappings used in run_agent_instructions.
        let generics = [
            "Vec<i64>",
            "Option<i64>",
            "HashMap<String, i64>",
            "HashSet<i64>",
            "Arc<String>",
            "&i64",
        ];
        for rust_type in generics {
            let mapped = rust_type_to_assura(rust_type);
            assert!(
                !mapped.is_empty(),
                "rust_type_to_assura({rust_type}) should produce a non-empty string"
            );
        }
    }
}
