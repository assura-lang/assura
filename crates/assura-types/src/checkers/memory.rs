use super::*;

// Memory region contracts (T046 - MEM.1)
// ---------------------------------------------------------------------------

/// A ghost memory region declaration, tracking a named range of valid indices.
///
/// In Assura, a region is a ghost construct: `region valid_range = 0..buf.len`.
/// It describes a set of indices that are valid for buffer access.
#[derive(Debug, Clone)]
pub(crate) struct MemoryRegion {
    /// Name of the region (e.g., "valid_range").
    pub name: String,
    /// Lower bound expression (as variable name or literal).
    pub lower: String,
    /// Upper bound expression (as variable name or literal).
    pub upper: String,
    /// The buffer variable this region is associated with.
    pub buffer: String,
}

/// An error produced by the memory checker.
///
/// Uses error codes from the spec:
/// - **A08101**: Buffer access without bounds check (requires clause missing
///   bounds check for array/buffer index)
/// - **A08102**: Region containment violation (sub-region not proven to be
///   within parent region)
/// - **A08103**: Ghost region references non-existent buffer
pub(crate) type MemoryError = CheckerError;

/// Memory checker for buffer safety contracts (MEM.1).
///
/// Validates that:
/// 1. Buffer access contracts include proper bounds checks in requires clauses
/// 2. Ghost region declarations reference buffers that exist in scope
/// 3. Region containment assertions are well-formed
///
/// The checker works on the type-checked AST and uses the type environment
/// to validate that variables referenced in memory contracts exist and have
/// appropriate types (Bytes, List, etc.).
///
/// # Error codes
///
/// - **A08101**: Buffer access without bounds check
/// - **A08102**: Region containment violation
/// - **A08103**: Ghost region references non-existent buffer
pub(crate) struct MemoryChecker {
    /// Known buffer-typed variables and their capacity expressions.
    /// Maps variable name -> capacity field name (e.g., "buf" -> "buf.len").
    buffers: HashMap<String, String>,
    /// Ghost region declarations.
    regions: Vec<MemoryRegion>,
}

impl MemoryChecker {
    /// Create a new memory checker.
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
            regions: Vec::new(),
        }
    }

    /// Register a buffer-typed variable with its capacity expression.
    ///
    /// Buffer types are: Bytes, List<T>, Sequence<T>, and any user type
    /// with `.len` or `.capacity` fields.
    pub fn register_buffer(&mut self, name: String, capacity: String) {
        self.buffers.insert(name, capacity);
    }

    /// Register a ghost region declaration.
    pub fn register_region(&mut self, region: MemoryRegion) {
        self.regions.push(region);
    }

    /// Returns all registered buffer names.
    pub fn buffer_names(&self) -> Vec<String> {
        self.buffers.keys().cloned().collect()
    }

    /// Returns true if the given variable name is a registered buffer.
    pub fn is_buffer(&self, name: &str) -> bool {
        self.buffers.contains_key(name)
    }

    /// Get the capacity expression for a buffer variable (test-only).
    #[cfg(test)]
    pub fn buffer_capacity(&self, name: &str) -> Option<&str> {
        self.buffers.get(name).map(|s| s.as_str())
    }

    /// Get all registered regions.
    pub fn regions(&self) -> &[MemoryRegion] {
        &self.regions
    }

    /// Check whether a contract's requires clauses contain a proper bounds
    /// check for buffer access.
    ///
    /// A bounds check is an expression of the form:
    ///   `offset + len <= buf.len` or `offset + len <= buf.capacity`
    ///
    /// This function looks for patterns in requires clause expressions
    /// that constrain buffer access to be within bounds.
    ///
    /// Returns `None` if a bounds check is found, or `Some(MemoryError)`
    /// with code A08101 if no bounds check is present.
    pub fn check_bounds_in_requires(
        &self,
        buffer_name: &str,
        requires_exprs: &[&SpExpr],
        span: &Range<usize>,
    ) -> Option<MemoryError> {
        if !self.is_buffer(buffer_name) {
            return None;
        }

        // Look for a bounds-checking pattern in the requires clauses
        let has_bounds_check = requires_exprs
            .iter()
            .any(|expr| self.expr_has_bounds_check(expr, buffer_name));

        if has_bounds_check {
            None
        } else {
            Some(MemoryError {
                code: "A08101".into(),
                message: format!(
                    "buffer `{buffer_name}` accessed without bounds check: \
                     add a `requires` clause constraining index/offset \
                     to be within `{buffer_name}.len`"
                ),
                span: span.clone(),
            })
        }
    }

    /// Check that all ghost region declarations reference existing buffers.
    ///
    /// Returns A08103 errors for regions whose buffer is not registered.
    pub fn check_region_buffers(&self, span: &Range<usize>) -> Vec<MemoryError> {
        let mut errors = Vec::new();
        for region in &self.regions {
            if !self.is_buffer(&region.buffer) {
                errors.push(MemoryError {
                    code: "A08103".into(),
                    message: format!(
                        "ghost region `{}` references non-existent buffer `{}`",
                        region.name, region.buffer,
                    ),
                    span: span.clone(),
                });
            }
        }
        errors
    }

    /// Check that a sub-region is contained within a parent region.
    ///
    /// Returns `None` if both regions are registered and the containment
    /// is well-formed, or `Some(MemoryError)` with code A08102 if the
    /// containment cannot be established structurally.
    pub fn check_region_containment(
        &self,
        sub_region: &str,
        parent_region: &str,
        span: &Range<usize>,
    ) -> Option<MemoryError> {
        let sub = self.regions.iter().find(|r| r.name == sub_region);
        let parent = self.regions.iter().find(|r| r.name == parent_region);

        match (sub, parent) {
            (Some(sub_r), Some(parent_r)) => {
                // Structural containment check is deferred to SMT encoding.
                // Here we validate that both regions exist, reference the
                // same buffer, and have non-empty bounds.
                if sub_r.lower.is_empty() || sub_r.upper.is_empty() {
                    return Some(MemoryError {
                        code: "A08102".into(),
                        message: format!(
                            "sub-region `{sub_region}` has incomplete bounds (lower=`{}`, upper=`{}`)",
                            sub_r.lower, sub_r.upper,
                        ),
                        span: span.clone(),
                    });
                }
                if parent_r.lower.is_empty() || parent_r.upper.is_empty() {
                    return Some(MemoryError {
                        code: "A08102".into(),
                        message: format!(
                            "parent region `{parent_region}` has incomplete bounds (lower=`{}`, upper=`{}`)",
                            parent_r.lower, parent_r.upper,
                        ),
                        span: span.clone(),
                    });
                }
                if sub_r.buffer != parent_r.buffer {
                    Some(MemoryError {
                        code: "A08102".into(),
                        message: format!(
                            "region `{sub_region}` (on buffer `{}`) cannot be contained in \
                             region `{parent_region}` (on buffer `{}`): different buffers",
                            sub_r.buffer, parent_r.buffer,
                        ),
                        span: span.clone(),
                    })
                } else {
                    None
                }
            }
            (None, _) => Some(MemoryError {
                code: "A08102".into(),
                message: format!("sub-region `{sub_region}` is not defined"),
                span: span.clone(),
            }),
            (_, None) => Some(MemoryError {
                code: "A08102".into(),
                message: format!("parent region `{parent_region}` is not defined"),
                span: span.clone(),
            }),
        }
    }

    /// Recursively check whether an expression contains a bounds-checking
    /// pattern for the given buffer.
    ///
    /// Recognized patterns:
    /// - `expr <= buf.len` or `expr <= buf.capacity`
    /// - `expr < buf.len` or `expr < buf.capacity`
    /// - `buf.len >= expr` or `buf.capacity >= expr`
    /// - Any comparison where one side references the buffer's length/capacity
    ///   and the other constrains an offset/index
    fn expr_has_bounds_check(&self, expr: &SpExpr, buffer_name: &str) -> bool {
        match &expr.node {
            Expr::BinOp { lhs, op, rhs } => {
                match op {
                    BinOp::Lte | BinOp::Lt => {
                        // Check: something <= buf.len
                        self.references_buffer_capacity(rhs, buffer_name)
                            || self.references_buffer_capacity(lhs, buffer_name)
                    }
                    BinOp::Gte | BinOp::Gt => {
                        // Check: buf.len >= something
                        self.references_buffer_capacity(lhs, buffer_name)
                            || self.references_buffer_capacity(rhs, buffer_name)
                    }
                    BinOp::And => {
                        // Conjunction: check both sides
                        self.expr_has_bounds_check(lhs, buffer_name)
                            || self.expr_has_bounds_check(rhs, buffer_name)
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Check if an expression references a buffer's capacity/length.
    ///
    /// Looks for `buf.len`, `buf.capacity`, `buf.length`, or the
    /// registered capacity expression for the buffer.
    fn references_buffer_capacity(&self, expr: &SpExpr, buffer_name: &str) -> bool {
        match &expr.node {
            Expr::Field(receiver, field) => {
                let is_len_field =
                    field == "len" || field == "capacity" || field == "length" || field == "size";
                if is_len_field && let Expr::Ident(name) = &receiver.as_ref().node {
                    return name == buffer_name;
                }
                false
            }
            Expr::Ident(name) => {
                // Check against registered capacity expression
                if let Some(cap) = self.buffers.get(buffer_name) {
                    name == cap
                } else {
                    false
                }
            }
            // Recurse into sub-expressions (e.g., offset + len <= buf.len)
            Expr::BinOp { lhs, rhs, .. } => {
                self.references_buffer_capacity(lhs, buffer_name)
                    || self.references_buffer_capacity(rhs, buffer_name)
            }
            _ => false,
        }
    }
}

impl Default for MemoryChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for MemoryChecker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryChecker")
            .field("buffers", &self.buffers)
            .field("regions", &self.regions)
            .finish()
    }
}

/// Check whether an expression references a variable by name.
pub fn expr_references_var(expr: &SpExpr, var_name: &str) -> bool {
    struct VarRefChecker<'a> {
        target: &'a str,
        found: bool,
    }
    impl ExprVisitor for VarRefChecker<'_> {
        fn visit_ident(&mut self, name: &str) {
            if name == self.target {
                self.found = true;
            }
        }
        fn visit_raw(&mut self, tokens: &[String]) {
            if tokens.iter().any(|t| t.trim() == self.target) {
                self.found = true;
            }
        }
    }
    let mut c = VarRefChecker {
        target: var_name,
        found: false,
    };
    c.visit_expr(expr);
    c.found
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::Spanned;

    fn span() -> Range<usize> {
        0..10
    }

    fn ident(s: &str) -> SpExpr {
        Spanned::no_span(Expr::Ident(s.to_string()))
    }

    #[test]
    fn register_buffer_and_query() {
        let mut mc = MemoryChecker::new();
        mc.register_buffer("buf".into(), "buf.len".into());
        assert!(mc.is_buffer("buf"));
        assert!(!mc.is_buffer("other"));
        assert_eq!(mc.buffer_capacity("buf"), Some("buf.len"));
        assert_eq!(mc.buffer_names().len(), 1);
    }

    #[test]
    fn bounds_check_present_no_error() {
        let mut mc = MemoryChecker::new();
        mc.register_buffer("buf".into(), "buf.len".into());
        // Build `offset <= buf.len`
        let requires_expr = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(ident("offset")),
            op: BinOp::Lte,
            rhs: Box::new(Spanned::no_span(Expr::Field(
                Box::new(ident("buf")),
                "len".into(),
            ))),
        });
        let result = mc.check_bounds_in_requires("buf", &[&requires_expr], &span());
        assert!(result.is_none());
    }

    #[test]
    fn bounds_check_missing_a08101() {
        let mut mc = MemoryChecker::new();
        mc.register_buffer("buf".into(), "buf.len".into());
        // requires clause with no bounds check
        let requires_expr = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(ident("x")),
            op: BinOp::Gt,
            rhs: Box::new(ident("y")),
        });
        let result = mc.check_bounds_in_requires("buf", &[&requires_expr], &span());
        assert!(result.is_some());
        assert_eq!(result.unwrap().code.as_ref(), "A08101");
    }

    #[test]
    fn region_references_nonexistent_buffer_a08103() {
        let mut mc = MemoryChecker::new();
        mc.register_region(MemoryRegion {
            name: "r1".into(),
            lower: "0".into(),
            upper: "10".into(),
            buffer: "nonexistent".into(),
        });
        let errs = mc.check_region_buffers(&span());
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code.as_ref(), "A08103");
    }

    #[test]
    fn region_containment_same_buffer_ok() {
        let mut mc = MemoryChecker::new();
        mc.register_buffer("buf".into(), "buf.len".into());
        mc.register_region(MemoryRegion {
            name: "sub".into(),
            lower: "0".into(),
            upper: "5".into(),
            buffer: "buf".into(),
        });
        mc.register_region(MemoryRegion {
            name: "parent".into(),
            lower: "0".into(),
            upper: "10".into(),
            buffer: "buf".into(),
        });
        let result = mc.check_region_containment("sub", "parent", &span());
        assert!(result.is_none());
    }

    #[test]
    fn region_containment_different_buffers_a08102() {
        let mut mc = MemoryChecker::new();
        mc.register_buffer("a".into(), "a.len".into());
        mc.register_buffer("b".into(), "b.len".into());
        mc.register_region(MemoryRegion {
            name: "sub".into(),
            lower: "0".into(),
            upper: "5".into(),
            buffer: "a".into(),
        });
        mc.register_region(MemoryRegion {
            name: "parent".into(),
            lower: "0".into(),
            upper: "10".into(),
            buffer: "b".into(),
        });
        let result = mc.check_region_containment("sub", "parent", &span());
        assert!(result.is_some());
        assert_eq!(result.unwrap().code.as_ref(), "A08102");
    }

    #[test]
    fn expr_references_var_found() {
        assert!(expr_references_var(&ident("target"), "target"));
    }

    #[test]
    fn expr_references_var_not_found() {
        assert!(!expr_references_var(&ident("other"), "target"));
    }
}

// ---------------------------------------------------------------------------
