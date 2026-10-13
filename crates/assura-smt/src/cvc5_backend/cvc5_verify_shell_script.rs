//! SMT-LIB2 script assembly helpers for the CVC5 shell-out path.

use std::collections::HashSet;

use assura_ast::{ClauseKind, SpExpr};

use crate::cvc5_expr_smtlib::expr_to_smtlib;
use crate::cvc5_verify_shared::{Cvc5TypeConstraint, collect_cvc5_type_constraints};
use crate::encode_atom_policy::sanitize_smt_name;
use crate::lemma_inject_policy::collect_apply_refs_from_expr;

/// Declare Option tag functions when the script uses `__adt_`.
///
/// The prelude's `forall` axioms make CVC5 answer `unknown` on sat
/// queries (a false `ensures` that should be a counterexample). Ground
/// the tag range and injectivity on the `declare-const` names instead.
pub(crate) fn insert_cvc5_adt_prelude_if_used(script: &mut String) {
    if script.contains("; adt: Option") || !script.contains("__adt_") {
        return;
    }
    let names = declared_const_names(script);
    let mut prelude = String::new();
    for line in crate::cvc5_adt::cvc5_adt_prelude_lines() {
        if line.contains("forall") {
            continue;
        }
        prelude.push_str(&line);
        if !line.ends_with('\n') {
            prelude.push('\n');
        }
    }
    if let Some(idx) = script.find('\n') {
        script.insert_str(idx + 1, &prelude);
    } else {
        script.insert_str(0, &prelude);
    }
    let axioms = ground_option_axioms(&names);
    if let Some(idx) = script.find("(check-sat)") {
        script.insert_str(idx, &axioms);
    } else {
        script.push_str(&axioms);
    }
}

fn declared_const_names(script: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in script.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("(declare-const ") else {
            continue;
        };
        let Some(name) = rest.split_whitespace().next() else {
            continue;
        };
        if name.starts_with("__adt_") {
            continue;
        }
        names.push(name.to_string());
    }
    names
}

fn ground_option_axioms(names: &[String]) -> String {
    let mut out = String::new();
    for name in names {
        out.push_str(&format!(
            "(assert (or (= (__adt_tag_Option {name}) 0) (= (__adt_tag_Option {name}) 1)))\n"
        ));
    }
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            let (a, b) = (&names[i], &names[j]);
            out.push_str(&format!(
                "(assert (=> (and (= (__adt_tag_Option {a}) 0) (= (__adt_tag_Option {b}) 0) (= (__adt_Option_value {a}) (__adt_Option_value {b}))) (= {a} {b})))\n"
            ));
            out.push_str(&format!(
                "(assert (=> (and (= (__adt_tag_Option {a}) 1) (= (__adt_tag_Option {b}) 1)) (= {a} {b})))\n"
            ));
        }
    }
    out
}

/// Declare `__canonical_len_*` constants the encoder mentions but does not declare.
///
/// Z3 creates these on use and asserts `length >= 0`. CVC5 rejects an
/// undeclared symbol, so `s.length() >= 0` became a parse error.
pub(crate) fn declare_missing_canonical_lengths(script: &mut String) {
    let mut names = Vec::new();
    let mut rest = script.as_str();
    while let Some(idx) = rest.find("__canonical_len_") {
        let tail = &rest[idx..];
        let end = tail
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(tail.len());
        let name = &tail[..end];
        if !names.iter().any(|existing: &String| existing == name) {
            names.push(name.to_string());
        }
        rest = &tail[end..];
    }
    let mut decls = String::new();
    for name in names {
        if script.contains(&format!("(declare-const {name} ")) {
            continue;
        }
        decls.push_str(&format!(
            "(declare-const {name} Int)\n(assert (>= {name} 0))\n"
        ));
    }
    if decls.is_empty() {
        return;
    }
    if let Some(idx) = script.find('\n') {
        script.insert_str(idx + 1, &decls);
    } else {
        script.insert_str(0, &decls);
    }
}

/// Tie each `__canonical_len_*` constant to `len` and `__field_len` on its receiver.
///
/// Z3 does this inside `canonical_length`, so `s == s__old` implies
/// `old(s.length())` equals `s.length()`. Bare constants let cvc5 pick a
/// larger old length and report a false counterexample.
pub(crate) fn link_canonical_lengths(script: &mut String) {
    let mut names = Vec::new();
    let mut rest = script.as_str();
    while let Some(idx) = rest.find("__canonical_len_") {
        let tail = &rest[idx..];
        let end = tail
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(tail.len());
        let name = &tail[..end];
        if !names.iter().any(|existing: &String| existing == name) {
            names.push(name.to_string());
        }
        rest = &tail[end..];
    }
    let mut links = String::new();
    for name in &names {
        let Some(obj) = name.strip_prefix("__canonical_len_") else {
            continue;
        };
        if obj.is_empty() {
            continue;
        }
        let declared = script.contains(&format!("(declare-const {obj} "))
            || script.contains(&format!("(declare-fun {obj} "));
        if !declared {
            continue;
        }
        for uf in ["len", "__field_len"] {
            let axiom = format!("(assert (= {name} ({uf} {obj})))");
            if script.contains(&axiom) {
                continue;
            }
            links.push_str(&axiom);
            links.push('\n');
        }
    }
    if links.is_empty() {
        return;
    }
    let mut header = String::new();
    for uf in ["len", "__field_len"] {
        let decl = format!("(declare-fun {uf} (Int) Int)\n");
        if !script.contains(&decl) {
            header.push_str(&decl);
        }
    }
    if !header.is_empty() {
        if let Some(idx) = script.find('\n') {
            script.insert_str(idx + 1, &header);
        } else {
            script.insert_str(0, &header);
        }
    }
    if script.contains("(check-sat)\n") {
        *script = script.replace("(check-sat)\n", &format!("{links}(check-sat)\n"));
    } else {
        script.push_str(&links);
    }
}

/// Declare `__str_*` constants and keep distinct literals unequal.
pub(crate) fn declare_missing_string_literals(script: &mut String) {
    let mut names = Vec::new();
    let mut rest = script.as_str();
    while let Some(idx) = rest.find("__str_") {
        let tail = &rest[idx..];
        let end = tail
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(tail.len());
        let name = &tail[..end];
        if name != "__str_" && !names.iter().any(|existing: &String| existing == name) {
            names.push(name.to_string());
        }
        rest = &tail[end..];
    }
    let mut decls = String::new();
    for name in &names {
        if script.contains(&format!("(declare-const {name} ")) {
            continue;
        }
        decls.push_str(&format!("(declare-const {name} Int)\n"));
    }
    for i in 0..names.len() {
        for j in (i + 1)..names.len() {
            decls.push_str(&format!("(assert (distinct {} {}))\n", names[i], names[j]));
        }
    }
    if decls.is_empty() {
        return;
    }
    if let Some(idx) = script.find('\n') {
        script.insert_str(idx + 1, &decls);
    } else {
        script.insert_str(0, &decls);
    }
}

pub(crate) fn append_cvc5_shellout_requires(script: &mut String, requires: &[&SpExpr]) {
    for req in requires {
        let (result, effects) =
            crate::cvc5_expr_smtlib::with_smtlib_side_effects(|| expr_to_smtlib(req));
        inject_side_effects(script, &effects);
        if let Some(smt) = result {
            script.push_str(&format!("(assert {smt})\n"));
        }
    }
}

/// True when `decl` is already visible at the end of `script`.
///
/// A `(push 1)` frame hides declarations from earlier frames that have
/// been popped. The base script, before the first push, stays visible.
fn declaration_is_in_scope(script: &str, decl: &str) -> bool {
    const PUSH: &str = "(push 1)\n";
    if let Some(frame) = script.rfind(PUSH) {
        if script[frame..].contains(decl) {
            return true;
        }
        if let Some(base_end) = script.find(PUSH) {
            return script[..base_end].contains(decl);
        }
    }
    script.contains(decl)
}

/// Inject accumulated declarations and axioms from tuple/list encoding.
pub(crate) fn inject_side_effects(
    script: &mut String,
    effects: &crate::cvc5_expr_smtlib::SmtlibSideEffects,
) {
    for decl in &effects.declarations {
        if declaration_is_in_scope(script, decl) {
            continue;
        }
        script.push_str(decl);
        script.push('\n');
    }
    for axiom in &effects.assertions {
        script.push_str(axiom);
        script.push('\n');
    }
}

pub(crate) fn append_cvc5_shellout_frame_axioms(
    script: &mut String,
    vars: &HashSet<String>,
    frame_vars: &[String],
) {
    for var_name in frame_vars {
        let current = sanitize_smt_name(var_name);
        let old = crate::encode_atom_policy::old_snapshot_name(var_name);
        if !vars.contains(&old) {
            script.push_str(&format!("(declare-const {old} Int)\n"));
        }
        script.push_str(&format!("(assert (= {current} {old}))\n"));
    }
}

pub(crate) fn append_cvc5_shellout_lemma_assumptions(
    script: &mut String,
    body: &SpExpr,
    defs: &std::collections::HashMap<String, Vec<&SpExpr>>,
) {
    let apply_refs = collect_apply_refs_from_expr(body);
    for lemma_name in &apply_refs {
        if let Some(ensures_bodies) = defs.get(lemma_name) {
            for ens_body in ensures_bodies {
                let (result, effects) =
                    crate::cvc5_expr_smtlib::with_smtlib_side_effects(|| expr_to_smtlib(ens_body));
                inject_side_effects(script, &effects);
                if let Some(smt) = result {
                    script.push_str(&format!("(assert {smt})\n"));
                }
            }
        }
    }
}

pub(crate) fn append_cvc5_shellout_clause_check(script: &mut String, kind: ClauseKind, smt: &str) {
    use crate::clause_policy::ClauseCheckPolarity;

    // Same table as Z3 and CVC5 native (`clause_check_polarity`).
    // `decreases` is not `(not <measure>)`; that term has the wrong sort.
    match crate::clause_policy::clause_check_polarity(&kind) {
        Some(ClauseCheckPolarity::ValidityNegateBody) => {
            script.push_str(&format!("(assert (not {smt}))\n"));
        }
        Some(
            ClauseCheckPolarity::SatisfiabilityAssertBody | ClauseCheckPolarity::ValidityAssertBody,
        ) => {
            script.push_str(&format!("(assert {smt})\n"));
        }
        Some(ClauseCheckPolarity::DecreasesNonNeg) => {
            script.push_str(&format!("(assert (not (>= {smt} 0)))\n"));
        }
        None => {}
    }
}

pub(crate) fn append_cvc5_shellout_constraints(
    script: &mut String,
    vars: &HashSet<String>,
    params: &[assura_ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
) {
    let constraints = collect_cvc5_type_constraints(vars, params, return_ty, constants, narrowings);
    for constraint in constraints {
        match constraint {
            Cvc5TypeConstraint::NatNonNegative(name) => {
                script.push_str(&format!(
                    "(assert (and (>= {name} 0) (<= {name} 18446744073709551615)))\n"
                ));
            }
            Cvc5TypeConstraint::IntBounded(name) => {
                script.push_str(&format!(
                    "(assert (and (>= {name} {min}) (<= {name} {max})))\n",
                    min = i64::MIN,
                    max = i64::MAX
                ));
            }
            Cvc5TypeConstraint::BoolZeroOrOne(name) => {
                script.push_str(&format!("(assert (and (>= {name} 0) (<= {name} 1)))\n"));
            }
            Cvc5TypeConstraint::ConstantEq(name, value) => {
                script.push_str(&format!("(assert (= {name} {value}))\n"));
            }
            Cvc5TypeConstraint::NarrowingLe(name, value) => {
                script.push_str(&format!("(assert (<= {name} {value}))\n"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        append_cvc5_shellout_clause_check, declare_missing_canonical_lengths,
        declare_missing_string_literals, inject_side_effects, insert_cvc5_adt_prelude_if_used,
        link_canonical_lengths,
    };
    use crate::cvc5_expr_smtlib::SmtlibSideEffects;
    use assura_ast::ClauseKind;

    fn script_for(kind: ClauseKind) -> String {
        let mut script = String::new();
        append_cvc5_shellout_clause_check(&mut script, kind, "m");
        script
    }

    #[test]
    fn decreases_asserts_measure_nonnegative() {
        assert_eq!(
            script_for(ClauseKind::Decreases),
            "(assert (not (>= m 0)))\n"
        );
    }

    #[test]
    fn adt_prelude_is_omitted_until_an_adt_symbol_appears() {
        let mut script =
            String::from("(set-logic ALL)\n(declare-const n Int)\n(assert (> n 100))\n");
        insert_cvc5_adt_prelude_if_used(&mut script);
        assert!(
            !script.contains("forall"),
            "quantified Option axioms must stay out of a quantifier-free query: {script}"
        );
        script.push_str("(assert (__adt_tag_Option n))\n");
        insert_cvc5_adt_prelude_if_used(&mut script);
        assert!(
            !script.contains("forall"),
            "Option axioms must be ground: {script}"
        );
        assert!(
            script.contains("(assert (or (= (__adt_tag_Option n) 0) (= (__adt_tag_Option n) 1)))"),
            "{script}"
        );
        let once = script.clone();
        insert_cvc5_adt_prelude_if_used(&mut script);
        assert_eq!(script, once, "prelude must not be inserted twice");
    }

    #[test]
    fn canonical_length_is_declared_before_use() {
        let mut script =
            String::from("(set-logic ALL)\n(assert (>= __canonical_len_s 0))\n(check-sat)\n");
        declare_missing_canonical_lengths(&mut script);
        let decl_at = script
            .find("(declare-const __canonical_len_s Int)")
            .expect("length const");
        let use_at = script
            .find("(assert (>= __canonical_len_s 0))")
            .expect("use");
        assert!(decl_at < use_at, "{script}");
        assert!(script.contains("(assert (>= __canonical_len_s 0))"));
    }

    #[test]
    fn bool_method_decl_is_not_repeated_in_one_scope() {
        let decl = "(declare-fun starts_with (Int Int) Bool)";
        let effects = SmtlibSideEffects {
            declarations: vec![decl.to_string()],
            assertions: vec!["(assert (starts_with s __str_a))".to_string()],
            ..SmtlibSideEffects::default()
        };
        let mut script = format!("(set-logic ALL)\n{decl}\n");
        inject_side_effects(&mut script, &effects);
        assert_eq!(script.matches(decl).count(), 1, "{script}");
        assert!(script.contains("(assert (starts_with s __str_a))"));
    }

    #[test]
    fn popped_frame_does_not_hide_a_later_declaration() {
        let decl = "(declare-fun starts_with (Int Int) Bool)";
        let effects = SmtlibSideEffects {
            declarations: vec![decl.to_string()],
            ..SmtlibSideEffects::default()
        };
        let mut script = format!("(set-logic ALL)\n(push 1)\n{decl}\n(pop 1)\n(push 1)\n");
        inject_side_effects(&mut script, &effects);
        assert_eq!(script.matches(decl).count(), 2, "{script}");
    }

    #[test]
    fn old_length_is_tied_to_the_framed_receiver() {
        let mut script = String::from(
            "(set-logic ALL)\n(declare-const s Int)\n(declare-const s__old Int)\n(assert (= s s__old))\n(assert (<= __canonical_len_s 3))\n(assert (not (<= __canonical_len_s__old 3)))\n(check-sat)\n",
        );
        declare_missing_canonical_lengths(&mut script);
        link_canonical_lengths(&mut script);
        let link = script
            .find("(assert (= __canonical_len_s__old (len s__old)))")
            .expect("old length link");
        let check = script.find("(check-sat)").expect("check");
        let decl_s = script.find("(declare-const s__old Int)").expect("s__old");
        assert!(decl_s < link, "{script}");
        assert!(link < check, "{script}");
        assert!(script.contains("(declare-fun len (Int) Int)"));
        assert!(script.contains("(assert (= __canonical_len_s (len s)))"));
        let Ok(out) = std::process::Command::new("cvc5").arg("--version").output() else {
            return;
        };
        if !out.status.success() {
            return;
        }
        match crate::cvc5_verify_shell_runner::run_cvc5_binary(&script, 5_000) {
            crate::cvc5_verify_shell_runner::Cvc5Result::Unsat => {}
            crate::cvc5_verify_shell_runner::Cvc5Result::Sat(model) => {
                panic!("framed old length must verify, sat: {model}\n{script}")
            }
            crate::cvc5_verify_shell_runner::Cvc5Result::Timeout => {
                panic!("framed old length must not time out\n{script}")
            }
            crate::cvc5_verify_shell_runner::Cvc5Result::Error(reason) => {
                panic!("framed old length cvc5 error: {reason}\n{script}")
            }
        }
    }

    #[test]
    fn string_literals_are_declared_and_distinct() {
        let mut script = String::from(
            "(set-logic ALL)\n(assert (starts_with s __str_a))\n(assert (not (starts_with s __str_b)))\n",
        );
        declare_missing_string_literals(&mut script);
        let decl_a = script.find("(declare-const __str_a Int)").expect("a");
        let use_a = script
            .find("(assert (starts_with s __str_a))")
            .expect("use");
        let distinct_at = script
            .find("(assert (distinct __str_a __str_b))")
            .expect("distinct");
        assert!(decl_a < distinct_at, "{script}");
        assert!(distinct_at < use_a, "{script}");
        assert!(script.contains("(declare-const __str_b Int)"));
    }

    #[test]
    fn ensures_negates_and_invariant_asserts() {
        assert_eq!(script_for(ClauseKind::Ensures), "(assert (not m))\n");
        assert_eq!(script_for(ClauseKind::Rule), "(assert (not m))\n");
        assert_eq!(script_for(ClauseKind::Invariant), "(assert m)\n");
        assert_eq!(script_for(ClauseKind::MustNot), "(assert m)\n");
    }
}
