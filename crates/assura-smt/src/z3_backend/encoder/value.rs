//! Z3 value wrapper and raw-token operator kinds.

use z3::ast;

/// A Z3 expression that can be either an integer or boolean sort.
#[derive(Clone)]
pub(crate) enum Z3Value {
    Bool(ast::Bool),
    Int(ast::Int),
    Real(ast::Real),
    /// Native Z3 string value (only used when `use_string_theory` is enabled).
    Str(ast::String),
    /// Fixed-width bitvector (#265).
    Bv(ast::BV),
}

/// Binary operator kind for raw token parsing.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RawOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
    Implies,
}

impl Z3Value {
    /// Extract as Bool. If Int, create `!= 0` comparison.
    pub(crate) fn as_bool(&self) -> ast::Bool {
        match self {
            Z3Value::Bool(b) => b.clone(),
            Z3Value::Int(i) => i.eq(ast::Int::from_i64(0)).not(),
            Z3Value::Real(r) => r.eq(ast::Real::from_rational(0, 1)).not(),
            // Str: non-empty string is truthy
            Z3Value::Str(s) => s.length().eq(ast::Int::from_i64(0)).not(),
            Z3Value::Bv(b) => b.eq(ast::BV::from_u64(0, b.get_size())).not(),
        }
    }

    /// Extract as Int. If Bool, use `ite(b, 1, 0)` for sound coercion.
    /// If Real, truncate via `real2int`. If Str, use `str.len`.
    pub(crate) fn as_int(&self, _counter: &mut u32) -> ast::Int {
        match self {
            Z3Value::Int(i) => i.clone(),
            Z3Value::Bool(b) => {
                // Sound coercion: true -> 1, false -> 0
                let one = ast::Int::from_i64(1);
                let zero = ast::Int::from_i64(0);
                b.ite(&one, &zero)
            }
            Z3Value::Real(r) => ast::Real::to_int(r),
            // Str: coerce to length for integer context
            Z3Value::Str(s) => s.length(),
            // BV: sound unsigned int interpretation (not a free UF).
            // Free `__bv_as_int_*` UFs were unsound: equal BVs could coerce to different ints.
            Z3Value::Bv(b) => b.to_int(false),
        }
    }

    pub(crate) fn as_bv(&self, width: u32) -> ast::BV {
        match self {
            Z3Value::Bv(b) => b.clone(),
            Z3Value::Int(i) => ast::BV::from_int(i, width),
            Z3Value::Bool(b) => {
                let one = ast::BV::from_u64(1, width);
                let zero = ast::BV::from_u64(0, width);
                b.ite(&one, &zero)
            }
            // Non-int/non-bool/non-bv: last-resort uninterpreted (caller should avoid this path).
            _ => ast::BV::new_const("__bv_coerce_unknown", width),
        }
    }

    /// Extract as Real. If Int, convert via `int2real`. If Bool, use
    /// `ite(b, 1.0, 0.0)` for sound coercion.
    pub(crate) fn as_real(&self, _counter: &mut u32) -> ast::Real {
        match self {
            Z3Value::Real(r) => r.clone(),
            Z3Value::Int(i) => ast::Real::from_int(i),
            Z3Value::Bool(b) => {
                let one = ast::Real::from_rational(1, 1);
                let zero = ast::Real::from_rational(0, 1);
                b.ite(&one, &zero)
            }
            // Str: coerce via length
            Z3Value::Str(s) => ast::Real::from_int(&s.length()),
            Z3Value::Bv(b) => ast::Real::from_int(&ast::Int::new_const(
                crate::encode_atom_policy::bv_as_real_name(b).as_str(),
            )),
        }
    }
}
