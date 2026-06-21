//! Safety-related checks.
//!
//! Constant-time, crypto conformance, secure erasure, unsafe escape.

use assura_parser::ast::{BinOp, BlockKind, ClauseKind, Decl, Expr, Span};

use crate::TypeError;
use crate::checkers::*;
use crate::domain::*;

// ---------------------------------------------------------------------------
// Constant-time wiring (T059)
// ---------------------------------------------------------------------------

/// Scan for functions annotated with `constant_time` clause or `#[secret]`
/// parameter annotations and run the ConstantTimeChecker on their bodies.
pub(crate) fn run_constant_time_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut all_errors = Vec::new();

    for decl in &source.decls {
        let (clauses, params) = match &decl.node {
            Decl::FnDef(f) => (&f.clauses, f.params.as_slice()),
            Decl::Contract(c) => (&c.clauses, &[] as &[_]),
            Decl::Extern(e) => (&e.clauses, e.params.as_slice()),
            _ => continue,
        };

        // Check if function has a constant_time clause
        let has_ct = clauses
            .iter()
            .any(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "constant_time"));
        if !has_ct {
            continue;
        }

        // Build checker: mark parameters with #[secret] or "secret" in type tokens
        let mut checker = ConstantTimeChecker::new();
        for param in params {
            let tokens = param.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
            let is_secret = tokens.iter().any(|t| t == "secret" || t == "#[secret]");
            if is_secret {
                checker.mark_secret(param.name.clone());
            }
        }

        // Check all clause bodies for timing leaks
        for clause in clauses {
            for err in checker.check_expr(&clause.body, &decl.span) {
                all_errors.push(err.into());
            }
        }
    }

    all_errors
}

// ---------------------------------------------------------------------------
// Crypto conformance wiring (G001)
// ---------------------------------------------------------------------------

/// Scan for contracts/functions with `conforms`, `crypto`, or `spec` clause
/// annotations referencing a cryptographic algorithm. Extract algorithm name
/// and any key_size/nonce_size literals from clause bodies, then run the
/// CryptoConformanceChecker against the declared parameters.
pub(crate) fn run_crypto_conformance_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut all_errors = Vec::new();
    let mut checker = CryptoConformanceChecker::new();

    // Pre-register custom algorithm specs from "crypto_spec" clauses
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => c.clauses.as_slice(),
            Decl::FnDef(f) => f.clauses.as_slice(),
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "crypto_spec"
                && let Expr::Raw(tokens) = &clause.body.node
                && !tokens.is_empty()
            {
                let name = tokens[0].trim_matches('"').to_string();
                let key_bits: Vec<u32> = tokens
                    .get(1)
                    .and_then(|s| s.parse().ok())
                    .into_iter()
                    .collect();
                let block_size = tokens.get(2).and_then(|s| s.parse().ok());
                let nonce_size = tokens.get(3).and_then(|s| s.parse().ok());
                let tag_size = tokens.get(4).and_then(|s| s.parse().ok());
                checker.register_spec(CryptoSpec {
                    name,
                    key_size_bits: key_bits,
                    block_size_bytes: block_size,
                    nonce_size_bytes: nonce_size,
                    tag_size_bytes: tag_size,
                });
            }
        }
    }

    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Extern(e) => &e.clauses,
            _ => continue,
        };

        // Look for conforms/crypto/spec clauses
        let mut algorithm: Option<String> = None;
        let mut key_size: Option<u32> = None;
        let mut nonce_size: Option<u32> = None;
        let mut has_tag_check = false;
        let mut nonce_source: Option<String> = None;
        let mut is_counter_nonce = false;
        let mut is_random_nonce = false;

        for clause in clauses {
            let kind_name = match &clause.kind {
                ClauseKind::Other(k) => k.as_str(),
                _ => continue,
            };

            match kind_name {
                "conforms" | "spec" | "crypto" => {
                    // Extract algorithm name from clause body
                    // Note: Literal::Str includes source quotes (e.g. `"AES-128-GCM"`)
                    if let Expr::Literal(assura_parser::ast::Literal::Str(name)) = &clause.body.node
                    {
                        algorithm = Some(name.trim_matches('"').to_string());
                    } else if let Expr::Ident(name) = &clause.body.node {
                        algorithm = Some(name.clone());
                    } else if let Expr::Call { func, .. } = &clause.body.node {
                        if let Expr::Ident(name) = &func.as_ref().node {
                            algorithm = Some(name.clone());
                        }
                    } else if let Expr::Raw(tokens) = &clause.body.node
                        && let Some(t) = tokens.first()
                    {
                        // Fallback: extract from raw tokens (strip quotes)
                        let name = t.trim_matches('"').to_string();
                        if !name.is_empty() {
                            algorithm = Some(name);
                        }
                    }
                }
                "key_size" => {
                    if let Expr::Literal(assura_parser::ast::Literal::Int(s)) = &clause.body.node {
                        key_size = s.parse().ok();
                    } else if let Expr::Raw(tokens) = &clause.body.node
                        && let Some(t) = tokens.first()
                    {
                        key_size = t.parse().ok();
                    }
                }
                "nonce_size" => {
                    if let Expr::Literal(assura_parser::ast::Literal::Int(s)) = &clause.body.node {
                        nonce_size = s.parse().ok();
                    } else if let Expr::Raw(tokens) = &clause.body.node
                        && let Some(t) = tokens.first()
                    {
                        nonce_size = t.parse().ok();
                    }
                }
                "tag_verified" | "tag_check" => {
                    has_tag_check = true;
                }
                "nonce" => {
                    if let Expr::Ident(src) = &clause.body.node {
                        nonce_source = Some(src.clone());
                        is_counter_nonce = src.contains("counter") || src.contains("ctr");
                        is_random_nonce = src.contains("random") || src.contains("rng");
                    } else if let Expr::Raw(tokens) = &clause.body.node
                        && let Some(src) = tokens.first()
                    {
                        nonce_source = Some(src.clone());
                        is_counter_nonce = src.contains("counter") || src.contains("ctr");
                        is_random_nonce = src.contains("random") || src.contains("rng");
                    }
                }
                _ => {}
            }
        }

        // Run checks if an algorithm was declared
        if let Some(ref algo) = algorithm {
            if let Some(ks) = key_size {
                for err in checker.check_key_size(algo, ks, &decl.span) {
                    all_errors.push(err.into());
                }
            }
            if let Some(ns) = nonce_size {
                for err in checker.check_nonce_size(algo, ns, &decl.span) {
                    all_errors.push(err.into());
                }
            }
            if let Some(ref ns_src) = nonce_source {
                for err in checker.check_nonce_uniqueness(
                    ns_src,
                    is_counter_nonce,
                    is_random_nonce,
                    &decl.span,
                ) {
                    all_errors.push(err.into());
                }
            }
            // Only check tag verification for decrypt-type operations
            let has_decrypt_clause = clauses.iter().any(
                |c| matches!(&c.kind, ClauseKind::Other(k) if k == "decrypt" || k == "decryption"),
            );
            if has_decrypt_clause {
                for err in checker.check_tag_verification(has_tag_check, &decl.span) {
                    all_errors.push(err.into());
                }
            }
        }
    }

    all_errors
}

// ---------------------------------------------------------------------------
// Secure erasure wiring (T060)
// ---------------------------------------------------------------------------

/// Scan for parameters annotated with `#[sensitive]` or `@sensitive` and
/// verify that functions handling sensitive data include erasure guarantees.
pub(crate) fn run_secure_erasure_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = SecureErasureChecker::new();
    let mut has_sensitive = false;

    for decl in &source.decls {
        let params = match &decl.node {
            Decl::FnDef(f) => f.params.as_slice(),
            Decl::Extern(e) => e.params.as_slice(),
            _ => continue,
        };

        for param in params {
            // Only `sensitive`/`#[sensitive]` triggers secure erasure.
            // `secret`/`#[secret]` is for constant-time checking (T059).
            let p_tokens = param.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
            let is_sensitive = p_tokens
                .iter()
                .any(|t| t == "sensitive" || t == "#[sensitive]");
            if is_sensitive {
                checker.mark_sensitive(param.name.clone());
                has_sensitive = true;
            }
        }
    }

    if !has_sensitive {
        return Vec::new();
    }

    // Check that sensitive variables have scope-exit erasure
    let mut errors = Vec::new();
    let sensitive_names = checker.sensitive_names();
    // Track the span where each sensitive variable was declared, for error reporting
    let mut sensitive_decl_span: std::collections::HashMap<String, Span> =
        std::collections::HashMap::new();
    for decl in &source.decls {
        let params = match &decl.node {
            Decl::FnDef(f) => f.params.as_slice(),
            Decl::Extern(e) => e.params.as_slice(),
            _ => continue,
        };
        for param in params {
            let p_tokens = param.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
            if p_tokens
                .iter()
                .any(|t| t == "sensitive" || t == "#[sensitive]")
            {
                sensitive_decl_span
                    .entry(param.name.clone())
                    .or_insert_with(|| decl.span.clone());
            }
        }
    }
    for name in &sensitive_names {
        for decl in &source.decls {
            let (clauses, return_ty_tokens) = match &decl.node {
                Decl::FnDef(f) => (
                    f.clauses.as_slice(),
                    f.return_ty
                        .as_ref()
                        .map(|t| t.to_tokens())
                        .unwrap_or_default(),
                ),
                Decl::Extern(e) => (
                    e.clauses.as_slice(),
                    e.return_ty
                        .as_ref()
                        .map(|t| t.to_tokens())
                        .unwrap_or_default(),
                ),
                _ => continue,
            };

            // Look for zeroize/erase patterns in ensures clauses
            let has_erasure = clauses
                .iter()
                .any(|c| c.kind == ClauseKind::Ensures && expr_references_var(&c.body, name));
            if has_erasure {
                checker.mark_zeroized(name.clone());
            }

            // Check for copies of sensitive data to non-sensitive variables
            for clause in clauses {
                if clause.kind == ClauseKind::Ensures {
                    // Look for assignment patterns: target == source
                    if let Expr::BinOp {
                        lhs,
                        op: BinOp::Eq,
                        rhs,
                    } = &clause.body.node
                        && let Expr::Ident(src) = &rhs.as_ref().node
                        && src == name
                        && let Expr::Ident(tgt) = &lhs.as_ref().node
                    {
                        let tgt_is_sensitive = checker.sensitive_names().contains(tgt);
                        for err in checker.check_copy(name, tgt, tgt_is_sensitive, &decl.span) {
                            errors.push(err.into());
                        }
                    }
                }
            }

            // Check if sensitive data is returned without @sensitive annotation
            let fn_return_is_sensitive = return_ty_tokens
                .iter()
                .any(|t| t == "sensitive" || t == "#[sensitive]");
            for err in checker.check_return(name, fn_return_is_sensitive, &decl.span) {
                errors.push(err.into());
            }
        }

        let fallback_span = 0..0usize;
        let scope_span = sensitive_decl_span.get(name).unwrap_or(&fallback_span);
        for err in checker.check_scope_exit(name, scope_span) {
            errors.push(err.into());
        }
    }

    // Final check: all sensitive variables should be erased.
    // Use the first sensitive variable's declaration span as the error location.
    let first_sensitive_span = sensitive_decl_span
        .values()
        .next()
        .cloned()
        .unwrap_or(0..0usize);
    for err in checker.check_all_erased(&first_sensitive_span) {
        errors.push(err.into());
    }

    errors
}

pub(crate) fn run_unsafe_escape_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = UnsafeEscapeChecker::new();
    let mut found = false;
    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                let mut obligations = Vec::new();
                for clause in &f.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && (k == "obligation" || k == "proof_obligation" || k == "must_prove")
                    {
                        if let Expr::Ident(obl) = &clause.body.node {
                            obligations.push(obl.clone());
                        } else if let Some((_, args)) = extract_call(&clause.body) {
                            for arg in args {
                                if let Some(name) = extract_ident(arg) {
                                    obligations.push(name.to_string());
                                }
                            }
                        }
                    }
                }
                for clause in &f.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind {
                        if k == "unsafe" || k == "unsafe_escape" || k == "trusted" {
                            found = true;
                            checker.declare_unsafe(
                                f.name.clone(),
                                obligations.clone(),
                                decl.span.clone(),
                            );
                        }
                        if k == "safety_proof" || k == "proof" {
                            checker.attach_proof(&f.name);
                        }
                    }
                }
            }
            Decl::Block {
                kind, name, body, ..
            } if *kind == BlockKind::UnsafeEscape => {
                found = true;
                let mut obligations = Vec::new();
                for clause in body {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && (k == "obligation" || k == "proof_obligation" || k == "must_prove")
                    {
                        if let Expr::Ident(obl) = &clause.body.node {
                            obligations.push(obl.clone());
                        } else if let Some((_, args)) = extract_call(&clause.body) {
                            for arg in args {
                                if let Some(name) = extract_ident(arg) {
                                    obligations.push(name.to_string());
                                }
                            }
                        }
                    }
                }
                checker.declare_unsafe(name.clone(), obligations, decl.span.clone());
                for clause in body {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && (k == "safety_proof" || k == "proof")
                    {
                        checker.attach_proof(name);
                    }
                }
            }
            _ => {}
        }
    }
    if !found {
        return Vec::new();
    }
    // Discharge obligations from proof clauses
    for decl in &source.decls {
        match &decl.node {
            Decl::FnDef(f) => {
                for clause in &f.clauses {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && (k == "discharges" || k == "proves")
                        && let Expr::Ident(obligation) = &clause.body.node
                    {
                        checker.discharge_obligation(&f.name, obligation.clone());
                    }
                }
            }
            Decl::Block {
                kind, name, body, ..
            } if *kind == BlockKind::UnsafeEscape => {
                for clause in body {
                    if let ClauseKind::Other(ref k) = clause.kind
                        && (k == "discharges" || k == "proves")
                        && let Expr::Ident(obligation) = &clause.body.node
                    {
                        checker.discharge_obligation(name, obligation.clone());
                    }
                }
            }
            _ => {}
        }
    }
    let mut errors = checker.check_unproven();
    errors.extend(checker.check_obligations());
    errors.extend(checker.check_empty_obligations());
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
        let (sf, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        sf.unwrap()
    }

    #[test]
    fn constant_time_no_annotation_no_errors() {
        let sf = parse_source(r#"contract Simple { requires { true } }"#);
        assert!(run_constant_time_checks(&sf).is_empty());
    }

    #[test]
    fn secure_erasure_no_annotation_no_errors() {
        let sf = parse_source(r#"contract Simple { requires { true } }"#);
        assert!(run_secure_erasure_checks(&sf).is_empty());
    }

    #[test]
    fn unsafe_escape_no_annotation_no_errors() {
        let sf = parse_source(r#"contract Simple { requires { true } }"#);
        assert!(run_unsafe_escape_checks(&sf).is_empty());
    }

    #[test]
    fn unsafe_escape_fn_without_proof_emits_a47001() {
        let src = "fn risky(p: Int) -> Int\n    unsafe_escape marker\n    requires { p > 0 }";
        let sf = parse_source(src);
        let errs = run_unsafe_escape_checks(&sf);
        assert!(errs.iter().any(|e| e.code == "A47001"), "got: {errs:?}");
    }
}
