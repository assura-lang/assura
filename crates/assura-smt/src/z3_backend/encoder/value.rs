//! Z3 value wrapper.

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
            // Real: truncate to int, then to BV (mirrors as_int Real path).
            Z3Value::Real(r) => ast::BV::from_int(&ast::Real::to_int(r), width),
            // Str: use length as int, then to BV (mirrors as_int Str path).
            Z3Value::Str(s) => ast::BV::from_int(&s.length(), width),
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
            // BV: sound unsigned int interpretation, then to Real (fixes #514).
            Z3Value::Bv(b) => ast::Real::from_int(&b.to_int(false)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_bool_passthrough() {
        z3::with_z3_config(&z3::Config::new(), || {
            let b = Z3Value::Bool(ast::Bool::from_bool(true));
            let _ = b.as_bool(); // should not panic
        });
    }

    #[test]
    fn as_bool_from_int() {
        z3::with_z3_config(&z3::Config::new(), || {
            let v = Z3Value::Int(ast::Int::from_i64(0));
            let _ = v.as_bool(); // produces `0 != 0` which is false
        });
    }

    #[test]
    fn as_int_from_bool() {
        z3::with_z3_config(&z3::Config::new(), || {
            let v = Z3Value::Bool(ast::Bool::from_bool(true));
            let mut counter = 0u32;
            let _ = v.as_int(&mut counter); // ite(true, 1, 0)
        });
    }

    #[test]
    fn as_int_from_real() {
        z3::with_z3_config(&z3::Config::new(), || {
            let v = Z3Value::Real(ast::Real::from_rational(7, 2));
            let mut counter = 0u32;
            let _ = v.as_int(&mut counter); // real2int(3.5) = 3
        });
    }

    #[test]
    fn as_bv_from_int() {
        z3::with_z3_config(&z3::Config::new(), || {
            let v = Z3Value::Int(ast::Int::from_i64(42));
            let bv = v.as_bv(8);
            assert_eq!(bv.get_size(), 8);
        });
    }

    #[test]
    fn as_bv_from_bool() {
        z3::with_z3_config(&z3::Config::new(), || {
            let v = Z3Value::Bool(ast::Bool::from_bool(false));
            let bv = v.as_bv(16);
            assert_eq!(bv.get_size(), 16);
        });
    }

    #[test]
    fn as_real_from_int() {
        z3::with_z3_config(&z3::Config::new(), || {
            let v = Z3Value::Int(ast::Int::from_i64(5));
            let mut counter = 0u32;
            let _ = v.as_real(&mut counter); // int2real(5)
        });
    }

    #[test]
    fn as_real_from_bool() {
        z3::with_z3_config(&z3::Config::new(), || {
            let v = Z3Value::Bool(ast::Bool::from_bool(true));
            let mut counter = 0u32;
            let _ = v.as_real(&mut counter); // ite(true, 1.0, 0.0)
        });
    }

    #[test]
    fn as_bool_from_bv() {
        z3::with_z3_config(&z3::Config::new(), || {
            let v = Z3Value::Bv(ast::BV::from_u64(0, 8));
            let _ = v.as_bool(); // bv == 0 then NOT => false
        });
    }

    #[test]
    fn as_int_from_bv() {
        z3::with_z3_config(&z3::Config::new(), || {
            let v = Z3Value::Bv(ast::BV::from_u64(255, 8));
            let mut counter = 0u32;
            let _ = v.as_int(&mut counter); // bv2int(255, unsigned)
        });
    }

    // Regression tests for #513: as_bv Real/Str coercion was unsound
    #[test]
    fn as_bv_from_real() {
        z3::with_z3_config(&z3::Config::new(), || {
            let v = Z3Value::Real(ast::Real::from_rational(7, 2));
            let bv = v.as_bv(16);
            assert_eq!(bv.get_size(), 16);
        });
    }

    #[test]
    fn as_bv_from_str() {
        z3::with_z3_config(&z3::Config::new(), || {
            let v = Z3Value::Str(ast::String::from("hello"));
            let bv = v.as_bv(8);
            assert_eq!(bv.get_size(), 8);
        });
    }

    // Regression test for #514: as_real BV used display-dependent UF name
    #[test]
    fn as_real_from_bv() {
        z3::with_z3_config(&z3::Config::new(), || {
            let v = Z3Value::Bv(ast::BV::from_u64(42, 8));
            let mut counter = 0u32;
            let _ = v.as_real(&mut counter); // should use bv2int, not UF
        });
    }
}
