use super::*;

// Taint tracking (T047 - SEC.1)
// ---------------------------------------------------------------------------

/// Taint label for tracking untrusted data flow.
///
/// Follows the information flow lattice from Section 2.7 of the spec:
/// `Untrusted < Validated < Trusted`
///
/// Data from external sources (network, files, user input) starts as
/// `Untrusted`. Explicit validation functions promote it to `Validated`.
/// Internal data is `Trusted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaintLabel {
    /// Data from an external, potentially malicious source.
    Untrusted,
    /// Data that has been explicitly validated/sanitized.
    Validated,
    /// Internal data known to be safe.
    Trusted,
}

impl std::fmt::Display for TaintLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaintLabel::Untrusted => write!(f, "untrusted"),
            TaintLabel::Validated => write!(f, "validated"),
            TaintLabel::Trusted => write!(f, "trusted"),
        }
    }
}

/// Extract a taint label from type annotation tokens.
///
/// Looks for patterns like `@taint:untrusted`, `@taint:validated`,
/// `@taint:trusted` in a sequence of type tokens (from `Param.ty` or
/// `FnDef.return_ty`). Also handles `@untrusted` short form.
///
/// Returns `Some(label)` if found, `None` if no taint annotation is present.
pub(crate) fn extract_taint_label(type_tokens: &[String]) -> Option<TaintLabel> {
    // Look for pattern: "@" "taint" ":" <label>
    for window in type_tokens.windows(4) {
        if window[0] == "@" && window[1] == "taint" && window[2] == ":" {
            return match window[3].as_str() {
                "untrusted" => Some(TaintLabel::Untrusted),
                "validated" => Some(TaintLabel::Validated),
                "trusted" => Some(TaintLabel::Trusted),
                _ => None,
            };
        }
    }
    // Check shorter form: "@" <label>
    for window in type_tokens.windows(2) {
        if window[0] == "@" {
            return match window[1].as_str() {
                "untrusted" => Some(TaintLabel::Untrusted),
                "validated" => Some(TaintLabel::Validated),
                "trusted" => Some(TaintLabel::Trusted),
                _ => None,
            };
        }
    }
    None
}

/// Taint checker that tracks taint labels through data flow.
///
/// Implements SEC.1 from Section 14 of the spec: untrusted data taint
/// tracking. Ensures that data from external sources (marked
/// `@taint:untrusted`) cannot flow to sensitive positions (array indices,
/// allocation sizes, etc.) without explicit validation.
///
/// # Error codes
///
/// - **A09101**: Tainted data used as array index without validation
/// - **A09102**: Tainted data used as allocation size without validation
/// - **A09103**: Tainted data flows to trusted sink
/// - **A09104**: Taint validation incomplete (partial sanitization)
#[derive(Debug, Clone)]
pub(crate) struct TaintChecker {
    /// Maps variable name to its taint label.
    labels: HashMap<std::string::String, TaintLabel>,
    /// Names of functions known to validate/sanitize input.
    /// These functions convert Untrusted -> Validated.
    validation_fns: std::collections::HashSet<std::string::String>,
    /// Names of functions whose parameters require validated/trusted input.
    /// Maps function name to its parameter taint requirements.
    trusted_sinks: HashMap<std::string::String, Vec<Option<TaintLabel>>>,
}

impl TaintChecker {
    /// Create an empty taint checker with built-in validation function names.
    pub fn new() -> Self {
        let mut validation_fns = std::collections::HashSet::new();
        // Built-in validation function names
        validation_fns.insert("validate".to_string());
        validation_fns.insert("sanitize".to_string());
        Self {
            labels: HashMap::new(),
            validation_fns,
            trusted_sinks: HashMap::new(),
        }
    }

    /// Declare a variable with a taint label.
    pub fn declare(&mut self, name: std::string::String, label: TaintLabel) {
        self.labels.insert(name, label);
    }

    /// Register a function as a validation/sanitization function.
    pub fn register_validator(&mut self, name: std::string::String) {
        self.validation_fns.insert(name);
    }

    /// Register a function as a trusted sink with parameter taint requirements.
    pub fn register_trusted_sink(
        &mut self,
        name: std::string::String,
        param_labels: Vec<Option<TaintLabel>>,
    ) {
        self.trusted_sinks.insert(name, param_labels);
    }

    /// Get the taint label for a variable.
    pub fn get_label(&self, name: &str) -> Option<TaintLabel> {
        self.labels.get(name).copied()
    }

    /// Returns true if any taint labels are tracked.
    pub fn has_taint_info(&self) -> bool {
        !self.labels.is_empty()
    }

    /// Infer the taint label of an expression.
    ///
    /// Taint propagates through operations: if any operand is tainted,
    /// the result is tainted. Uses the minimum in the lattice
    /// (Untrusted < Validated < Trusted).
    pub fn infer_taint(&self, expr: &Expr) -> TaintLabel {
        match expr {
            Expr::Ident(name) => self.get_label(name).unwrap_or(TaintLabel::Trusted),
            Expr::Literal(_) => TaintLabel::Trusted,
            Expr::Field(receiver, _) => self.infer_taint(receiver),
            Expr::BinOp { lhs, rhs, .. } => {
                std::cmp::min(self.infer_taint(lhs), self.infer_taint(rhs))
            }
            Expr::UnaryOp { expr: inner, .. } => self.infer_taint(inner),
            Expr::Call { func, args } => {
                // Validation functions produce Validated output
                if let Expr::Ident(name) = func.as_ref()
                    && self.validation_fns.contains(name)
                {
                    return TaintLabel::Validated;
                }
                // Taint propagates from arguments
                args.iter().fold(TaintLabel::Trusted, |acc, arg| {
                    std::cmp::min(acc, self.infer_taint(arg))
                })
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                if self.validation_fns.contains(method) {
                    return TaintLabel::Validated;
                }
                let r = self.infer_taint(receiver);
                args.iter()
                    .fold(r, |acc, arg| std::cmp::min(acc, self.infer_taint(arg)))
            }
            Expr::Index { expr: base, index } => {
                std::cmp::min(self.infer_taint(base), self.infer_taint(index))
            }
            Expr::Old(inner) | Expr::Paren(inner) | Expr::Cast { expr: inner, .. } => {
                self.infer_taint(inner)
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let mut r = std::cmp::min(self.infer_taint(cond), self.infer_taint(then_branch));
                if let Some(e) = else_branch {
                    r = std::cmp::min(r, self.infer_taint(e));
                }
                r
            }
            Expr::List(items) => items.iter().fold(TaintLabel::Trusted, |a, i| {
                std::cmp::min(a, self.infer_taint(i))
            }),
            Expr::Block(exprs) => exprs.iter().fold(TaintLabel::Trusted, |a, e| {
                std::cmp::min(a, self.infer_taint(e))
            }),
            Expr::Forall { body, .. } | Expr::Exists { body, .. } => self.infer_taint(body),
            Expr::Apply { args, .. } => args.iter().fold(TaintLabel::Trusted, |a, arg| {
                std::cmp::min(a, self.infer_taint(arg))
            }),
            Expr::Match { scrutinee, arms } => {
                let mut r = self.infer_taint(scrutinee);
                for arm in arms {
                    r = std::cmp::min(r, self.infer_taint(&arm.body));
                }
                r
            }
            Expr::Let { value, body, .. } => {
                std::cmp::min(self.infer_taint(value), self.infer_taint(body))
            }
            Expr::Tuple(elems) => elems.iter().fold(TaintLabel::Trusted, |a, e| {
                std::cmp::min(a, self.infer_taint(e))
            }),
            Expr::Ghost(_) | Expr::Raw(_) => TaintLabel::Trusted,
        }
    }

    /// Check an expression for taint violations.
    ///
    /// Walks the expression tree looking for sensitive positions where
    /// untrusted data is used without validation.
    pub fn check_expr(&self, expr: &Expr, span: &Range<usize>) -> Vec<TypeError> {
        let mut errors = Vec::new();
        self.check_expr_inner(expr, span, &mut errors);
        errors
    }

    /// Inner recursive checker for taint violations.
    fn check_expr_inner(&self, expr: &Expr, span: &Range<usize>, errors: &mut Vec<TypeError>) {
        match expr {
            // A09101: tainted data as array index
            Expr::Index { expr: base, index } => {
                let index_taint = self.infer_taint(index);
                if index_taint == TaintLabel::Untrusted {
                    errors.push(TypeError {
                        code: "A09101".into(),
                        message: "tainted data used as array index without validation: \
                             validate the index before using it to access an array"
                            .into(),
                        span: span.clone(),
                        secondary: None,
                    });
                }
                self.check_expr_inner(base, span, errors);
                self.check_expr_inner(index, span, errors);
            }

            // A09102 / A09103: tainted data at function call sites
            Expr::Call { func, args } => {
                if let Expr::Ident(name) = func.as_ref() {
                    // A09102: allocation size
                    if is_alloc_function(name) {
                        for arg in args {
                            if self.infer_taint(arg) == TaintLabel::Untrusted {
                                errors.push(TypeError {
                                    code: "A09102".into(),
                                    message: format!(
                                        "tainted data used as allocation size without \
                                         validation: argument to `{name}` is untrusted"
                                    ),
                                    span: span.clone(),
                                    secondary: None,
                                });
                            }
                        }
                    }

                    // A09103: trusted sink
                    if let Some(param_labels) = self.trusted_sinks.get(name) {
                        for (i, arg) in args.iter().enumerate() {
                            let arg_taint = self.infer_taint(arg);
                            if let Some(Some(required)) = param_labels.get(i)
                                && arg_taint < *required
                            {
                                errors.push(TypeError {
                                    code: "A09103".into(),
                                    message: format!(
                                        "tainted data flows to trusted sink: \
                                         argument {i} to `{name}` is `{arg_taint}` \
                                         but parameter requires `{required}`"
                                    ),
                                    span: span.clone(),
                                    secondary: None,
                                });
                            }
                        }
                    }
                }
                self.check_expr_inner(func, span, errors);
                for arg in args {
                    self.check_expr_inner(arg, span, errors);
                }
            }

            // Recurse into sub-expressions
            Expr::BinOp { lhs, rhs, .. } => {
                self.check_expr_inner(lhs, span, errors);
                self.check_expr_inner(rhs, span, errors);
            }
            Expr::UnaryOp { expr: inner, .. }
            | Expr::Old(inner)
            | Expr::Paren(inner)
            | Expr::Cast { expr: inner, .. }
            | Expr::Ghost(inner) => {
                self.check_expr_inner(inner, span, errors);
            }
            Expr::Field(receiver, _) => {
                self.check_expr_inner(receiver, span, errors);
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.check_expr_inner(receiver, span, errors);
                for arg in args {
                    self.check_expr_inner(arg, span, errors);
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check_expr_inner(cond, span, errors);
                self.check_expr_inner(then_branch, span, errors);
                if let Some(else_br) = else_branch {
                    self.check_expr_inner(else_br, span, errors);
                }
            }
            Expr::List(items) => {
                for item in items {
                    self.check_expr_inner(item, span, errors);
                }
            }
            Expr::Block(exprs) => {
                for e in exprs {
                    self.check_expr_inner(e, span, errors);
                }
            }
            Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
                self.check_expr_inner(domain, span, errors);
                self.check_expr_inner(body, span, errors);
            }
            Expr::Apply { args, .. } => {
                for arg in args {
                    self.check_expr_inner(arg, span, errors);
                }
            }
            Expr::Match { scrutinee, arms } => {
                self.check_expr_inner(scrutinee, span, errors);
                for arm in arms {
                    self.check_expr_inner(&arm.body, span, errors);
                }
            }
            Expr::Let { value, body, .. } => {
                self.check_expr_inner(value, span, errors);
                self.check_expr_inner(body, span, errors);
            }
            Expr::Tuple(elems) => {
                for e in elems {
                    self.check_expr_inner(e, span, errors);
                }
            }
            Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
        }
    }

    /// Check taint flow in a complete source file.
    ///
    /// Extracts taint labels from function parameter and return types,
    /// registers validation functions, then checks all clause expressions
    /// for taint violations. Returns empty if no taint annotations exist.
    pub fn check_file(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = TaintChecker::new();
        let mut has_taint_annotations = false;

        // Pass 1: discover validation functions and trusted sinks
        for decl in &source.decls {
            match &decl.node {
                Decl::FnDef(f) => {
                    if let Some(TaintLabel::Validated) = extract_taint_label(&f.return_ty) {
                        checker.register_validator(f.name.clone());
                        has_taint_annotations = true;
                    }
                    let param_labels: Vec<Option<TaintLabel>> = f
                        .params
                        .iter()
                        .map(|p| extract_taint_label(&p.ty))
                        .collect();
                    // If any param requires validated/trusted, register as sink
                    if param_labels
                        .iter()
                        .any(|l| matches!(l, Some(TaintLabel::Validated | TaintLabel::Trusted)))
                    {
                        checker.register_trusted_sink(f.name.clone(), param_labels.clone());
                        has_taint_annotations = true;
                    }
                    if param_labels.iter().any(|l| l.is_some()) {
                        has_taint_annotations = true;
                    }
                }
                Decl::Extern(e) => {
                    if let Some(TaintLabel::Validated) = extract_taint_label(&e.return_ty) {
                        checker.register_validator(e.name.clone());
                        has_taint_annotations = true;
                    }
                    let param_labels: Vec<Option<TaintLabel>> = e
                        .params
                        .iter()
                        .map(|p| extract_taint_label(&p.ty))
                        .collect();
                    if param_labels
                        .iter()
                        .any(|l| matches!(l, Some(TaintLabel::Validated | TaintLabel::Trusted)))
                    {
                        checker.register_trusted_sink(e.name.clone(), param_labels.clone());
                        has_taint_annotations = true;
                    }
                    if param_labels.iter().any(|l| l.is_some()) {
                        has_taint_annotations = true;
                    }
                }
                _ => {}
            }
        }

        // If no taint annotations, skip the check
        if !has_taint_annotations {
            return Vec::new();
        }

        let mut errors = Vec::new();

        // Pass 2: check each declaration with scoped taint labels
        for decl in &source.decls {
            match &decl.node {
                Decl::FnDef(f) => {
                    let mut fn_checker = checker.clone();
                    for param in &f.params {
                        if let Some(label) = extract_taint_label(&param.ty) {
                            fn_checker.declare(param.name.clone(), label);
                        }
                    }
                    if fn_checker.has_taint_info() {
                        for clause in &f.clauses {
                            errors.extend(fn_checker.check_expr(&clause.body, &decl.span));
                        }
                    }
                }
                Decl::Extern(e) => {
                    let mut fn_checker = checker.clone();
                    for param in &e.params {
                        if let Some(label) = extract_taint_label(&param.ty) {
                            fn_checker.declare(param.name.clone(), label);
                        }
                    }
                    if fn_checker.has_taint_info() {
                        for clause in &e.clauses {
                            errors.extend(fn_checker.check_expr(&clause.body, &decl.span));
                        }
                    }
                }
                Decl::Contract(c) => {
                    if checker.has_taint_info() {
                        for clause in &c.clauses {
                            errors.extend(checker.check_expr(&clause.body, &decl.span));
                        }
                    }
                }
                Decl::Service(s) => {
                    for item in &s.items {
                        match item {
                            ServiceItem::Operation { clauses, .. }
                            | ServiceItem::Query { clauses, .. } => {
                                for clause in clauses {
                                    errors.extend(checker.check_expr(&clause.body, &decl.span));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Decl::Block { body, .. } => {
                    for clause in body {
                        errors.extend(checker.check_expr(&clause.body, &decl.span));
                    }
                }
                Decl::Bind(b) => {
                    let mut fn_checker = checker.clone();
                    for param in &b.params {
                        if let Some(label) = extract_taint_label(&param.ty) {
                            fn_checker.declare(param.name.clone(), label);
                        }
                    }
                    if fn_checker.has_taint_info() {
                        for clause in &b.clauses {
                            errors.extend(fn_checker.check_expr(&clause.body, &decl.span));
                        }
                    }
                }
                // Prophecy, CodecRegistry, TypeDef, EnumDef: no taint tracking needed.
                Decl::Prophecy(_)
                | Decl::CodecRegistry(_)
                | Decl::TypeDef(_)
                | Decl::EnumDef(_) => {}
            }
        }

        errors
    }
}

impl Default for TaintChecker {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns `true` if the function name is an allocation function.
fn is_alloc_function(name: &str) -> bool {
    matches!(
        name,
        "alloc" | "allocate" | "malloc" | "realloc" | "reserve" | "resize"
    )
}

// ---------------------------------------------------------------------------
