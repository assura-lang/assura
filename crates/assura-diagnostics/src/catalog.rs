use std::collections::HashMap;
use std::sync::LazyLock;

use super::ErrorInfo;

/// Return the full catalog of known error codes with explanations.
pub fn error_catalog() -> Vec<ErrorInfo> {
    vec![
        ErrorInfo {
            code: "A01001",
            name: "Unexpected character",
            description: "The lexer encountered a character that is not part of any valid \
                          token. This usually means a stray symbol or an unsupported \
                          Unicode character in the source file.",
            example: r#"  contract Foo {
      requires: x > 0 @ y   // '@' is not a valid Assura operator
  }"#,
            fix: "Remove or replace the invalid character. Check for copy-paste \
                 artifacts, smart quotes, or characters from other languages.",
        },
        ErrorInfo {
            code: "A01002",
            name: "Unexpected token",
            description: "The parser found a token that does not fit the expected grammar \
                          at this position. This is the most common syntax error and can \
                          indicate a missing keyword, misplaced punctuation, or an \
                          incomplete declaration.",
            example: r#"  contract Foo {
      requires x > 0   // missing ':' after 'requires'
  }"#,
            fix: "Check for missing colons after clause keywords (requires:, ensures:), \
                 unmatched braces or parentheses, or misspelled keywords. The error \
                 message shows what was expected vs. what was found.",
        },
        ErrorInfo {
            code: "A02001",
            name: "Undefined name",
            description: "A name was used that has not been defined in the current scope \
                          or any enclosing scope. This applies to type names, variable \
                          names, contract names, and function names.",
            example: r#"  contract Foo {
      requires: bar > 0   // 'bar' is not defined anywhere
  }

  type Alias = Unknown   // 'Unknown' is not a known type"#,
            fix: "Check spelling of the name. Ensure the type or variable is defined \
                 before use, or add an import if it comes from another module. \
                 Built-in types (Int, Bool, String, etc.) are always available.",
        },
        ErrorInfo {
            code: "A02003",
            name: "Duplicate definition",
            description: "Two declarations in the same scope share the same name. Each \
                          name must be unique within its scope (module, service, contract, \
                          or function body).",
            example: r#"  contract Foo {
      requires: x > 0
  }

  contract Foo {            // duplicate: 'Foo' already defined
      requires: y > 0
  }"#,
            fix: "Rename one of the conflicting declarations to a unique name. If you \
                 intended to extend a contract, use the 'extends' keyword instead of \
                 redefining it.",
        },
        ErrorInfo {
            code: "A02005",
            name: "Circular import",
            description: "The import graph contains a cycle. Module A imports module B, \
                          which (directly or indirectly) imports module A. Assura does \
                          not allow circular dependencies between modules.",
            example: r#"  // file: a.assura
  import b

  // file: b.assura
  import a               // circular: a -> b -> a"#,
            fix: "Break the cycle by extracting shared definitions into a third module \
                 that both modules can import, or restructure the dependency so it flows \
                 in one direction.",
        },
        ErrorInfo {
            code: "A03001",
            name: "Type mismatch",
            description: "An expression has a type that does not match the expected type \
                          in context. This includes operand type mismatches in binary \
                          operations, wrong return types, assignment type conflicts, \
                          and invalid empty tuple types such as `(,)` or `(Int,,Bool)`.",
            example: r#"  contract Add {
      requires: x > "hello"   // comparing Int with String
  }

  contract BadOut {
      output(result: (,))     // empty tuple type is invalid
  }"#,
            fix: "Ensure both sides of an operation have compatible types. Check that \
                 function return types match their declared output type. Use explicit \
                 conversions when needed (e.g., 'as Int'). For tuples, use `()` for Unit \
                 or `(T,)` for a 1-tuple; empty elements are not allowed.",
        },
        ErrorInfo {
            code: "A03002",
            name: "Argument count mismatch",
            description: "A function or contract was called with the wrong number of \
                          arguments. The call must provide exactly the number of \
                          parameters declared in the function signature.",
            example: r#"  fn add(a: Int, b: Int) -> Int

  // ...
  add(1)          // error: expected 2 arguments, got 1
  add(1, 2, 3)    // error: expected 2 arguments, got 3"#,
            fix: "Provide exactly the number of arguments that the function expects. \
                 Check the function signature to see its parameter list.",
        },
        ErrorInfo {
            code: "A03005",
            name: "Unknown field",
            description: "A field or method access (expr.field / expr.method()) refers to \
                          a name that does not exist on the type of the expression. This \
                          includes misspelled fields, out-of-range tuple indices, and \
                          unknown methods on built-in types.",
            example: r#"  type Point { x: Int, y: Int }

  contract CheckPoint {
      requires: p.z > 0   // Point has no field 'z'
  }

  contract BadTuple {
      input(t: (Int, Bool))
      ensures: result == t.2   // index 2 out of range (arity 2)
  }"#,
            fix: "Check the type definition for available field names or valid tuple \
                 indices (0-based, less than arity). Fix the spelling or use a valid \
                 field. If the field should exist, add it to the type definition.",
        },
        ErrorInfo {
            code: "A03006",
            name: "Clause type mismatch",
            description: "A 'requires' or 'ensures' clause must evaluate to a Bool. \
                          The expression in the clause has a non-Bool type, which means \
                          it cannot serve as a logical predicate.",
            example: r#"  contract Foo {
      requires: x + 1     // Int expression, not Bool
      ensures: "done"     // String, not Bool
  }"#,
            fix: "Ensure requires/ensures clauses are boolean expressions. Use \
                 comparison operators (==, !=, <, >, <=, >=), logical operators \
                 (and, or, not), or boolean-valued function calls.",
        },
        ErrorInfo {
            code: "A10001",
            name: "Non-exhaustive pattern",
            description: "A match expression does not cover all possible variants of the \
                          enum being matched. Every variant must be handled either \
                          explicitly or via a wildcard pattern to ensure the match is \
                          total.",
            example: r#"  enum Color { Red, Green, Blue }

  match c {
      Red => 1,
      Green => 2
      // missing: Blue
  }"#,
            fix: "Add the missing variant(s) to the match expression, or add a wildcard \
                 pattern (_ => ...) to handle all remaining cases. The error message \
                 lists which variants are not covered.",
        },
        // -- Phase 1: Linearity errors (A05xxx) --
        ErrorInfo {
            code: "A05001",
            name: "Linear variable used more than once",
            description: "A variable with linear grade (:_1) was used more than once \
                          computationally. Linear variables must be consumed exactly once. \
                          Refinement predicates (ghost/logical uses) do not count.",
            example: r#"  fn bad(x: Int :_1) -> (Int, Int)
      effects: pure
  { (x, x) }   // x used twice"#,
            fix: "Restructure the code to use the linear variable exactly once. If you \
                 need the value in two places, clone it first (if the type supports it) \
                 or refactor to avoid the double use.",
        },
        ErrorInfo {
            code: "A05002",
            name: "Linear variable not consumed",
            description: "A variable with linear grade (:_1) was never used. Linear \
                          variables must be consumed exactly once before going out of scope.",
            example: r#"  fn bad(x: Int :_1) -> Int
      effects: pure
  { 42 }   // x is never used"#,
            fix: "Use the variable before it goes out of scope, or explicitly drop it. \
                 If you intentionally do not need the value, consider changing its grade \
                 to :_omega (unlimited).",
        },
        ErrorInfo {
            code: "A05003",
            name: "Usage grade violation",
            description: "A variable was used a number of times that does not match its \
                          declared usage grade. Grade :_n means exactly n uses.",
            example: r#"  fn bad(x: Int :_2) -> Int
      effects: pure
  { x }   // used once, but grade requires exactly 2"#,
            fix: "Adjust the code to use the variable the exact number of times \
                 specified by its grade, or change the grade to match actual usage.",
        },
        ErrorInfo {
            code: "A05004",
            name: "Linear variable consumed in only one branch",
            description: "A linear variable was consumed in one branch of a conditional \
                          but not the other. Linear variables must be consumed in all \
                          branches or none.",
            example: r#"  fn bad(x: Int :_1, flag: Bool) -> Int
      effects: pure
  { if flag then x else 0 }
  // x consumed in 'then' branch but not 'else'"#,
            fix: "Ensure the linear variable is consumed in every branch of the \
                 conditional, or restructure to consume it before the branch point.",
        },
        // -- Phase 1: Typestate errors (A06xxx) --
        ErrorInfo {
            code: "A06001",
            name: "Invalid state transition",
            description: "An operation was called on an object that is not in the required \
                          state. Each operation declares which state the object must be in \
                          before the operation is valid.",
            example: r#"  service OrderService {
      states: [Created, Paid, Shipped]
      operation ship(order) {
          requires: state == Paid   // must be Paid
      }
  }
  // calling ship() on a Created order -> A06001"#,
            fix: "Check the object's current state before calling the operation. Use a \
                 prior state transition to move the object to the required state first.",
        },
        ErrorInfo {
            code: "A06002",
            name: "Typestate variable not linear",
            description: "A variable with typestate tracking must be linear (:_1). \
                          Typestate requires that the object is consumed and recreated \
                          at each state transition, which requires linearity.",
            example: r#"  fn bad(conn: Connection)  // missing :_1
  // conn has states but is not linear -> A06002"#,
            fix: "Add the linear grade :_1 to the variable declaration. Typestate \
                 variables must be linear to ensure state transitions are tracked.",
        },
        ErrorInfo {
            code: "A06003",
            name: "Unknown state",
            description: "A state name used in a transition or assertion does not match \
                          any state in the object's state declaration.",
            example: r#"  service Foo {
      states: [A, B, C]
      operation go_to_d(x) {
          ensures: state == D   // D is not in [A, B, C] -> A06003
      }
  }"#,
            fix: "Use one of the declared states from the 'states:' declaration. \
                 If you need a new state, add it to the states list.",
        },
        ErrorInfo {
            code: "A06004",
            name: "Ambiguous state after branch",
            description: "After a conditional (if/match) where different branches lead to \
                          different states, the object's state is ambiguous. The type \
                          checker cannot determine which state the object is in.",
            example: r#"  if condition then
      order.pay()      // state -> Paid
  else
      order.cancel()   // state -> Cancelled
  // order state is ambiguous: Paid or Cancelled -> A06004"#,
            fix: "Restructure the code so that all branches end with the object in the \
                 same state, or consume the object before the branch point.",
        },
        // -- Phase 1: Effect errors (A07xxx) --
        ErrorInfo {
            code: "A07001",
            name: "Undeclared effect",
            description: "A function performs an effect that is not listed in its \
                          'effects' clause. Every side effect must be explicitly declared.",
            example: r#"  fn save(data: Data) -> Unit
      effects: database.read   // only declares read
  {
      db.write(data)   // database.write not declared -> A07001
  }"#,
            fix: "Add the missing effect to the function's 'effects' clause. If the \
                 function should be pure, remove the effectful operation.",
        },
        ErrorInfo {
            code: "A07002",
            name: "Effect containment violation",
            description: "A function calls another function whose effects are not a \
                          subset of the caller's declared effects. A pure function \
                          cannot call an effectful function.",
            example: r#"  fn helper() -> Unit
      effects: io.write

  fn pure_fn() -> Unit
      effects: pure
  { helper() }   // calls io.write from pure context -> A07002"#,
            fix: "Either add the callee's effects to the caller's effect declaration, \
                 or avoid calling effectful functions from restricted contexts.",
        },
        ErrorInfo {
            code: "A07003",
            name: "Unknown or forbidden effect",
            description: "An effect name in `effects` or `must-not` is not a known \
                          effect, or a known effect appears in the must-not list. \
                          Built-in groups: io, database, logging, mem, net, fs, \
                          rng, time, alloc, diverge, random, pure. Dotted leaves include \
                          network.connect, network.send, network.receive (not `network` \
                          or `crypto`).",
            example: r#"  fn bad() -> Unit
      effects: teleport   // 'teleport' is not a known effect -> A07003

  contract Forbidden {
      effects { io }
      must-not { io }     // io is in the must-not list -> A07003
  }"#,
            fix: "If the name is unknown, use a built-in name. `network` and `crypto` \
                 are not effect names; use `net` or `network.connect`. If a known \
                 effect is listed in `must-not`, remove it from `effects` or drop \
                 it from `must-not`.",
        },
        // -- A02006: Duplicate import --
        ErrorInfo {
            code: "A02006",
            name: "Duplicate import",
            description: "The same module is imported more than once. Duplicate \
                          imports are redundant and may indicate a copy-paste error.",
            example: r#"  import std.collections;
  import std.collections;  // duplicate"#,
            fix: "Remove the duplicate import statement.",
        },
        // -- A02007: Unused import --
        ErrorInfo {
            code: "A02007",
            name: "Unused import",
            description: "An import was declared but none of its symbols are used \
                          in the file. This is a warning, not an error.",
            example: r#"  import std.math;  // unused

  contract Foo {
      input { x: Int }  // does not use std.math
  }"#,
            fix: "Remove the unused import, or use a symbol from the imported module.",
        },
        // -- A02008: Invalid import path segment --
        ErrorInfo {
            code: "A02008",
            name: "Invalid import path segment",
            description: "An import path contains a segment that is not a valid module \
                          name. Segments must start with a lowercase ASCII letter or \
                          underscore, followed by letters, digits, or underscores.",
            example: r#"  import std.Math;  // A02008: 'Math' starts with uppercase"#,
            fix: "Use lowercase module names: `import std.math;`",
        },
        // -- A02010: Cannot resolve import module --
        ErrorInfo {
            code: "A02010",
            name: "Cannot resolve import",
            description: "An import names a module that is not present in the project \
                          module map. In project mode this is an error. In single-file \
                          mode it is a warning because external modules may not be loaded.",
            example: r#"  import missing_mod;  // A02010: module not found"#,
            fix: "Fix the module path, add the missing .assura file under the project \
                 root, or run `assura check <project-dir>` for multi-file projects.",
        },
        // -- A03010: Division by zero --
        ErrorInfo {
            code: "A03010",
            name: "Division by zero",
            description: "A division or modulo operation has a constant zero divisor, \
                          which would cause a runtime panic.",
            example: r#"  contract DivZero {
      input { x: Int }
      ensures { x / 0 == 0 }  // A03010: division by zero
  }"#,
            fix: "Use a non-zero divisor, or add a requires clause that the \
                 divisor is non-zero.",
        },
        // -- A08001: Taint flow violation --
        ErrorInfo {
            code: "A08001",
            name: "Taint flow violation",
            description: "A value with an untrusted taint label flows to a \
                          sink that requires a higher trust level. This \
                          indicates a potential information flow vulnerability.",
            example: r#"  contract TaintViolation {
      input { user_data: @Untrusted String }
      ensures { db.write(user_data) }  // needs @Trusted
  }"#,
            fix: "Validate or sanitize the untrusted input before passing it \
                 to the trusted sink, or adjust the taint labels.",
        },
        // -- A05025: Unresolved prophecy variable --
        ErrorInfo {
            code: "A05025",
            name: "Unresolved prophecy variable",
            description: "A prophecy variable is referenced in a contract clause but \
                          never resolved via a resolve() or resolve_prophecy() call. \
                          Prophecy variables represent future values that must eventually \
                          be determined; leaving one unresolved means the verification \
                          is incomplete.",
            example: r#"  prophecy future_val: Int

  contract UseProphecy {
      input(x: Int)
      requires { x > 0 }
      ensures { result > future_val }   // A05025: never resolved
  }"#,
            fix: "Add a resolve(future_val) call in an ensures or requires clause to \
                 bind the prophecy variable to a concrete value.",
        },
        ErrorInfo {
            code: "A08002",
            name: "Information flow: implicit leak",
            description: "A secret value influences a public output through \
                          control flow (e.g., an if-branch on a secret condition). \
                          This is an implicit information flow violation.",
            example: r#"  fn check(secret: @Confidential Bool) -> @Public Int
  {
      if secret then 1 else 0   // A08002: implicit leak
  }"#,
            fix: "Remove the dependency of the public output on the secret \
                 value, or explicitly declassify the information.",
        },
        ErrorInfo {
            code: "A08003",
            name: "Declassification without justification",
            description: "A declassify operation lowers the security label of data \
                          without providing the required justification label.",
            example: r#"  fn leak(x: @Confidential Int) -> @Public Int
  {
      declassify(x)   // missing purpose -> A08003
  }"#,
            fix: "Provide a purpose label for the declassification: \
                 declassify(x, purpose: \"user_consent\").",
        },
        ErrorInfo {
            code: "A08004",
            name: "Missing taint label",
            description: "A function accepts external input without a taint label. \
                          All data from external sources must be explicitly labeled.",
            example: r#"  extern fn read_input() -> String   // missing @Untrusted
  // Should be: -> @Untrusted String"#,
            fix: "Add a taint annotation to the return type: @Untrusted.",
        },
        ErrorInfo {
            code: "A08005",
            name: "Security label hierarchy violation",
            description: "An assignment or operation violates the security label \
                          hierarchy. Data cannot flow from higher security levels \
                          to lower ones without explicit declassification.",
            example: r#"  fn bad(secret: @Restricted Data) -> @Public Data
  {
      secret   // Restricted -> Public without declassify -> A08005
  }"#,
            fix: "Add a declassify operation with appropriate justification, \
                 or adjust the security labels.",
        },
        // NOTE: Spec Section 7.2 defines A09001 as "Non-exhaustive pattern match",
        // but this implementation uses A09001 for "Missing decreases clause"
        // (totality). Non-exhaustive patterns use A10001 instead. This deviation
        // is intentional: the totality checker emits A09001 at runtime and
        // renaming would require updating totality.rs, meta.rs, and all tests.
        ErrorInfo {
            code: "A09001",
            name: "Missing decreases clause",
            description: "A recursive function does not have a 'decreases' clause. \
                          Recursive functions must prove termination by providing a \
                          measure that decreases on each recursive call.",
            example: r#"  fn factorial(n: Int) -> Int
  {
      if n == 0 then 1 else n * factorial(n - 1)
      // missing: decreases { n }
  }"#,
            fix: "Add a 'decreases' clause with a non-negative expression that \
                 strictly decreases on each recursive call. Example: decreases { n }.",
        },
        ErrorInfo {
            code: "A09002",
            name: "Decreases clause not proven",
            description: "The SMT solver could not prove that the decreases measure \
                          strictly decreases on every recursive call, or that the \
                          measure remains non-negative.",
            example: r#"  fn bad(n: Int) -> Int
      decreases { n }
  {
      bad(n + 1)   // n increases, not decreases -> A09002
  }"#,
            fix: "Ensure the decreases expression becomes strictly smaller on \
                 each recursive call and remains non-negative. The base case \
                 must be reachable.",
        },
        ErrorInfo {
            code: "A09003",
            name: "Partial function without 'partial' marker",
            description: "A function may not terminate but is not marked as 'partial'. \
                          Functions that may loop forever must be explicitly annotated.",
            example: r#"  fn server_loop() -> Never
  {
      loop { handle_request() }
      // infinite loop without 'partial' -> A09003
  }"#,
            fix: "Mark the function as 'partial' to acknowledge it may not \
                 terminate, or add a termination proof with 'decreases'.",
        },
        ErrorInfo {
            code: "A09004",
            name: "Mutual recursion without termination proof",
            description: "Two or more functions call each other recursively without \
                          a combined termination measure that decreases across the \
                          call cycle.",
            example: r#"  fn is_even(n: Nat) -> Bool { if n == 0 then true else is_odd(n-1) }
  fn is_odd(n: Nat) -> Bool { if n == 0 then false else is_even(n-1) }
  // need decreases { n } on both"#,
            fix: "Add 'decreases' clauses to all functions in the recursive \
                 group. The measure must decrease on every call in the cycle.",
        },
        // -- Phase 1: SMT verification (A05100) --
        ErrorInfo {
            code: "A05100",
            name: "Verification failed (counterexample found)",
            description: "The SMT solver found a counterexample showing that a contract \
                          clause does not hold. The model shows concrete values for \
                          variables that violate the property.",
            example: r#"  contract AlwaysPositive {
      requires: true
      ensures: x > 0
  }
  // Counterexample: x = 0 or x = -1"#,
            fix: "Either strengthen the requires clause to eliminate the counterexample \
                 inputs, or weaken the ensures clause to account for the case. The \
                 counterexample model shows exactly which inputs break the contract.",
        },
        ErrorInfo {
            code: "A05101",
            name: "Verification timeout",
            description: "The SMT solver timed out before it could prove or disprove the \
                          contract clause. This does not mean the contract is wrong; the \
                          solver just could not decide within the configured time limit.",
            example: r#"  contract ComplexArithmetic {
      requires: a * b * c > 0   // may time out on non-linear
      ensures: a * b * c < 1000
  }"#,
            fix: "Increase the solver timeout with `--timeout <seconds>`. If that does \
                 not help, try simplifying the contract: break complex conditions into \
                 smaller contracts, add intermediate lemmas, or avoid non-linear \
                 arithmetic.",
        },
        ErrorInfo {
            code: "A05102",
            name: "Verification skipped (known compiler limitation)",
            description: "The contract clause could not be proved. Two common cases: \
                          the SMT encoder does not yet support a language feature used \
                          in the clause, or `result` is unconstrained because there is \
                          no IR body to bind it. Both are known compiler limitations, \
                          not necessarily a problem with your contract. The clause is \
                          skipped with a warning (error under `--strict`).",
            example: r#"  contract UnconstrainedResult {
      requires { x >= 0 }
      ensures { result >= 0 }   // result has no IR binding
  }

  contract DeepAccess {
      requires { obj.nested.field > 0 }   // deep field chains
      ensures { true }
  }"#,
            fix: "If `result` is unconstrained, simplify ensures (`result == expr` / \
                 bounds), materialize IR with `assura build --write-ir`, or use \
                 `--auto-implement` for LLM residuals. True encoder gaps stay skipped \
                 until the feature is encoded; you can track progress in the project \
                 roadmap.",
        },
        ErrorInfo {
            code: "A05103",
            name: "Verification inconclusive",
            description: "The SMT solver returned 'unknown' for a reason other than a known \
                          compiler limitation. The solver could not determine whether the \
                          contract clause holds. This often happens with non-linear \
                          arithmetic, quantifiers over unbounded domains, or very complex \
                          constraint systems.",
            example: r#"  contract NonLinear {
      requires: x > 0
      ensures: x * x * x > 0   // non-linear: solver may be unable to decide
  }"#,
            fix: "Simplify the contract: linearize arithmetic where possible, add bounds \
                 on quantifier domains, or split into smaller verifiable pieces. \
                 Try a different solver with `--solver cvc5`.",
        },
        // -- A04008: Verification clause quality warnings --
        ErrorInfo {
            code: "A04008",
            name: "Ensures references unconstrained output",
            description: "An ensures clause references `result` or an output parameter. \
                          These are free variables in SMT; the solver can assign them \
                          any value, causing spurious counterexamples.",
            example: r#"  contract safe_add(x: Int, y: Int) -> Int
    requires { x >= 0 }
    ensures  { result >= 0 }    // result is unconstrained"#,
            fix: "Bind `result` with IR (`assura build --write-ir` or `--auto-implement`) \
                 or write an equality/bound the synthesizer can plan (`result == x`, \
                 `result >= lo`). Input-only ensures remain valid when you do not need \
                 a result witness. For extern Bytes/String, result.length() >= 0 is safe \
                 (background axiom).",
        },
        // -- A22003: Unbounded allocation --
        ErrorInfo {
            code: "A22003",
            name: "Unbounded allocation detected",
            description: "An allocation has no proved upper bound on its size. Without a \
                          bound, the allocator may consume unlimited memory.",
            example: r#"  contract LeakyBuffer {
    input(size: Nat)
    alloc buf                   // -> A22003 (no bounded clause)
    requires { size > 0 }
  }"#,
            fix: "Add a `bounded buf` clause to prove the allocation has an upper bound.",
        },
        // -- A23016, A23019: Weak memory ordering --
        ErrorInfo {
            code: "A23016",
            name: "Relaxed read without view check",
            description: "A relaxed memory ordering is used in a contract with an ensures \
                          clause. Values read with Relaxed ordering may be stale; use \
                          Acquire for value-dependent assertions.",
            example: r#"  contract ReadCounter {
    requires(ordering: relaxed)
    ensures(result >= 0)  // -> A23016 (relaxed + ensures)
  }"#,
            fix: "Use Acquire ordering when the read value is used in postconditions.",
        },
        ErrorInfo {
            code: "A23019",
            name: "Fence ordering mismatch",
            description: "A fence operation uses an unknown memory ordering. Expected one \
                          of: relaxed, acquire, release, acqrel, seq_cst.",
            example: r#"  contract Sync { requires(ordering: invalid_ordering) }  // -> A23019"#,
            fix: "Use a valid memory ordering: relaxed, acquire, release, acqrel, or seq_cst.",
        },
        ErrorInfo {
            code: "A31006",
            name: "Liveness unproven within bound K",
            description: "A liveness block has no `prove` clause. At least one liveness \
                          property must be stated for verification.",
            example: r#"  liveness block HeartbeatLiveness { }  // -> A31006 (no prove clause)"#,
            fix: "Add a `prove` clause with a liveness property (e.g., `prove { leads_to(...) }`).",
        },
        ErrorInfo {
            code: "A31007",
            name: "Missing fairness assumption",
            description: "A liveness block uses `leads_to` but has no `assume fair` clause. \
                          Fairness assumptions are required for leads-to proofs.",
            example: r#"  liveness block Progress {
    prove { leads_to(waiting, served) }
    // Missing: assume { fair }  -> A31007
  }"#,
            fix: "Add an `assume { fair }` clause to the liveness block.",
        },
        ErrorInfo {
            code: "A43004",
            name: "Invalid encoding: byte sequence not valid",
            description: "A byte sequence does not form a valid string in the declared \
                          encoding (UTF-8, UTF-16, etc.).",
            example: r#"  // Bytes [0xFF, 0xFE] not valid UTF-8 -> A43004"#,
            fix: "Validate byte sequences before interpreting as strings, or use \
                 the correct encoding.",
        },
        ErrorInfo {
            code: "A43005",
            name: "Implicit transcode detected",
            description: "An encoding conversion happens implicitly without an explicit \
                          transcode() call, which could lose data.",
            example: r#"  // Assigning UTF-16 string to UTF-8 variable without
  // explicit transcode() -> A43005"#,
            fix: "Use explicit transcode() for encoding conversions.",
        },
        ErrorInfo {
            code: "A53003",
            name: "After-all predicate not satisfied",
            description: "The final output of a multi-pass computation does not satisfy \
                          the after_all predicate.",
            example: r#"  // after_all { output.is_sorted() }
  // Final output is not sorted -> A53003"#,
            fix: "Fix the multi-pass algorithm to satisfy the after_all predicate.",
        },
        ErrorInfo {
            code: "A53006",
            name: "Quantifier missing trigger annotation",
            description: "A quantifier has no `triggers` clause. Without triggers, \
                          the SMT solver may enumerate all possible values, causing \
                          verification timeouts.",
            example: r#"  forall x in xs: x > 0  // -> A53006 (no triggers clause)"#,
            fix: "Add a `triggers` clause to guide the SMT solver.",
        },
        // -- A01000: File read error --
        ErrorInfo {
            code: "A01000",
            name: "File read error",
            description: "The compiler could not read the source file. The file may \
                          not exist, the path may be incorrect, or the process may \
                          lack permission to read it.",
            example: r#"  assura check nonexistent_file.assura"#,
            fix: "Check that the file path is correct and the file exists.",
        },
        ErrorInfo {
            code: "A03007",
            name: "Invalid dependent type index",
            description: "A dependent type index expression is not valid for its kind. \
                          Nat indices require integer arithmetic over index variables. \
                          Also reported for length-preserving collection operations \
                          missing a postcondition.",
            example: r#"  contract Sort<n: Nat>(items: List<Int>)
      // missing: ensures { len(result) == len(items) }"#,
            fix: "For collection operations, add a `len(result) == len(input)` \
                 postcondition. For indices, use only arithmetic on index variables.",
        },
        ErrorInfo {
            code: "A03008",
            name: "Invalid Bool index expression",
            description: "A Bool-kinded dependent type index must be a direct variable \
                          reference or boolean literal. Arithmetic expressions are not \
                          allowed in Bool index positions.",
            example: r#"  type Guarded<b: Bool> = { value: Int }
      fn bad(x: Guarded<1 + 1>) -> Int  // not a Bool"#,
            fix: "Use a boolean variable reference or literal (true/false) as the index.",
        },
        ErrorInfo {
            code: "A03009",
            name: "Invalid Enum index expression",
            description: "An Enum-kinded dependent type index must be a direct variable \
                          reference or a variant name of the expected enum type.",
            example: r#"  type Tagged<s: Status> = { data: Bytes }
      fn bad(x: Tagged<1 + 2>) -> Int  // not a Status variant"#,
            fix: "Use a variable reference or variant name matching the expected enum type.",
        },
        ErrorInfo {
            code: "A03011",
            name: "Dependent type index kind mismatch",
            description: "Two dependent types being compared or unified have indices of \
                          different kinds at the same position (e.g., one is Nat and the \
                          other is Bool).",
            example: r#"  fn mismatch(a: Vec<Nat, n>, b: Vec<Bool, flag>) -> Bool
      // index 0 kinds differ: Nat vs Bool"#,
            fix: "Ensure both types use indices of the same kind at each position.",
        },
        ErrorInfo {
            code: "A03012",
            name: "Index variable used at runtime",
            description: "A dependent type index variable was used in a runtime expression. \
                          Index variables exist only at the type level and must be erased \
                          before code generation.",
            example: r#"  contract Slice<n: Nat>(buf: Bytes)
      ensures { result == n }  // n is type-level only"#,
            fix: "Do not use dependent type index variables in runtime expressions. \
                 Use a regular parameter instead.",
        },
        // -- A05026: Unconstrained prophecy --
        ErrorInfo {
            code: "A05026",
            name: "Unconstrained prophecy variable",
            description: "A prophecy variable was declared but never constrained. \
                          Prophecy variables must have at least one constraint to be \
                          meaningful in verification.",
            example: r#"  contract Foo(x: Int)
      prophecy { pv: Int }   // no constraint on pv"#,
            fix: "Add a constraint relating the prophecy variable to the contract's \
                 inputs or outputs, or remove the unused declaration.",
        },
        // -- A05200: Unbounded quantifier warning --
        ErrorInfo {
            code: "A05200",
            name: "Unbounded quantifier warning",
            description: "A forall or exists quantifier has no finite domain bound. \
                          Unbounded quantifiers may cause SMT solver timeouts or \
                          incomplete verification.",
            example: r#"  contract Search(haystack: List<Int>)
      ensures { forall i: Int :: haystack[i] >= 0 }"#,
            fix: "Bound the quantifier variable to a finite range, e.g., \
                 forall i :: 0 <= i && i < len(xs) ==> xs[i] >= 0.",
        },
        ErrorInfo {
            code: "A08101",
            name: "Missing buffer bounds check",
            description: "A buffer is accessed without a bounds check in the \
                          precondition. Buffer accesses must have a requires clause \
                          constraining the index to be within the buffer length.",
            example: r#"  contract Read(buf: Bytes, idx: Nat)
      // missing: requires { idx < buf.len }
      ensures { result == buf[idx] }"#,
            fix: "Add a requires clause constraining the index to be within buf.len.",
        },
        ErrorInfo {
            code: "A08102",
            name: "Invalid memory region containment",
            description: "A sub-region cannot be proven to be contained within its \
                          parent region. This can occur when regions reference \
                          different buffers or when bounds are incomplete.",
            example: r#"  ghost region sub = buf_a[0..10]
      ghost region parent = buf_b[0..20]
      // sub is on buf_a, parent is on buf_b"#,
            fix: "Ensure both regions reference the same buffer and that the sub-region's \
                 bounds are within the parent's bounds.",
        },
        ErrorInfo {
            code: "A08103",
            name: "Ghost region references missing buffer",
            description: "A ghost memory region references a buffer name that does not \
                          exist in the current scope.",
            example: r#"  ghost region r = missing_buf[0..10]
      // missing_buf is not defined"#,
            fix: "Ensure the buffer name in the ghost region declaration matches \
                 an existing parameter or variable.",
        },
        ErrorInfo {
            code: "A09101",
            name: "Tainted data used as array index",
            description: "Data from an untrusted source is used as an array index \
                          without prior validation. This can lead to out-of-bounds \
                          access vulnerabilities.",
            example: r#"  contract Read(buf: Bytes, idx: @taint:untrusted Nat)
      ensures { result == buf[idx] }  // idx is untrusted"#,
            fix: "Validate the tainted index before using it (e.g., add a bounds \
                 check in the requires clause).",
        },
        ErrorInfo {
            code: "A09102",
            name: "Tainted data used as allocation size",
            description: "An untrusted value is passed as an argument to an allocation \
                          function. This can lead to integer overflow or \
                          denial-of-service attacks.",
            example: r#"  contract Allocate(sz: @taint:untrusted Nat)
      ensures { result == alloc(sz) }  // sz is untrusted"#,
            fix: "Validate and bound the allocation size before passing it to an \
                 allocation function.",
        },
        ErrorInfo {
            code: "A09103",
            name: "Tainted data flows to trusted sink",
            description: "An untrusted or insufficiently validated value is passed to \
                          a function parameter that requires a higher trust level.",
            example: r#"  contract Process(data: @taint:untrusted Bytes)
      ensures { result == trusted_write(data) }  // needs trusted"#,
            fix: "Validate the data before passing it to the trusted sink, or \
                 promote its taint label via a validation step.",
        },
        // -- A10002: Match without wildcard --
        ErrorInfo {
            code: "A10002",
            name: "Match without wildcard arm",
            description: "A match expression on a value of unknown type has no wildcard \
                          catch-all arm. Without a wildcard, the match may be \
                          non-exhaustive at runtime.",
            example: r#"  match x { 1 => "one", 2 => "two" }
      // missing wildcard arm for other values"#,
            fix: "Add a wildcard _ => ... arm to handle all unmatched cases.",
        },
        ErrorInfo {
            code: "A10101",
            name: "Potential integer overflow",
            description: "An arithmetic operation on fixed-width integer types can \
                          produce a result outside the representable range.",
            example: r#"  contract Add(a: U8, b: U8)
      ensures { result == a + b }  // U8 + U8 can exceed 255"#,
            fix: "Use checked arithmetic (e.g., checked_add) or widen the result \
                 type to accommodate the full range.",
        },
        ErrorInfo {
            code: "A10102",
            name: "Unsafe narrowing cast",
            description: "A cast between fixed-width integer types can lose data because \
                          the source range does not fit in the target type.",
            example: r#"  contract Truncate(x: U32)
      ensures { result == x as U16 }  // U32 > U16 range"#,
            fix: "Add a bounds check before the cast, or use a wider target type.",
        },
        ErrorInfo {
            code: "A10103",
            name: "Signed/unsigned comparison mismatch",
            description: "A comparison operator is applied to a signed and an unsigned \
                          integer type. This can produce unexpected results when the \
                          signed value is negative.",
            example: r#"  contract Compare(a: I32, b: U32)
      ensures { result == (a < b) }  // signed vs unsigned"#,
            fix: "Cast both operands to a common type before comparing, or use \
                 same-signedness types.",
        },
        ErrorInfo {
            code: "A10104",
            name: "Fixed-width division by zero",
            description: "The divisor in a fixed-width division or modulo operation \
                          is a literal zero, causing a runtime panic.",
            example: r#"  contract Bad(x: U32)
      ensures { result == x / 0 }  // literal zero divisor"#,
            fix: "Add a requires clause that the divisor is non-zero, or use a \
                 checked division alternative.",
        },
        ErrorInfo {
            code: "A11001",
            name: "Extern missing trust boundary",
            description: "An extern declaration has no trust boundary annotation. \
                          Every extern must be annotated with @trust:trusted, \
                          @trust:audited, or @trust:untrusted.",
            example: r#"  extern fn malloc(size: Nat) -> Bytes
      // missing @trust annotation"#,
            fix: "Add a trust boundary annotation: @trust:trusted, @trust:audited, \
                 or @trust:untrusted.",
        },
        ErrorInfo {
            code: "A11002",
            name: "Untrusted extern without contract",
            description: "An extern declared @trust:untrusted has no requires or ensures \
                          clauses. Untrusted externs must have contracts to validate \
                          their inputs and outputs.",
            example: r#"  extern fn read_bytes(fd: Int) -> Bytes
      @trust:untrusted   // no requires/ensures"#,
            fix: "Add requires and ensures clauses to validate inputs and constrain \
                 outputs of the untrusted extern.",
        },
        ErrorInfo {
            code: "A11003",
            name: "Unvalidated FFI call result",
            description: "The return value of an untrusted FFI call is used without \
                          validation. Results from untrusted externs must be validated \
                          before use.",
            example: r#"  contract Process(fd: Int)
      ensures { result == read_raw(fd) }  // unvalidated"#,
            fix: "Wrap the return value of the untrusted FFI call in a validate \
                 block before using it.",
        },
        ErrorInfo {
            code: "A11004",
            name: "Unsafe code outside FFI wrapper",
            description: "A function uses unsafe operations but is not marked as an \
                          FFI wrapper. Unsafe code must be confined to dedicated extern \
                          wrapper functions.",
            example: r#"  fn compute(x: Int) -> Int
      // uses unsafe internally, not an FFI wrapper"#,
            fix: "Move unsafe operations into a dedicated extern wrapper function.",
        },
        ErrorInfo {
            code: "A11005",
            name: "Trust boundary without contracts",
            description: "An extern has a trust boundary annotation but no requires or \
                          ensures clauses. The trust boundary is meaningless without \
                          contracts.",
            example: r#"  extern fn read(fd: Int) -> Bytes
      @trust:untrusted   // no requires/ensures"#,
            fix: "Add requires and ensures clauses to enforce the declared trust boundary.",
        },
        ErrorInfo {
            code: "A12001",
            name: "Swallowed must-propagate error",
            description: "An error code with a must_propagate policy is being silently \
                          swallowed in a catch block. These errors must be propagated.",
            example: r#"  contract Handle()
      catch { SQLITE_CORRUPT => swallow }  // must propagate"#,
            fix: "Propagate the error to the caller instead of swallowing it, or \
                 translate it to an appropriate error code.",
        },
        ErrorInfo {
            code: "A12002",
            name: "Forbidden error translation",
            description: "An error code is being translated to another code forbidden \
                          by a must_not_mask policy. This prevents important error \
                          information from being lost.",
            example: r#"  contract Handle()
      catch { SQLITE_CORRUPT => translate(SQLITE_OK) }"#,
            fix: "Use a different target error code that does not mask the original, \
                 or propagate the error unchanged.",
        },
        ErrorInfo {
            code: "A12003",
            name: "Unchecked return value",
            description: "The return value of a function with a must_check policy is \
                          not used. Functions that return Result or error codes with \
                          this policy must have their return values checked.",
            example: r#"  contract Process()
      ensures { sqlite3_reset(stmt); true }  // result ignored"#,
            fix: "Capture and check the return value of the function call.",
        },
        ErrorInfo {
            code: "A13001",
            name: "Missing interface method",
            description: "A type claims to implement an interface but does not provide \
                          all required methods, including inherited super-interface methods.",
            example: r#"  interface Serializable { fn serialize(); fn deserialize() }
      impl Serializable for MyType { fn serialize() { ... } }
      // missing deserialize"#,
            fix: "Implement all required methods from the interface and its super-interfaces.",
        },
        ErrorInfo {
            code: "A13002",
            name: "Interface signature mismatch",
            description: "A method implementation does not match the signature required \
                          by the interface: parameter count, types, return type, or \
                          contract clauses differ.",
            example: r#"  interface Hasher { fn hash(data: Bytes) -> U64 }
      impl Hasher for X { fn hash(data: Bytes) -> Bool }
      // return type mismatch"#,
            fix: "Match the method signature exactly as declared in the interface.",
        },
        ErrorInfo {
            code: "A13003",
            name: "Reentrancy violation",
            description: "A method marked no_reentrancy on an interface is called \
                          reentrantly. Reentrant calls to such methods can cause \
                          state corruption.",
            example: r#"  interface Callback { @no_reentrancy fn on_event() }
      // on_event called reentrantly"#,
            fix: "Restructure the code to avoid reentrant calls to no_reentrancy methods.",
        },
        ErrorInfo {
            code: "A14001",
            name: "Secret-dependent branch",
            description: "A branch condition depends on secret data, creating a timing \
                          side-channel. Also reported when a modifies clause references \
                          an out-of-scope variable.",
            example: r#"  contract Check(key: @secret Bytes, input: Bytes)
      ensures { result == if key[0] == input[0] then 1 else 0 }"#,
            fix: "Use constant-time comparison functions instead of branching on \
                 secret data.",
        },
        ErrorInfo {
            code: "A14002",
            name: "Secret-dependent array index",
            description: "An array index expression depends on secret data. Variable-time \
                          memory access patterns leak information through cache timing \
                          side-channels.",
            example: r#"  contract Lookup(table: List<Int>, idx: @secret Nat)
      ensures { result == table[idx] }  // cache timing leak"#,
            fix: "Use a constant-time table lookup function instead of direct array \
                 indexing with secret-dependent indices.",
        },
        ErrorInfo {
            code: "A15001",
            name: "Structural invariant on non-recursive type",
            description: "A structural invariant (tree balance, BST ordering) was applied \
                          to a type that is not recursive. Structural invariants only \
                          make sense on recursive data structures.",
            example: r#"  // @invariant(tree_balance) on a non-recursive struct
      contract Flat { value: Int }"#,
            fix: "Apply the structural invariant only to types with recursive fields.",
        },
        ErrorInfo {
            code: "A15002",
            name: "Tree invariant insufficient fields",
            description: "A tree invariant requires at least 2 recursive fields \
                          (e.g., left and right children), but the type has fewer.",
            example: r#"  type Node { value: Int, next: Option<Node> }
      // only 1 recursive field for tree invariant"#,
            fix: "Add missing recursive fields, or use a list invariant like sorted instead.",
        },
        ErrorInfo {
            code: "A15003",
            name: "Sort invariant wrong field count",
            description: "A sort invariant requires exactly 1 recursive field (a next \
                          pointer), but the type has a different number.",
            example: r#"  type TreeNode { value: Int, left: Option<TreeNode>, right: Option<TreeNode> }
      // 2 recursive fields, sorted needs exactly 1"#,
            fix: "Use a tree invariant for multi-field recursive types, or restructure \
                 to have exactly one recursive field.",
        },
        ErrorInfo {
            code: "A15004",
            name: "Operation may violate invariant",
            description: "An operation modifies a type with a structural invariant but \
                          provides no preservation proof.",
            example: r#"  fn insert(tree: BalancedTree, value: Int) -> BalancedTree
      // no ensures proving balance is maintained"#,
            fix: "Add an ensures clause proving the structural invariant is preserved.",
        },
        ErrorInfo {
            code: "A16001",
            name: "Sensitive data not zeroized",
            description: "A variable holding sensitive data (keys, passwords) is dropped \
                          without secure erasure. The data may remain in memory.",
            example: r#"  let key: Bytes = get_secret_key()
      // key goes out of scope without zeroize()"#,
            fix: "Call zeroize() on the sensitive variable before it goes out of scope.",
        },
        ErrorInfo {
            code: "A16002",
            name: "Sensitive copy unmarked",
            description: "Sensitive data was copied to a variable not marked as sensitive. \
                          The copy will not be automatically zeroized.",
            example: r#"  let key: @sensitive Bytes = get_key()
      let backup = key  // backup is not @sensitive"#,
            fix: "Mark the target variable as @sensitive so it will also be zeroized.",
        },
        ErrorInfo {
            code: "A16003",
            name: "Sensitive return unmarked",
            description: "A function returns a sensitive variable but the return type \
                          is not annotated @sensitive. Callers will not know to zeroize.",
            example: r#"  fn get_key() -> Bytes {
      let key: @sensitive Bytes = derive_key()
      return key  // return type not @sensitive
  }"#,
            fix: "Annotate the function's return type with @sensitive.",
        },
        ErrorInfo {
            code: "A17001",
            name: "Crypto wrong key size",
            description: "The key size does not match the cryptographic algorithm \
                          specification. Using the wrong key size will cause runtime \
                          failures or weaken security.",
            example: r#"  let key: Bytes = random_bytes(16)  // only 128 bits
      aes256_encrypt(key, data)  // AES-256 needs 256 bits"#,
            fix: "Use a key size that matches the algorithm specification.",
        },
        ErrorInfo {
            code: "A17002",
            name: "Crypto wrong nonce size",
            description: "The nonce size does not match the cryptographic algorithm \
                          specification. An incorrect nonce size will cause encryption \
                          failures.",
            example: r#"  let nonce: Bytes = random_bytes(8)  // only 8 bytes
      aes_gcm_encrypt(key, nonce, data)  // needs 12 bytes"#,
            fix: "Use the nonce size required by the algorithm (e.g., 12 bytes \
                 for AES-GCM).",
        },
        ErrorInfo {
            code: "A17003",
            name: "Crypto nonce reuse risk",
            description: "A nonce is neither counter-based nor random, creating a risk \
                          of nonce reuse. Reusing a nonce with the same key completely \
                          breaks AEAD cipher security.",
            example: r#"  let nonce: Bytes = fixed_value()  // not counter or random
      aes_gcm_encrypt(key, nonce, data)"#,
            fix: "Use a counter-based (incrementing) or cryptographically random \
                 nonce for each encryption operation.",
        },
        ErrorInfo {
            code: "A17004",
            name: "Crypto tag not verified before use",
            description: "Decrypted data is used before the authentication tag has been \
                          verified. This allows an attacker to manipulate ciphertext \
                          and have corrupted plaintext processed.",
            example: r#"  let plaintext = decrypt(ciphertext)
      process(plaintext)  // tag not checked yet"#,
            fix: "Verify the authentication tag before using decrypted data. Use \
                 authenticated decryption APIs that fail on tag mismatch.",
        },
        ErrorInfo {
            code: "A18001",
            name: "Shared memory read without access mode",
            description: "A read access to a shared memory object was performed without \
                          acquiring shared_read or exclusive mode.",
            example: r#"  let val = shared_counter.value
      // no access mode acquired"#,
            fix: "Acquire shared_read or exclusive access mode before reading.",
        },
        ErrorInfo {
            code: "A18002",
            name: "Shared memory write without exclusive",
            description: "A write access to a shared memory object was performed without \
                          exclusive mode. Concurrent writes without exclusion cause \
                          data races.",
            example: r#"  shared_counter.value = 42
      // only shared_read access, not exclusive"#,
            fix: "Acquire exclusive access mode before writing.",
        },
        ErrorInfo {
            code: "A18003",
            name: "Shared memory data race",
            description: "Two threads access the same shared memory object with \
                          incompatible modes (write+read or write+write).",
            example: r#"  // Thread A: exclusive(counter) -> write
      // Thread B: shared_read(counter) -> read (conflict)"#,
            fix: "Ensure only one thread holds exclusive access at a time, or use \
                 atomic operations.",
        },
        ErrorInfo {
            code: "A20001",
            name: "Deterministic function uses non-deterministic source",
            description: "A function marked as deterministic uses a non-deterministic \
                          source such as HashMap, HashSet, or an unseeded RNG.",
            example: r#"  @deterministic
      fn process(data: List<Int>) -> List<Int>
      // uses HashMap internally"#,
            fix: "Replace non-deterministic sources with deterministic alternatives \
                 (BTreeMap, BTreeSet, seeded RNG).",
        },
        ErrorInfo {
            code: "A20002",
            name: "Deterministic function iterates hash collection",
            description: "A deterministic function iterates over a HashMap or HashSet, \
                          which have non-deterministic iteration order.",
            example: r#"  @deterministic
      fn keys(map: HashMap<String, Int>) -> List<String>
      // iteration order is non-deterministic"#,
            fix: "Use BTreeMap/BTreeSet for deterministic iteration, or sort results.",
        },
        ErrorInfo {
            code: "A21001",
            name: "Lock order violation",
            description: "A lock with a lower priority was acquired while holding a \
                          lock with a higher priority, violating the declared lock \
                          ordering. This can lead to deadlocks.",
            example: r#"  acquire(lock_a)  // priority 2
      acquire(lock_b)  // priority 1 -- violation"#,
            fix: "Always acquire locks in ascending priority order.",
        },
        ErrorInfo {
            code: "A21002",
            name: "Lock release out of order",
            description: "A lock was released while another lock acquired after it is \
                          still held. Locks must be released in reverse acquisition \
                          order (LIFO).",
            example: r#"  acquire(lock_a); acquire(lock_b)
      release(lock_a)  // lock_b still held"#,
            fix: "Release locks in reverse acquisition order.",
        },
        ErrorInfo {
            code: "A21003",
            name: "Lock with no defined order",
            description: "A lock is used without a defined position in the lock \
                          hierarchy. Without ordering, deadlock-freedom cannot \
                          be verified.",
            example: r#"  acquire(unranked_lock)  // no priority defined"#,
            fix: "Add the lock to the lock hierarchy with a defined priority.",
        },
        ErrorInfo {
            code: "A22001",
            name: "Unpaired allocation (memory leak)",
            description: "A heap allocation has no corresponding deallocation and is \
                          not managed by an arena. This is a memory leak.",
            example: r#"  let buf = allocate(1024)
      // buf never freed"#,
            fix: "Add a matching deallocate() call, or allocate from an arena.",
        },
        ErrorInfo {
            code: "A22002",
            name: "Double free",
            description: "An allocation was freed more than once. Double-free is \
                          undefined behavior and can lead to use-after-free vulnerabilities.",
            example: r#"  let buf = allocate(1024)
      deallocate(buf)
      deallocate(buf)  // double free"#,
            fix: "Remove the duplicate deallocation.",
        },
        ErrorInfo {
            code: "A22004",
            name: "Arena use after drop",
            description: "An allocation from an arena was used after the arena was \
                          dropped. All arena allocations become invalid when the arena \
                          is destroyed.",
            example: r#"  let arena = Arena::new()
      let buf = arena.alloc(64)
      drop(arena)
      read(buf)  // use after arena drop"#,
            fix: "Ensure all uses of arena-allocated memory occur before the arena \
                 is dropped.",
        },
        ErrorInfo {
            code: "A23001",
            name: "Circular buffer index exceeds capacity",
            description: "A logical index into a circular buffer equals or exceeds \
                          the buffer's capacity.",
            example: r#"  let buf = CircularBuffer::new(capacity: 8)
      buf.get(10)  // index 10 >= capacity 8"#,
            fix: "Ensure the index is less than the buffer's capacity.",
        },
        ErrorInfo {
            code: "A23002",
            name: "Circular buffer zero capacity",
            description: "A circular buffer has zero capacity, making the modular wrap \
                          computation undefined due to division by zero.",
            example: r#"  let buf = CircularBuffer::new(capacity: 0)
      buf.push(42)  // wrap undefined"#,
            fix: "Ensure the circular buffer has a capacity of at least 1.",
        },
        ErrorInfo {
            code: "A23003",
            name: "Circular buffer read when empty",
            description: "A read operation was attempted on an empty circular buffer.",
            example: r#"  let buf = CircularBuffer::new(capacity: 8)
      buf.read()  // buffer is empty"#,
            fix: "Check that the buffer is not empty before reading.",
        },
        ErrorInfo {
            code: "A24001",
            name: "Reentrant call detected",
            description: "A function marked as non-reentrant was called while already \
                          on the call stack.",
            example: r#"  @non_reentrant
      fn update(state: State)
      // update called recursively or reentrantly"#,
            fix: "Remove the reentrant call, or remove @non_reentrant if reentrancy \
                 is intentional.",
        },
        ErrorInfo {
            code: "A24002",
            name: "Callback registered in non-reentrant context",
            description: "A callback targeting a non-reentrant function was registered \
                          while already inside that function.",
            example: r#"  @non_reentrant
      fn process(data: Data)
      // registers callback that would re-enter process"#,
            fix: "Register the callback outside the non-reentrant function.",
        },
        ErrorInfo {
            code: "A24003",
            name: "Callback depth exceeded",
            description: "The callback chain depth exceeds the maximum allowed depth. \
                          Unbounded callback chains risk stack overflow.",
            example: r#"  // Chain: a -> b -> c -> ... -> exceeds max depth"#,
            fix: "Reduce callback nesting depth, or increase the max_depth limit.",
        },
        ErrorInfo {
            code: "A25001",
            name: "Deadline exceeded",
            description: "An operation's worst-case execution time exceeds the active \
                          deadline.",
            example: r#"  @deadline(response, 100)  // 100ms deadline
      fn handle() requires db_query()  // worst-case 500ms"#,
            fix: "Use a faster operation, increase the deadline, or move the slow \
                 operation outside the deadline scope.",
        },
        ErrorInfo {
            code: "A25002",
            name: "Nested deadline exceeds outer",
            description: "An inner deadline is longer than the enclosing outer deadline. \
                          The inner deadline can never be fully utilized.",
            example: r#"  @deadline(outer, 100)   // 100ms
      @deadline(inner, 200)  // 200ms > outer"#,
            fix: "Set the inner deadline to be at most the outer deadline.",
        },
        ErrorInfo {
            code: "A25003",
            name: "Unbounded operation in deadline",
            description: "An operation with no known worst-case time bound is used inside \
                          a deadline context.",
            example: r#"  @deadline(response, 50)
      fn handle() requires unknown_op()  // no time bound"#,
            fix: "Register a worst-case time bound with @worst_case, or move the \
                 operation outside the deadline scope.",
        },
        ErrorInfo {
            code: "A26001",
            name: "Binary field out of bounds",
            description: "A binary format field's offset plus size exceeds the buffer \
                          length.",
            example: r#"  // Buffer is 16 bytes, field at offset 12, size 8
      field header at offset 12, size 8  // 12 + 8 = 20 > 16"#,
            fix: "Adjust the field offset or size to fit within the buffer.",
        },
        ErrorInfo {
            code: "A26003",
            name: "Binary field missing endianness",
            description: "A multi-byte binary field has no endianness annotation. \
                          Without specifying byte order, interpretation is ambiguous.",
            example: r#"  field length: u32 at offset 0
      // big-endian or little-endian?"#,
            fix: "Add an endianness annotation (@big_endian or @little_endian) \
                 to multi-byte fields.",
        },
        ErrorInfo {
            code: "A26004",
            name: "Binary fields overlap",
            description: "Two binary format fields occupy overlapping byte ranges.",
            example: r#"  field a: u32 at offset 0, size 4  // [0, 4)
      field b: u16 at offset 2, size 2  // [2, 4) overlaps"#,
            fix: "Adjust field offsets so they do not overlap, or use a union type.",
        },
        ErrorInfo {
            code: "A27001",
            name: "Bit field out of bounds",
            description: "A bit field's offset plus width exceeds the container size \
                          in bits.",
            example: r#"  // 8-bit container
      field flags: bits(6..12)  // bit 12 > 8 bits"#,
            fix: "Adjust the bit offset or width to fit within the container.",
        },
        ErrorInfo {
            code: "A27002",
            name: "Bit field crosses byte boundary",
            description: "A bit field spans across a byte boundary without explicit \
                          permission.",
            example: r#"  field value: bits(6..10)  // crosses byte boundary"#,
            fix: "Split the field at the byte boundary, or add @cross_byte annotation.",
        },
        ErrorInfo {
            code: "A27003",
            name: "Bit width mismatch",
            description: "The sum of all bit field widths does not match the declared \
                          container size.",
            example: r#"  container: u16 { a: 4 bits, b: 8 bits }
      // 4 + 8 = 12 != 16"#,
            fix: "Add padding bits or adjust field widths to match the container size.",
        },
        ErrorInfo {
            code: "A28001",
            name: "Raw bytes used as string",
            description: "Raw bytes or data with unknown encoding were used as a string. \
                          The bytes may not be valid in any text encoding.",
            example: r#"  let data: Bytes = read_file("input.bin")
      print(data)  // data has unknown encoding"#,
            fix: "Validate the encoding first (e.g., validate_utf8(data)), or declare \
                 the variable with a known encoding.",
        },
        ErrorInfo {
            code: "A28002",
            name: "String encoding mismatch",
            description: "A string with one encoding was used in a context expecting a \
                          different encoding (e.g., UTF-16 data as UTF-8).",
            example: r#"  let s: @utf16 String = read_utf16("input.txt")
      let t: @utf8 String = s  // encoding mismatch"#,
            fix: "Explicitly convert between encodings using a transcoding function.",
        },
        ErrorInfo {
            code: "A28003",
            name: "String truncation splits code unit",
            description: "A string was truncated at a byte offset in the middle of a \
                          multi-byte code unit, producing an invalid string.",
            example: r#"  let s: @utf16 String = "hello"
      let t = s.truncate(3)  // splits a UTF-16 pair"#,
            fix: "Truncate at a code-unit-aligned byte offset.",
        },
        ErrorInfo {
            code: "A29001",
            name: "Data used before checksum verification",
            description: "A data region was used before its checksum was verified. \
                          Processing unverified data may lead to silent corruption.",
            example: r#"  let packet = receive()
      process(packet.data)  // checksum not verified
      verify_checksum(packet)"#,
            fix: "Verify the checksum before processing the data.",
        },
        ErrorInfo {
            code: "A29002",
            name: "Checksum algorithm mismatch",
            description: "The checksum was verified using a different algorithm than \
                          the one declared for the data region.",
            example: r#"  @checksum(crc32)
      let data = receive()
      verify_adler32(data)  // wrong algorithm"#,
            fix: "Use the same checksum algorithm as declared in the data region.",
        },
        ErrorInfo {
            code: "A29003",
            name: "Checksum range mismatch",
            description: "The checksum covers a different byte range than the data \
                          being protected.",
            example: r#"  @checksum(crc32, range: 0..100)
      verify(data, range: 0..80)  // only 80 of 100 bytes"#,
            fix: "Ensure the checksum range exactly covers the data being protected.",
        },
        ErrorInfo {
            code: "A30001",
            name: "Protocol invalid transition",
            description: "A protocol state transition was attempted that is not defined \
                          in the protocol state machine.",
            example: r#"  // In state "connected", no "login" transition defined
      send(login_request)  // invalid transition"#,
            fix: "Check the protocol state machine and use a valid transition.",
        },
        ErrorInfo {
            code: "A30002",
            name: "Protocol wrong state for message",
            description: "A message was sent in a protocol state where it is not allowed.",
            example: r#"  // Must be "authenticated" to send queries
      state: connected
      send(query)  // wrong state"#,
            fix: "Complete required protocol steps to reach the correct state first.",
        },
        ErrorInfo {
            code: "A30003",
            name: "Protocol missing required field",
            description: "A required field is missing from a protocol message.",
            example: r#"  // login_request needs "username" and "password"
      send(login_request { username: "alice" })  // missing password"#,
            fix: "Add the missing required field to the message.",
        },
        ErrorInfo {
            code: "A31001",
            name: "Undefined axiom reference",
            description: "An axiom references a symbol that is neither another declared \
                          axiom nor a known built-in symbol.",
            example: r#"  axiom Foo { references: [bar] }
      // bar is not declared"#,
            fix: "Declare the referenced symbol as an axiom or ensure it is a \
                 known built-in.",
        },
        ErrorInfo {
            code: "A31002",
            name: "Circular axiom dependency",
            description: "An axiom has a circular dependency chain, directly or \
                          indirectly referencing itself through other axioms.",
            example: r#"  axiom A { references: [B] }
      axiom B { references: [A] }  // circular"#,
            fix: "Break the cycle by removing one of the circular references.",
        },
        ErrorInfo {
            code: "A31003",
            name: "Unused axiom",
            description: "An axiom is declared but never referenced in any proof or \
                          verification context.",
            example: r#"  axiom Symmetry { ... }
      // Symmetry is never used"#,
            fix: "Use the axiom in a proof or remove it if not needed.",
        },
        ErrorInfo {
            code: "A32001",
            name: "Opaque function called without contract",
            description: "An opaque function is called but has no contract \
                          (requires/ensures), so its behavior cannot be verified.",
            example: r#"  opaque fn secret_fn(x: Int) -> Int
      // no contract attached"#,
            fix: "Add a contract (requires/ensures clauses) to the opaque function.",
        },
        ErrorInfo {
            code: "A32002",
            name: "Opaque body access without reveal",
            description: "The body of an opaque function is accessed without first \
                          using reveal to make it visible.",
            example: r#"  opaque fn hidden() -> Int { 42 }
      // accessing hidden's body without reveal"#,
            fix: "Use `reveal hidden` in a proof context before accessing the body.",
        },
        ErrorInfo {
            code: "A32003",
            name: "Reveal outside proof context",
            description: "A reveal statement is used outside of a proof context. \
                          Revealing opaque function bodies is only allowed during proofs.",
            example: r#"  reveal hidden  // used outside a proof block"#,
            fix: "Move the reveal statement inside a proof or verification block.",
        },
        ErrorInfo {
            code: "A33001",
            name: "Write without preceding WAL entry",
            description: "A data write was performed without first writing a \
                          corresponding write-ahead log entry, violating crash \
                          recovery guarantees.",
            example: r#"  begin_write("txn1")
      write_data("txn1")  // no WAL entry first"#,
            fix: "Write the WAL entry before performing the data write.",
        },
        ErrorInfo {
            code: "A33002",
            name: "Commit without fsync",
            description: "A transaction was committed without an fsync to ensure \
                          durability. Data may be lost on crash.",
            example: r#"  begin_write("txn1")
      commit("txn1")  // no fsync"#,
            fix: "Call fsync after writing data and before committing.",
        },
        ErrorInfo {
            code: "A33003",
            name: "Fsync before data write",
            description: "An fsync was issued before the corresponding data write \
                          completed, violating the expected write ordering.",
            example: r#"  begin_write("txn1")
      write_wal("txn1")
      fsync("txn1")  // data not yet written"#,
            fix: "Ensure data is written before calling fsync.",
        },
        ErrorInfo {
            code: "A34001",
            name: "Evict pinned page",
            description: "Attempted to evict a page from the page cache that is \
                          currently pinned. Pinned pages must be unpinned first.",
            example: r#"  cache.pin(1)
      cache.evict(1)  // page is pinned"#,
            fix: "Unpin the page before evicting it.",
        },
        ErrorInfo {
            code: "A34002",
            name: "Evict dirty page without flush",
            description: "Attempted to evict a dirty page without flushing it first. \
                          This would lose unflushed writes.",
            example: r#"  cache.mark_dirty(1)
      cache.evict(1)  // dirty, not flushed"#,
            fix: "Flush the dirty page to disk before evicting it.",
        },
        ErrorInfo {
            code: "A34003",
            name: "Page cache capacity exceeded",
            description: "The number of pages in the cache exceeds the declared \
                          maximum capacity.",
            example: r#"  // Cache capacity is 2, but 3 pages loaded"#,
            fix: "Evict pages to stay within the declared cache capacity.",
        },
        ErrorInfo {
            code: "A35001",
            name: "Write-write conflict",
            description: "Multiple uncommitted transactions have written to the same \
                          key, creating a write-write conflict under MVCC.",
            example: r#"  tx1.write("key1")
      tx2.write("key1")  // both uncommitted"#,
            fix: "Commit or abort one transaction before the other writes to the \
                 same key.",
        },
        ErrorInfo {
            code: "A35002",
            name: "Snapshot isolation violation",
            description: "A transaction reads uncommitted data written by another \
                          active transaction, violating snapshot isolation.",
            example: r#"  tx1.write("key")  // uncommitted
      tx2.read("key")   // sees uncommitted data"#,
            fix: "Ensure reads only see data committed before the reader's snapshot.",
        },
        ErrorInfo {
            code: "A35003",
            name: "Phantom read",
            description: "A transaction observes a committed version written by a \
                          later transaction, indicating a phantom read anomaly.",
            example: r#"  // tx1 starts, then tx2 writes and commits
      // tx1 sees tx2's committed data"#,
            fix: "Use serializable isolation or predicate locking to prevent \
                 phantom reads.",
        },
        ErrorInfo {
            code: "A36001",
            name: "Rollback to unknown savepoint",
            description: "Attempted to roll back to a savepoint that was never created.",
            example: r#"  rollback_to("sp1")  // "sp1" not created"#,
            fix: "Create the savepoint with create_savepoint before rolling back.",
        },
        ErrorInfo {
            code: "A36002",
            name: "Resource leak after rollback",
            description: "A resource acquired before a rollback was not released \
                          after the rollback, causing a resource leak.",
            example: r#"  create_savepoint("sp1")
      acquire_resource("lock")
      rollback_to("sp1")  // "lock" not released"#,
            fix: "Release all acquired resources after performing a rollback.",
        },
        ErrorInfo {
            code: "A36003",
            name: "Duplicate savepoint name",
            description: "A savepoint was created with a name that already exists.",
            example: r#"  create_savepoint("sp1")
      create_savepoint("sp1")  // duplicate"#,
            fix: "Use unique names for each savepoint.",
        },
        ErrorInfo {
            code: "A37001",
            name: "Monotonicity violation",
            description: "A monotonic variable was updated in a direction that violates \
                          its declared constraint (e.g., decreasing an increasing variable).",
            example: r#"  monotonic increasing seq = 10
      seq = 5  // decreases"#,
            fix: "Ensure updates to monotonic variables respect the declared direction.",
        },
        ErrorInfo {
            code: "A37002",
            name: "Illegal monotonic variable reset",
            description: "Attempted to reset a monotonic variable. Monotonic variables \
                          cannot be reset once declared.",
            example: r#"  monotonic increasing seq = 10
      reset(seq)  // illegal"#,
            fix: "Remove the reset. Monotonic variables must only move in their \
                 declared direction.",
        },
        ErrorInfo {
            code: "A37003",
            name: "Undeclared monotonic variable",
            description: "Attempted to access a variable as monotonic, but it was never \
                          declared with a monotonicity constraint.",
            example: r#"  update("counter", 5)  // "counter" not monotonic"#,
            fix: "Declare the variable with a monotonicity direction before accessing it.",
        },
        ErrorInfo {
            code: "A38001",
            name: "Unhandled storage failure mode",
            description: "A declared storage failure mode has no handler.",
            example: r#"  declare_failure(PartialWrite)
      // no handler for PartialWrite"#,
            fix: "Add a handler for every declared storage failure mode.",
        },
        ErrorInfo {
            code: "A38002",
            name: "Handler for undeclared failure mode",
            description: "A handler exists for a failure mode that was never declared, \
                          indicating dead code or a typo.",
            example: r#"  mark_handled("nonexistent")  // not declared"#,
            fix: "Declare the failure mode or remove the spurious handler.",
        },
        ErrorInfo {
            code: "A38003",
            name: "Critical failure mode unhandled",
            description: "A critical storage failure mode (e.g., partial write, torn \
                          page) has no handler. Critical failures must always be handled.",
            example: r#"  declare_failure(TornPage)
      // no handler for critical TornPage"#,
            fix: "Add a handler for the critical failure mode.",
        },
        ErrorInfo {
            code: "A42001",
            name: "Numerical precision loss",
            description: "An operation produces a result with fewer bits than the \
                          variable requires, causing precision loss.",
            example: r#"  precision x: 64-bit
      let y: f32 = x  // 64-bit narrowed to 32-bit"#,
            fix: "Use an operation or type with sufficient bit width.",
        },
        ErrorInfo {
            code: "A42002",
            name: "ULP bound violation",
            description: "The unit of least precision (ULP) of a computation exceeds \
                          the declared minimum ULP bound.",
            example: r#"  precision x: ulp <= 1e-15
      // actual ULP is 1e-10"#,
            fix: "Use a higher-precision algorithm or widen the ULP bound.",
        },
        ErrorInfo {
            code: "A42003",
            name: "Catastrophic cancellation risk",
            description: "Subtraction of nearly equal operands detected, which can \
                          cause catastrophic loss of significant digits.",
            example: r#"  let result = a - b  // a/b ratio ~0.99999"#,
            fix: "Reformulate the expression to avoid subtracting nearly equal values.",
        },
        ErrorInfo {
            code: "A43001",
            name: "Incomplete table verification",
            description: "A precomputed lookup table does not have all its entries \
                          verified. Unverified entries may contain incorrect values.",
            example: r#"  table crc32: 256 entries
      // only 100 verified"#,
            fix: "Verify all entries against the generator function.",
        },
        ErrorInfo {
            code: "A43002",
            name: "Table missing generator function",
            description: "A precomputed table is declared without a generator function, \
                          so its entries cannot be verified.",
            example: r#"  table lookup: 16 entries
      // no generator function"#,
            fix: "Specify a generator function for the precomputed table.",
        },
        ErrorInfo {
            code: "A43003",
            name: "Zero-size table",
            description: "A precomputed table is declared with zero entries.",
            example: r#"  table empty: 0 entries"#,
            fix: "Set the table size to the correct number of entries, or remove it.",
        },
        ErrorInfo {
            code: "A44001",
            name: "Missing platform implementation",
            description: "A platform abstraction is missing an implementation for one \
                          of the declared target platforms.",
            example: r#"  platforms: [linux, windows]
      abstraction fs_ops: [linux]  // missing windows"#,
            fix: "Add an implementation for each target platform.",
        },
        ErrorInfo {
            code: "A44002",
            name: "Direct platform reference",
            description: "Code directly references a platform name instead of using a \
                          platform abstraction, reducing portability.",
            example: r#"  use_platform("linux")  // direct reference"#,
            fix: "Use a platform abstraction instead of referencing the platform \
                 directly.",
        },
        ErrorInfo {
            code: "A44003",
            name: "Unknown platform in abstraction",
            description: "A platform abstraction references a platform not in the \
                          declared set of target platforms.",
            example: r#"  platforms: [linux]
      abstraction net: [linux, freebsd]  // freebsd unknown"#,
            fix: "Add the platform to the declared list or remove it from the \
                 abstraction.",
        },
        ErrorInfo {
            code: "A45001",
            name: "Unused feature flag",
            description: "A feature flag is declared but never referenced anywhere.",
            example: r#"  feature experimental = false
      // never used"#,
            fix: "Use the feature flag in conditional code or remove the declaration.",
        },
        ErrorInfo {
            code: "A45002",
            name: "Conflicting feature flags",
            description: "Two feature flags that are declared as conflicting are both \
                          enabled at the same time.",
            example: r#"  feature debug = true, conflicts: [release]
      feature release = true  // both enabled"#,
            fix: "Disable one of the conflicting feature flags.",
        },
        ErrorInfo {
            code: "A45003",
            name: "Undeclared feature flag",
            description: "Code references a feature flag that was never declared.",
            example: r#"  if feature("unknown") { ... }  // not declared"#,
            fix: "Declare the feature flag before referencing it.",
        },
        ErrorInfo {
            code: "A46001",
            name: "Resource limit exceeded",
            description: "The measured usage of a resource exceeds its declared \
                          maximum limit.",
            example: r#"  limit memory: 1024 bytes
      // actual usage is 2000 bytes"#,
            fix: "Reduce resource usage or increase the declared limit.",
        },
        ErrorInfo {
            code: "A46002",
            name: "Unbounded resource usage",
            description: "A resource is used without any declared limit, so its \
                          consumption is unconstrained.",
            example: r#"  use_resource("cpu")  // no limit declared"#,
            fix: "Declare a resource limit before using the resource.",
        },
        ErrorInfo {
            code: "A46003",
            name: "Resource near limit",
            description: "A resource's usage has reached 90% or more of its declared \
                          limit.",
            example: r#"  limit fds: 100
      // usage at 95 (95%)"#,
            fix: "Reduce resource usage or increase the limit to add headroom.",
        },
        ErrorInfo {
            code: "A47001",
            name: "Unsafe block without safety proof",
            description: "An unsafe escape block has no attached safety proof. Every \
                          unsafe block must justify its safety.",
            example: r#"  unsafe raw_ptr { ... }
      // no safety proof attached"#,
            fix: "Attach a safety proof to the unsafe block.",
        },
        ErrorInfo {
            code: "A47002",
            name: "Undischarged safety obligation",
            description: "A proof obligation declared in an unsafe block has not been \
                          discharged. All obligations must be proven.",
            example: r#"  unsafe cast {
      obligations: [valid_repr, aligned]
  }  // valid_repr not discharged"#,
            fix: "Discharge all proof obligations by providing proofs for each.",
        },
        ErrorInfo {
            code: "A47003",
            name: "Empty proof obligations",
            description: "An unsafe block declares no proof obligations. Every unsafe \
                          block should specify what must be proven for safety.",
            example: r#"  unsafe noop { obligations: [] }  // empty"#,
            fix: "Add proof obligations that justify why the unsafe block is safe.",
        },
        ErrorInfo {
            code: "A48001",
            name: "Complexity bound exceeded",
            description: "A function's measured complexity class exceeds its declared \
                          bound (e.g., declared O(n) but measured O(n^2)).",
            example: r#"  bound sort: O(n log n)
      // measured as O(n^2)"#,
            fix: "Optimize the implementation to meet the declared complexity bound.",
        },
        ErrorInfo {
            code: "A48002",
            name: "Unverified complexity bound",
            description: "A complexity bound is declared but no measurement has \
                          verified it.",
            example: r#"  bound search: O(n)
      // never measured"#,
            fix: "Measure the function's complexity to verify the declared bound.",
        },
        ErrorInfo {
            code: "A48003",
            name: "Exponential complexity warning",
            description: "A function declares an exponential complexity bound. \
                          Exponential algorithms may be impractical for large inputs.",
            example: r#"  bound solve: O(2^n)  // exponential"#,
            fix: "Consider whether an exponential bound is acceptable or if a \
                 better algorithm exists.",
        },
        ErrorInfo {
            code: "A49001",
            name: "Unverified behavioral equivalence",
            description: "A behavioral equivalence between two implementations is \
                          declared but has not been verified.",
            example: r#"  equivalence sort_eq: impl_a ~ impl_b
      // not verified"#,
            fix: "Verify the equivalence using the appropriate proof technique.",
        },
        ErrorInfo {
            code: "A49002",
            name: "Trivial self-equivalence",
            description: "A behavioral equivalence compares an implementation to itself, \
                          which is trivially true and likely a mistake.",
            example: r#"  equivalence eq1: sort_a ~ sort_a  // same impl"#,
            fix: "Specify two different implementations to compare.",
        },
        ErrorInfo {
            code: "A49003",
            name: "Equivalence missing contract reference",
            description: "A behavioral equivalence declaration has no contract reference \
                          to specify what behavior should be equivalent.",
            example: r#"  equivalence eq1: a ~ b
      // no contract reference"#,
            fix: "Add a contract reference that defines the expected behavior.",
        },
        ErrorInfo {
            code: "A50001",
            name: "Incomplete refinement pass",
            description: "A multi-pass refinement has obligations that have not all \
                          been discharged.",
            example: r#"  refinement p1: L0 -> L1, obligations: 5
      // only 3 discharged"#,
            fix: "Discharge all remaining obligations in the refinement pass.",
        },
        ErrorInfo {
            code: "A50002",
            name: "Refinement chain gap",
            description: "Consecutive refinement passes have mismatched levels: one \
                          pass ends at a level different from where the next begins.",
            example: r#"  pass p1: L0 -> L1
      pass p2: L2 -> L3  // gap: L1 != L2"#,
            fix: "Ensure each pass starts at the level where the previous one ended.",
        },
        ErrorInfo {
            code: "A50003",
            name: "Trivial refinement pass",
            description: "A refinement pass declares zero proof obligations.",
            example: r#"  pass p1: spec -> design, obligations: 0"#,
            fix: "Add proof obligations or remove the trivial pass.",
        },
        ErrorInfo {
            code: "A51001",
            name: "Precondition strengthened",
            description: "A newer contract version has more requires clauses than \
                          the previous version, breaking backward compatibility.",
            example: r#"  contract SafeDiv v1 { requires { x != 0 } }
      contract SafeDiv v2 { requires { x != 0 && x > 0 } }"#,
            fix: "Keep preconditions the same or weaker in newer versions.",
        },
        ErrorInfo {
            code: "A51002",
            name: "Postcondition weakened",
            description: "A newer contract version has fewer ensures clauses, weakening \
                          postconditions and breaking guarantees.",
            example: r#"  contract SafeDiv v1 { ensures { result >= 0 && result < x } }
      contract SafeDiv v2 { ensures { result >= 0 } }"#,
            fix: "Keep postconditions the same or stronger in newer versions.",
        },
        ErrorInfo {
            code: "A51003",
            name: "Contract version gap",
            description: "A contract has non-consecutive version numbers, indicating \
                          a missing intermediate version.",
            example: r#"  contract SafeDiv v1 { ... }
      contract SafeDiv v5 { ... }  // gap: v2-v4 missing"#,
            fix: "Use consecutive version numbers for contract evolution.",
        },
        ErrorInfo {
            code: "A52001",
            name: "Invariant suspension violation",
            description: "A scoped invariant is either suspended when already suspended, \
                          referenced while suspended, or still suspended at scope exit.",
            example: r#"  suspend("balance_positive")
      // scope exits without restore"#,
            fix: "Ensure each suspended invariant is restored before scope ends.",
        },
        ErrorInfo {
            code: "A52002",
            name: "Suspend undeclared invariant",
            description: "Attempted to suspend an invariant that was never declared.",
            example: r#"  suspend("unknown_inv")  // not declared"#,
            fix: "Declare the invariant before suspending it.",
        },
        ErrorInfo {
            code: "A52003",
            name: "Restore non-suspended invariant",
            description: "Attempted to restore an invariant that is not currently \
                          suspended.",
            example: r#"  restore("inv1")  // not suspended"#,
            fix: "Only restore invariants that have been previously suspended.",
        },
        ErrorInfo {
            code: "A53001",
            name: "CRUD operation missing auth policy",
            description: "A CRUD operation that requires authentication has no \
                          corresponding authorization policy.",
            example: r#"  crud create_user { requires_auth: true }
      // no auth policy defined"#,
            fix: "Define an authorization policy for the CRUD operation.",
        },
        ErrorInfo {
            code: "A53002",
            name: "Delete without authentication",
            description: "A delete operation does not require authentication. Delete \
                          operations should always be protected.",
            example: r#"  crud delete_item { requires_auth: false }"#,
            fix: "Set requires_auth: true on the delete operation.",
        },
        ErrorInfo {
            code: "A54001",
            name: "Ghost function with non-pure effects",
            description: "A ghost function has non-pure effects. Ghost functions must \
                          be pure since they are erased at runtime. Also reported for \
                          contract composition referencing a non-existent parent.",
            example: r#"  ghost fn helper() effects { io }
      // ghost functions must be pure"#,
            fix: "Remove non-pure effects from ghost functions, or declare the \
                 parent contract for composition.",
        },
        ErrorInfo {
            code: "A54002",
            name: "Circular contract extends chain",
            description: "Contract composition has a circular extends chain where \
                          contracts form a dependency cycle.",
            example: r#"  contract A extends B { ... }
      contract B extends A { ... }  // circular"#,
            fix: "Break the circular extends chain.",
        },
        ErrorInfo {
            code: "A54003",
            name: "Diamond inheritance in contracts",
            description: "A contract inherits from the same ancestor through multiple \
                          paths, creating diamond inheritance ambiguity.",
            example: r#"  contract Base { ... }
      contract Left extends Base { ... }
      contract Right extends Base { ... }
      contract D extends Left, Right { ... }  // diamond"#,
            fix: "Restructure the contract hierarchy to avoid diamond inheritance.",
        },
        ErrorInfo {
            code: "A55001",
            name: "Lemma with non-pure effects / empty library",
            description: "A lemma function declares non-pure effects, or a contract \
                          library has no exported contracts. Lemmas must be pure; \
                          libraries must export at least one contract.",
            example: r#"  lemma fn helper() effects { io }
      // lemma functions must be pure"#,
            fix: "Remove non-pure effects from lemma functions, or add exports.",
        },
        ErrorInfo {
            code: "A55002",
            name: "Library self-dependency",
            description: "A contract library declares a dependency on itself.",
            example: r#"  library math { depends: [math] }  // self-dep"#,
            fix: "Remove the self-dependency.",
        },
        ErrorInfo {
            code: "A55003",
            name: "Duplicate library name",
            description: "Multiple contract libraries are declared with the same name.",
            example: r#"  library math v1.0
      library math v2.0  // duplicate name"#,
            fix: "Use unique names for each contract library.",
        },
        // -- A64001: Non-propagatable error caught --
        ErrorInfo {
            code: "A64001",
            name: "Non-propagatable error code caught",
            description: "An error code marked as must-propagate is being caught \
                          instead of propagated to the caller.",
            example: r#"  fn process() {
      catch(critical_error)  // must propagate, not catch
  }"#,
            fix: "Propagate the error to the caller instead of catching it.",
        },
        // -- A99999: Test sentinel --
        ErrorInfo {
            code: "A99999",
            name: "Reserved test sentinel",
            description: "Reserved error code used for testing the error explanation \
                          system. This code should never appear in real compiler output.",
            example: r#"  // This code is used internally for testing only"#,
            fix: "If you see this error in practice, report it as a compiler bug.",
        },
    ]
}

/// Lazily-initialized catalog as a flat list (for iteration/enumeration).
static CATALOG: LazyLock<Vec<ErrorInfo>> = LazyLock::new(error_catalog);

/// Lazily-initialized lookup table for O(1) access by error code.
static CATALOG_MAP: LazyLock<HashMap<&'static str, usize>> = LazyLock::new(|| {
    let catalog = &*CATALOG;
    let mut map = HashMap::with_capacity(catalog.len());
    for (i, info) in catalog.iter().enumerate() {
        map.insert(info.code, i);
    }
    map
});

/// Look up an error code in the catalog (O(1) via HashMap).
///
/// Returns `None` if the code is not recognized.
pub fn explain(code: &str) -> Option<&'static ErrorInfo> {
    CATALOG_MAP.get(code).map(|&idx| &CATALOG[idx])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn no_duplicate_error_codes() {
        let catalog = error_catalog();
        let mut seen = HashSet::new();
        for info in &catalog {
            assert!(
                seen.insert(info.code),
                "duplicate error code: {}",
                info.code
            );
        }
    }

    #[test]
    fn all_codes_follow_format() {
        let catalog = error_catalog();
        for info in &catalog {
            assert!(
                info.code.starts_with('A'),
                "code {} does not start with 'A'",
                info.code
            );
            assert!(
                info.code.len() == 6,
                "code {} is not 6 characters (Axxxxx)",
                info.code
            );
            assert!(
                info.code[1..].chars().all(|c| c.is_ascii_digit()),
                "code {} has non-digit suffix",
                info.code
            );
        }
    }

    #[test]
    fn explain_returns_known_code() {
        let info = explain("A01001").expect("A01001 should exist");
        assert_eq!(info.code, "A01001");
        assert!(!info.name.is_empty());
        assert!(!info.description.is_empty());
    }

    #[test]
    fn explain_returns_none_for_unknown() {
        assert!(explain("A00000").is_none());
        assert!(explain("BOGUS").is_none());
    }

    #[test]
    fn catalog_has_entries() {
        let catalog = error_catalog();
        assert!(
            catalog.len() >= 50,
            "catalog has only {} entries, expected 50+",
            catalog.len()
        );
    }

    #[test]
    fn every_entry_has_example_and_fix() {
        let catalog = error_catalog();
        for info in &catalog {
            assert!(!info.example.is_empty(), "{} has empty example", info.code);
            assert!(!info.fix.is_empty(), "{} has empty fix", info.code);
        }
    }
}
