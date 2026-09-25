use super::*;

#[test]
fn tuple_first_projection_matches_element() {
    let src = r#"
        contract T {
            input(x: Int, y: Int)
            ensures { (x, y).0 == x }
        }
    "#;
    let results = verify_source(src);
    assert!(
        matches!(results.first(), Some(VerificationResult::Verified { .. })),
        "tuple .0 should read the first element, got {results:?}"
    );
}

#[test]
fn old_tuple_projection_matches_old_element() {
    let src = r#"
        contract T {
            input(x: Int, y: Int)
            ensures { old((x, y).0) == old(x) }
        }
    "#;
    let results = verify_source(src);
    assert!(
        matches!(results.first(), Some(VerificationResult::Verified { .. })),
        "old((x, y).0) should match old(x), got {results:?}"
    );
}

#[test]
fn tuple_first_projection_is_not_the_second_element() {
    let src = r#"
        contract T {
            input(x: Int, y: Int)
            requires { x != y }
            ensures { (x, y).0 == y }
        }
    "#;
    let results = verify_source(src);
    assert!(
        matches!(
            results.first(),
            Some(VerificationResult::Counterexample { .. })
        ),
        "(x, y).0 == y must not verify when x != y, got {results:?}"
    );
}
