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
                          operations, wrong return types, and assignment type conflicts.",
            example: r#"  contract Add {
      requires: x > "hello"   // comparing Int with String
  }

  fn double(x: Int) -> Bool {
      x * 2                    // returns Int, expected Bool
  }"#,
            fix: "Ensure both sides of an operation have compatible types. Check that \
                 function return types match their declared output type. Use explicit \
                 conversions when needed (e.g., 'as Int').",
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
            code: "A03003",
            name: "Wrong number of type arguments",
            description: "A generic type was instantiated with the wrong number of type \
                          parameters. For example, List takes 1 type argument, Map takes \
                          2, and non-generic types take 0.",
            example: r#"  type Pair = List<Int, Bool>   // List takes 1 type arg, got 2

  type Bad = Option             // Option takes 1 type arg, got 0"#,
            fix: "Check how many type parameters the generic type expects. Common ones: \
                 List<T> (1), Map<K, V> (2), Set<T> (1), Option<T> (1), \
                 Result<T, E> (2).",
        },
        ErrorInfo {
            code: "A03004",
            name: "Unknown field",
            description: "A field access (expr.field) refers to a field that does not \
                          exist on the type of the expression. The type either has no \
                          fields, or the field name is misspelled.",
            example: r#"  type Point { x: Int, y: Int }

  contract CheckPoint {
      requires: p.z > 0   // Point has no field 'z'
  }"#,
            fix: "Check the type definition for available field names. Fix the spelling \
                 or use a valid field. If the field should exist, add it to the type \
                 definition.",
        },
        ErrorInfo {
            code: "A03005",
            name: "Not callable",
            description: "An expression was used in a function call position, but its \
                          type is not a function or callable. Only functions, extern \
                          functions, and service operations can be called.",
            example: r#"  type Foo { x: Int }

  contract Bad {
      requires: Foo(42) > 0   // Foo is a type, not a function
  }"#,
            fix: "Ensure you are calling a function, not a type or variable. If you \
                 meant to construct a value, use struct literal syntax. If you \
                 meant to call a method, check that the method exists on the type.",
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
            name: "Unknown effect name",
            description: "An effect name in an 'effects' clause does not match any known \
                          effect. Built-in effects include: io, io.read, io.write, \
                          database, database.read, database.write, network, crypto, pure.",
            example: r#"  fn bad() -> Unit
      effects: teleport   // 'teleport' is not a known effect -> A07003"#,
            fix: "Use a valid effect name from the built-in effect hierarchy. Check \
                 the documentation for the complete list of effects.",
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
        // -- A02002: Undefined type (spec Section 7.2) --
        ErrorInfo {
            code: "A02002",
            name: "Undefined type",
            description: "A type name was used but not declared in scope. The \
                          compiler could not find a type definition matching \
                          the name.",
            example: r#"  contract Foo {
      input { x: Widget }  // 'Widget' is not defined
      requires { x > 0 }
  }"#,
            fix: "Define the type, import it from the module where it is declared, \
                 or fix a typo in the type name.",
        },
        // -- A02004: Ambiguous import (spec Section 7.2) --
        ErrorInfo {
            code: "A02004",
            name: "Ambiguous import",
            description: "A name could refer to multiple definitions because of \
                          overlapping imports. The compiler cannot determine which \
                          definition was intended.",
            example: r#"  import a { Foo }
  import b { Foo }   // both modules export 'Foo'

  contract Bar {
      requires: Foo > 0   // ambiguous: a.Foo or b.Foo?
  }"#,
            fix: "Use a qualified name (module.Foo) to disambiguate, or use an \
                 alias on one of the imports: import b { Foo as BFoo }.",
        },
        // -- A02009: Visibility violation (moved from A02004 to avoid spec conflict) --
        ErrorInfo {
            code: "A02009",
            name: "Visibility violation",
            description: "An attempt was made to access a field or member that \
                          is not public. Non-pub fields are only accessible within \
                          the module that defines the type.",
            example: r#"  type Wallet {
      balance: Int   // private (no pub)
  }

  contract Check {
      requires: w.balance > 0   // A02009: balance is private
  }"#,
            fix: "Mark the field as 'pub' in the type definition if external \
                 access is intended, or access it through a public getter method.",
        },
        // -- A05005: Ghost/linear interaction --
        ErrorInfo {
            code: "A05005",
            name: "Ghost code modifies linear variable",
            description: "A ghost block attempted to consume or modify a linear \
                          variable. Ghost code is erased at runtime, so it must not \
                          affect the usage count of linear variables.",
            example: r#"  fn bad(x: Int :_1) -> Int
      effects: pure
  {
      ghost { let _ = x; }   // ghost uses linear var -> A05005
      x
  }"#,
            fix: "Remove the linear variable reference from the ghost block. \
                 Ghost code should only read or reference non-linear variables.",
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
        // -- A07004: Pure function has side effects --
        ErrorInfo {
            code: "A07004",
            name: "Pure function has side effects",
            description: "A function declared as 'effects: pure' performs an \
                          operation that has side effects. Pure functions may not \
                          perform I/O, mutate shared state, or call effectful functions.",
            example: r#"  fn pure_fn(x: Int) -> Int
      effects: pure
  {
      println(x)   // I/O in pure function -> A07004
      x
  }"#,
            fix: "Remove the effectful operation from the pure function, or \
                 change the effects declaration to include the required effects.",
        },
        // -- A07005: Effect row mismatch --
        ErrorInfo {
            code: "A07005",
            name: "Effect row mismatch",
            description: "A function's declared effect row does not match the \
                          effect rows of higher-order function parameters or \
                          closures passed to it.",
            example: r#"  fn map(f: fn(Int) -> Int effects: pure, xs: List<Int>)
      effects: pure
  // calling with an effectful closure -> A07005"#,
            fix: "Ensure the function or closure passed as an argument has \
                 effects that are a subset of the expected effects.",
        },
        // -- A08002-A08005: Information flow --
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
        // -- A09001-A09004: Totality / termination --
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
        // -- A01003-A01005: Parser errors --
        ErrorInfo {
            code: "A01003",
            name: "Invalid numeric literal",
            description: "The lexer encountered a malformed number. Numbers must be \
                          valid integer or floating-point literals.",
            example: r#"  contract Foo {
      requires { 0x_invalid > 0 }   // malformed hex literal
  }"#,
            fix: "Fix the numeric literal. Valid formats: 42, 3.14, 0xFF, 0b1010, 0o77.",
        },
        ErrorInfo {
            code: "A01004",
            name: "Reserved keyword used as identifier",
            description: "An identifier uses a name that is a reserved keyword in Assura. \
                          Keywords like 'contract', 'requires', 'ensures' cannot be used \
                          as variable or type names.",
            example: r#"  contract contract {   // 'contract' is a keyword
      requires { true }
  }"#,
            fix: "Choose a different name that is not a reserved keyword.",
        },
        ErrorInfo {
            code: "A01005",
            name: "Mismatched braces",
            description: "The parser found unbalanced braces, brackets, or parentheses. \
                          Every opening delimiter must have a matching closing delimiter.",
            example: r#"  contract Foo {
      requires { x > 0
  }   // missing closing brace for requires"#,
            fix: "Add the missing closing delimiter or remove the extra opening one. \
                 Use an editor with bracket matching to find the mismatch.",
        },
        // -- A04001-A04007: Verification errors --
        ErrorInfo {
            code: "A04001",
            name: "Precondition may not hold",
            description: "The SMT solver found that a requires clause may be violated \
                          at a call site. The caller does not guarantee the precondition.",
            example: r#"  contract Div {
      input(a: Int, b: Int)
      requires { b != 0 }
  }
  // calling Div(x, y) without proving y != 0 -> A04001"#,
            fix: "Add a guard or assertion at the call site to ensure the precondition \
                 holds. Check the requires clause to see what must be true.",
        },
        ErrorInfo {
            code: "A04002",
            name: "Postcondition may not hold",
            description: "The SMT solver could not prove that the ensures clause \
                          holds for all inputs satisfying the preconditions.",
            example: r#"  contract AlwaysPositive {
      input(x: Int)
      output(result: Int)
      ensures { result > 0 }
  }
  // If implementation can return 0 -> A04002"#,
            fix: "Either strengthen the requires clause to restrict inputs, or \
                 weaken the ensures clause, or fix the implementation.",
        },
        ErrorInfo {
            code: "A04003",
            name: "Refinement subtype check failed",
            description: "A value of refinement type {v: T | P} was used where a \
                          different refinement type {v: T | Q} was expected, and the \
                          solver could not prove that P implies Q.",
            example: r#"  type Positive = { v: Int | v > 0 }
  type BigPositive = { v: Int | v > 100 }
  // assigning Positive to BigPositive fails: v > 0 does not imply v > 100"#,
            fix: "Strengthen the source refinement predicate or weaken the target, \
                 or add a runtime check.",
        },
        ErrorInfo {
            code: "A04004",
            name: "Division by zero possible",
            description: "The verifier found that a division or modulo operation may \
                          have a zero divisor at runtime.",
            example: r#"  contract Unsafe {
      input(x: Int, y: Int)
      ensures { x / y > 0 }   // y could be 0
  }"#,
            fix: "Add a requires clause: requires { y != 0 }, or guard the division \
                 with a conditional check.",
        },
        ErrorInfo {
            code: "A04005",
            name: "Index out of bounds possible",
            description: "The verifier found that an array or collection index may \
                          exceed the valid range at runtime.",
            example: r#"  contract Unsafe {
      input(data: List<Int>, i: Nat)
      ensures { data[i] >= 0 }   // i may be >= length(data)
  }"#,
            fix: "Add a requires clause: requires { i < length(data) }, or use a \
                 bounds-checked access method.",
        },
        ErrorInfo {
            code: "A04006",
            name: "Arithmetic overflow possible",
            description: "The verifier found that an arithmetic operation may produce \
                          a result that exceeds the bounds of its type.",
            example: r#"  contract Unsafe {
      input(a: Nat, b: Nat)
      output(result: Nat)
      ensures { result == a + b }   // may overflow
  }"#,
            fix: "Add a requires clause bounding the inputs, use a wider type, or \
                 use checked arithmetic operations.",
        },
        ErrorInfo {
            code: "A04007",
            name: "Refinement timeout",
            description: "The SMT solver timed out while checking a refinement type \
                          constraint. The property may be too complex for automated \
                          verification within the configured timeout.",
            example: r#"  // Complex nested quantifiers or non-linear arithmetic
  // may cause the solver to time out"#,
            fix: "Simplify the refinement predicate, add intermediate lemmas, \
                 or increase the solver timeout in assura.toml.",
        },
        // -- A04008-A04009: Verification clause quality warnings --
        ErrorInfo {
            code: "A04008",
            name: "Ensures references unconstrained output",
            description: "An ensures clause references `result` or an output parameter. \
                          These are free variables in SMT; the solver can assign them \
                          any value, causing spurious counterexamples.",
            example: r#"  contract safe_add(x: Int, y: Int) -> Int
    requires { x >= 0 }
    ensures  { result >= 0 }    // result is unconstrained"#,
            fix: "Write ensures clauses that reference only input variables: \
                 ensures { x + y >= 0 }. For extern functions returning Bytes/String, \
                 result.length() >= 0 is safe (background axiom).",
        },
        ErrorInfo {
            code: "A04009",
            name: "Feature_max constant in verification clause",
            description: "A feature_max constant is used in a requires, ensures, or \
                          invariant clause. The SMT encoder treats feature_max constants \
                          as unconstrained integer variables, not their defined values.",
            example: r#"  feature_max HEADER_SIZE: Nat = 3
  contract check(data: Bytes)
    requires { data.length() >= HEADER_SIZE }  // SMT sees 0, not 3"#,
            fix: "Inline the value directly: requires { data.length() >= 3 }.",
        },
        // -- A06005: Typestate --
        ErrorInfo {
            code: "A06005",
            name: "Missing transition guard",
            description: "A typestate transition is missing a required guard predicate. \
                          The transition should have a condition that must hold before \
                          the state change is allowed.",
            example: r#"  service Account {
      states: [Active, Frozen]
      operation freeze(account) {
          // missing: requires { balance >= 0 }
      }
  }"#,
            fix: "Add a requires clause with the guard predicate for the transition.",
        },
        // -- A13004: Integer overflow --
        ErrorInfo {
            code: "A13004",
            name: "Integer overflow possible",
            description: "An arithmetic operation may produce a result that exceeds \
                          the representable range of the target integer type.",
            example: r#"  contract Multiply {
      input(a: Int, b: Int)
      output(result: Int)
      ensures { result == a * b }   // a*b may overflow
  }"#,
            fix: "Add bounds on the inputs via requires clauses, use a wider type, \
                 or use checked arithmetic.",
        },
        // -- A29004-A29005: Protocol errors --
        ErrorInfo {
            code: "A29004",
            name: "Protocol violation: step out of order",
            description: "A protocol step was called in an order that violates the \
                          declared protocol grammar.",
            example: r#"  // Protocol declares: init -> process -> finalize
  // Calling process before init -> A29004"#,
            fix: "Follow the declared protocol sequence. Check the protocol grammar \
                 for the correct order of operations.",
        },
        ErrorInfo {
            code: "A29005",
            name: "Reader may see partial write",
            description: "A non-atomic multi-field update may allow a concurrent reader \
                          to observe an inconsistent intermediate state.",
            example: r#"  // Updating struct.x and struct.y non-atomically
  // Reader may see new x with old y -> A29005"#,
            fix: "Use atomic operations or a lock to ensure multi-field updates \
                 are observed as a single unit.",
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
        // -- A31004-A31005: Binary format --
        ErrorInfo {
            code: "A31004",
            name: "Format exceeds expected size",
            description: "A binary format header or structure is larger than the \
                          specification declares.",
            example: r#"  // Header declared as 16 bytes but parsed data is 20 bytes"#,
            fix: "Check the format specification for correct sizes. Ensure serialization \
                 matches the declared format.",
        },
        ErrorInfo {
            code: "A31005",
            name: "Reserved space violated",
            description: "A reserved field in a binary format contains a non-zero value \
                          when the spec requires it to be zero.",
            example: r#"  // Reserved bytes at offset 12-15 must be 0x00
  // Found non-zero values -> A31005"#,
            fix: "Ensure reserved fields are zeroed out. Check the format specification.",
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
        // -- A32004: Crash recovery --
        ErrorInfo {
            code: "A32004",
            name: "Recovery procedure has side effects beyond repair",
            description: "A crash recovery procedure does more than restoring consistency. \
                          Recovery should only repair, not modify application state.",
            example: r#"  // Recovery function modifies user data beyond undoing
  // the interrupted transaction -> A32004"#,
            fix: "Limit recovery to consistency restoration. Move application-level \
                 changes to a separate post-recovery step.",
        },
        // -- A34004-A34005: Callback --
        ErrorInfo {
            code: "A34004",
            name: "Callback may fail but is marked infallible",
            description: "A callback declared as infallible contains an error path \
                          that could fail at runtime.",
            example: r#"  // Callback marked as infallible but has a path
  // that returns an error -> A34004"#,
            fix: "Either handle the error inside the callback or mark it as fallible.",
        },
        ErrorInfo {
            code: "A34005",
            name: "Callback invariant not satisfiable",
            description: "The callback's declared invariant (e.g., transitivity, \
                          antisymmetry for comparison callbacks) cannot be proven.",
            example: r#"  // Comparison callback does not satisfy antisymmetry:
  // f(a,b) = true and f(b,a) = true for some a,b -> A34005"#,
            fix: "Fix the callback implementation to satisfy the declared invariant.",
        },
        // -- A35004-A35005: Determinism --
        ErrorInfo {
            code: "A35004",
            name: "Pointer-derived value in deterministic context",
            description: "An address or pointer-derived value is used in a computation \
                          that is declared deterministic. Pointer addresses vary between \
                          runs.",
            example: r#"  // Using ptr as hash key in deterministic function -> A35004"#,
            fix: "Replace pointer-derived values with content-based values. Use \
                 a stable identifier instead of a memory address.",
        },
        ErrorInfo {
            code: "A35005",
            name: "Callee is not deterministic",
            description: "A function declared as deterministic calls another function \
                          that is not deterministic.",
            example: r#"  // Deterministic function calls random() -> A35005"#,
            fix: "Either mark the caller as non-deterministic or avoid calling \
                 non-deterministic functions.",
        },
        // -- A36004: Atomic error handling --
        ErrorInfo {
            code: "A36004",
            name: "Nested atomic function swallows error",
            description: "An inner atomic function's failure is caught without \
                          propagating to the outer atomic scope.",
            example: r#"  // Nested atomic { try { inner_atomic() } catch {} }
  // Inner failure silently swallowed -> A36004"#,
            fix: "Propagate errors from nested atomic operations to the outer scope.",
        },
        // -- A37004-A37005: FFI --
        ErrorInfo {
            code: "A37004",
            name: "FFI null pointer not checked",
            description: "A nullable pointer returned from an FFI call is used without \
                          a null check.",
            example: r#"  extern fn get_data() -> *const u8
  // Using return value without null check -> A37004"#,
            fix: "Check for null before using the pointer: if ptr != null { ... }.",
        },
        ErrorInfo {
            code: "A37005",
            name: "FFI thread safety violation",
            description: "An FFI function is called from a threading context that \
                          violates its thread safety requirements.",
            example: r#"  // FFI function marked as thread-unsafe called from
  // a multi-threaded context -> A37005"#,
            fix: "Ensure FFI calls respect thread safety requirements. Use \
                 synchronization if needed.",
        },
        // -- A38004: Feature max --
        ErrorInfo {
            code: "A38004",
            name: "Feature max too small for invariant",
            description: "A feature_max value is too small to satisfy the associated \
                          contract invariant, making it unsatisfiable.",
            example: r#"  // feature_max(page_size, 4096) but invariant requires
  // page_size > 8192 -> A38004"#,
            fix: "Increase the feature_max value or relax the invariant.",
        },
        // -- A39001-A39004: Resource limits --
        ErrorInfo {
            code: "A39001",
            name: "Limit may be exceeded without check",
            description: "A resource limit may be exceeded because there is no bounds \
                          check before the limit-bounded operation.",
            example: r#"  // Allocating memory without checking against max_memory limit
  // -> A39001"#,
            fix: "Add a bounds check before the operation that compares against \
                 the declared limit.",
        },
        ErrorInfo {
            code: "A39002",
            name: "Limit default outside [min, max]",
            description: "A resource limit's default value is outside the declared \
                          minimum and maximum range.",
            example: r#"  // limit(timeout, min=1, max=60, default=120)
  // default exceeds max -> A39002"#,
            fix: "Set the default value within the [min, max] range.",
        },
        ErrorInfo {
            code: "A39003",
            name: "Limit max exceeds compile-time feature_max",
            description: "A runtime limit maximum is larger than the compile-time \
                          feature_max, which could allow values beyond the verified range.",
            example: r#"  // feature_max(buffer_size, 4096)
  // limit(buffer, max=8192)  -> A39003"#,
            fix: "Set the limit max to be at most the feature_max value.",
        },
        ErrorInfo {
            code: "A39004",
            name: "Limit change may invalidate existing state",
            description: "Lowering a limit after objects have been created at the old \
                          limit may invalidate existing state.",
            example: r#"  // Created 100 items with max=200
  // Lowering max to 50 invalidates 50 items -> A39004"#,
            fix: "Drain or validate existing state before lowering a limit.",
        },
        // -- A40001-A40004: Incremental computation --
        ErrorInfo {
            code: "A40001",
            name: "Step called in invalid state",
            description: "An incremental computation step was called after the \
                          computation reached a terminal state (Done or Aborted).",
            example: r#"  // iter.step() called after iter reached Done -> A40001"#,
            fix: "Check the computation state before calling step(). Do not \
                 step after reaching a terminal state.",
        },
        ErrorInfo {
            code: "A40002",
            name: "Incremental value not finalized",
            description: "An incremental computation value was dropped without reaching \
                          a terminal state (Done or Aborted).",
            example: r#"  // let iter = start_computation()
  // iter dropped without calling finalize() -> A40002"#,
            fix: "Call finalize() or abort() before the value goes out of scope.",
        },
        ErrorInfo {
            code: "A40003",
            name: "Incremental progress not guaranteed",
            description: "An incremental computation step may loop without yielding \
                          or completing, violating progress guarantees.",
            example: r#"  // step() may loop forever without producing output -> A40003"#,
            fix: "Ensure each step makes progress toward completion. Add a \
                 decreasing measure or yield point.",
        },
        ErrorInfo {
            code: "A40004",
            name: "Resources not released on terminal state",
            description: "Resources (locks, temp tables, etc.) are not released when \
                          the incremental computation reaches a terminal state.",
            example: r#"  // Computation aborted but temp table not dropped -> A40004"#,
            fix: "Release all held resources in both Done and Aborted terminal handlers.",
        },
        // -- A41001-A41005: Output divergence --
        ErrorInfo {
            code: "A41001",
            name: "Output divergence detected",
            description: "Two implementations of the same specification produce different \
                          results for the same input.",
            example: r#"  // Implementation A returns [1,2,3]
  // Implementation B returns [1,3,2] for same input -> A41001"#,
            fix: "Align the implementations or document the divergence in an \
                 'except' list.",
        },
        ErrorInfo {
            code: "A41002",
            name: "Error code mismatch",
            description: "Two implementations return different error codes for the \
                          same invalid input.",
            example: r#"  // Impl A returns INVALID_INPUT
  // Impl B returns OVERFLOW for same input -> A41002"#,
            fix: "Standardize error codes across implementations.",
        },
        ErrorInfo {
            code: "A41003",
            name: "Row ordering difference",
            description: "Two implementations return the same rows but in a different \
                          order when ordering was specified.",
            example: r#"  // Same query returns [a,b,c] vs [a,c,b] -> A41003"#,
            fix: "Ensure ORDER BY is applied consistently, or document that ordering \
                 is not guaranteed.",
        },
        ErrorInfo {
            code: "A41004",
            name: "Type coercion difference",
            description: "Two implementations store the same value but with different \
                          type affinity or coercion behavior.",
            example: r#"  // Impl A stores "123" as TEXT
  // Impl B stores 123 as INTEGER -> A41004"#,
            fix: "Use explicit type casts or document coercion rules.",
        },
        ErrorInfo {
            code: "A41005",
            name: "Undocumented exclusion",
            description: "A behavioral divergence was found that is not listed in the \
                          'except' exclusion list.",
            example: r#"  // Divergence in edge case not covered by except list -> A41005"#,
            fix: "Add the divergence to the except list with justification, or fix the \
                 implementation to match.",
        },
        // -- A42004-A42005: Unsafe escape --
        ErrorInfo {
            code: "A42004",
            name: "Unsafe escape without proof obligation",
            description: "An unsafe escape hatch was used without providing any proof \
                          obligation to justify the unsafe operation.",
            example: r#"  // unsafe_cast(x) without proof { ... } -> A42004"#,
            fix: "Add a proof obligation block justifying why the unsafe operation \
                 is safe in this context.",
        },
        ErrorInfo {
            code: "A42005",
            name: "Proof obligation references out-of-scope variable",
            description: "A proof obligation block references a variable that is not \
                          accessible in the current scope.",
            example: r#"  // proof { old_var > 0 }   // old_var not in scope -> A42005"#,
            fix: "Use only in-scope variables in proof obligations.",
        },
        // -- A43004-A43005: String encoding --
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
        // -- A44004-A44005: Page cache --
        ErrorInfo {
            code: "A44004",
            name: "Double unpin: pin count already zero",
            description: "A page unpin operation was called when the pin count is \
                          already zero.",
            example: r#"  // page.unpin() called twice without intervening pin -> A44004"#,
            fix: "Track pin/unpin calls to ensure they are balanced.",
        },
        ErrorInfo {
            code: "A44005",
            name: "Dirtying unpinned page",
            description: "A make_dirty operation was called on a page cache entry that \
                          is not pinned (pin_count is 0).",
            example: r#"  // page.make_dirty() when pin_count == 0 -> A44005"#,
            fix: "Pin the page before modifying it: page.pin(); page.make_dirty();",
        },
        // -- A45004-A45005: MVCC --
        ErrorInfo {
            code: "A45004",
            name: "Stale snapshot: version no longer available",
            description: "A snapshot references a version that has been removed by \
                          a WAL checkpoint or garbage collection.",
            example: r#"  // Reading from snapshot v5 after checkpoint advanced to v10
  // -> A45004"#,
            fix: "Refresh the snapshot or ensure checkpoints do not remove active \
                 snapshot versions.",
        },
        ErrorInfo {
            code: "A45005",
            name: "Write to read-only transaction",
            description: "A modification was attempted within a read-only transaction.",
            example: r#"  // tx.read_only().write(page) -> A45005"#,
            fix: "Use a read-write transaction for modifications.",
        },
        // -- A46004: IO bounds --
        ErrorInfo {
            code: "A46004",
            name: "IO bound exceeded",
            description: "The actual number of I/O operations exceeds the declared \
                          O(N) bound.",
            example: r#"  // Declared O(log N) reads but performed O(N) reads -> A46004"#,
            fix: "Optimize the algorithm to meet the declared IO bound, or update \
                 the bound to match actual behavior.",
        },
        // -- A47004: Monotonic overflow --
        ErrorInfo {
            code: "A47004",
            name: "Monotonic value overflows without wrap policy",
            description: "A monotonically increasing value may overflow without a \
                          declared saturates_at or wraps_at policy.",
            example: r#"  // Counter increments without bound, no wraps_at -> A47004"#,
            fix: "Add a wraps_at or saturates_at policy to the monotonic declaration.",
        },
        // -- A48004-A48005: Checksum --
        ErrorInfo {
            code: "A48004",
            name: "Return value of reset not checked",
            description: "The return value of a reset operation was not checked, \
                          potentially missing an error condition.",
            example: r#"  // sqlite3_reset(stmt) return value ignored -> A48004"#,
            fix: "Check the return value and handle errors appropriately.",
        },
        ErrorInfo {
            code: "A48005",
            name: "Must-preserve detail violated",
            description: "An extended error code or detail is lost when converting \
                          to a simpler error representation.",
            example: r#"  // Extended error code lost in error conversion -> A48005"#,
            fix: "Preserve the extended error code or detail through the conversion.",
        },
        // -- A49004-A49005: Bit-level --
        ErrorInfo {
            code: "A49004",
            name: "Bit cursor used after byte-level read",
            description: "A bit-level cursor was used after a byte-level read without \
                          re-aligning, causing incorrect bit positions.",
            example: r#"  // read_byte(); read_bits(3);  // misaligned -> A49004"#,
            fix: "Re-align the cursor after switching between bit and byte reads.",
        },
        ErrorInfo {
            code: "A49005",
            name: "Bit field constraint not satisfiable",
            description: "A where predicate on a bit field is always false given the \
                          field's width, making it unsatisfiable.",
            example: r#"  // 3-bit field with where { v > 10 } -> always false -> A49005"#,
            fix: "Adjust the constraint to be satisfiable within the bit field's range.",
        },
        // -- A50004-A50005: Precomputed table --
        ErrorInfo {
            code: "A50004",
            name: "Generating function is not total over range",
            description: "The function used to generate a precomputed table may fail \
                          for some index in the declared range.",
            example: r#"  // table[i] = 1/i for i in 0..256
  // Fails at i=0 (division by zero) -> A50004"#,
            fix: "Ensure the generating function handles all values in the range, \
                 or adjust the range to exclude problematic inputs.",
        },
        ErrorInfo {
            code: "A50005",
            name: "Table size mismatch",
            description: "The declared table size does not match the actual range of \
                          the generating function.",
            example: r#"  // Declared size 256 but range produces 255 entries -> A50005"#,
            fix: "Align the declared size with the actual range of the generating function.",
        },
        // -- A51004-A51005: Numerical precision --
        ErrorInfo {
            code: "A51004",
            name: "No reference function for precision contract",
            description: "A precision block has no reference function to compare against. \
                          Precision contracts require a reference implementation.",
            example: r#"  // precision { tolerance: 1e-6 }
  // Missing: reference { exact_impl(x) } -> A51004"#,
            fix: "Add a reference function that provides the exact result to compare against.",
        },
        ErrorInfo {
            code: "A51005",
            name: "Reference function uses restricted operations",
            description: "The reference function in a precision contract uses operations \
                          that are not deterministic or total.",
            example: r#"  // reference { random_impl(x) }  // non-deterministic -> A51005"#,
            fix: "Ensure the reference function is deterministic and total over its domain.",
        },
        // -- A52004-A52005: Multi-pass --
        ErrorInfo {
            code: "A52004",
            name: "Probe function has side effects",
            description: "A probe function used for format detection has side effects. \
                          Probes must be pure since they may be called speculatively.",
            example: r#"  // probe fn that writes to log -> A52004"#,
            fix: "Make the probe function pure. Move side effects to the processing phase.",
        },
        ErrorInfo {
            code: "A52005",
            name: "No codec matches input",
            description: "All codec magic patterns failed and no fallback codec is \
                          declared, so the input cannot be processed.",
            example: r#"  // Input doesn't match any declared format pattern -> A52005"#,
            fix: "Add a fallback codec or handle the unrecognized format case.",
        },
        // -- A53003-A53005: Multi-pass refinement --
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
            code: "A53004",
            name: "Pass count exceeds declared maximum",
            description: "The computation performed more passes than the declared maximum.",
            example: r#"  // passes { max: 3 }
  // Algorithm needed 5 passes -> A53004"#,
            fix: "Optimize the algorithm to converge within the declared pass limit, \
                 or increase the maximum.",
        },
        ErrorInfo {
            code: "A53005",
            name: "Refinement state not initialized before first pass",
            description: "The refinement state is used in the first pass without \
                          being initialized.",
            example: r#"  // Using refinement.prev_result in pass 0 -> A53005"#,
            fix: "Initialize the refinement state before the first pass.",
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
        // -- A54004-A54005: Ghost variables --
        ErrorInfo {
            code: "A54004",
            name: "Ghost variable not updated to match runtime state",
            description: "An invariant links a ghost variable to runtime state, but \
                          the ghost variable is not updated when the runtime state changes.",
            example: r#"  ghost { size: Int }
  invariant { size == length(items) }
  // items.push(x) without updating ghost size -> A54004"#,
            fix: "Update the ghost variable whenever the linked runtime state changes.",
        },
        ErrorInfo {
            code: "A54005",
            name: "Ghost type used in runtime signature",
            description: "A function parameter or return type uses a ghost-only type, \
                          which would be erased at runtime.",
            example: r#"  fn bad(proof: ghost Proof) -> ghost Evidence
  // ghost types in runtime signature -> A54005"#,
            fix: "Remove ghost types from runtime function signatures. Ghost types \
                 are only valid in ghost blocks and specifications.",
        },
        // -- A55004-A55005: Lemmas --
        ErrorInfo {
            code: "A55004",
            name: "Lemma has side effects",
            description: "A lemma function performs side effects. Lemmas must be pure \
                          since they are ghost code used only for verification.",
            example: r#"  lemma log_positive(x: Int) {
      println(x);   // side effect in lemma -> A55004
  }"#,
            fix: "Remove all side effects from the lemma. Lemmas must be pure.",
        },
        ErrorInfo {
            code: "A55005",
            name: "Circular lemma dependency",
            description: "Lemma A depends on lemma B which depends on lemma A, \
                          creating a circular proof.",
            example: r#"  lemma A() { by B() }
  lemma B() { by A() }   // circular -> A55005"#,
            fix: "Break the circular dependency by proving one lemma from first \
                 principles or restructuring the proof.",
        },
        // -- A56001-A56005: Frame conditions --
        ErrorInfo {
            code: "A56001",
            name: "Function modifies undeclared target",
            description: "A function writes to a variable or field that is not listed \
                          in its modifies clause.",
            example: r#"  fn update(x: mut Int)
      modifies { x }
  {
      y = 42;   // y not in modifies clause -> A56001
  }"#,
            fix: "Add the target to the modifies clause: modifies { x, y }.",
        },
        ErrorInfo {
            code: "A56002",
            name: "Called function modifies outside caller's frame",
            description: "A called function modifies targets that are outside the \
                          caller's modifies set.",
            example: r#"  fn caller()
      modifies { x }
  {
      callee();   // callee modifies y, not in caller's frame -> A56002
  }"#,
            fix: "Add the callee's modifies targets to the caller's modifies clause.",
        },
        ErrorInfo {
            code: "A56003",
            name: "Function reads undeclared source",
            description: "A function reads from a variable or field not listed in \
                          its reads clause.",
            example: r#"  fn compute()
      reads { x }
  {
      y + x   // y not in reads clause -> A56003
  }"#,
            fix: "Add the source to the reads clause: reads { x, y }.",
        },
        ErrorInfo {
            code: "A56004",
            name: "Modifies clause on pure function",
            description: "A pure function declares a modifies clause, which is \
                          contradictory since pure functions cannot have side effects.",
            example: r#"  fn pure_fn(x: Int) -> Int
      effects { pure }
      modifies { state }   // contradicts pure -> A56004"#,
            fix: "Remove the modifies clause or change the effects declaration.",
        },
        ErrorInfo {
            code: "A56005",
            name: "Frame condition conflict with effects",
            description: "The modifies clause contradicts the declared effects. For \
                          example, modifying a database field without database effects.",
            example: r#"  fn update_db()
      effects { io }
      modifies { db.table }   // needs database effect -> A56005"#,
            fix: "Ensure the effects declaration covers all targets in the modifies clause.",
        },
        // -- A57001-A57005: Axioms --
        ErrorInfo {
            code: "A57001",
            name: "Axiom is inconsistent",
            description: "An axiom definition is self-contradictory. The axiom's \
                          property and definition cannot both be true.",
            example: r#"  axiom impossible {
      define { x == x + 1 }   // self-contradictory -> A57001
  }"#,
            fix: "Fix the axiom definition to be consistent.",
        },
        ErrorInfo {
            code: "A57002",
            name: "Recursive axiom not well-founded",
            description: "A recursive axiom does not have a structural decrease, \
                          which could lead to infinite unfolding.",
            example: r#"  axiom bad_rec(n: Int) {
      define { bad_rec(n) }   // no decrease -> A57002
  }"#,
            fix: "Add a structural decrease to the recursive axiom definition.",
        },
        ErrorInfo {
            code: "A57003",
            name: "Axiom property does not follow from definition",
            description: "The property clause of an axiom cannot be proven from its \
                          definition clause.",
            example: r#"  axiom wrong {
      define { x > 0 }
      property { x > 100 }   // does not follow -> A57003
  }"#,
            fix: "Ensure the property is a logical consequence of the definition.",
        },
        ErrorInfo {
            code: "A57004",
            name: "Axiom used at runtime",
            description: "An axiom is referenced in non-ghost, non-contract context. \
                          Axioms are ghost-level concepts and cannot affect runtime behavior.",
            example: r#"  fn compute() -> Int {
      by axiom_foo();   // axiom in runtime code -> A57004
  }"#,
            fix: "Use axioms only in ghost blocks, lemmas, and contract specifications.",
        },
        ErrorInfo {
            code: "A57005",
            name: "Conflicting axiom definitions",
            description: "Two axioms define the same concept differently, leading to \
                          an inconsistent axiom set.",
            example: r#"  axiom def1 { define { f(0) == 1 } }
  axiom def2 { define { f(0) == 2 } }   // conflict -> A57005"#,
            fix: "Remove one of the conflicting axiom definitions.",
        },
        // -- A58001-A58005: Triggers --
        ErrorInfo {
            code: "A58001",
            name: "Trigger does not mention bound variable",
            description: "A trigger pattern for a quantifier does not mention the \
                          bound variable, making the trigger useless for instantiation.",
            example: r#"  forall x :: { f(y) } :: P(x)
  // trigger f(y) doesn't mention x -> A58001"#,
            fix: "Include the bound variable in the trigger pattern: { f(x) }.",
        },
        ErrorInfo {
            code: "A58002",
            name: "Potential matching loop in trigger",
            description: "The trigger pattern may cause infinite quantifier instantiation \
                          by matching its own output.",
            example: r#"  forall x :: { f(x) } :: f(x) == f(f(x))
  // f(f(x)) matches trigger, causing loop -> A58002"#,
            fix: "Choose a trigger that does not match terms produced by the quantifier body.",
        },
        ErrorInfo {
            code: "A58003",
            name: "Quantifier timeout (no trigger specified)",
            description: "The SMT solver timed out on a quantifier that has no explicit \
                          trigger. Without triggers, the solver may try too many instantiations.",
            example: r#"  forall x : Int :: P(x) && Q(x)
  // no trigger, solver times out -> A58003"#,
            fix: "Add an explicit trigger annotation: forall x :: { P(x) } :: P(x) && Q(x).",
        },
        ErrorInfo {
            code: "A58004",
            name: "Conflicting triggers on same quantifier",
            description: "Multiple trigger annotations on the same quantifier conflict \
                          with each other.",
            example: r#"  forall x :: { f(x) } :: { g(x) } :: P(x)
  // conflicting triggers -> A58004"#,
            fix: "Use a single trigger set or combine into a multi-pattern trigger.",
        },
        ErrorInfo {
            code: "A58005",
            name: "Trigger pattern not found in formula",
            description: "A trigger annotation references an expression that does not \
                          appear in the quantifier body.",
            example: r#"  forall x :: { h(x) } :: f(x) > 0
  // h(x) not in body -> A58005"#,
            fix: "Use a trigger pattern that appears in the quantifier body.",
        },
        // -- A59001-A59005: Opaque functions --
        ErrorInfo {
            code: "A59001",
            name: "Cannot prove property: function is opaque",
            description: "The verifier cannot prove a property because the function's \
                          body is hidden (opaque). Use 'reveal' to expose the body.",
            example: r#"  opaque fn secret(x: Int) -> Int
      ensures { result > 0 }

  // Caller cannot prove secret(5) > 0 without reveal -> A59001"#,
            fix: "Add 'reveal secret;' at the call site, or add a stronger contract \
                 to the opaque function.",
        },
        ErrorInfo {
            code: "A59002",
            name: "Reveal of non-opaque function",
            description: "A 'reveal' directive was applied to a function that is not \
                          marked as opaque. This is a no-op and likely a mistake.",
            example: r#"  fn visible(x: Int) -> Int { x + 1 }
  reveal visible;   // not opaque, no-op -> A59002"#,
            fix: "Remove the unnecessary 'reveal' directive, or mark the function \
                 as 'opaque' if hiding was intended.",
        },
        ErrorInfo {
            code: "A59003",
            name: "Opaque function contract insufficient",
            description: "An opaque function's body satisfies a property that its \
                          contract does not expose, potentially hiding useful information.",
            example: r#"  opaque fn abs(x: Int) -> Int
      ensures { result >= 0 }
  // Body also ensures result <= max(x, -x) but contract doesn't say so"#,
            fix: "Strengthen the opaque function's contract to expose the property \
                 that callers need.",
        },
        ErrorInfo {
            code: "A59004",
            name: "Recursive reveal exceeded fuel",
            description: "A 'reveal' on a recursive opaque function hit the unfolding \
                          limit. The solver cannot unfold the recursion further.",
            example: r#"  opaque fn fib(n: Int) -> Int
  reveal fib;   // needs too many unfoldings -> A59004"#,
            fix: "Increase the fuel limit, add intermediate lemmas, or provide a \
                 direct proof without relying on full unfolding.",
        },
        ErrorInfo {
            code: "A59005",
            name: "Opaque type field accessed externally",
            description: "Code outside the defining module accesses a field of an \
                          opaque type, violating information hiding.",
            example: r#"  // In module B:
  let x = opaque_value.hidden_field;   // -> A59005"#,
            fix: "Access opaque type internals only through the module's public API.",
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
        // -- A03007-A03012: Dependent type / info-flow type errors --
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
        // -- A08101-A08103: Memory safety --
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
        // -- A09101-A09103: Taint analysis --
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
        // -- A10101-A10104: Fixed-width integer safety --
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
        // -- A11001-A11005: FFI trust boundary --
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
        // -- A12001-A12003: Error propagation --
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
        // -- A13001-A13003: Interface compliance --
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
        // -- A14001-A14002: Timing side-channel --
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
        // -- A15001-A15004: Structural invariants --
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
        // -- A16001-A16003: Sensitive data zeroization --
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
        // -- A17001-A17004: Cryptographic conformance --
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
        // -- A18001-A18003: Shared memory access --
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
        // -- A19001-A19002: Audit trail (spec Section 7.1) --
        ErrorInfo {
            code: "A19001",
            name: "Missing audit trail",
            description: "A contract or function marked with an audit annotation does \
                          not produce a corresponding audit trail entry. Every auditable \
                          operation must emit a structured log entry for compliance review.",
            example: r#"  @auditable
  contract TransferFunds {
      input { from: Account, to: Account, amount: Nat }
      requires { from.balance >= amount }
      // missing audit trail emission
  }"#,
            fix: "Add an audit trail emission in the contract body, or use the \
                 built-in audit effect to generate entries automatically.",
        },
        ErrorInfo {
            code: "A19002",
            name: "Incomplete audit trail",
            description: "An audit trail entry is missing required fields. Audit entries \
                          must include timestamp, actor, action, and result.",
            example: r#"  audit.emit({
      action: "transfer",
      // missing: actor, timestamp, result
  })"#,
            fix: "Include all required audit fields: timestamp, actor, action, and result.",
        },
        // -- A20001-A20002: Determinism --
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
        // -- A21001-A21003: Lock ordering --
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
        // -- A22001-A22004: Allocation safety --
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
        // -- A23001-A23003: Circular buffer --
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
        // -- A24001-A24003: Reentrancy / callback depth --
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
        // -- A25001-A25003: Temporal deadlines --
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
        // -- A26001-A26004: Binary format field validation --
        ErrorInfo {
            code: "A26001",
            name: "Binary field out of bounds",
            description: "A binary format field's offset plus size exceeds the buffer \
                          length.",
            example: r#"  // Buffer is 16 bytes, field at offset 12, size 8
      field header at offset 12, size 8  // 12 + 8 = 20 > 16"#,
            fix: "Adjust the field offset or size to fit within the buffer.",
        },
        // -- A26002: i18n completeness (spec Section 7.1) --
        ErrorInfo {
            code: "A26002",
            name: "Incomplete i18n coverage",
            description: "A string literal or user-facing message is not covered by \
                          the internationalization table. Every user-visible string \
                          must have translations for all declared locales.",
            example: r#"  @i18n(locales: ["en", "fr", "de"])
  contract Greet {
      ensures { result == "Hello" }  // no fr/de translations
  }"#,
            fix: "Add translations for all declared locales in the i18n table, \
                 or mark the string as locale-independent with @no_i18n.",
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
        // -- A27001-A27003: Bit field layout --
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
        // -- A28001-A28003: String encoding safety --
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
        // -- A29001-A29003: Checksum verification --
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
        // -- A30001-A30003: Protocol state machine --
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
        // -- A31001-A31003: Axiom references --
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
        // -- A32001-A32003: Opaque function verification --
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
        // -- A33001-A33003: Crash recovery (WAL) --
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
        // -- A34001-A34003: Page cache --
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
        // -- A35001-A35003: MVCC write conflicts --
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
        // -- A36001-A36003: Rollback / savepoint --
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
        // -- A37001-A37003: Monotonicity --
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
        // -- A38001-A38003: Storage failure modes --
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
        // -- A42001-A42003: Numerical precision --
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
        // -- A43001-A43003: Precomputed table verification --
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
        // -- A44001-A44003: Platform abstraction --
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
        // -- A45001-A45003: Feature flags --
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
        // -- A46001-A46003: Resource limits --
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
        // -- A47001-A47003: Unsafe escape proofs --
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
        // -- A48001-A48003: Complexity bounds --
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
        // -- A49001-A49003: Behavioral equivalence --
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
        // -- A50001-A50003: Refinement passes --
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
        // -- A51001-A51003: Contract versioning --
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
        // -- A52001-A52003: Scoped invariant suspension --
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
        // -- A53001-A53002: CRUD authorization --
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
        // -- A54001-A54003: Ghost functions / contract composition --
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
        // -- A55001-A55003: Contract libraries --
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
