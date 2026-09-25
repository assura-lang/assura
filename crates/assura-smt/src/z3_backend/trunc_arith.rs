//! Toward-zero integer division and remainder, matching Rust `/` and `%`.

use z3::ast::{Bool, Int};

pub(crate) fn rust_trunc_div(lhs: &Int, rhs: &Int) -> Int {
    let q = lhs.div(rhs);
    let remainder = lhs.modulo(rhs);
    let zero = Int::from_i64(0);
    let inexact = remainder.eq(&zero).not();
    // Z3 floors for a positive divisor and ceilings for a negative one.
    let pos_div_floor = Bool::and(&[&lhs.lt(&zero), &rhs.gt(&zero), &inexact]);
    let neg_div_ceil = Bool::and(&[&lhs.lt(&zero), &rhs.lt(&zero), &inexact]);
    let q_up = Int::add(&[&q, &Int::from_i64(1)]);
    let q_down = Int::sub(&[&q, &Int::from_i64(1)]);
    pos_div_floor.ite(&q_up, &neg_div_ceil.ite(&q_down, &q))
}

pub(crate) fn rust_trunc_mod(lhs: &Int, rhs: &Int) -> Int {
    Int::sub(&[lhs, &Int::mul(&[rhs, &rust_trunc_div(lhs, rhs)])])
}

#[cfg(test)]
mod tests {
    use super::*;
    use z3::{SatResult, Solver};

    fn eval_i64(term: &Int) -> i64 {
        let solver = Solver::new();
        assert_eq!(solver.check(), SatResult::Sat);
        solver
            .get_model()
            .unwrap()
            .eval(term, true)
            .unwrap()
            .as_i64()
            .unwrap()
    }

    #[test]
    fn z3_floor_and_trunc_on_negative_divisor() {
        let five = Int::from_i64(5);
        let neg5 = Int::from_i64(-5);
        let two = Int::from_i64(2);
        let neg2 = Int::from_i64(-2);
        assert_eq!(eval_i64(&rust_trunc_div(&neg5, &two)), -2);
        assert_eq!(eval_i64(&rust_trunc_mod(&neg5, &two)), -1);
        assert_eq!(eval_i64(&rust_trunc_div(&five, &neg2)), -2);
        assert_eq!(eval_i64(&rust_trunc_mod(&five, &neg2)), 1);
        let neg4 = Int::from_i64(-4);
        assert_eq!(eval_i64(&rust_trunc_div(&neg4, &two)), -2);
        assert_eq!(eval_i64(&rust_trunc_div(&neg5, &neg2)), 2);
        assert_eq!(eval_i64(&rust_trunc_mod(&neg5, &neg2)), -1);
    }
}
