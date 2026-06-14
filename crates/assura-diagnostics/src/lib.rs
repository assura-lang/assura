//! Unified diagnostic types for the Assura compiler.
//!
//! All compiler passes (parser, resolver, type checker, SMT verifier)
//! emit `Diagnostic` values. The CLI renders these uniformly via
//! ariadne (human mode) or serde (JSON mode).

use std::ops::Range;
use std::sync::LazyLock;

/// Source location span (byte offsets into the source file).
pub type Span = Range<usize>;

/// Diagnostic severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational message, not an error.
    Info,
    /// Potential problem that does not prevent compilation.
    Warning,
    /// Error that prevents compilation or verification.
    Error,
}

/// A secondary span with a label, used for additional context in diagnostics.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct SecondaryLabel {
    /// The source span for this secondary label.
    pub span: Span,
    /// A description of what this secondary location refers to.
    pub message: String,
}

/// A suggested fix for a diagnostic.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Suggestion {
    /// Human-readable description of what the fix does.
    pub message: String,
    /// The span to replace.
    pub span: Span,
    /// The replacement text.
    pub replacement: String,
}

/// A compiler diagnostic with structured location and severity.
///
/// This is the unified error type emitted by all compiler passes.
/// The CLI consumes `Vec<Diagnostic>` and renders them via ariadne
/// (for human-readable output) or serializes them (for JSON output).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Diagnostic {
    /// Error code from the spec (e.g., "A01001", "A03005").
    pub code: String,
    /// Severity level.
    pub severity: Severity,
    /// Human-readable error message.
    pub message: String,
    /// Source file name (may be empty for in-memory compilations).
    pub file: String,
    /// Primary source location where the error was detected.
    pub primary: Span,
    /// Secondary spans with labels (e.g., "expected type declared here").
    pub secondary: Vec<SecondaryLabel>,
    /// Optional suggested fix.
    pub suggestion: Option<Suggestion>,
}

impl Diagnostic {
    /// Create a new error diagnostic with a code, message, and span.
    pub fn error(code: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            file: String::new(),
            primary: span,
            secondary: Vec::new(),
            suggestion: None,
        }
    }

    /// Create a new warning diagnostic.
    pub fn warning(code: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
            file: String::new(),
            primary: span,
            secondary: Vec::new(),
            suggestion: None,
        }
    }

    /// Set the source file name for this diagnostic.
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = file.into();
        self
    }

    /// Add a secondary span with a label.
    pub fn with_secondary(mut self, span: Span, label: impl Into<String>) -> Self {
        self.secondary.push(SecondaryLabel {
            span,
            message: label.into(),
        });
        self
    }

    /// Add a suggested fix.
    pub fn with_suggestion(
        mut self,
        message: impl Into<String>,
        span: Span,
        replacement: impl Into<String>,
    ) -> Self {
        self.suggestion = Some(Suggestion {
            message: message.into(),
            span,
            replacement: replacement.into(),
        });
        self
    }

    /// Check if this diagnostic is an error.
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Info => write!(f, "info"),
            Severity::Warning => write!(f, "warning"),
            Severity::Error => write!(f, "error"),
        }
    }
}

// ---------------------------------------------------------------------------
// Error catalog: `assura explain <error-code>`
// ---------------------------------------------------------------------------

/// A human-readable explanation of a specific error code.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorInfo {
    /// The error code (e.g. "A01001").
    pub code: &'static str,
    /// Short descriptive name.
    pub name: &'static str,
    /// Multi-line explanation of the error.
    pub description: &'static str,
    /// Example source code that triggers the error.
    pub example: &'static str,
    /// How to fix the error.
    pub fix: &'static str,
}

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
        // -- A02002: Ambiguous name --
        ErrorInfo {
            code: "A02002",
            name: "Ambiguous name",
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
        // -- A02004: Visibility violation --
        ErrorInfo {
            code: "A02004",
            name: "Visibility violation",
            description: "An attempt was made to access a field or member that \
                          is not public. Non-pub fields are only accessible within \
                          the module that defines the type.",
            example: r#"  type Wallet {
      balance: Int   // private (no pub)
  }

  contract Check {
      requires: w.balance > 0   // A02004: balance is private
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
    ]
}

/// Lazily-initialized catalog for O(1) lookups.
static CATALOG: LazyLock<Vec<ErrorInfo>> = LazyLock::new(error_catalog);

/// Look up an error code in the catalog.
///
/// Returns `None` if the code is not recognized.
pub fn explain(code: &str) -> Option<&'static ErrorInfo> {
    CATALOG.iter().find(|e| e.code == code)
}

// ---------------------------------------------------------------------------
// Ariadne rendering (human-readable terminal output)
// ---------------------------------------------------------------------------

/// Render a single `Diagnostic` to stderr using ariadne.
pub fn render_diagnostic(diag: &Diagnostic, filename: &str, source: &str) {
    use ariadne::{Color, Label, Report, ReportKind, Source};

    let kind = match diag.severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Info => ReportKind::Advice,
    };
    let color = match diag.severity {
        Severity::Error => Color::Red,
        Severity::Warning => Color::Yellow,
        Severity::Info => Color::Blue,
    };
    let mut builder = Report::build(kind, filename, diag.primary.start)
        .with_message(format!("[{}] {}", diag.code, diag.message))
        .with_label(
            Label::new((filename, diag.primary.clone()))
                .with_message(&diag.message)
                .with_color(color),
        );
    for sec in &diag.secondary {
        builder = builder.with_label(
            Label::new((filename, sec.span.clone()))
                .with_message(&sec.message)
                .with_color(Color::Blue),
        );
    }
    builder
        .finish()
        .eprint((filename, Source::from(source)))
        .ok();
}

/// Render a list of diagnostics to stderr using ariadne.
pub fn report_diagnostics_human(diagnostics: &[Diagnostic], filename: &str, source: &str) {
    for d in diagnostics {
        render_diagnostic(d, filename, source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_diagnostic_creation() {
        let d = Diagnostic::error("A03001", "type mismatch", 10..20);
        assert_eq!(d.code, "A03001");
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.primary, 10..20);
        assert!(d.is_error());
    }

    #[test]
    fn warning_diagnostic_creation() {
        let d = Diagnostic::warning("A05001", "unused variable", 5..10);
        assert_eq!(d.severity, Severity::Warning);
        assert!(!d.is_error());
    }

    #[test]
    fn diagnostic_with_secondary() {
        let d = Diagnostic::error("A03002", "expected Int", 10..20)
            .with_secondary(30..40, "declared here");
        assert_eq!(d.secondary.len(), 1);
        assert_eq!(d.secondary[0].message, "declared here");
    }

    #[test]
    fn diagnostic_with_suggestion() {
        let d = Diagnostic::error("A01001", "unexpected token", 5..8).with_suggestion(
            "try adding a semicolon",
            7..8,
            ";",
        );
        assert!(d.suggestion.is_some());
        let s = d.suggestion.unwrap();
        assert_eq!(s.replacement, ";");
    }

    #[test]
    fn diagnostic_display() {
        let d = Diagnostic::error("A03001", "type mismatch", 0..1);
        assert_eq!(format!("{d}"), "[A03001] type mismatch");
    }

    #[test]
    fn severity_ordering() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
    }

    #[test]
    fn test_error_diagnostic_is_error() {
        let d = Diagnostic::error("A01001", "syntax error", 0..5);
        assert!(d.is_error());
        assert_eq!(d.severity, Severity::Error);
    }

    #[test]
    fn test_warning_diagnostic_is_not_error() {
        let d = Diagnostic::warning("A02007", "unused import", 10..20);
        assert!(!d.is_error());
        assert_eq!(d.severity, Severity::Warning);
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(format!("{}", Severity::Info), "info");
        assert_eq!(format!("{}", Severity::Warning), "warning");
        assert_eq!(format!("{}", Severity::Error), "error");
    }

    #[test]
    fn test_diagnostic_with_file() {
        let d = Diagnostic::error("A03001", "type mismatch", 0..10).with_file("test.assura");
        assert_eq!(d.file, "test.assura");
    }

    #[test]
    fn test_diagnostic_multiple_secondary_spans() {
        let d = Diagnostic::error("A03001", "type mismatch", 10..20)
            .with_secondary(30..40, "expected type here")
            .with_secondary(50..60, "found type here");
        assert_eq!(d.secondary.len(), 2);
        assert_eq!(d.secondary[0].message, "expected type here");
        assert_eq!(d.secondary[0].span, 30..40);
        assert_eq!(d.secondary[1].message, "found type here");
        assert_eq!(d.secondary[1].span, 50..60);
    }

    #[test]
    fn test_diagnostic_suggestion_fields() {
        let d = Diagnostic::error("A01002", "unexpected token", 5..8).with_suggestion(
            "add a colon",
            7..8,
            ":",
        );
        let s = d.suggestion.as_ref().unwrap();
        assert_eq!(s.message, "add a colon");
        assert_eq!(s.span, 7..8);
        assert_eq!(s.replacement, ":");
    }

    #[test]
    fn test_diagnostic_json_serialization() {
        let d = Diagnostic::error("A03001", "type mismatch", 10..20)
            .with_file("main.assura")
            .with_secondary(30..40, "declared here");
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("A03001"));
        assert!(json.contains("type mismatch"));
        assert!(json.contains("main.assura"));
        assert!(json.contains("declared here"));
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["code"], "A03001");
        assert_eq!(val["severity"], "error");
        assert_eq!(val["message"], "type mismatch");
    }

    #[test]
    fn test_diagnostic_collection() {
        let diags = vec![
            Diagnostic::error("A01001", "unexpected char", 0..1),
            Diagnostic::warning("A02007", "unused import", 10..20),
            Diagnostic::error("A03001", "type mismatch", 30..40),
        ];
        assert_eq!(diags.len(), 3);
        let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
        assert_eq!(errors.len(), 2);
        let warnings: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .collect();
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn test_diagnostic_empty_secondary_spans() {
        let d = Diagnostic::error("A03001", "error", 0..5);
        assert!(d.secondary.is_empty());
        assert!(d.suggestion.is_none());
    }

    #[test]
    fn test_error_code_formatting_display() {
        let d = Diagnostic::error("A05001", "linear variable used twice", 0..10);
        let display = format!("{d}");
        assert_eq!(display, "[A05001] linear variable used twice");
    }

    #[test]
    fn test_error_catalog_not_empty() {
        let catalog = error_catalog();
        assert!(!catalog.is_empty());
        for entry in &catalog {
            assert!(!entry.code.is_empty());
            assert!(!entry.name.is_empty());
            assert!(!entry.description.is_empty());
            assert!(!entry.example.is_empty());
            assert!(!entry.fix.is_empty());
        }
    }

    #[test]
    fn test_explain_known_code() {
        let info = explain("A01001");
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.code, "A01001");
        assert_eq!(info.name, "Unexpected character");
    }

    #[test]
    fn test_explain_unknown_code() {
        let info = explain("A99999");
        assert!(info.is_none());
    }

    #[test]
    fn test_explain_all_catalog_codes() {
        let catalog = error_catalog();
        for entry in &catalog {
            let found = explain(entry.code);
            assert!(found.is_some(), "should find {}", entry.code);
            assert_eq!(found.unwrap().code, entry.code);
        }
    }

    #[test]
    fn test_warning_serialization() {
        let d = Diagnostic::warning("A02007", "unused import", 5..15);
        let json = serde_json::to_string(&d).unwrap();
        let val: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(val["severity"], "warning");
    }

    #[test]
    fn test_suggestion_serialization() {
        let s = Suggestion {
            message: "add semicolon".to_string(),
            span: 10..11,
            replacement: ";".to_string(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("add semicolon"));
    }

    #[test]
    fn test_secondary_label_equality() {
        let a = SecondaryLabel {
            span: 0..5,
            message: "here".to_string(),
        };
        let b = SecondaryLabel {
            span: 0..5,
            message: "here".to_string(),
        };
        assert_eq!(a, b);
    }
}
