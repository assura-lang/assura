use super::*;

// -----------------------------------------------------------------------
// T045: Frame condition (modifies clause) SMT tests
// -----------------------------------------------------------------------

#[test]
fn test_frame_axiom_unmodified_var_verified() {
    // modifies { x }, ensures { y == old(y) }
    // y is NOT modified, so frame axiom y == old(y) is injected.
    // This should VERIFY because the axiom makes it trivially true.
    let src = r#"
        contract FrameUnmodified {
            modifies { x }
            ensures { y == old(y) }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "unmodified var y == old(y) should verify with frame axiom, got: {:?}",
        results[0]
    );
}

#[test]
fn test_frame_no_axiom_for_modified_var() {
    // modifies { x }, ensures { x == old(x) }
    // x IS modified, so no frame axiom is injected.
    // Without a requires binding x to old(x), this should produce
    // a COUNTEREXAMPLE because x is unconstrained.
    let src = r#"
        contract FrameModified {
            modifies { x }
            ensures { x == old(x) }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty(), "should have verification results");
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "modified var x == old(x) should produce counterexample, got: {:?}",
        results[0]
    );
}

#[test]
fn test_frame_axiom_with_requires() {
    // modifies { x }, requires { x > 0 }, ensures { y == old(y) }
    // Frame axiom for y, requires assumed for x.
    let src = r#"
        contract FrameWithReq {
            modifies { x }
            requires { x > 0 }
            ensures { y == old(y) }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "frame axiom + requires should verify, got: {:?}",
        results[0]
    );
}

#[test]
fn test_no_modifies_no_frame_axiom() {
    // No modifies clause: y == old(y) should produce counterexample
    // because no frame axiom is injected.
    let src = r#"
        contract NoModifies {
            ensures { y == old(y) }
        }
    "#;
    let results = verify_source(src);
    assert!(!results.is_empty());
    assert!(
        matches!(&results[0], VerificationResult::Counterexample { .. }),
        "without modifies clause, y == old(y) should be counterexample, got: {:?}",
        results[0]
    );
}

