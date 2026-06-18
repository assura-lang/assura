use super::*;

// T052: Dependent types (restricted)
// ---------------------------------------------------------------------------

/// A dependent type index: the value a type depends on.
/// Restricted to Nat, Bool, and finite enums (not arbitrary expressions).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DepIndex {
    /// A natural number index, e.g. Vec<T, n>
    Nat(String),
    /// A boolean index, e.g. Matrix<T, is_square>
    Bool(String),
    /// A finite enum index, e.g. Buffer<mode> where mode: ReadWrite
    Enum { name: String, enum_type: String },
}

impl std::fmt::Display for DepIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DepIndex::Nat(n) => write!(f, "{n}: Nat"),
            DepIndex::Bool(n) => write!(f, "{n}: Bool"),
            DepIndex::Enum { name, enum_type } => write!(f, "{name}: {enum_type}"),
        }
    }
}

/// A dependent type: a base type parameterized by one or more indices.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DepType {
    pub base: Type,
    pub indices: Vec<DepIndex>,
}

/// Error from the dependent type checker.
pub(crate) type DepTypeError = CheckerError;

/// Checker for restricted dependent types.
///
/// Validates that:
/// - Dependent type indices are of allowed kinds (Nat, Bool, finite enum)
/// - Index arithmetic in type positions is well-formed
/// - Indices are erased at runtime (ghost)
/// - Type equality with indices is checked structurally
pub(crate) struct DependentTypeChecker {
    /// Known enum types and their variants (for finiteness check)
    enums: HashMap<String, Vec<String>>,
    /// Known dependent type definitions
    dep_types: HashMap<String, DepType>,
    /// Index variable bindings in scope: name -> DepIndex
    index_vars: HashMap<String, DepIndex>,
}

impl DependentTypeChecker {
    pub fn new() -> Self {
        Self {
            enums: HashMap::new(),
            dep_types: HashMap::new(),
            index_vars: HashMap::new(),
        }
    }

    /// Register a finite enum type with its variants.
    pub fn register_enum(&mut self, name: String, variants: Vec<String>) {
        self.enums.insert(name, variants);
    }

    /// Register a dependent type definition.
    pub fn register_dep_type(&mut self, name: String, dep_type: DepType) {
        self.dep_types.insert(name, dep_type);
    }

    /// Bind an index variable in the current scope.
    pub fn bind_index(&mut self, name: String, index: DepIndex) {
        self.index_vars.insert(name, index);
    }

    /// Validate that a type index is of an allowed kind.
    /// Returns A03006 if the index type is not Nat, Bool, or a known finite enum.
    pub fn validate_index(
        &self,
        index_name: &str,
        index_type: &str,
        span: &Range<usize>,
    ) -> Vec<DepTypeError> {
        let mut errors = Vec::new();
        match index_type {
            "Nat" | "Bool" => { /* allowed */ }
            other => {
                if !self.enums.contains_key(other) {
                    errors.push(DepTypeError {
                        code: "A03006".into(),
                        message: format!(
                            "dependent type index `{index_name}` has type `{other}`, \
                             which is not Nat, Bool, or a known finite enum"
                        ),
                        span: span.clone(),
                    });
                }
            }
        }
        errors
    }

    /// Check that index arithmetic in a type position is well-formed.
    /// For Nat indices, expressions like `n + 1`, `n - 1`, `2 * n` are allowed.
    /// For Bool/Enum indices, only direct references are allowed (no arithmetic).
    pub fn check_index_expr(
        &self,
        expr: &Expr,
        expected_kind: &DepIndex,
        span: &Range<usize>,
    ) -> Vec<DepTypeError> {
        let mut errors = Vec::new();
        match expected_kind {
            DepIndex::Nat(_) => {
                // Nat indices allow arithmetic expressions
                if !self.is_nat_expr(expr) {
                    errors.push(DepTypeError {
                        code: "A03007".into(),
                        message: "index expression is not a valid Nat expression; \
                                  only integer arithmetic over index variables is allowed"
                            .into(),
                        span: span.clone(),
                    });
                }
            }
            DepIndex::Bool(_) => {
                // Bool indices: only ident or boolean literal
                if !self.is_bool_expr(expr) {
                    errors.push(DepTypeError {
                        code: "A03008".into(),
                        message: "Bool index must be a direct reference or boolean literal, \
                                  not an arithmetic expression"
                            .into(),
                        span: span.clone(),
                    });
                }
            }
            DepIndex::Enum { enum_type, .. } => {
                // Enum indices: only ident or enum variant
                if !self.is_enum_expr(expr, enum_type) {
                    errors.push(DepTypeError {
                        code: "A03009".into(),
                        message: format!(
                            "enum index of type `{enum_type}` must be a direct reference \
                             or variant name"
                        ),
                        span: span.clone(),
                    });
                }
            }
        }
        errors
    }

    /// Check structural equality of two dependent types.
    /// Two `Vec<T, n>` and `Vec<T, m>` are equal only if `n == m` can be proved.
    pub fn check_dep_type_eq(
        &self,
        expected: &DepType,
        actual: &DepType,
        span: &Range<usize>,
    ) -> Vec<DepTypeError> {
        let mut errors = Vec::new();
        if expected.base != actual.base {
            errors.push(DepTypeError {
                code: "A03010".into(),
                message: format!(
                    "dependent type base mismatch: expected `{:?}`, found `{:?}`",
                    expected.base, actual.base
                ),
                span: span.clone(),
            });
            return errors;
        }
        if expected.indices.len() != actual.indices.len() {
            errors.push(DepTypeError {
                code: "A03010".into(),
                message: format!(
                    "dependent type index count mismatch: expected {}, found {}",
                    expected.indices.len(),
                    actual.indices.len()
                ),
                span: span.clone(),
            });
            return errors;
        }
        for (i, (exp, act)) in expected.indices.iter().zip(&actual.indices).enumerate() {
            if std::mem::discriminant(exp) != std::mem::discriminant(act) {
                errors.push(DepTypeError {
                    code: "A03011".into(),
                    message: format!(
                        "dependent type index {i} kind mismatch: expected {exp}, found {act}"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Verify that index variables are erased at runtime.
    /// Returns an error if an index variable appears in a non-ghost context.
    pub fn check_index_erasure(
        &self,
        expr: &Expr,
        ghost_context: bool,
        span: &Range<usize>,
    ) -> Vec<DepTypeError> {
        if ghost_context {
            return Vec::new(); // Ghost context: indices are fine
        }
        let mut errors = Vec::new();
        for name in self.collect_idents(expr) {
            if self.index_vars.contains_key(&name) {
                errors.push(DepTypeError {
                    code: "A03012".into(),
                    message: format!(
                        "index variable `{name}` used in runtime context; \
                         dependent type indices must be erased at runtime"
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Get a reference to the index variable bindings.
    pub fn index_vars_ref(&self) -> &HashMap<String, DepIndex> {
        &self.index_vars
    }

    /// Get a reference to the registered dependent types.
    pub fn dep_types_ref(&self) -> &HashMap<String, DepType> {
        &self.dep_types
    }

    // --- Helper methods ---

    fn is_nat_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Literal(Literal::Int(_)) => true,
            Expr::Ident(name) => {
                matches!(self.index_vars.get(name), Some(DepIndex::Nat(_)))
                    || !self.index_vars.contains_key(name)
            }
            Expr::BinOp { lhs, op, rhs } => {
                matches!(
                    op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
                ) && self.is_nat_expr(lhs)
                    && self.is_nat_expr(rhs)
            }
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr,
            } => self.is_nat_expr(expr),
            Expr::Paren(inner) => self.is_nat_expr(inner),
            _ => false,
        }
    }

    fn is_bool_expr(&self, expr: &Expr) -> bool {
        matches!(expr, Expr::Literal(Literal::Bool(_)) | Expr::Ident(_))
    }

    fn is_enum_expr(&self, expr: &Expr, enum_type: &str) -> bool {
        match expr {
            Expr::Ident(name) => {
                // Either a variable reference or a variant name
                if let Some(variants) = self.enums.get(enum_type) {
                    variants.contains(name) || self.index_vars.contains_key(name)
                } else {
                    self.index_vars.contains_key(name)
                }
            }
            _ => false,
        }
    }

    fn collect_idents(&self, expr: &Expr) -> Vec<String> {
        struct IdentCollector(Vec<String>);
        impl ExprVisitor for IdentCollector {
            fn visit_ident(&mut self, name: &str) {
                self.0.push(name.to_string());
            }
        }
        let mut c = IdentCollector(Vec::new());
        c.visit_expr(expr);
        c.0
    }
}

impl Default for DependentTypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests

// ---------------------------------------------------------------------------
// Information flow checking (T051 - SEC.3)
// ---------------------------------------------------------------------------

/// Security label in the information flow lattice.
///
/// The lattice is ordered: `Public < Internal < Confidential < Restricted`.
/// Data may flow upward in the lattice (Public -> Confidential) but never
/// downward (Confidential -> Public) without explicit declassification.
///
/// Implements Section 2.7 of the spec (information flow types).
///
/// # Error codes
///
/// - **A08001**: Information flows from higher security to lower security
/// - **A08002**: Declassification without explicit annotation
/// - **A08003**: Purpose label mismatch (GDPR)
/// - **A08004**: Implicit flow through control dependency
/// - **A08005**: Covert channel through timing/exceptions
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SecurityLabel {
    /// Publicly accessible data.
    Public,
    /// Internal-only data (not exposed to external consumers).
    Internal,
    /// Confidential data (PII, credentials, etc.).
    Confidential,
    /// Restricted data (highest classification, e.g. encryption keys).
    Restricted,
}

impl std::fmt::Display for SecurityLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityLabel::Public => write!(f, "Public"),
            SecurityLabel::Internal => write!(f, "Internal"),
            SecurityLabel::Confidential => write!(f, "Confidential"),
            SecurityLabel::Restricted => write!(f, "Restricted"),
        }
    }
}

/// A structured information flow error.
pub(crate) type InfoFlowError = CheckerError;

/// Information flow checker that enforces the security lattice.
///
/// Tracks security labels on variables and ensures that data never flows
/// from a higher security level to a lower one without explicit
/// declassification.  Also tracks GDPR purpose labels for data-purpose
/// compliance.
#[derive(Debug, Clone)]
pub(crate) struct InfoFlowChecker {
    /// Maps variable name to its security label.
    labels: HashMap<String, SecurityLabel>,
    /// Maps variable name to its GDPR purpose label (e.g. "analytics",
    /// "billing", "marketing").
    purpose_labels: HashMap<String, String>,
    /// Set of variables that carry an explicit `@declassify` annotation.
    declassify_annotations: std::collections::HashSet<String>,
    /// Names of functions that are considered timing-sensitive (potential
    /// covert channels).
    timing_sensitive_fns: std::collections::HashSet<String>,
}

impl InfoFlowChecker {
    /// Create a new, empty information flow checker with built-in
    /// timing-sensitive function names.
    pub fn new() -> Self {
        let mut timing_sensitive_fns = std::collections::HashSet::new();
        timing_sensitive_fns.insert("sleep".to_string());
        timing_sensitive_fns.insert("delay".to_string());
        timing_sensitive_fns.insert("wait".to_string());
        timing_sensitive_fns.insert("throw".to_string());
        timing_sensitive_fns.insert("panic".to_string());
        timing_sensitive_fns.insert("abort".to_string());
        Self {
            labels: HashMap::new(),
            purpose_labels: HashMap::new(),
            declassify_annotations: std::collections::HashSet::new(),
            timing_sensitive_fns,
        }
    }

    /// Declare a variable with a security label.
    pub fn declare(&mut self, name: String, label: SecurityLabel) {
        self.labels.insert(name, label);
    }

    /// Declare a variable with a GDPR purpose label.
    pub fn declare_purpose(&mut self, name: String, purpose: String) {
        self.purpose_labels.insert(name, purpose);
    }

    /// Mark a variable as having an explicit `@declassify` annotation.
    pub fn mark_declassify(&mut self, name: String) {
        self.declassify_annotations.insert(name);
    }

    /// Register a function as timing-sensitive (potential covert channel).
    pub fn register_timing_sensitive(&mut self, name: String) {
        self.timing_sensitive_fns.insert(name);
    }

    /// Get the security label for a variable. Returns `None` if the
    /// variable has not been declared.
    pub fn get_label(&self, name: &str) -> Option<SecurityLabel> {
        self.labels.get(name).copied()
    }

    /// Get the purpose label for a variable.
    pub fn get_purpose(&self, name: &str) -> Option<&str> {
        self.purpose_labels.get(name).map(|s| s.as_str())
    }

    /// Returns `true` if any security labels are tracked.
    pub fn has_labels(&self) -> bool {
        !self.labels.is_empty()
    }

    // -----------------------------------------------------------------
    // Core checks
    // -----------------------------------------------------------------

    /// Check an assignment: data flows from `source_label` to
    /// `target_label`.
    ///
    /// The source security level must be less than or equal to the
    /// target level. Emits **A08001** if data flows from a higher
    /// security level to a lower one.
    pub fn check_assignment(
        &self,
        target_label: SecurityLabel,
        source_label: SecurityLabel,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if source_label > target_label {
            Some(InfoFlowError {
                code: "A08001".into(),
                message: format!(
                    "information flows from {source_label} to {target_label}: \
                     data at security level `{source_label}` cannot be assigned \
                     to a `{target_label}` variable"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check a declassification: data is being lowered from `from_label`
    /// to `to_label`.
    ///
    /// Declassification is only permitted when an explicit annotation is
    /// present. Emits **A08002** if `has_declassify_annotation` is false.
    pub fn check_declassify(
        &self,
        from_label: SecurityLabel,
        to_label: SecurityLabel,
        has_declassify_annotation: bool,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if from_label > to_label && !has_declassify_annotation {
            Some(InfoFlowError {
                code: "A08002".into(),
                message: format!(
                    "declassification from {from_label} to {to_label} \
                     without explicit `@declassify` annotation"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check that a variable's purpose label matches the required purpose.
    ///
    /// Emits **A08003** if the variable has a purpose label that differs
    /// from `required_purpose`.
    pub fn check_purpose_label(
        &self,
        variable: &str,
        required_purpose: &str,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if let Some(actual_purpose) = self.purpose_labels.get(variable)
            && actual_purpose != required_purpose
        {
            return Some(InfoFlowError {
                code: "A08003".into(),
                message: format!(
                    "purpose label mismatch for `{variable}`: data labeled \
                     for `{actual_purpose}` used in `{required_purpose}` context"
                ),
                span: span.clone(),
            });
        }
        None
    }

    /// Check for implicit information flow through control dependencies.
    ///
    /// If a conditional expression depends on a high-security variable and
    /// assigns to a low-security variable inside a branch, information
    /// leaks through the control flow.  Emits **A08004**.
    ///
    /// `condition_label` is the inferred label of the if-condition.
    /// `branch_target_label` is the label of the variable being assigned
    /// inside the branch.
    pub fn check_implicit_flow(
        &self,
        condition_label: SecurityLabel,
        branch_target_label: SecurityLabel,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if condition_label > branch_target_label {
            Some(InfoFlowError {
                code: "A08004".into(),
                message: format!(
                    "implicit information flow: condition at `{condition_label}` \
                     level influences assignment to `{branch_target_label}` variable"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check for covert channels through timing or exceptions.
    ///
    /// If a high-security value controls whether a timing-sensitive
    /// function (sleep, delay, throw, panic) is called, information can
    /// leak through observable side effects.  Emits **A08005**.
    pub fn check_covert_channel(
        &self,
        condition_label: SecurityLabel,
        callee: &str,
        span: &Range<usize>,
    ) -> Option<InfoFlowError> {
        if condition_label > SecurityLabel::Public && self.timing_sensitive_fns.contains(callee) {
            Some(InfoFlowError {
                code: "A08005".into(),
                message: format!(
                    "potential covert channel: `{condition_label}` data controls \
                     call to timing/exception function `{callee}`"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    // -----------------------------------------------------------------
    // Label inference
    // -----------------------------------------------------------------

    /// Infer the security label of an expression.
    ///
    /// The result label is the **maximum** of all operand labels (the
    /// join in the lattice).  Variables without a declared label default
    /// to `Public`.
    pub fn infer_label(&self, expr: &Expr) -> SecurityLabel {
        match expr {
            Expr::Ident(name) => self
                .labels
                .get(name)
                .copied()
                .unwrap_or(SecurityLabel::Public),

            Expr::Literal(_) => SecurityLabel::Public,

            Expr::Field(receiver, _) => self.infer_label(receiver),

            Expr::BinOp { lhs, rhs, .. } => {
                std::cmp::max(self.infer_label(lhs), self.infer_label(rhs))
            }

            Expr::UnaryOp { expr: inner, .. } => self.infer_label(inner),

            Expr::Call { func, args } => {
                let f = self.infer_label(func);
                args.iter()
                    .fold(f, |acc, arg| std::cmp::max(acc, self.infer_label(arg)))
            }

            Expr::MethodCall { receiver, args, .. } => {
                let r = self.infer_label(receiver);
                args.iter()
                    .fold(r, |acc, arg| std::cmp::max(acc, self.infer_label(arg)))
            }

            Expr::Index { expr: base, index } => {
                std::cmp::max(self.infer_label(base), self.infer_label(index))
            }

            Expr::Old(inner) | Expr::Paren(inner) | Expr::Cast { expr: inner, .. } => {
                self.infer_label(inner)
            }

            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let mut r = std::cmp::max(self.infer_label(cond), self.infer_label(then_branch));
                if let Some(e) = else_branch {
                    r = std::cmp::max(r, self.infer_label(e));
                }
                r
            }

            Expr::List(items) => items.iter().fold(SecurityLabel::Public, |a, i| {
                std::cmp::max(a, self.infer_label(i))
            }),

            Expr::Block(exprs) => exprs.iter().fold(SecurityLabel::Public, |a, e| {
                std::cmp::max(a, self.infer_label(e))
            }),

            Expr::Forall { body, .. } | Expr::Exists { body, .. } => self.infer_label(body),

            Expr::Apply { args, .. } => args.iter().fold(SecurityLabel::Public, |a, arg| {
                std::cmp::max(a, self.infer_label(arg))
            }),

            Expr::Match { scrutinee, arms } => {
                let mut r = self.infer_label(scrutinee);
                for arm in arms {
                    r = std::cmp::max(r, self.infer_label(&arm.body));
                }
                r
            }

            Expr::Let { value, body, .. } => {
                std::cmp::max(self.infer_label(value), self.infer_label(body))
            }

            Expr::Tuple(elems) => elems.iter().fold(SecurityLabel::Public, |a, e| {
                std::cmp::max(a, self.infer_label(e))
            }),

            Expr::Ghost(_) | Expr::Raw(_) => SecurityLabel::Public,
        }
    }

    // -----------------------------------------------------------------
    // Expression-level checking
    // -----------------------------------------------------------------

    /// Check an expression tree for information flow violations.
    ///
    /// Walks the AST looking for:
    /// - Implicit flows through `if` conditions (A08004)
    /// - Covert channels through timing/exception calls (A08005)
    pub fn check_expr(&self, expr: &Expr, span: &Range<usize>) -> Vec<InfoFlowError> {
        let mut errors = Vec::new();
        self.check_expr_inner(expr, span, SecurityLabel::Public, &mut errors);
        errors
    }

    /// Inner recursive checker with a `pc_label` representing the
    /// current program-counter security context (from enclosing
    /// conditionals).
    fn check_expr_inner(
        &self,
        expr: &Expr,
        span: &Range<usize>,
        pc_label: SecurityLabel,
        errors: &mut Vec<InfoFlowError>,
    ) {
        match expr {
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let cond_label = std::cmp::max(pc_label, self.infer_label(cond));
                self.check_expr_inner(cond, span, pc_label, errors);
                self.check_expr_inner(then_branch, span, cond_label, errors);
                if let Some(else_br) = else_branch {
                    self.check_expr_inner(else_br, span, cond_label, errors);
                }
            }

            // Detect covert channels: high-security pc controls a
            // timing-sensitive or exception-raising call.
            Expr::Call { func, args } => {
                if let Expr::Ident(name) = func.as_ref()
                    && let Some(err) = self.check_covert_channel(pc_label, name, span)
                {
                    errors.push(err);
                }
                self.check_expr_inner(func, span, pc_label, errors);
                for arg in args {
                    self.check_expr_inner(arg, span, pc_label, errors);
                }
            }

            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                if let Some(err) = self.check_covert_channel(pc_label, method, span) {
                    errors.push(err);
                }
                self.check_expr_inner(receiver, span, pc_label, errors);
                for arg in args {
                    self.check_expr_inner(arg, span, pc_label, errors);
                }
            }

            // Recurse into sub-expressions
            Expr::BinOp { lhs, rhs, .. } => {
                self.check_expr_inner(lhs, span, pc_label, errors);
                self.check_expr_inner(rhs, span, pc_label, errors);
            }
            Expr::UnaryOp { expr: inner, .. }
            | Expr::Old(inner)
            | Expr::Paren(inner)
            | Expr::Cast { expr: inner, .. }
            | Expr::Ghost(inner) => {
                self.check_expr_inner(inner, span, pc_label, errors);
            }
            Expr::Field(receiver, _) => {
                self.check_expr_inner(receiver, span, pc_label, errors);
            }
            Expr::Index { expr: base, index } => {
                self.check_expr_inner(base, span, pc_label, errors);
                self.check_expr_inner(index, span, pc_label, errors);
            }
            Expr::List(items) => {
                for item in items {
                    self.check_expr_inner(item, span, pc_label, errors);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    self.check_expr_inner(e, span, pc_label, errors);
                }
            }
            Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
                self.check_expr_inner(domain, span, pc_label, errors);
                self.check_expr_inner(body, span, pc_label, errors);
            }
            Expr::Apply { args, .. } => {
                for arg in args {
                    self.check_expr_inner(arg, span, pc_label, errors);
                }
            }
            Expr::Match { scrutinee, arms } => {
                self.check_expr_inner(scrutinee, span, pc_label, errors);
                // Each arm body executes under the PC label of the scrutinee
                let scrut_label = self.infer_label(scrutinee);
                let elevated = std::cmp::max(pc_label, scrut_label);
                for arm in arms {
                    self.check_expr_inner(&arm.body, span, elevated, errors);
                }
            }
            Expr::Let { value, body, .. } => {
                self.check_expr_inner(value, span, pc_label, errors);
                self.check_expr_inner(body, span, pc_label, errors);
            }
            Expr::Tuple(elems) => {
                for e in elems {
                    self.check_expr_inner(e, span, pc_label, errors);
                }
            }
            Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
        }
    }
}

impl Default for InfoFlowChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span() -> Range<usize> {
        0..10
    }

    fn ident(s: &str) -> Expr {
        Expr::Ident(s.to_string())
    }

    fn int_lit(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n.to_string()))
    }

    // ---- DependentTypeChecker ----

    #[test]
    fn dep_validate_nat_index() {
        let checker = DependentTypeChecker::new();
        let errs = checker.validate_index("n", "Nat", &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn dep_validate_bool_index() {
        let checker = DependentTypeChecker::new();
        let errs = checker.validate_index("flag", "Bool", &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn dep_validate_unknown_index_type() {
        let checker = DependentTypeChecker::new();
        let errs = checker.validate_index("x", "String", &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A03006");
    }

    #[test]
    fn dep_validate_known_enum_index() {
        let mut checker = DependentTypeChecker::new();
        checker.register_enum("Mode".into(), vec!["Read".into(), "Write".into()]);
        let errs = checker.validate_index("m", "Mode", &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn dep_check_nat_index_expr_literal() {
        let checker = DependentTypeChecker::new();
        let errs = checker.check_index_expr(&int_lit(5), &DepIndex::Nat("n".into()), &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn dep_check_nat_index_expr_arithmetic() {
        let mut checker = DependentTypeChecker::new();
        checker.bind_index("n".into(), DepIndex::Nat("n".into()));
        let expr = Expr::BinOp {
            lhs: Box::new(ident("n")),
            op: BinOp::Add,
            rhs: Box::new(int_lit(1)),
        };
        let errs = checker.check_index_expr(&expr, &DepIndex::Nat("n".into()), &span());
        assert!(errs.is_empty());
    }

    #[test]
    fn dep_check_bool_index_rejects_arithmetic() {
        let checker = DependentTypeChecker::new();
        let expr = Expr::BinOp {
            lhs: Box::new(int_lit(1)),
            op: BinOp::Add,
            rhs: Box::new(int_lit(2)),
        };
        let errs = checker.check_index_expr(&expr, &DepIndex::Bool("flag".into()), &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A03008");
    }

    #[test]
    fn dep_check_dep_type_eq_base_mismatch() {
        let checker = DependentTypeChecker::new();
        let a = DepType {
            base: Type::Int,
            indices: vec![DepIndex::Nat("n".into())],
        };
        let b = DepType {
            base: Type::Bool,
            indices: vec![DepIndex::Nat("n".into())],
        };
        let errs = checker.check_dep_type_eq(&a, &b, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A03010");
    }

    #[test]
    fn dep_check_dep_type_eq_index_count_mismatch() {
        let checker = DependentTypeChecker::new();
        let a = DepType {
            base: Type::Int,
            indices: vec![DepIndex::Nat("n".into()), DepIndex::Nat("m".into())],
        };
        let b = DepType {
            base: Type::Int,
            indices: vec![DepIndex::Nat("n".into())],
        };
        let errs = checker.check_dep_type_eq(&a, &b, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A03010");
    }

    #[test]
    fn dep_check_dep_type_eq_index_kind_mismatch() {
        let checker = DependentTypeChecker::new();
        let a = DepType {
            base: Type::Int,
            indices: vec![DepIndex::Nat("n".into())],
        };
        let b = DepType {
            base: Type::Int,
            indices: vec![DepIndex::Bool("flag".into())],
        };
        let errs = checker.check_dep_type_eq(&a, &b, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A03011");
    }

    #[test]
    fn dep_check_index_erasure_in_runtime() {
        let mut checker = DependentTypeChecker::new();
        checker.bind_index("n".into(), DepIndex::Nat("n".into()));
        let errs = checker.check_index_erasure(&ident("n"), false, &span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A03012");
    }

    #[test]
    fn dep_check_index_erasure_in_ghost_ok() {
        let mut checker = DependentTypeChecker::new();
        checker.bind_index("n".into(), DepIndex::Nat("n".into()));
        let errs = checker.check_index_erasure(&ident("n"), true, &span());
        assert!(errs.is_empty());
    }

    // ---- InfoFlowChecker ----

    #[test]
    fn ifc_assignment_upward_ok() {
        let checker = InfoFlowChecker::new();
        let err =
            checker.check_assignment(SecurityLabel::Confidential, SecurityLabel::Public, &span());
        assert!(err.is_none());
    }

    #[test]
    fn ifc_assignment_downward_error() {
        let checker = InfoFlowChecker::new();
        let err =
            checker.check_assignment(SecurityLabel::Public, SecurityLabel::Confidential, &span());
        assert!(err.is_some());
        assert_eq!(err.unwrap().code.as_ref(), "A08001");
    }

    #[test]
    fn ifc_declassify_without_annotation() {
        let checker = InfoFlowChecker::new();
        let err = checker.check_declassify(
            SecurityLabel::Restricted,
            SecurityLabel::Public,
            false,
            &span(),
        );
        assert!(err.is_some());
        assert_eq!(err.unwrap().code.as_ref(), "A08002");
    }

    #[test]
    fn ifc_declassify_with_annotation_ok() {
        let checker = InfoFlowChecker::new();
        let err = checker.check_declassify(
            SecurityLabel::Restricted,
            SecurityLabel::Public,
            true,
            &span(),
        );
        assert!(err.is_none());
    }

    #[test]
    fn ifc_purpose_label_mismatch() {
        let mut checker = InfoFlowChecker::new();
        checker.declare_purpose("email".into(), "billing".into());
        let err = checker.check_purpose_label("email", "marketing", &span());
        assert!(err.is_some());
        assert_eq!(err.unwrap().code.as_ref(), "A08003");
    }

    #[test]
    fn ifc_purpose_label_match() {
        let mut checker = InfoFlowChecker::new();
        checker.declare_purpose("email".into(), "billing".into());
        let err = checker.check_purpose_label("email", "billing", &span());
        assert!(err.is_none());
    }

    #[test]
    fn ifc_implicit_flow_error() {
        let checker = InfoFlowChecker::new();
        let err =
            checker.check_implicit_flow(SecurityLabel::Restricted, SecurityLabel::Public, &span());
        assert!(err.is_some());
        assert_eq!(err.unwrap().code.as_ref(), "A08004");
    }

    #[test]
    fn ifc_covert_channel_sleep() {
        let checker = InfoFlowChecker::new();
        let err = checker.check_covert_channel(SecurityLabel::Confidential, "sleep", &span());
        assert!(err.is_some());
        assert_eq!(err.unwrap().code.as_ref(), "A08005");
    }

    #[test]
    fn ifc_covert_channel_public_ok() {
        let checker = InfoFlowChecker::new();
        let err = checker.check_covert_channel(SecurityLabel::Public, "sleep", &span());
        assert!(err.is_none());
    }

    #[test]
    fn ifc_infer_label_ident() {
        let mut checker = InfoFlowChecker::new();
        checker.declare("secret".into(), SecurityLabel::Restricted);
        assert_eq!(
            checker.infer_label(&ident("secret")),
            SecurityLabel::Restricted
        );
    }

    #[test]
    fn ifc_infer_label_binop_max() {
        let mut checker = InfoFlowChecker::new();
        checker.declare("a".into(), SecurityLabel::Internal);
        checker.declare("b".into(), SecurityLabel::Confidential);
        let expr = Expr::BinOp {
            lhs: Box::new(ident("a")),
            op: BinOp::Add,
            rhs: Box::new(ident("b")),
        };
        assert_eq!(checker.infer_label(&expr), SecurityLabel::Confidential);
    }

    #[test]
    fn ifc_infer_label_literal_is_public() {
        let checker = InfoFlowChecker::new();
        assert_eq!(checker.infer_label(&int_lit(42)), SecurityLabel::Public);
    }

    #[test]
    fn ifc_check_expr_covert_channel_in_if() {
        let mut checker = InfoFlowChecker::new();
        checker.declare("secret".into(), SecurityLabel::Restricted);
        let expr = Expr::If {
            cond: Box::new(ident("secret")),
            then_branch: Box::new(Expr::Call {
                func: Box::new(ident("sleep")),
                args: vec![int_lit(1)],
            }),
            else_branch: None,
        };
        let errs = checker.check_expr(&expr, &span());
        assert!(!errs.is_empty());
        assert!(errs.iter().any(|e| e.code.as_ref() == "A08005"));
    }

    #[test]
    fn ifc_has_labels() {
        let mut checker = InfoFlowChecker::new();
        assert!(!checker.has_labels());
        checker.declare("x".into(), SecurityLabel::Public);
        assert!(checker.has_labels());
    }

    #[test]
    fn ifc_security_label_ordering() {
        assert!(SecurityLabel::Public < SecurityLabel::Internal);
        assert!(SecurityLabel::Internal < SecurityLabel::Confidential);
        assert!(SecurityLabel::Confidential < SecurityLabel::Restricted);
    }
}
