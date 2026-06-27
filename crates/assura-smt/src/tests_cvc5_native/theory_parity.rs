use super::*;

// ===========================================================================
// CVC5 parity tests for dedicated SMT theory verifiers (#516-#522)
// Mirrors Z3 tests in tests_z3.rs lines 3517-4196
// ===========================================================================

#[cfg(feature = "cvc5-verify")]
mod dedicated_theory_parity {
    use super::*;
    use crate::cvc5_backend::{
        verify_constant_time_cvc5, verify_crash_recovery_cvc5, verify_crypto_conformance_cvc5,
        verify_lock_ordering_cvc5, verify_monotonic_state_cvc5, verify_mvcc_isolation_cvc5,
        verify_secure_erasure_cvc5,
    };

    fn ident_expr(name: &str) -> Expr {
        Expr::Ident(name.to_string())
    }

    fn int_lit_expr(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n.to_string()))
    }

    fn binop_expr(lhs: Expr, op: BinOp, rhs: Expr) -> Expr {
        Expr::BinOp {
            lhs: Box::new(Spanned::no_span(lhs)),
            op,
            rhs: Box::new(Spanned::no_span(rhs)),
        }
    }

    fn sp(e: Expr) -> assura_ast::SpExpr {
        Spanned::no_span(e)
    }

    // -------------------------------------------------------------------
    // #519: Monotonic state (STOR.5)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_monotonic_state_valid_non_decrease() {
        let requires1 = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("old_state"), BinOp::Gte, int_lit_expr(0)),
        );
        let requires2 = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("new_state"), BinOp::Gte, ident_expr("old_state")),
        );
        let body = sp(binop_expr(
            ident_expr("new_state"),
            BinOp::Gte,
            ident_expr("old_state"),
        ));
        let clauses = vec![requires1, requires2];

        let results = verify_monotonic_state_cvc5("MonoTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Verified { .. })),
            "CVC5 monotonic non-decrease should verify, got: {results:?}"
        );
    }

    #[test]
    fn cvc5_monotonic_state_decrease_counterexample() {
        let requires = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("old_state"), BinOp::Gte, int_lit_expr(0)),
        );
        let body = sp(binop_expr(
            ident_expr("new_state"),
            BinOp::Gte,
            ident_expr("old_state"),
        ));
        let clauses = vec![requires];

        let results = verify_monotonic_state_cvc5("MonoTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
            "CVC5 monotonic decrease should produce counterexample, got: {results:?}"
        );
    }

    // -------------------------------------------------------------------
    // #517: Lock ordering (CONC.4)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_lock_ordering_consistent() {
        let req1 = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("lock_a"), BinOp::Lt, ident_expr("lock_b")),
        );
        let req2 = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("lock_b"), BinOp::Lt, ident_expr("lock_c")),
        );
        let body = sp(binop_expr(
            ident_expr("lock_a"),
            BinOp::Lt,
            ident_expr("lock_c"),
        ));
        let clauses = vec![req1, req2];

        let results = verify_lock_ordering_cvc5("LockTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Verified { .. })),
            "CVC5 consistent lock ordering should verify, got: {results:?}"
        );
    }

    #[test]
    fn cvc5_lock_ordering_cycle_counterexample() {
        let req1 = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("lock_a"), BinOp::Lt, ident_expr("lock_b")),
        );
        let req2 = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("lock_b"), BinOp::Lt, ident_expr("lock_a")),
        );
        let body = sp(binop_expr(
            ident_expr("lock_a"),
            BinOp::Lt,
            ident_expr("lock_b"),
        ));
        let clauses = vec![req1, req2];

        let results = verify_lock_ordering_cvc5("LockTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
            "CVC5 cyclic lock ordering should produce counterexample, got: {results:?}"
        );
    }

    // -------------------------------------------------------------------
    // #518: Constant-time (SEC.2)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_constant_time_valid() {
        let req1 = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("x"), BinOp::Gte, int_lit_expr(0)),
        );
        let req2 = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("x"), BinOp::Lt, int_lit_expr(256)),
        );
        let body = sp(binop_expr(ident_expr("x"), BinOp::Lt, int_lit_expr(256)));
        let clauses = vec![req1, req2];

        let results = verify_constant_time_cvc5("CTTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Verified { .. })),
            "CVC5 constant-time valid should verify, got: {results:?}"
        );
    }

    #[test]
    fn cvc5_constant_time_secret_dependent() {
        let req = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("secret"), BinOp::Gte, int_lit_expr(0)),
        );
        let body = sp(binop_expr(ident_expr("secret"), BinOp::Eq, int_lit_expr(0)));
        let clauses = vec![req];

        let results = verify_constant_time_cvc5("CTTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
            "CVC5 secret-dependent branch should produce counterexample, got: {results:?}"
        );
    }

    // -------------------------------------------------------------------
    // #520: Secure erasure (SEC.3)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_secure_erasure_valid() {
        let req = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("buf_size"), BinOp::Gt, int_lit_expr(0)),
        );
        let ens = make_clause(
            ClauseKind::Ensures,
            binop_expr(
                ident_expr("bytes_erased"),
                BinOp::Eq,
                ident_expr("buf_size"),
            ),
        );
        let body = sp(binop_expr(
            ident_expr("bytes_erased"),
            BinOp::Eq,
            ident_expr("buf_size"),
        ));
        let clauses = vec![req, ens];

        let results = verify_secure_erasure_cvc5("EraseTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Verified { .. })),
            "CVC5 full erasure should verify, got: {results:?}"
        );
    }

    #[test]
    fn cvc5_secure_erasure_partial() {
        let req = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("buf_size"), BinOp::Gt, int_lit_expr(0)),
        );
        let body = sp(binop_expr(
            ident_expr("bytes_erased"),
            BinOp::Eq,
            ident_expr("buf_size"),
        ));
        let clauses = vec![req];

        let results = verify_secure_erasure_cvc5("EraseTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
            "CVC5 missing erasure should produce counterexample, got: {results:?}"
        );
    }

    // -------------------------------------------------------------------
    // #516: Crash recovery (STOR.1)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_crash_recovery_with_wal() {
        let req1 = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("has_wal"), BinOp::Eq, int_lit_expr(1)),
        );
        let req2 = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("data_size"), BinOp::Gt, int_lit_expr(0)),
        );
        let ens = make_clause(
            ClauseKind::Ensures,
            binop_expr(ident_expr("recovered"), BinOp::Eq, int_lit_expr(1)),
        );
        let body = sp(binop_expr(
            ident_expr("recovered"),
            BinOp::Eq,
            int_lit_expr(1),
        ));
        let clauses = vec![req1, req2, ens];

        let results = verify_crash_recovery_cvc5("CrashTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Verified { .. })),
            "CVC5 crash recovery with WAL should verify, got: {results:?}"
        );
    }

    #[test]
    fn cvc5_crash_recovery_missing_wal() {
        let req = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("data_size"), BinOp::Gt, int_lit_expr(0)),
        );
        let body = sp(binop_expr(
            ident_expr("recovered"),
            BinOp::Eq,
            int_lit_expr(1),
        ));
        let clauses = vec![req];

        let results = verify_crash_recovery_cvc5("CrashTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
            "CVC5 crash recovery without WAL should produce counterexample, got: {results:?}"
        );
    }

    // -------------------------------------------------------------------
    // #521: MVCC isolation (STOR.3)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_mvcc_isolation_valid() {
        let req1 = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("start_ts"), BinOp::Gte, int_lit_expr(0)),
        );
        let req2 = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("commit_ts"), BinOp::Gte, ident_expr("start_ts")),
        );
        let req3 = make_clause(
            ClauseKind::Requires,
            binop_expr(
                ident_expr("other_commit_ts"),
                BinOp::Lt,
                ident_expr("start_ts"),
            ),
        );
        let body = sp(binop_expr(
            ident_expr("other_commit_ts"),
            BinOp::Lt,
            ident_expr("start_ts"),
        ));
        let clauses = vec![req1, req2, req3];

        let results = verify_mvcc_isolation_cvc5("MvccTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Verified { .. })),
            "CVC5 valid MVCC isolation should verify, got: {results:?}"
        );
    }

    #[test]
    fn cvc5_mvcc_isolation_dirty_read() {
        let req = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("start_ts"), BinOp::Gte, int_lit_expr(0)),
        );
        let body = sp(binop_expr(
            ident_expr("other_commit_ts"),
            BinOp::Lt,
            ident_expr("start_ts"),
        ));
        let clauses = vec![req];

        let results = verify_mvcc_isolation_cvc5("MvccTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
            "CVC5 dirty read scenario should produce counterexample, got: {results:?}"
        );
    }

    // -------------------------------------------------------------------
    // #522: Crypto conformance (SEC.4)
    // -------------------------------------------------------------------

    #[test]
    fn cvc5_crypto_conformance_valid_key_size() {
        let req = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("key_size"), BinOp::Eq, int_lit_expr(32)),
        );
        let body = sp(binop_expr(
            ident_expr("key_size"),
            BinOp::Gte,
            int_lit_expr(16),
        ));
        let clauses = vec![req];

        let results = verify_crypto_conformance_cvc5("CryptoTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Verified { .. })),
            "CVC5 valid key size should verify, got: {results:?}"
        );
    }

    #[test]
    fn cvc5_crypto_conformance_nonce_reuse() {
        let req = make_clause(
            ClauseKind::Requires,
            binop_expr(ident_expr("nonce_counter"), BinOp::Gte, int_lit_expr(0)),
        );
        let body = sp(binop_expr(
            ident_expr("nonce_counter"),
            BinOp::Gt,
            ident_expr("prev_nonce"),
        ));
        let clauses = vec![req];

        let results = verify_crypto_conformance_cvc5("CryptoTest", &body, &clauses);
        assert!(
            results
                .iter()
                .any(|r| matches!(r, VerificationResult::Counterexample { .. })),
            "CVC5 nonce reuse should produce counterexample, got: {results:?}"
        );
    }
}
