use super::*;

// T055 MEM.2: Fixed-width integer checker
// ---------------------------------------------------------------------------

/// A structured error from fixed-width integer checking.
pub(crate) type FixedWidthError = CheckerError;

/// Checker for fixed-width integer types with overflow detection.
///
/// Tracks fixed-width integer types in expressions, detects potential
/// arithmetic overflow, validates cast safety, and flags signed/unsigned
/// mismatches.
///
/// Implements MEM.2 from Section 14 of the specification.
///
/// # Error codes
///
/// - **A10101**: Potential integer overflow in arithmetic operation
/// - **A10102**: Unsafe narrowing cast (e.g., U32 to U16 without bounds check)
/// - **A10103**: Signed/unsigned mismatch in comparison
/// - **A10104**: Division/modulo by zero not guarded
#[derive(Debug, Clone)]
pub(crate) struct FixedWidthChecker {
    /// Maps variable name to its fixed-width type.
    bindings: HashMap<String, Type>,
}

impl FixedWidthChecker {
    /// Create an empty fixed-width checker.
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }

    /// Register a variable with its fixed-width integer type.
    pub fn declare(&mut self, name: String, ty: Type) {
        self.bindings.insert(name, ty);
    }

    /// Look up the type of a registered variable.
    pub fn get_type(&self, name: &str) -> Option<&Type> {
        self.bindings.get(name)
    }

    /// Return the valid numeric range `(min, max)` for a fixed-width type.
    ///
    /// Returns `None` for non-fixed-width types.
    pub fn range_for_type(ty: &Type) -> Option<(i128, i128)> {
        match ty {
            Type::U8 => Some((0, u8::MAX as i128)),
            Type::U16 => Some((0, u16::MAX as i128)),
            Type::U32 => Some((0, u32::MAX as i128)),
            Type::U64 => Some((0, u64::MAX as i128)),
            Type::I8 => Some((i8::MIN as i128, i8::MAX as i128)),
            Type::I16 => Some((i16::MIN as i128, i16::MAX as i128)),
            Type::I32 => Some((i32::MIN as i128, i32::MAX as i128)),
            Type::I64 => Some((i64::MIN as i128, i64::MAX as i128)),
            _ => None,
        }
    }

    /// Returns `true` if the given type is a fixed-width integer type.
    pub fn is_fixed_width(ty: &Type) -> bool {
        Self::range_for_type(ty).is_some()
    }

    /// Returns `true` if the given type is an unsigned fixed-width integer.
    pub fn is_unsigned(ty: &Type) -> bool {
        matches!(ty, Type::U8 | Type::U16 | Type::U32 | Type::U64)
    }

    /// Returns `true` if the given type is a signed fixed-width integer.
    pub fn is_signed(ty: &Type) -> bool {
        matches!(ty, Type::I8 | Type::I16 | Type::I32 | Type::I64)
    }

    /// Check whether an arithmetic operation can overflow given the operand
    /// type ranges.
    ///
    /// Returns `true` if the result of `op` applied to values in
    /// `left_range` and `right_range` can produce a value outside
    /// `result_range`.
    pub fn can_overflow(
        op: &BinOp,
        left_range: (i128, i128),
        right_range: (i128, i128),
        result_range: (i128, i128),
    ) -> bool {
        let (result_min, result_max) = result_range;
        match op {
            BinOp::Add => {
                let worst_low = left_range.0.saturating_add(right_range.0);
                let worst_high = left_range.1.saturating_add(right_range.1);
                worst_low < result_min || worst_high > result_max
            }
            BinOp::Sub => {
                let worst_low = left_range.0.saturating_sub(right_range.1);
                let worst_high = left_range.1.saturating_sub(right_range.0);
                worst_low < result_min || worst_high > result_max
            }
            BinOp::Mul => {
                let products = [
                    left_range.0.saturating_mul(right_range.0),
                    left_range.0.saturating_mul(right_range.1),
                    left_range.1.saturating_mul(right_range.0),
                    left_range.1.saturating_mul(right_range.1),
                ];
                let worst_low = products.iter().copied().min().unwrap_or(0);
                let worst_high = products.iter().copied().max().unwrap_or(0);
                worst_low < result_min || worst_high > result_max
            }
            _ => false,
        }
    }

    /// Check whether a cast from `from_type` to `to_type` is always safe.
    ///
    /// A cast is safe if every value in the source range fits in the
    /// destination range. Returns `true` for safe (widening) casts,
    /// `false` for potentially unsafe (narrowing) casts.
    pub fn is_safe_cast(from_type: &Type, to_type: &Type) -> bool {
        let from_range = match Self::range_for_type(from_type) {
            Some(r) => r,
            None => return true, // Non-fixed-width types are outside our scope
        };
        let to_range = match Self::range_for_type(to_type) {
            Some(r) => r,
            None => return true,
        };
        from_range.0 >= to_range.0 && from_range.1 <= to_range.1
    }

    /// Check potential overflow in an arithmetic operation on two typed
    /// operands.
    ///
    /// Returns `None` if the operation is safe, or `Some(FixedWidthError)`
    /// with code A10101 if overflow is possible.
    pub fn check_arithmetic_overflow(
        &self,
        op: &BinOp,
        left_type: &Type,
        right_type: &Type,
        span: &Range<usize>,
    ) -> Option<FixedWidthError> {
        // Only check arithmetic ops
        if !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) {
            return None;
        }

        let left_range = Self::range_for_type(left_type)?;
        let right_range = Self::range_for_type(right_type)?;

        // Result type is the wider of the two (or left if same width)
        let result_range = Self::wider_range(left_range, right_range);

        if Self::can_overflow(op, left_range, right_range, result_range) {
            let op_name = op.as_str();
            Some(FixedWidthError {
                code: "A10101".into(),
                message: format!(
                    "potential integer overflow: `{left_type:?} {op_name} {right_type:?}` \
                     can exceed the target range [{}, {}]; consider using `{}`",
                    result_range.0,
                    result_range.1,
                    Self::suggest_checked_alternative(op),
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check whether a cast expression is safe.
    ///
    /// Returns `None` if safe, or `Some(FixedWidthError)` with code
    /// A10102 for an unsafe narrowing cast.
    pub fn check_cast_safety(
        from_type: &Type,
        to_type: &Type,
        span: &Range<usize>,
    ) -> Option<FixedWidthError> {
        if !Self::is_fixed_width(from_type) || !Self::is_fixed_width(to_type) {
            return None;
        }
        if Self::is_safe_cast(from_type, to_type) {
            None
        } else {
            Some(FixedWidthError {
                code: "A10102".into(),
                message: format!(
                    "unsafe narrowing cast from `{from_type:?}` to `{to_type:?}`: \
                     source range [{}, {}] does not fit in target range [{}, {}]; \
                     add a bounds check before casting",
                    Self::range_for_type(from_type).map_or(0, |r| r.0),
                    Self::range_for_type(from_type).map_or(0, |r| r.1),
                    Self::range_for_type(to_type).map_or(0, |r| r.0),
                    Self::range_for_type(to_type).map_or(0, |r| r.1),
                ),
                span: span.clone(),
            })
        }
    }

    /// Check for signed/unsigned mismatch in a comparison operation.
    ///
    /// Returns `None` if both sides have the same signedness, or
    /// `Some(FixedWidthError)` with code A10103.
    pub fn check_signedness_mismatch(
        op: &BinOp,
        left_type: &Type,
        right_type: &Type,
        span: &Range<usize>,
    ) -> Option<FixedWidthError> {
        // Only flag comparison operators
        if !matches!(
            op,
            BinOp::Eq | BinOp::Neq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte
        ) {
            return None;
        }
        if !Self::is_fixed_width(left_type) || !Self::is_fixed_width(right_type) {
            return None;
        }
        let left_signed = Self::is_signed(left_type);
        let right_signed = Self::is_signed(right_type);
        if left_signed != right_signed {
            Some(FixedWidthError {
                code: "A10103".into(),
                message: format!(
                    "signed/unsigned mismatch in comparison: `{left_type:?}` vs \
                     `{right_type:?}`; comparing signed and unsigned integers \
                     can produce unexpected results"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Check whether a division or modulo operation has a zero-guard on
    /// the divisor.
    ///
    /// This is a simplified check: if the RHS is a literal zero, flag it.
    /// Full divisor analysis (tracking which requires clauses guard the
    /// divisor) is deferred to SMT encoding.
    ///
    /// Returns `None` if safe, or `Some(FixedWidthError)` with code
    /// A10104.
    pub fn check_division_by_zero(
        op: &BinOp,
        rhs: &Expr,
        left_type: &Type,
        span: &Range<usize>,
    ) -> Option<FixedWidthError> {
        if !matches!(op, BinOp::Div | BinOp::Mod) {
            return None;
        }
        if !Self::is_fixed_width(left_type) {
            return None;
        }
        if Self::is_literal_zero(rhs) {
            let op_name = if *op == BinOp::Div {
                "division"
            } else {
                "modulo"
            };
            Some(FixedWidthError {
                code: "A10104".into(),
                message: format!(
                    "{op_name} by zero: the divisor is a literal zero; \
                     add a guard `requires {{ divisor != 0 }}` or use \
                     a checked alternative"
                ),
                span: span.clone(),
            })
        } else {
            None
        }
    }

    /// Suggest a checked alternative for an arithmetic operator.
    pub fn suggest_checked_alternative(op: &BinOp) -> String {
        match op {
            BinOp::Add => "checked_add".into(),
            BinOp::Sub => "checked_sub".into(),
            BinOp::Mul => "checked_mul".into(),
            BinOp::Div => "checked_div".into(),
            BinOp::Mod => "checked_rem".into(),
            _ => "checked operation".into(),
        }
    }

    /// Check a binary expression for fixed-width integer issues.
    ///
    /// Combines overflow, signedness, and division-by-zero checks.
    pub fn check_binop(
        &self,
        op: &BinOp,
        left_type: &Type,
        right_type: &Type,
        rhs_expr: &Expr,
        span: &Range<usize>,
    ) -> Vec<FixedWidthError> {
        let mut errors = Vec::new();

        if let Some(e) = self.check_arithmetic_overflow(op, left_type, right_type, span) {
            errors.push(e);
        }

        if let Some(e) = Self::check_signedness_mismatch(op, left_type, right_type, span) {
            errors.push(e);
        }

        if let Some(e) = Self::check_division_by_zero(op, rhs_expr, left_type, span) {
            errors.push(e);
        }

        errors
    }

    // -- internal helpers ---------------------------------------------------

    /// Return `true` if an expression is a literal `0`.
    fn is_literal_zero(expr: &Expr) -> bool {
        match expr {
            Expr::Literal(Literal::Int(s)) => s == "0",
            _ => false,
        }
    }

    /// Return the wider of two ranges (union of both ranges).
    fn wider_range(a: (i128, i128), b: (i128, i128)) -> (i128, i128) {
        (std::cmp::min(a.0, b.0), std::cmp::max(a.1, b.1))
    }
}

impl Default for FixedWidthChecker {
    fn default() -> Self {
        Self::new()
    }
}
