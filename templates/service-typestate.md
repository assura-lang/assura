# Service/Typestate Contract Generation

Given this Rust struct or trait that manages state transitions:

```rust
{rust_type_and_methods}
```

Generate Assura `service` and `contract` declarations that enforce
valid state transitions. Use these rules:

## When to Use Service Declarations

Use a `service` when the Rust code has:
- A struct with a state field (`enum State { ... }`)
- Methods that are only valid in certain states (e.g., `send()` only
  after `connect()`)
- Protocol compliance requirements (TCP, TLS, HTTP)
- Resource lifecycle (open/read/close, acquire/use/release)

## Service Structure

```assura
service {ServiceName} {
    states { State1, State2, State3 }

    // What data is available in each state
    data {
        State1 { config: Config }
        State2 { config: Config, handle: Handle }
    }

    transitions {
        // method: FromState -> ToState
        init:    State1 -> State2
        process: State2 -> State2   // self-loop allowed
        finish:  State2 -> State3
        reset:   * -> State1        // wildcard: any state
    }

    invariant { /* holds in ALL states */ }
}
```

## Companion Contracts

Each transition method needs a `contract` or `bind` with state-aware
clauses:

```assura
contract Connect {
    input(host: String, port: Nat)
    output(conn: Connection)

    requires { port > 0 && port <= 65535 }
    requires { length(host) > 0 }
    ensures  { conn in Connected }
    effects  { net }
}

contract Send {
    input(conn: linear Connection, data: Bytes)
    output(conn: Connection)

    requires { conn in Connected }
    requires { length(data) > 0 }
    ensures  { conn in Connected }
    effects  { net }
}

contract Close {
    input(conn: linear Connection)
    output(result: Unit)

    ensures  { consumed(conn) }
    effects  { net }
}
```

## Key Patterns

### Linear resources

Use `linear` to ensure the resource is consumed exactly once:

```assura
input(handle: linear FileHandle)
ensures { consumed(handle) }
```

### State queries

Use `in` to check current state:

```assura
requires { conn in Authenticated }
ensures  { conn in Disconnected }
```

### Invariants across states

```assura
service Database {
    states { Idle, Transaction, Error }

    invariant { connection_count >= 0 }
    invariant { connection_count <= max_connections }

    transitions {
        begin:    Idle -> Transaction
        commit:   Transaction -> Idle
        rollback: Transaction -> Idle
        error:    Transaction -> Error
        recover:  Error -> Idle
    }
}
```

### Wildcard transitions

`*` means "from any state":

```assura
transitions {
    emergency_stop: * -> Stopped
}
```

## Type Mapping

See `templates/single-function.md` for the complete Rust-to-Assura
type table.

## Assura Features Used

- **TYPE.4** (Typestate): state machine enforcement
- **TYPE.3** (Linear types): resource lifecycle
- **CORE.3** (Frame conditions): modifies clauses
- **CONC.1** (Shared memory): if the state is shared across threads

## Common Mistakes to Avoid

- Do NOT forget the `linear` modifier on resources that must be
  consumed
- Do NOT omit `effects` clauses on methods with side effects
- Do NOT allow transitions from `Error` states without explicit
  recovery
- Do NOT use `*` wildcard for transitions that should be restricted