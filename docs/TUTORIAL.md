# Assura Tutorial

## Getting Started

### Installation

```bash
cargo install --git https://github.com/assura-lang/assura assura
```

### Your First Contract

Create a file called `safe_division.assura`:

```assura
contract SafeDivision {
  input(a: Int, b: Int)
  output(result: Int)
  requires { b != 0 }
  ensures  { result * b + (a mod b) == a }
  effects  { pure }
}
```

### Check Your Contract

```bash
assura check safe_division.assura
```

### Build to Rust

```bash
assura build safe_division.assura
```

This generates a Rust project in `generated/` with runtime assertions
derived from your contract.

## Verification Layers

Assura has three verification layers:

| Layer | Command | What it checks |
|-------|---------|----------------|
| 0 | `assura check --layer 0` | Type checking, linearity, exhaustiveness |
| 1 | `assura check --layer 1` | SMT verification (Z3): refinement types, arithmetic |
| 2 | `assura check --layer 2` | Full verification: quantifiers, termination, liveness |

## Contract Features

### Refinement Types

```assura
type Pos = { v: Int | v > 0 }
type Percentage = { v: Float | v >= 0.0 && v <= 100.0 }
```

### Linear Types

```assura
contract FileHandle {
  input(path: String)
  output(handle: linear FileHandle)
  ensures { handle.is_valid }
}
```

### Typestate

```assura
service Connection {
  states: [Disconnected, Connected, Authenticated]

  operation connect() {
    requires_state: Disconnected
    transitions_to: Connected
  }

  operation authenticate(token: String) {
    requires_state: Connected
    transitions_to: Authenticated
  }
}
```

### Effects

```assura
contract PureComputation {
  effects { pure }
}

contract IoOperation {
  effects { io, mem }
}
```

### Ghost Code

```assura
contract BinarySearch {
  ghost {
    invariant sorted(arr)
    decreases high - low
  }
}
```

## Error Codes

Use `assura explain <code>` to get details on any error:

```bash
assura explain A03001  # Type mismatch
assura explain A05001  # Linear type double use
assura explain A06001  # Invalid typestate transition
```

## API Integration

### gRPC

```bash
assura-server  # Starts on port 50051
```

### HTTP/JSON

```bash
curl -X POST http://localhost:8080/v1/check \
  -H "Content-Type: application/json" \
  -d '{"source": "contract Foo { input(x: Int) output(y: Int) requires { x > 0 } ensures { y > 0 } effects { pure } }"}'
```

## VS Code Extension

Install from the Marketplace or build from source:

```bash
cd editors/vscode
npm install && npm run compile
```

Features: syntax highlighting, inline diagnostics, go-to-definition, hover.