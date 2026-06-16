//! Unified diagnostic types for the Assura compiler.
//!
//! All compiler passes (parser, resolver, type checker, SMT verifier)
//! emit `Diagnostic` values. The CLI renders these uniformly via
//! ariadne (human mode) or serde (JSON mode).

use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

/// Source location span (byte offsets into the source file).
pub type Span = Range<usize>;

/// A strongly-typed error code from the Assura specification.
///
/// Wraps the raw code string (e.g. `"A03001"`) so that error code
/// fields are distinguishable from arbitrary strings at the type level.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(transparent)]
pub struct ErrorCode(String);

impl ErrorCode {
    /// Return the code as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ErrorCode {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ErrorCode {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for ErrorCode {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl PartialEq<str> for ErrorCode {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for ErrorCode {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for ErrorCode {
    fn eq(&self, other: &String) -> bool {
        self.0 == *other
    }
}

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
    pub code: ErrorCode,
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
    pub fn error(code: impl Into<ErrorCode>, message: impl Into<String>, span: Span) -> Self {
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
    pub fn warning(code: impl Into<ErrorCode>, message: impl Into<String>, span: Span) -> Self {
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
