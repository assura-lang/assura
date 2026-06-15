# Error Propagation Contract Generation

Given this Rust function that uses `Result<T, E>` and the `?` operator:

```rust
{rust_function_with_error_handling}
```

Generate Assura contracts that verify error handling correctness. Use
these rules:

## When to Use Error Propagation Contracts

Use these patterns when the Rust code has:
- Functions returning `Result<T, E>` or `Option<T>`
- Error propagation with `?`
- Custom error enums with `From` conversions
- Fallible constructors or parsers
- Chains of operations that can each fail

## Contract Patterns

### Basic Result function

```assura
bind "mylib::parse_config" as parse_config_checked {
    input(raw: String)
    output(result: Result<Config, ParseError>)

    // Preconditions for success
    requires { length(raw) > 0 }

    // What holds when it succeeds
    ensures { result.is_ok() ==> result.value.is_valid() }

    // What holds when it fails
    ensures { result.is_err() ==>
        result.error.kind in [InvalidFormat, MissingField] }

    effects { pure }
}
```

### Error propagation chain

When a function calls multiple fallible operations:

```assura
bind "mylib::process_file" as process_file_checked {
    input(path: String)
    output(result: Result<Data, ProcessError>)

    requires { length(path) > 0 }

    // If all steps succeed, the output is valid
    ensures { result.is_ok() ==>
        result.value.checksum == compute_checksum(result.value.bytes) }

    // If it fails, the error indicates which step failed
    ensures { result.is_err() ==> result.error.kind in
        [FileNotFound, ParseFailed, ValidationFailed] }

    // Must not leave partial state on failure
    ensures { result.is_err() ==> no_side_effects_occurred() }

    effects { fs }
}
```

### Option unwrapping

For functions that convert `Option<T>` to `Result<T, E>`:

```assura
bind "mylib::get_user" as get_user_checked {
    input(users: Map<String, User>, id: String)
    output(result: Result<User, NotFoundError>)

    // Clear mapping between None and Err
    ensures { result.is_ok() <==> id in users }
    ensures { result.is_ok() ==> result.value == users[id] }
    ensures { result.is_err() ==> result.error.id == id }

    effects { pure }
}
```

### Fallible constructors

```assura
contract ValidatedEmail {
    input(raw: String)
    output(result: Result<Email, ValidationError>)

    // Valid emails must contain @ and a domain
    ensures { result.is_ok() ==>
        contains(result.value.address, "@") }
    ensures { result.is_ok() ==>
        length(result.value.domain) > 0 }

    // Invalid emails must explain why
    ensures { result.is_err() ==> length(result.error.message) > 0 }

    effects { pure }
}
```

### Error conversion (From trait)

When errors are converted between types:

```assura
contract IoToAppError {
    input(io_err: IoError)
    output(result: AppError)

    // Conversion preserves the original error info
    ensures { result.source == io_err }
    ensures { result.kind == ErrorKind::Io }
    ensures { length(result.message) > 0 }

    effects { pure }
}
```

### Recovery patterns

For functions with fallback behavior:

```assura
bind "mylib::load_config" as load_config_checked {
    input(path: String, defaults: Config)
    output(result: Config)

    // Always succeeds (falls back to defaults)
    ensures { result.is_valid() }

    // If file exists and is valid, uses file config
    // If file missing or invalid, uses defaults
    ensures { !file_exists(path) ==> result == defaults }

    effects { fs }
}
```

## Patterns for Error Exhaustiveness

### All error variants are reachable

```assura
// Verify that each error variant can actually be produced
contract ParseErrors {
    input(inputs: List<String>)

    // There exist inputs that produce each error kind
    ensures {
        exists bad: String, bad in inputs ==>
            parse(bad).error.kind == InvalidSyntax
    }
    ensures {
        exists bad: String, bad in inputs ==>
            parse(bad).error.kind == UnexpectedEof
    }
}
```

### No silent error swallowing

```assura
bind "mylib::batch_process" as batch_process_checked {
    input(items: List<Item>)
    output(result: Result<List<Output>, BatchError>)

    // All items are processed (none silently dropped)
    ensures { result.is_ok() ==>
        length(result.value) == length(items) }

    // Partial failure reports which items failed
    ensures { result.is_err() ==>
        length(result.error.failed_items) > 0 }
    ensures { result.is_err() ==>
        length(result.error.failed_items) +
        length(result.error.succeeded_items) == length(items) }

    effects { io }
}
```

## Type Mapping

See `templates/single-function.md` for the complete Rust-to-Assura
type table.

Additional error types:
- `Result<T, E>` -> `Result<T, E>` (kept as-is)
- `Option<T>` -> `T?`
- `anyhow::Error` -> `Error`
- `Box<dyn std::error::Error>` -> `Error`

## Assura Features Used

- **TYPE.3** (Error propagation): ensures errors carry context
- **CORE.3** (Frame conditions): no partial state on failure
- **TYPE.1** (Interface contracts): error type hierarchies

## Common Mistakes to Avoid

- Do NOT assume `?` always propagates correctly (check error
  conversion)
- Do NOT ignore the error variant in `ensures` (test both Ok and Err
  paths)
- Do NOT allow empty error messages (callers need context)
- Do NOT forget that `unwrap()` panics; prefer contracts that prove
  the value is `Some`/`Ok`
- Always verify that partial failure does not leave inconsistent state